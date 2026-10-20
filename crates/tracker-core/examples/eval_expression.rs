//! Évalue la correction des expressions sur une session de grimaces (tête immobile).
//!
//! `cargo run --release -p tracker-core --example eval_expression -- recordings/session-XXX.lmk`
use tracker_core::expression::{FEATURE_NAMES, features, fit};
use tracker_core::landmark_log::from_bytes;
use tracker_core::processing::AXES;
use tracker_core::{CameraModel, pose};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage : eval_expression <fichier.lmk>")?;
    let frames = from_bytes(&std::fs::read(&path)?)?;
    let mut neutral = None;
    let samples: Vec<([f64; 3], [f64; 6])> = frames
        .iter()
        .filter_map(|f| {
            let camera = CameraModel {
                width: f.width,
                height: f.height,
                hfov_deg: f.hfov_deg,
            };
            let head = pose::estimate(&f.landmarks, &camera)?;
            let n = *neutral.get_or_insert(head);
            Some((features(&f.landmarks)?, head.relative_to(&n).to_array()))
        })
        .collect();
    let neutral_count = 150; // 5 s au repos
    let report = fit(&samples, neutral_count).ok_or("pas assez d'images")?;

    println!(
        "Apprentissage sur toute la session ({} images) :",
        samples.len()
    );
    for (axis, name) in AXES.iter().enumerate() {
        let c = report.model.coeffs[axis];
        let used = c != [0.0; 3];
        println!(
            "  {:<8} écart {:6.3} → {:6.3}  {}",
            name,
            report.before[axis],
            report.after[axis],
            if used {
                format!(
                    "({} {:+.0}, {} {:+.0}, {} {:+.0}, borne {:.2})",
                    FEATURE_NAMES[0],
                    c[0],
                    FEATURE_NAMES[1],
                    c[1],
                    FEATURE_NAMES[2],
                    c[2],
                    report.model.max_correction[axis]
                )
            } else {
                "(non corrigé)".into()
            }
        );
    }

    // Validation : on apprend sur les images paires et on mesure sur les impaires.
    let train: Vec<_> = samples.iter().step_by(2).copied().collect();
    let test: Vec<_> = samples.iter().skip(1).step_by(2).copied().collect();
    let model = fit(&train, neutral_count / 2)
        .ok_or("apprentissage impossible")?
        .model;
    println!("\nValidation (apprises sur les images paires, mesurées sur les impaires) :");
    for axis in 0..6 {
        let y: Vec<f64> = test.iter().map(|(_, p)| p[axis]).collect();
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let before = (y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / y.len() as f64).sqrt();
        let corrected: Vec<f64> = test
            .iter()
            .map(|(f, p)| model.correct(*p, f)[axis])
            .collect();
        let cm = corrected.iter().sum::<f64>() / corrected.len() as f64;
        let after = (corrected.iter().map(|v| (v - cm).powi(2)).sum::<f64>()
            / corrected.len() as f64)
            .sqrt();
        println!("  {:<8} écart {before:6.3} → {after:6.3}", AXES[axis]);
    }
    Ok(())
}
