//! Cherche le meilleur réglage d'un filtre sur un enregistrement.
//!
//! `cargo run -p tracker-core --example tune_filter -- <fichier.csv> [rot|pos] [retard_max_ms]`

use tracker_core::autotune::{AxisGroup, Candidate, evaluate, search};
use tracker_core::processing::TrackingSettings;
use tracker_core::recording::from_csv;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage : tune_filter <fichier.csv> [rot|pos] [retard_max_ms]")?;
    let group = match args.next().as_deref() {
        None | Some("rot") => AxisGroup::Rotation,
        Some("pos") => AxisGroup::Translation,
        Some(other) => return Err(format!("groupe inconnu « {other} » (rot ou pos)").into()),
    };
    let max_lag_ms: f64 = args.next().map_or(Ok(60.0), |v| v.parse())?;

    let samples = from_csv(&std::fs::read_to_string(&path)?)?;
    let duration = samples.last().map_or(0.0, |s| s.t) - samples.first().map_or(0.0, |s| s.t);
    println!(
        "{path} : {} images, {duration:.1} s, {group:?}, retard max. {max_lag_ms} ms\n",
        samples.len()
    );

    let base = TrackingSettings::default();
    let unit = if group == AxisGroup::Rotation {
        "°"
    } else {
        " cm"
    };
    let show = |label: &str, c: &Candidate| {
        let lag = c
            .lag_s
            .map_or("  —  ".to_string(), |l| format!("{:4.0} ms", l * 1000.0));
        println!(
            "{label:<8} coupure {:5.3} Hz  β {:6.4}  d_cutoff {:4.2} Hz   rugosité sortie {:6.3}{unit}   retard {lag}",
            c.params.min_cutoff, c.params.beta, c.params.d_cutoff, c.roughness
        );
    };
    show(
        "actuel",
        &evaluate(&samples, &base, group, group.filter(&base)),
    );
    let results = search(&samples, &base, group, max_lag_ms / 1000.0);
    if results.first().is_some_and(|c| !c.measured_on_output) {
        println!("\n⚠ jamais hors zone morte : rugosité mesurée avant les courbes.");
    }
    if results.first().is_some_and(|c| c.lag_s.is_none()) {
        println!("\n⚠ aucun mouvement suffisant : retard non mesurable, résultats trop lissés.");
    }
    println!();
    for (i, c) in results.iter().take(8).enumerate() {
        show(&format!("#{}", i + 1), c);
    }
    Ok(())
}
