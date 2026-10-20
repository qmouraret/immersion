//! Diagnostic d'une session avec les réglages par défaut : pour chaque axe,
//! part du temps hors zone morte et tremblement de la sortie.
//!
//! `cargo run -p tracker-core --example analyze_session -- recordings/session-XXX.csv`
//! `cargo run -p tracker-core --example analyze_session -- recordings/session-XXX.lmk`
//!
//! Avec un `.lmk`, la pose est **recalculée** à partir des points avec le calcul
//! actuel (et recentrée sur la première image) : on voit l'effet d'un changement
//! du calcul de pose, pas seulement des filtres et des courbes.
use tracker_core::landmark_log;
use tracker_core::processing::{AXES, TrackingSettings};
use tracker_core::recording::RawSample;
use tracker_core::recording::{from_csv, replay};
use tracker_core::stats::{active_still_roughness, still_roughness};
use tracker_core::{CameraModel, pose};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage : analyze_session <fichier.csv>")?;
    let samples = if path.ends_with(".lmk") {
        from_landmarks(&std::fs::read(&path)?)?
    } else {
        from_csv(&std::fs::read_to_string(&path)?)?
    };
    let settings = TrackingSettings::default();
    let out = replay(&samples, &settings);
    println!(
        "{:<8} {:>9} {:>9} {:>11} {:>12} {:>12} {:>14}",
        "axe", "brut ±", "zone m.", "hors z.m.", "rug. filtré", "rug. sortie", "repos sortie*"
    );
    for (i, name) in AXES.iter().enumerate() {
        let raw: Vec<f64> = samples.iter().map(|s| s.pose[i]).collect();
        let filtered: Vec<f64> = out.iter().map(|p| p.filtered.to_array()[i]).collect();
        let output: Vec<f64> = out.iter().map(|p| p.output.to_array()[i]).collect();
        let active = output.iter().filter(|v| **v != 0.0).count() as f64 / output.len() as f64;
        let unit = if i < 3 { "°" } else { "cm" };
        // Écart de la sortie à sa médiane des 5 premières secondes : n'a de sens
        // que si la tête n'a pas bougé (session de grimaces).
        let t0 = samples.first().map_or(0.0, |s| s.t);
        let mut rest: Vec<f64> = samples
            .iter()
            .zip(&output)
            .filter(|(s, _)| s.t - t0 < 5.0)
            .map(|(_, v)| *v)
            .collect();
        rest.sort_by(f64::total_cmp);
        let median = rest.get(rest.len() / 2).copied().unwrap_or(0.0);
        let rest_rms =
            (output.iter().map(|v| (v - median).powi(2)).sum::<f64>() / output.len() as f64).sqrt();
        println!(
            "{name:<8} {:>7.3}{unit} {:>7.2}{unit} {:>10.0} % {:>10.4}{unit} {:>10} {:>11.2}{unit}",
            still_roughness(&raw, 15).unwrap_or(0.0),
            settings.curves[i].deadzone,
            active * 100.0,
            still_roughness(&filtered, 15).unwrap_or(0.0),
            active_still_roughness(&output, 15).map_or("—".into(), |r| format!("{r:.4}{unit}")),
            rest_rms,
        );
    }
    println!(
        "\n* écart de la sortie au repos (5 premières s) : seulement pour une session tête immobile."
    );
    Ok(())
}

/// Pose recalculée avec le calcul actuel, relative à la première image.
fn from_landmarks(bytes: &[u8]) -> Result<Vec<RawSample>, Box<dyn std::error::Error>> {
    let frames = landmark_log::from_bytes(bytes)?;
    let mut neutral = None;
    Ok(frames
        .iter()
        .filter_map(|f| {
            let camera = CameraModel {
                width: f.width,
                height: f.height,
                hfov_deg: f.hfov_deg,
            };
            let head = pose::estimate(&f.landmarks, &camera)?;
            let neutral = *neutral.get_or_insert(head);
            Some(RawSample {
                t: f.t,
                pose: head.relative_to(&neutral).to_array(),
            })
        })
        .collect())
}
