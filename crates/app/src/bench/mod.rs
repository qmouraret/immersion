//! Banc de test : mire, scène 3D, courbes temporelles, enregistrements.
//!
//! Tout part de l'historique des [`Sample`] (une entrée par image traitée) :
//! chaque onglet n'est qu'une manière différente de le regarder.

mod plots;
mod recordings;
mod scene;
mod target;

use std::collections::VecDeque;
use std::path::PathBuf;

use eframe::egui;
use tracker_core::autotune::AxisGroup;
use tracker_core::filter::OneEuroParams;
use tracker_core::processing::TrackingSettings;

/// Durée d'historique conservée en mémoire, en secondes.
const HISTORY_S: f64 = 20.0;

/// Ce qui a été calculé pour une image, aux trois étapes de la chaîne.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Secondes depuis le lancement.
    pub t: f64,
    /// Pose recentrée, avant filtre.
    pub raw: [f64; 6],
    /// Après le filtre One-Euro.
    pub filtered: [f64; 6],
    /// Après la courbe : ce qui ira au simulateur.
    pub output: [f64; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Camera,
    Target,
    Scene,
    Plots,
    Recordings,
    Calibration,
}

impl Tab {
    pub const ALL: [(Tab, &'static str); 6] = [
        (Tab::Camera, "Caméra"),
        (Tab::Target, "Mire"),
        (Tab::Scene, "Scène 3D"),
        (Tab::Plots, "Courbes"),
        (Tab::Recordings, "Enregistrements"),
        (Tab::Calibration, "Étalonnage"),
    ];
}

pub struct Bench {
    history: VecDeque<Sample>,
    target: target::TargetGame,
    scene: scene::SceneView,
    plots: plots::Plots,
    recordings: recordings::Recordings,
}

impl Bench {
    pub fn new(recordings_dir: PathBuf) -> Self {
        Self {
            history: VecDeque::new(),
            target: target::TargetGame::default(),
            scene: scene::SceneView::default(),
            plots: plots::Plots::default(),
            recordings: recordings::Recordings::new(recordings_dir),
        }
    }

    pub fn push(&mut self, sample: Sample) {
        while self
            .history
            .front()
            .is_some_and(|s| sample.t - s.t > HISTORY_S)
        {
            self.history.pop_front();
        }
        self.history.push_back(sample);
        self.target.update(&sample);
        self.recordings.push(&sample);
    }

    /// Les valeurs sautent (recentrage…) : on repart d'un historique vide.
    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Points du visage d'une image, pour l'enregistrement en cours.
    pub fn push_landmarks(&mut self, frame: tracker_core::landmark_log::LandmarkFrame) {
        self.recordings.push_landmarks(frame);
    }

    pub fn is_recording(&self) -> bool {
        self.recordings.is_recording()
    }

    /// Dessine l'onglet. Renvoie un réglage de filtre proposé par l'onglet
    /// Enregistrements (avec son groupe d'axes), à appliquer par l'appelant.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        tab: Tab,
        settings: &TrackingSettings,
    ) -> Option<(AxisGroup, OneEuroParams)> {
        let latest = self.history.back().copied();
        match tab {
            Tab::Camera | Tab::Calibration => {} // affichés par la vue principale
            Tab::Target => self.target.ui(ui, latest.as_ref(), settings),
            Tab::Scene => self.scene.ui(ui, latest.as_ref()),
            Tab::Plots => self.plots.ui(ui, &self.history),
            Tab::Recordings => return self.recordings.ui(ui, settings),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t: f64) -> Sample {
        Sample {
            t,
            raw: [0.0; 6],
            filtered: [0.0; 6],
            output: [0.0; 6],
        }
    }

    #[test]
    fn l_historique_ne_garde_que_les_dernieres_secondes() {
        let mut bench = Bench::new(std::env::temp_dir().join("immersion-unused"));
        for i in 0..=60 {
            bench.push(sample(f64::from(i)));
        }
        assert_eq!(bench.history.back().unwrap().t, 60.0);
        assert!(bench.history.front().unwrap().t >= 60.0 - HISTORY_S);
        bench.clear();
        assert!(bench.history.is_empty());
    }
}
