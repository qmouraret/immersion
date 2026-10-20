//! Enregistrer une session, puis la rejouer avec les réglages actuels.
//!
//! On enregistre la pose *brute* (recentrée, avant filtre) : en la rejouant,
//! on peut comparer n'importe quels réglages sur exactement les mêmes mouvements.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use tracker_core::autotune::{self, AxisGroup};
use tracker_core::filter::OneEuroParams;
use tracker_core::landmark_log::{self, LandmarkFrame};
use tracker_core::processing::{AXES, TrackingSettings};
use tracker_core::recording::{self, RawSample};

use super::Sample;
use super::plots::{metrics_ui, series_plot};

pub struct Recordings {
    dir: PathBuf,
    recording: Option<Vec<RawSample>>,
    /// Points du visage de la session en cours (fichier `.lmk` à côté du CSV).
    landmarks: Vec<LandmarkFrame>,
    files: Vec<PathBuf>,
    loaded: Option<(String, Vec<RawSample>)>,
    axis: usize,
    message: Option<String>,
    /// Retard maximal accepté par la recherche automatique, en ms.
    max_lag_ms: f64,
}

impl Recordings {
    pub fn new(dir: PathBuf) -> Self {
        let mut r = Self {
            dir,
            recording: None,
            landmarks: Vec::new(),
            files: Vec::new(),
            loaded: None,
            axis: 0,
            message: None,
            max_lag_ms: 60.0,
        };
        r.refresh();
        r
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Ajoute les points d'une image à la session en cours (ignoré hors enregistrement).
    pub fn push_landmarks(&mut self, frame: LandmarkFrame) {
        if self.recording.is_some() {
            self.landmarks.push(frame);
        }
    }

    pub fn push(&mut self, s: &Sample) {
        if let Some(rec) = &mut self.recording {
            rec.push(RawSample {
                t: s.t,
                pose: s.raw,
            });
        }
    }

    fn refresh(&mut self) {
        // Dossier absent = aucun enregistrement : ce n'est pas une erreur.
        self.files = fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "csv"))
                    .collect()
            })
            .unwrap_or_default();
        self.files.sort();
        self.files.reverse(); // plus récent en premier (noms horodatés)
    }

    fn save(&mut self, samples: Vec<RawSample>) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let path = self.dir.join(format!("session-{stamp}.csv"));
        let landmarks = std::mem::take(&mut self.landmarks);
        // Points du visage : même nom, extension `.lmk` (pour `examples/eval_pose`).
        let result = fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(&path, recording::to_csv(&samples)))
            .and_then(|()| {
                fs::write(
                    path.with_extension("lmk"),
                    landmark_log::to_bytes(&landmarks),
                )
            });
        self.message = Some(match result {
            Ok(()) => format!(
                "{} images (et {} jeux de points) enregistrées dans {}",
                samples.len(),
                landmarks.len(),
                path.display()
            ),
            Err(e) => format!("échec de l'écriture : {e}"),
        });
        self.refresh();
        self.load(&path);
    }

    fn load(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        // `and_then` + `map_err` : on enchaîne deux opérations faillibles dont les
        // erreurs sont de types différents (E/S et format), converties en texte.
        let result = fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| recording::from_csv(&text).map_err(|e| e.to_string()));
        match result {
            Ok(samples) => self.loaded = Some((name, samples)),
            Err(e) => self.message = Some(format!("{name} : {e}")),
        }
    }

    /// Renvoie un réglage de filtre (et son groupe d'axes) si l'utilisateur a
    /// demandé une proposition automatique ; à appliquer par l'appelant.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        settings: &TrackingSettings,
    ) -> Option<(AxisGroup, OneEuroParams)> {
        ui.horizontal(|ui| {
            match self.recording.take() {
                Some(samples) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(240, 70, 70),
                        format!("● {} images", samples.len()),
                    );
                    if ui.button("Arrêter et enregistrer").clicked() {
                        self.save(samples);
                    } else {
                        self.recording = Some(samples); // on remet ce qu'on avait pris
                    }
                }
                None => {
                    if ui.button("● Enregistrer").clicked() {
                        self.recording = Some(Vec::new());
                        self.landmarks.clear();
                        self.message = None;
                    }
                }
            }
            ui.separator();
            let current = self
                .loaded
                .as_ref()
                .map_or("(aucun)", |(n, _)| n.as_str())
                .to_owned();
            let mut choice = None;
            egui::ComboBox::from_id_salt("recording")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for f in &self.files {
                        let name = f
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                        if ui.selectable_label(false, name).clicked() {
                            choice = Some(f.clone());
                        }
                    }
                });
            if let Some(path) = choice {
                self.load(&path);
            }
            if ui.button("Actualiser").clicked() {
                self.refresh();
            }
        });
        if let Some(m) = &self.message {
            ui.small(m);
        }
        ui.small(format!("Dossier : {}", self.dir.display()));

        let Some((_, raw)) = &self.loaded else {
            ui.label("Enregistrer quelques secondes de mouvements (immobile, puis rotations vives), puis les rejouer ici.");
            return None;
        };

        let mut proposal = None;
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut self.max_lag_ms, 20.0..=150.0).suffix(" ms").text("Retard max."));
            for (group, label) in [
                (AxisGroup::Rotation, "Proposer : rotations"),
                (AxisGroup::Translation, "Proposer : translations"),
            ] {
                if ui.button(label).clicked() {
                    let best = autotune::search(raw, settings, group, self.max_lag_ms / 1000.0).into_iter().next();
                    self.message = Some(match best {
                        Some(c) => {
                            proposal = Some((group, c.params));
                            let lag = c.lag_s.map_or("non mesurable (pas assez de mouvement)".into(), |l| {
                                format!("{:.0} ms", l * 1000.0)
                            });
                            format!(
                                "{group:?} : coupure {:.3} Hz, β {:.4}, coupure vitesse {:.2} Hz → rugosité de sortie {:.3}, retard {lag}",
                                c.params.min_cutoff, c.params.beta, c.params.d_cutoff, c.roughness
                            )
                        }
                        None => "Aucun réglage ne tient ce retard : l'augmenter.".into(),
                    });
                }
            }
        });
        ui.horizontal(|ui| {
            for (i, name) in AXES.iter().enumerate() {
                ui.selectable_value(&mut self.axis, i, *name);
            }
        });
        ui.small("Rejoué avec les réglages actuels : modifier le panneau de droite met à jour les courbes.");

        // Rejeu complet à chaque image : ~1 000 échantillons × 6 axes, négligeable.
        let processed = recording::replay(raw, settings);
        let samples: Vec<Sample> = raw
            .iter()
            .zip(&processed)
            .map(|(r, p)| Sample {
                t: r.t,
                raw: r.pose,
                filtered: p.filtered.to_array(),
                output: p.output.to_array(),
            })
            .collect();
        metrics_ui(ui, &samples, self.axis);
        let t0 = samples.first().map_or(0.0, |s| s.t);
        let height = (ui.available_height() - 20.0) / 2.0;
        series_plot(ui, "replay_in", &samples, self.axis, t0, height, false);
        series_plot(ui, "replay_out", &samples, self.axis, t0, height, true);
        proposal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire propre à ce test, supprimé à la fin (même en cas d'échec,
    /// grâce à `Drop`).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("immersion-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample(t: f64) -> Sample {
        Sample {
            t,
            raw: [t; 6],
            filtered: [0.0; 6],
            output: [0.0; 6],
        }
    }

    #[test]
    fn enregistre_puis_recharge() {
        let tmp = TempDir::new("rec");
        let mut r = Recordings::new(tmp.0.clone());
        assert!(
            r.files.is_empty(),
            "dossier absent : pas d'erreur, liste vide"
        );

        r.recording = Some(Vec::new());
        for i in 0..5 {
            r.push(&sample(f64::from(i) / 30.0));
        }
        let samples = r.recording.take().unwrap();
        r.save(samples);

        assert_eq!(r.files.len(), 1);
        let (_, loaded) = r.loaded.as_ref().expect("rechargé après l'enregistrement");
        assert_eq!(loaded.len(), 5);
        assert_eq!(
            loaded[4].pose,
            [4.0 / 30.0; 6].map(|v: f64| (v * 1e4).round() / 1e4)
        );
    }

    #[test]
    fn rien_n_est_enregistre_hors_enregistrement() {
        let tmp = TempDir::new("idle");
        let mut r = Recordings::new(tmp.0.clone());
        r.push(&sample(0.0));
        assert!(!r.is_recording());
    }

    #[test]
    fn fichier_invalide_signale_sans_planter() {
        let tmp = TempDir::new("bad");
        fs::create_dir_all(&tmp.0).unwrap();
        let path = tmp.0.join("bad.csv");
        fs::write(&path, "pas un enregistrement").unwrap();
        let mut r = Recordings::new(tmp.0.clone());
        r.load(&path);
        assert!(r.loaded.is_none());
        assert!(r.message.as_deref().unwrap_or("").contains("en-tête"));
    }
}
