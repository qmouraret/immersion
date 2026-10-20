//! Vue caméra : flux vidéo, points du visage superposés, mesures.

use std::path::PathBuf;
use std::time::Instant;

use capture::CameraInfo;
use eframe::egui::{self, Color32, ColorImage, Pos2, Stroke, TextureHandle, TextureOptions};
use nalgebra::Vector3;
use tracker_core::autotune::AxisGroup;
use tracker_core::face::index;
use tracker_core::landmark_log::LandmarkFrame;
use tracker_core::processing::{Processed, TrackingSettings};
use tracker_core::stats::Rolling;
use tracker_core::{HeadPose, Pose6};
use vision::{FaceResult, TrackMode};

use crate::bench::{Bench, Sample, Tab};
use crate::pipeline::{FrameOutput, Pipeline, PipelineCommand, PipelineEvent};
use crate::tuning::Tuning;

/// Champ de vision horizontal par défaut, à ajuster (étalonnage en phase 5).
const DEFAULT_HFOV_DEG: f32 = 66.0;

/// Mesures du flux, affichées dans le panneau latéral.
#[derive(Default)]
struct Stats {
    /// Images par seconde reçues, lissées (moyenne mobile exponentielle).
    fps: f64,
    /// Temps entre la capture et l'affichage, en ms.
    latency_ms: f64,
    /// Conversion du format caméra en RGB (thread de capture), en ms.
    decode_ms: f64,
    /// Conversion RGB → RGBA et envoi de la texture au GPU (thread UI), en ms.
    upload_ms: f64,
    /// Images sautées (trous dans les numéros de séquence).
    dropped: u64,
    /// Nombre de fois où le détecteur a dû être relancé (visage perdu).
    detections: u64,
    last_frame_at: Option<Instant>,
    last_seq: Option<u64>,
}

impl Stats {
    fn on_frame(&mut self, seq: u64, latency_ms: f64) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame_at {
            let instant_fps = 1.0 / now.duration_since(prev).as_secs_f64().max(1e-6);
            self.fps = if self.fps == 0.0 {
                instant_fps
            } else {
                0.9 * self.fps + 0.1 * instant_fps
            };
        }
        if let Some(prev) = self.last_seq {
            self.dropped += seq.saturating_sub(prev + 1);
        }
        self.latency_ms = latency_ms;
        self.last_frame_at = Some(now);
        self.last_seq = Some(seq);
    }
}

/// Options d'affichage.
struct Overlay {
    mirror: bool,
    points: bool,
    roi: bool,
    axes: bool,
}

pub struct CameraView {
    cameras: Vec<CameraInfo>,
    selected: Option<u32>,
    models_dir: PathBuf,
    pipeline: Option<Pipeline>,
    texture: Option<TextureHandle>,
    /// Taille de l'image affichée, pour placer les points.
    frame_size: (f32, f32),
    face: Option<FaceResult>,
    head: Option<HeadPose>,
    pose: Option<Pose6>,
    /// La pose affichée est-elle relative à une pose neutre ?
    recentered: bool,
    hfov_deg: f32,
    processed: Option<Processed>,
    tuning: Tuning,
    bench: Bench,
    tab: Tab,
    /// Origine des temps du banc de test.
    started_at: Instant,
    /// Fenêtres glissantes d'une seconde par axe, avant et après le filtre.
    jitter_raw: [Rolling; 6],
    jitter_filtered: [Rolling; 6],
    /// Format négocié, déjà mis en forme pour l'affichage.
    format: Option<String>,
    error: Option<String>,
    stats: Stats,
    overlay: Overlay,
}

impl CameraView {
    pub fn new(models_dir: PathBuf) -> Self {
        let mut view = Self {
            cameras: Vec::new(),
            selected: None,
            models_dir,
            pipeline: None,
            texture: None,
            frame_size: (1.0, 1.0),
            face: None,
            head: None,
            pose: None,
            recentered: false,
            hfov_deg: DEFAULT_HFOV_DEG,
            processed: None,
            tuning: Tuning::new(TrackingSettings::default()),
            bench: Bench::new(PathBuf::from("recordings")),
            tab: Tab::Camera,
            started_at: Instant::now(),
            jitter_raw: std::array::from_fn(|_| Rolling::new(30)),
            jitter_filtered: std::array::from_fn(|_| Rolling::new(30)),
            format: None,
            error: None,
            stats: Stats::default(),
            overlay: Overlay {
                mirror: true,
                points: true,
                roi: true,
                axes: true,
            },
        };
        view.refresh_cameras();
        if let Some(first) = view.cameras.first().map(|c| c.index) {
            view.open(first);
        }
        view
    }

    fn refresh_cameras(&mut self) {
        match capture::list_cameras() {
            Ok(cams) => self.cameras = cams,
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn open(&mut self, index: u32) {
        // Remplacer l'Option détruit l'ancien pipeline → son `Drop` arrête
        // l'ancien thread (et donc la caméra) avant d'ouvrir la nouvelle.
        self.pipeline = None;
        self.pipeline = Some(Pipeline::start(
            index,
            self.models_dir.clone(),
            self.hfov_deg,
            self.tuning.settings.clone(),
        ));
        self.selected = Some(index);
        self.format = None;
        self.error = None;
        self.face = None;
        self.head = None;
        self.pose = None;
        self.recentered = true; // le pipeline recentre sur la première pose
        self.reset_jitter();
        self.stats = Stats::default();
    }

    /// Traite les événements du pipeline ; ne garde que la dernière image.
    fn poll_pipeline(&mut self, ctx: &egui::Context) {
        let Some(pipeline) = &self.pipeline else {
            return;
        };

        let mut latest = None;
        let mut stopped = false;
        for event in pipeline.drain() {
            match event {
                PipelineEvent::Started(format) => self.format = Some(format),
                PipelineEvent::Frame(output) => {
                    // Chaque image alimente le banc de test (même celles que l'on
                    // n'affichera pas) : les courbes et les enregistrements sont complets.
                    let t = output
                        .frame
                        .captured_at
                        .duration_since(self.started_at)
                        .as_secs_f64();
                    // Les points ne sont copiés que pendant un enregistrement.
                    if self.bench.is_recording()
                        && let Some(lm) = output.face.as_ref().and_then(|f| f.landmarks.as_ref())
                    {
                        self.bench.push_landmarks(LandmarkFrame {
                            t,
                            width: output.frame.width as f32,
                            height: output.frame.height as f32,
                            hfov_deg: self.hfov_deg,
                            landmarks: lm.clone(),
                        });
                    }
                    if let (Some(pose), Some(p)) = (output.pose, output.processed) {
                        self.bench.push(Sample {
                            t,
                            raw: pose.to_array(),
                            filtered: p.filtered.to_array(),
                            output: p.output.to_array(),
                        });
                    }
                    latest = Some(output);
                }
                PipelineEvent::Error { message, fatal } => {
                    self.error = Some(message);
                    stopped |= fatal;
                }
            }
        }

        if stopped {
            // Le thread s'est arrêté : on libère le pipeline (et on cesse de
            // redessiner en continu). Choisir une caméra le relance.
            self.pipeline = None;
        }

        if let Some(output) = latest {
            // Déstructuration d'une `Box` : on déplace ses champs hors du tas.
            let FrameOutput {
                frame,
                decode_ms,
                face,
                head,
                pose,
                processed,
            } = *output;
            let upload_start = Instant::now();
            let size = [frame.width as usize, frame.height as usize];
            let image = ColorImage::from_rgb(size, &frame.rgb);
            match &mut self.texture {
                // On réutilise la même texture GPU plutôt que d'en créer une par image.
                Some(tex) => tex.set(image, TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture("camera", image, TextureOptions::LINEAR))
                }
            }
            self.stats.upload_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
            self.stats.decode_ms = decode_ms;
            self.stats.on_frame(frame.seq, frame.age_ms());
            if face.as_ref().is_some_and(|f| f.mode == TrackMode::Detected) {
                self.stats.detections += 1;
            }
            self.frame_size = (frame.width as f32, frame.height as f32);
            self.face = face;
            self.head = head;
            self.pose = pose;
            self.processed = processed;
            if let (Some(p), Some(f)) = (pose, processed) {
                let (raw, filtered) = (p.to_array(), f.filtered.to_array());
                for i in 0..6 {
                    self.jitter_raw[i].push(raw[i]);
                    self.jitter_filtered[i].push(filtered[i]);
                }
            }
        }
    }

    /// Les valeurs sautent (recentrage, autre focale) : les anciennes mesures
    /// ne veulent plus rien dire.
    fn reset_jitter(&mut self) {
        self.jitter_raw.iter_mut().for_each(Rolling::clear);
        self.jitter_filtered.iter_mut().for_each(Rolling::clear);
    }

    fn send(&self, command: PipelineCommand) {
        if let Some(p) = &self.pipeline {
            p.send(command);
        }
    }

    fn recenter(&mut self) {
        self.bench.clear();
        self.send(PipelineCommand::Recenter);
        self.recentered = true;
        self.reset_jitter();
    }

    fn head_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tête");
        match &self.pose {
            None => {
                ui.label("—");
            }
            Some(_) => {
                // Sortie : ce qui ira au simulateur (filtré + courbe).
                // Gigue : écart-type sur 1 s, avant et après le filtre.
                let names = [
                    ("Lacet", "°"),
                    ("Tangage", "°"),
                    ("Roulis", "°"),
                    ("X", "cm"),
                    ("Y", "cm"),
                    ("Z", "cm"),
                ];
                let output = self
                    .processed
                    .map(|p| p.output.to_array())
                    .unwrap_or_default();
                egui::Grid::new("pose")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Axe");
                        ui.strong("Sortie");
                        ui.strong("± brut");
                        ui.strong("± filtré");
                        ui.end_row();
                        for (i, (name, unit)) in names.iter().enumerate() {
                            ui.label(*name);
                            ui.monospace(format!("{:+7.1} {unit}", output[i]));
                            ui.monospace(format!(
                                "{:.2}",
                                self.jitter_raw[i].std_dev().unwrap_or(0.0)
                            ));
                            ui.monospace(format!(
                                "{:.3}",
                                self.jitter_filtered[i].std_dev().unwrap_or(0.0)
                            ));
                            ui.end_row();
                        }
                    });
                if let Some(h) = &self.head {
                    ui.label(format!("Distance caméra : {:.0} cm", -h.position.z));
                    ui.label(format!("Erreur d'ajustement : {:.1} px", h.residual_px));
                }
            }
        }
        ui.label(if self.recentered {
            "Relatif à la pose neutre"
        } else {
            "Absolu (repère caméra)"
        });
        ui.horizontal(|ui| {
            if ui.button("Recentrer (espace)").clicked() {
                self.recenter();
            }
            if ui
                .add_enabled(self.recentered, egui::Button::new("Absolu"))
                .clicked()
            {
                self.send(PipelineCommand::ClearNeutral);
                self.recentered = false;
                self.reset_jitter();
            }
        });
        let fov = ui.add(
            egui::Slider::new(&mut self.hfov_deg, 40.0..=110.0)
                .suffix("°")
                .text("FOV horizontal"),
        );
        if fov.changed() {
            self.send(PipelineCommand::SetHfov(self.hfov_deg));
            self.recentered = true; // recentrage automatique après changement de focale
            self.reset_jitter();
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Caméra");

        let current = self
            .cameras
            .iter()
            .find(|c| Some(c.index) == self.selected)
            .map_or("aucune", |c| c.name.as_str())
            .to_owned();
        let mut choice = self.selected;
        egui::ComboBox::from_id_salt("camera")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for cam in &self.cameras {
                    ui.selectable_value(&mut choice, Some(cam.index), &cam.name);
                }
            });
        // `if ... && let` (« let chains », Rust 2024) : condition + déstructuration.
        if choice != self.selected
            && let Some(index) = choice
        {
            self.open(index);
        }
        if ui.button("Actualiser la liste").clicked() {
            self.refresh_cameras();
        }

        ui.separator();
        ui.heading("Affichage");
        ui.checkbox(&mut self.overlay.mirror, "Effet miroir");
        ui.checkbox(&mut self.overlay.points, "Points du visage");
        ui.checkbox(&mut self.overlay.roi, "Zone de recadrage");
        ui.checkbox(&mut self.overlay.axes, "Axes de la tête");

        ui.separator();
        ui.heading("Flux");
        ui.label(format!(
            "Format : {}",
            self.format.as_deref().unwrap_or("ouverture…")
        ));
        ui.label(format!("Reçu : {:.1} img/s", self.stats.fps));
        ui.label(format!(
            "Latence capture → écran : {:.1} ms",
            self.stats.latency_ms
        ));
        ui.label(format!("  dont décodage : {:.1} ms", self.stats.decode_ms));
        ui.label(format!("  dont envoi GPU : {:.1} ms", self.stats.upload_ms));
        ui.label(format!("Images sautées : {}", self.stats.dropped));

        ui.separator();
        ui.heading("Visage");
        match &self.face {
            None => {
                ui.label("Modèles non chargés");
            }
            Some(face) => {
                let presence = face.landmarks.as_ref().map_or(0.0, |l| l.presence);
                ui.label(format!("Confiance : {:.0} %", presence * 100.0));
                ui.add(egui::ProgressBar::new(presence).desired_height(6.0));
                let mode = match (face.mode, face.landmarks.is_some()) {
                    (_, false) => "aucun visage",
                    (TrackMode::Detected, true) => "détection",
                    (TrackMode::Tracked, true) => "suivi",
                };
                ui.label(format!("Mode : {mode}"));
                ui.label(format!("Détecteur : {:.1} ms", face.detect_ms));
                ui.label(format!("Points : {:.1} ms", face.landmarks_ms));
                ui.label(format!("Redétections : {}", self.stats.detections));
            }
        }

        if let Some(e) = &self.error {
            ui.separator();
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    fn camera_image(&self, ui: &mut egui::Ui) {
        let Some(tex) = &self.texture else {
            ui.centered_and_justified(|ui| ui.label("En attente de la caméra…"));
            return;
        };
        // Miroir : on inverse les coordonnées de texture horizontalement.
        let uv = if self.overlay.mirror {
            egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
        } else {
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        };
        // On calcule nous-mêmes le rectangle de l'image (proportions conservées,
        // centrée) : c'est le *même* rectangle qui sert à dessiner l'image et à
        // placer les points. Avant, on prenait celui du widget, qui couvre toute la
        // zone disponible : dès que la fenêtre n'était plus en 16:9, les points
        // s'étiraient.
        let available = ui.available_rect_before_wrap();
        ui.allocate_rect(available, egui::Sense::hover());
        let rect = fit_aspect(available, self.frame_size.0 / self.frame_size.1);
        let painter = ui.painter_at(available);
        painter.image(tex.id(), rect, uv, Color32::WHITE);

        if let Some(face) = &self.face {
            self.draw_face(painter, rect, face);
        }
    }

    /// Dessine les points et la zone de recadrage par-dessus l'image.
    fn draw_face(&self, painter: egui::Painter, rect: egui::Rect, face: &FaceResult) {
        let (w, h) = self.frame_size;
        let mirror = self.overlay.mirror;
        // Pixels de l'image source → position à l'écran.
        let to_screen = |x: f32, y: f32| -> Pos2 {
            let u = if mirror { 1.0 - x / w } else { x / w };
            rect.min + egui::vec2(u * rect.width(), y / h * rect.height())
        };

        if self.overlay.roi
            && let Some(roi) = &face.roi
        {
            let color = match face.mode {
                TrackMode::Detected => Color32::from_rgb(255, 170, 0),
                TrackMode::Tracked => Color32::from_rgb(80, 160, 255),
            };
            let corners = roi.corners().map(|(x, y)| to_screen(x, y)).to_vec();
            painter.add(egui::Shape::closed_line(corners, Stroke::new(1.5, color)));
        }

        if self.overlay.points
            && let Some(lm) = &face.landmarks
        {
            // Vert si confiant, orange sinon : utile pour repérer les reflets des lunettes.
            let color = if lm.presence > 0.8 {
                Color32::from_rgb(60, 220, 120)
            } else {
                Color32::from_rgb(255, 150, 60)
            };
            for (i, p) in lm.points.iter().enumerate() {
                let iris = i >= index::RIGHT_IRIS;
                let (radius, c) = if iris {
                    (1.8, Color32::from_rgb(0, 220, 255))
                } else {
                    (1.1, color)
                };
                painter.circle_filled(to_screen(p[0], p[1]), radius, c);
            }
        }

        if self.overlay.axes
            && let Some(head) = &self.head
        {
            // Trois axes de 6 cm partant du bout du nez, dans le repère du modèle :
            // x (rouge) vers la gauche de la personne, y (vert) vers le haut,
            // z (bleu) vers l'avant. Projetés avec l'ajustement image.
            let nose = tracker_core::canonical_face::CANONICAL_FACE[index::NOSE_TIP];
            let nose = Vector3::new(f64::from(nose[0]), f64::from(nose[1]), f64::from(nose[2]));
            let origin = head.project_model_point(nose);
            let origin = to_screen(origin.0, origin.1);
            for (axis, color) in [
                (Vector3::x(), Color32::from_rgb(240, 70, 70)),
                (Vector3::y(), Color32::from_rgb(70, 220, 90)),
                (Vector3::z(), Color32::from_rgb(70, 140, 255)),
            ] {
                let tip = head.project_model_point(nose + axis * 6.0);
                painter.line_segment([origin, to_screen(tip.0, tip.1)], Stroke::new(3.0, color));
            }
        }
    }
}

impl eframe::App for CameraView {
    /// Logique (sans dessin) : appelée avant chaque `ui`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_pipeline(ctx);

        // Barre d'espace : recentrer (quand aucun champ de texte n'a le focus).
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.recenter();
        }

        // Tant que la caméra tourne, on redessine en continu.
        if self.pipeline.is_some() {
            ctx.request_repaint();
        }
    }

    /// Dessin de l'interface.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Les panneaux défilent : leur contenu dépasse la hauteur d'un écran de portable.
        egui::Panel::left("controls")
            .resizable(false)
            .min_size(240.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.side_panel(ui));
            });
        egui::Panel::right("tracking")
            .resizable(false)
            .min_size(290.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.head_panel(ui);
                    ui.separator();
                    let filtered = self.processed.map(|p| p.filtered.to_array());
                    if self.tuning.ui(ui, filtered) {
                        let settings = Box::new(self.tuning.settings.clone());
                        self.send(PipelineCommand::SetSettings(settings));
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (tab, name) in Tab::ALL {
                    ui.selectable_value(&mut self.tab, tab, name);
                }
                if self.bench.is_recording() {
                    ui.colored_label(Color32::from_rgb(240, 70, 70), "● enregistrement");
                }
            });
            ui.separator();
            match self.tab {
                Tab::Camera => self.camera_image(ui),
                tab => {
                    if let Some((group, params)) = self.bench.ui(ui, tab, &self.tuning.settings) {
                        match group {
                            AxisGroup::Rotation => self.tuning.settings.rotation_filter = params,
                            AxisGroup::Translation => self.tuning.settings.position_filter = params,
                        }
                        self.tuning.settings.filter_enabled = true;
                        let settings = Box::new(self.tuning.settings.clone());
                        self.send(PipelineCommand::SetSettings(settings));
                    }
                }
            }
        });
    }
}

/// Plus grand rectangle de proportions `aspect` (largeur / hauteur) centré dans `outer`.
fn fit_aspect(outer: egui::Rect, aspect: f32) -> egui::Rect {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        16.0 / 9.0
    };
    let size = if outer.width() / outer.height() > aspect {
        egui::vec2(outer.height() * aspect, outer.height()) // fenêtre trop large : bandes à gauche/droite
    } else {
        egui::vec2(outer.width(), outer.width() / aspect) // fenêtre trop haute : bandes en haut/bas
    };
    egui::Rect::from_center_size(outer.center(), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_16_9_dans_une_fenetre_haute() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 900.0));
        let r = fit_aspect(outer, 16.0 / 9.0);
        assert_eq!(r.width(), 800.0);
        assert_eq!(r.height(), 450.0);
        assert_eq!(r.center(), outer.center());
    }

    #[test]
    fn image_16_9_dans_une_fenetre_large() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(2000.0, 450.0));
        let r = fit_aspect(outer, 16.0 / 9.0);
        assert_eq!((r.width(), r.height()), (800.0, 450.0));
    }
}
