//! Tests d'intégration avec les vrais modèles ONNX.
//!
//! Ils ont besoin de `models/` (`scripts/fetch_models.sh`). S'il manque, chaque
//! test l'indique et s'arrête sans échouer : `cargo test` reste utilisable sur
//! une machine neuve.

use std::path::PathBuf;
use std::time::Instant;

use tracker_core::Roi;
use tracker_core::testing::gray_frame;
use vision::{FaceTracker, TrackMode, VisionError};

fn models_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` = crates/vision, à la compilation : indépendant du dossier courant.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models")
}

/// Le tracker, ou `None` (avec un message) si les modèles ne sont pas là.
fn tracker() -> Option<FaceTracker> {
    match FaceTracker::new(&models_dir()) {
        Ok(t) => Some(t),
        Err(VisionError::MissingModel(path)) => {
            eprintln!("modèle absent ({path}) : test ignoré, lancer scripts/fetch_models.sh");
            None
        }
        Err(e) => panic!("chargement des modèles : {e}"),
    }
}

#[test]
fn modele_absent_donne_une_erreur_claire() {
    let err = FaceTracker::new(&PathBuf::from("/dossier/inexistant"))
        .err()
        .expect("doit échouer");
    assert!(matches!(err, VisionError::MissingModel(_)));
    assert!(err.to_string().contains("fetch_models"));
}

#[test]
fn aucun_visage_dans_une_image_unie() {
    let Some(mut tracker) = tracker() else { return };
    let frame = gray_frame(1280, 720, 0, Instant::now());
    let result = tracker.process(&frame).unwrap();
    assert_eq!(
        result.mode,
        TrackMode::Detected,
        "sans visage connu, le détecteur tourne"
    );
    assert!(result.landmarks.is_none());
    assert!(result.roi.is_none());
}

#[test]
fn presence_faible_sans_visage_et_478_points() {
    let Some(mut tracker) = tracker() else { return };
    let frame = gray_frame(1280, 720, 0, Instant::now());
    let roi = Roi {
        cx: 640.0,
        cy: 360.0,
        size: 400.0,
        rotation: 0.0,
    };
    let (ms, presence) = tracker.landmarks_only(&frame, &roi).unwrap();
    assert!(presence < 0.5, "présence {presence} sur une image grise");
    assert!(ms > 0.0);
}

#[test]
fn le_detecteur_est_relance_tant_que_rien_n_est_trouve() {
    let Some(mut tracker) = tracker() else { return };
    let frame = gray_frame(640, 480, 0, Instant::now());
    for _ in 0..3 {
        assert_eq!(tracker.process(&frame).unwrap().mode, TrackMode::Detected);
    }
}
