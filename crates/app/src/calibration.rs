//! Assistant d'étalonnage : FOV, posture neutre, amplitudes, expressions, profil.
//!
//! L'assistant ne touche à rien directement : il collecte les mesures image par
//! image ([`Calibration::push`]) et renvoie des [`CalibAction`] que la vue
//! applique. Sa logique se teste donc sans interface.

use eframe::egui;
use tracker_core::Pose6;
use tracker_core::calibration::{
    comfortable_range, curve_input, hfov_from_distance, nose_root_depth_cm,
};
use tracker_core::expression::{self, ExpressionModel, FEATURE_NAMES, FitReport, NUM_FEATURES};
use tracker_core::processing::AXES;

/// Durées des captures, en secondes.
const DISTANCE_S: f64 = 2.0;
const AMPLITUDE_S: f64 = 10.0;
const EXPRESSION_NEUTRAL_S: f64 = 3.0;
const EXPRESSION_S: f64 = 18.0;
/// Entrée max. minimale proposée pour une courbe (°) : en dessous, la vue
/// deviendrait trop nerveuse.
const MIN_CURVE_INPUT: f64 = 10.0;

/// Ce que l'assistant demande à la vue d'appliquer.
#[derive(Debug, Clone, PartialEq)]
pub enum CalibAction {
    /// Nouveau FOV horizontal (°).
    SetHfov(f32),
    /// Recentrer (posture neutre).
    Recenter,
    /// Nouvelles « entrées max. » des courbes de lacet et de tangage (°).
    SetCurveInputs { yaw: f64, pitch: f64 },
    /// Correction des expressions (ou `None` pour la désactiver).
    SetExpression(Option<Box<ExpressionModel>>),
    /// Enregistrer le profil de la webcam.
    SaveProfile,
}

/// Ce que la vue transmet à l'assistant pour chaque image.
#[derive(Debug, Clone, Copy)]
pub struct CalibSample {
    /// Secondes depuis le lancement.
    pub t: f64,
    /// Échelle de la pose (px/cm), si une tête a été trouvée.
    pub depth_scale: Option<f64>,
    /// Largeur de l'image, en pixels.
    pub image_width: f32,
    /// Pose relative (avant filtre).
    pub pose: Option<Pose6>,
    /// Mesure de l'expression.
    pub expression: Option<[f64; NUM_FEATURES]>,
}

/// Capture en cours.
#[derive(Debug)]
enum Capture {
    Distance {
        until: f64,
        scales: Vec<f64>,
        width: f32,
    },
    Amplitude {
        until: f64,
        yaw: Vec<f64>,
        pitch: Vec<f64>,
    },
    Expression {
        start: f64,
        samples: Vec<([f64; NUM_FEATURES], [f64; 6])>,
    },
}

/// État de l'assistant.
pub struct Calibration {
    /// Distance mesurée objectif → racine du nez, en cm.
    measured_cm: f64,
    capture: Option<Capture>,
    /// Dernier instant reçu (pour les comptes à rebours).
    now: f64,
    /// Actions prêtes, rendues au prochain appel de `ui` ou `take_actions`.
    pending: Vec<CalibAction>,
    done: [bool; 5],
    hfov_result: Option<f32>,
    amplitude_result: Option<((f64, f64), (f64, f64))>,
    expression_report: Option<FitReport>,
    message: Option<String>,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            measured_cm: 60.0,
            capture: None,
            now: 0.0,
            pending: Vec::new(),
            done: [false; 5],
            hfov_result: None,
            amplitude_result: None,
            expression_report: None,
            message: None,
        }
    }
}

impl Calibration {
    /// Une capture est-elle en cours ?
    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Reçoit les mesures d'une image ; termine la capture en cours si son temps est écoulé.
    pub fn push(&mut self, s: &CalibSample) {
        self.now = s.t;
        let finished = match &mut self.capture {
            None => false,
            Some(Capture::Distance {
                until,
                scales,
                width,
            }) => {
                if let Some(scale) = s.depth_scale {
                    scales.push(scale);
                    *width = s.image_width;
                }
                s.t >= *until
            }
            Some(Capture::Amplitude { until, yaw, pitch }) => {
                if let Some(p) = s.pose {
                    yaw.push(p.angles.yaw);
                    pitch.push(p.angles.pitch);
                }
                s.t >= *until
            }
            Some(Capture::Expression { start, samples }) => {
                if let (Some(p), Some(f)) = (s.pose, s.expression) {
                    samples.push((f, p.to_array()));
                }
                s.t >= *start + EXPRESSION_S
            }
        };
        if finished {
            self.finish();
        }
    }

    /// Termine la capture en cours et prépare les actions qui en découlent.
    fn finish(&mut self) {
        match self.capture.take() {
            None => {}
            Some(Capture::Distance { scales, width, .. }) => {
                match hfov_from_distance(&scales, self.measured_cm, width) {
                    Some(hfov) => {
                        self.hfov_result = Some(hfov);
                        self.done[0] = true;
                        self.pending.push(CalibAction::SetHfov(hfov));
                        self.message = Some(format!(
                            "FOV mesuré : {hfov:.1}° ({} images).",
                            scales.len()
                        ));
                    }
                    None => {
                        self.message =
                            Some("Aucun visage vu pendant la mesure : recommencer.".into())
                    }
                }
            }
            Some(Capture::Amplitude { yaw, pitch, .. }) => {
                match (comfortable_range(&yaw), comfortable_range(&pitch)) {
                    (Some(y), Some(p)) => {
                        self.amplitude_result = Some((y, p));
                        self.done[2] = true;
                        self.pending.push(CalibAction::SetCurveInputs {
                            yaw: curve_input(y, MIN_CURVE_INPUT),
                            pitch: curve_input(p, MIN_CURVE_INPUT),
                        });
                        self.message = Some(
                            "Courbes du lacet et du tangage ajustées à tes amplitudes.".into(),
                        );
                    }
                    _ => {
                        self.message =
                            Some("Pas assez d'images avec un visage : recommencer.".into())
                    }
                }
            }
            Some(Capture::Expression { samples, .. }) => {
                let neutral_count = (EXPRESSION_NEUTRAL_S * 30.0) as usize;
                match expression::fit(&samples, neutral_count) {
                    Some(report) => {
                        self.done[3] = true;
                        self.message = Some(
                            "Correction apprise : vérifier son effet ci-dessous, puis l'appliquer."
                                .into(),
                        );
                        self.expression_report = Some(report);
                    }
                    None => {
                        self.message =
                            Some("Pas assez d'images avec un visage : recommencer.".into())
                    }
                }
            }
        }
    }

    /// Actions prêtes (et les retire).
    pub fn take_actions(&mut self) -> Vec<CalibAction> {
        std::mem::take(&mut self.pending)
    }

    fn start(&mut self, capture: Capture) {
        self.message = None;
        self.capture = Some(capture);
    }

    fn remaining(&self) -> f64 {
        match &self.capture {
            Some(Capture::Distance { until, .. } | Capture::Amplitude { until, .. }) => {
                until - self.now
            }
            Some(Capture::Expression { start, .. }) => start + EXPRESSION_S - self.now,
            None => 0.0,
        }
    }

    /// Dessine l'assistant ; renvoie les actions à appliquer.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        camera: &str,
        profile_path: Option<&str>,
    ) -> Vec<CalibAction> {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(format!("Étalonnage — {camera}"));
            ui.label("Les étapes peuvent être refaites séparément. L'ordre conseillé est de haut en bas.");
            if let Some(m) = &self.message {
                ui.colored_label(egui::Color32::from_rgb(80, 160, 255), m);
            }
            if self.capture.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.strong(format!("Mesure en cours… {:.0} s", self.remaining().max(0.0).ceil()));
                    if ui.button("Annuler").clicked() {
                        self.capture = None;
                    }
                });
            }
            let busy = self.capture.is_some();
            ui.separator();

            self.step(ui, 0, "1. Champ de vision de la caméra", |this, ui| {
                ui.label(format!(
                    "Face à la caméra, mesurer au mètre la distance entre l'objectif et la racine du nez \
                     (entre les yeux). La pose donne la distance au centre de la tête, {:.1} cm plus loin : \
                     le calcul en tient compte.",
                    nose_root_depth_cm()
                ));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut this.measured_cm).range(25.0..=200.0).suffix(" cm").speed(0.5));
                    if ui.add_enabled(!busy, egui::Button::new("Mesurer (2 s, ne pas bouger)")).clicked() {
                        let until = this.now + DISTANCE_S;
                        this.start(Capture::Distance { until, scales: Vec::new(), width: 0.0 });
                    }
                });
                if let Some(h) = this.hfov_result {
                    ui.label(format!("Résultat : {h:.1}° (appliqué)."));
                }
            });

            self.step(ui, 1, "2. Posture neutre", |this, ui| {
                ui.label(
                    "Assis normalement, face à l'écran (pas à la caméra), regarder son centre et ne pas \
                     bouger. Ce repère définit l'écran : une webcam posée de côté ou en contrebas est corrigée.",
                );
                if ui.add_enabled(!busy, egui::Button::new("Recentrer (moyenne sur 0,5 s)")).clicked() {
                    this.pending.push(CalibAction::Recenter);
                    this.done[1] = true;
                    this.message = Some("Posture neutre enregistrée.".into());
                }
            });

            self.step(ui, 2, "3. Amplitudes confortables", |this, ui| {
                ui.label(
                    "Pendant 10 s, tourner la tête à gauche, à droite, en haut et en bas, aussi loin que \
                     possible en gardant les yeux sur l'écran. Ces limites donneront la vue maximale.",
                );
                if ui.add_enabled(!busy, egui::Button::new("Commencer (10 s)")).clicked() {
                    let until = this.now + AMPLITUDE_S;
                    this.start(Capture::Amplitude { until, yaw: Vec::new(), pitch: Vec::new() });
                }
                if let Some(((yl, yr), (pd, pu))) = this.amplitude_result {
                    ui.label(format!(
                        "Lacet : {yl:.0}° à gauche, {yr:.0}° à droite. Tangage : {pd:.0}° en bas, {pu:.0}° en haut."
                    ));
                }
            });

            self.step(ui, 3, "4. Expressions (facultatif)", |this, ui| {
                ui.label(
                    "Tête immobile (appuyée contre le dossier si possible). 3 s sans expression, puis \
                     cligner des yeux, ouvrir la bouche, lever les sourcils, sourire, jusqu'à la fin des 18 s.",
                );
                if ui.add_enabled(!busy, egui::Button::new("Commencer (18 s)")).clicked() {
                    // La capture doit voir la pose *non corrigée*.
                    this.pending.push(CalibAction::SetExpression(None));
                    this.start(Capture::Expression { start: this.now, samples: Vec::new() });
                }
                if let Some(report) = &this.expression_report {
                    expression_report_ui(ui, report);
                    ui.horizontal(|ui| {
                        if ui.button("Appliquer la correction").clicked() {
                            this.pending.push(CalibAction::SetExpression(Some(Box::new(report.model.clone()))));
                            this.message = Some("Correction des expressions activée.".into());
                        }
                        if ui.button("Ne pas corriger").clicked() {
                            this.pending.push(CalibAction::SetExpression(None));
                            this.message = Some("Correction des expressions désactivée.".into());
                        }
                    });
                }
            });

            self.step(ui, 4, "5. Enregistrer le profil", |this, ui| {
                ui.label("FOV, filtres, courbes et correction des expressions, pour cette webcam.");
                if ui.add_enabled(!busy, egui::Button::new("Enregistrer")).clicked() {
                    this.pending.push(CalibAction::SaveProfile);
                    this.done[4] = true;
                }
                if let Some(path) = profile_path {
                    ui.small(format!("Fichier : {path}"));
                }
            });
        });
        self.take_actions()
    }

    /// Un bloc d'étape avec sa coche.
    fn step(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        title: &str,
        body: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        let mark = if self.done[index] { "✅" } else { "⬜" };
        egui::CollapsingHeader::new(format!("{mark} {title}"))
            .default_open(true)
            .show(ui, |ui| body(self, ui));
    }
}

fn expression_report_ui(ui: &mut egui::Ui, report: &FitReport) {
    egui::Grid::new("expr_report")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Axe");
            ui.strong("Écart sans → avec");
            ui.strong("Influence");
            ui.end_row();
            for (axis, name) in AXES.iter().enumerate() {
                ui.label(*name);
                let unit = if axis < 3 { "°" } else { " cm" };
                ui.monospace(format!(
                    "{:.2} → {:.2}{unit}",
                    report.before[axis], report.after[axis]
                ));
                let c = report.model.coeffs[axis];
                if c == [0.0; NUM_FEATURES] {
                    ui.label("non corrigé");
                } else {
                    let main = (0..NUM_FEATURES)
                        .max_by(|&a, &b| c[a].abs().total_cmp(&c[b].abs()))
                        .unwrap_or(0);
                    ui.label(format!("surtout : {}", FEATURE_NAMES[main]));
                }
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracker_core::pose::Angles;

    fn sample(t: f64, yaw: f64, pitch: f64) -> CalibSample {
        CalibSample {
            t,
            depth_scale: Some(20.0),
            image_width: 1920.0,
            pose: Some(Pose6 {
                angles: Angles {
                    yaw,
                    pitch,
                    roll: 0.0,
                },
                ..Default::default()
            }),
            expression: Some([0.01, 0.1, 0.05]),
        }
    }

    #[test]
    fn mesure_du_fov_apres_2_secondes() {
        let mut c = Calibration::default();
        c.start(Capture::Distance {
            until: DISTANCE_S,
            scales: Vec::new(),
            width: 0.0,
        });
        for i in 0..=60 {
            c.push(&sample(f64::from(i) / 30.0, 0.0, 0.0));
        }
        let actions = c.take_actions();
        let expected = hfov_from_distance(&[20.0], 60.0, 1920.0).unwrap();
        assert_eq!(actions, vec![CalibAction::SetHfov(expected)]);
        assert!(!c.is_capturing());
    }

    #[test]
    fn amplitudes_vers_les_courbes() {
        let mut c = Calibration::default();
        c.start(Capture::Amplitude {
            until: AMPLITUDE_S,
            yaw: Vec::new(),
            pitch: Vec::new(),
        });
        for i in 0..=300 {
            let t = f64::from(i) / 30.0;
            // Balayage : lacet ±35°, tangage −20° / +25°.
            let phase = t * 2.0;
            c.push(&sample(
                t,
                35.0 * phase.sin(),
                if phase.cos() > 0.0 { 25.0 } else { -20.0 } * phase.cos().abs(),
            ));
        }
        let actions = c.take_actions();
        let [CalibAction::SetCurveInputs { yaw, pitch }] = actions[..] else {
            panic!("action attendue : {actions:?}");
        };
        assert!((30.0..=35.0).contains(&yaw), "lacet {yaw}");
        assert!((15.0..=20.0).contains(&pitch), "tangage {pitch}");
    }

    #[test]
    fn rien_sans_visage() {
        let mut c = Calibration::default();
        c.start(Capture::Distance {
            until: 1.0,
            scales: Vec::new(),
            width: 0.0,
        });
        c.push(&CalibSample {
            t: 2.0,
            depth_scale: None,
            image_width: 1920.0,
            pose: None,
            expression: None,
        });
        assert!(c.take_actions().is_empty());
        assert!(c.message.as_deref().unwrap_or("").contains("recommencer"));
    }

    #[test]
    fn les_expressions_produisent_un_rapport() {
        let mut c = Calibration::default();
        c.start(Capture::Expression {
            start: 0.0,
            samples: Vec::new(),
        });
        for i in 0..=(EXPRESSION_S as i32 * 30) {
            let t = f64::from(i) / 30.0;
            let mouth = if (5.0..9.0).contains(&t) { 0.15 } else { 0.0 };
            let mut s = sample(t, 0.0, 20.0 * mouth + if i % 2 == 0 { 0.1 } else { -0.1 });
            s.expression = Some([0.01 + mouth, 0.1, 0.05]);
            c.push(&s);
        }
        let report = c.expression_report.as_ref().expect("rapport");
        assert!(report.after[1] < report.before[1] / 3.0);
        // L'application n'est pas automatique : l'utilisateur doit valider.
        assert!(c.take_actions().is_empty());
    }
}
