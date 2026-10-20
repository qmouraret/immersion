//! Diagnostic d'une session avec les réglages par défaut : pour chaque axe,
//! part du temps hors zone morte et tremblement de la sortie.
//!
//! `cargo run -p tracker-core --example analyze_session -- recordings/session-XXX.csv`
use tracker_core::processing::{AXES, TrackingSettings};
use tracker_core::recording::{from_csv, replay};
use tracker_core::stats::{active_still_roughness, still_roughness};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage : analyze_session <fichier.csv>")?;
    let samples = from_csv(&std::fs::read_to_string(&path)?)?;
    let settings = TrackingSettings::default();
    let out = replay(&samples, &settings);
    println!(
        "{:<8} {:>9} {:>9} {:>11} {:>12} {:>12}",
        "axe", "brut ±", "zone m.", "hors z.m.", "rug. filtré", "rug. sortie"
    );
    for (i, name) in AXES.iter().enumerate() {
        let raw: Vec<f64> = samples.iter().map(|s| s.pose[i]).collect();
        let filtered: Vec<f64> = out.iter().map(|p| p.filtered.to_array()[i]).collect();
        let output: Vec<f64> = out.iter().map(|p| p.output.to_array()[i]).collect();
        let active = output.iter().filter(|v| **v != 0.0).count() as f64 / output.len() as f64;
        let unit = if i < 3 { "°" } else { "cm" };
        println!(
            "{name:<8} {:>7.3}{unit} {:>7.2}{unit} {:>10.0} % {:>10.4}{unit} {:>10}",
            still_roughness(&raw, 15).unwrap_or(0.0),
            settings.curves[i].deadzone,
            active * 100.0,
            still_roughness(&filtered, 15).unwrap_or(0.0),
            active_still_roughness(&output, 15).map_or("—".into(), |r| format!("{r:.4}{unit}")),
        );
    }
    Ok(())
}
