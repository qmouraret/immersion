//! Compare quelques réglages d'un filtre sur un enregistrement.
//! `cargo run -p tracker-core --example compare_filters -- <fichier.csv> rot|pos cutoff:beta[:d_cutoff] …`
use tracker_core::autotune::{AxisGroup, evaluate};
use tracker_core::filter::OneEuroParams;
use tracker_core::processing::TrackingSettings;
use tracker_core::recording::from_csv;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage : compare_filters <fichier.csv> rot|pos cutoff:beta[:d_cutoff]…";
    let path = args.next().ok_or(usage)?;
    let group = match args.next().as_deref() {
        Some("rot") => AxisGroup::Rotation,
        Some("pos") => AxisGroup::Translation,
        _ => return Err(usage.into()),
    };
    let samples = from_csv(&std::fs::read_to_string(&path)?)?;
    let base = TrackingSettings::default();
    for spec in args {
        let parts: Vec<&str> = spec.split(':').collect();
        let [c, b, d] = match parts[..] {
            [c, b] => [c, b, "1.0"],
            [c, b, d] => [c, b, d],
            _ => return Err("format attendu : cutoff:beta[:d_cutoff]".into()),
        };
        let params = OneEuroParams {
            min_cutoff: c.parse()?,
            beta: b.parse()?,
            d_cutoff: d.parse()?,
        };
        let r = evaluate(&samples, &base, group, params);
        let lag = r
            .lag_s
            .map_or("—".into(), |l| format!("{:.0} ms", l * 1000.0));
        println!(
            "coupure {c:>5} Hz  β {b:>6}  d_cutoff {d:>4} Hz   rugosité sortie {:.3}   retard {lag}",
            r.roughness
        );
    }
    Ok(())
}
