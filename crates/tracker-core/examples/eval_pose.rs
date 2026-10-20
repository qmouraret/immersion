//! Compare des variantes du calcul de pose sur des points enregistrés (`.lmk`).
//!
//! `cargo run --release -p tracker-core --example eval_pose -- recordings/session-XXX.lmk`
//!
//! Pour chaque variante : bruit de la pose **avant filtre** (rugosité à l'arrêt,
//! par axe) et écart moyen avec le calcul actuel (pour repérer un biais).
use tracker_core::CameraModel;
use tracker_core::landmark_log::from_bytes;
use tracker_core::pose::{EstimateOptions, PointSet, RIGID_POINTS, estimate_with};
use tracker_core::processing::AXES;
use tracker_core::stats::{spikes, still_roughness};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage : eval_pose <fichier.lmk>")?;
    let frames = from_bytes(&std::fs::read(&path)?)?;
    let duration = frames.last().map_or(0.0, |f| f.t) - frames.first().map_or(0.0, |f| f.t);
    println!("{path} : {} images, {duration:.1} s\n", frames.len());

    // Sous-ensembles expérimentaux de RIGID_POINTS (indices par zone).
    const FOREHEAD: [usize; 13] = [10, 151, 9, 108, 337, 69, 299, 104, 333, 67, 297, 109, 338];
    const NOSE: [usize; 16] = [
        168, 6, 197, 195, 5, 4, 1, 19, 94, 2, 98, 327, 64, 294, 48, 278,
    ];
    const EYES: [usize; 6] = [33, 133, 263, 362, 130, 359];
    const CHEEKS: [usize; 14] = [
        116, 345, 117, 346, 118, 347, 123, 352, 50, 280, 36, 266, 101, 330,
    ];
    const TEMPLES: [usize; 10] = [234, 454, 127, 356, 162, 389, 21, 251, 93, 323];
    // `concat` à la compilation n'existe pas pour les tableaux : on les assemble
    // une fois dans un `Vec` puis on le « fuit » pour obtenir un `&'static [usize]`.
    let set =
        |parts: &[&[usize]]| -> &'static [usize] { Box::leak(parts.concat().into_boxed_slice()) };
    let no_forehead = set(&[&NOSE, &EYES, &CHEEKS, &TEMPLES]);
    let nose_cheeks_temples = set(&[&NOSE, &CHEEKS, &TEMPLES]);
    let nose_temples = set(&[&NOSE, &TEMPLES]);
    let upper = set(&[&FOREHEAD, &NOSE[..4], &EYES, &TEMPLES]);
    debug_assert_eq!(RIGID_POINTS.len(), 59);
    let custom = |list| EstimateOptions {
        points: PointSet::Custom(list),
        inlier_ratio: 1.0,
        depth_points: None,
    };
    let variants = [
        (
            "actuel (hybride : distance par le haut)",
            EstimateOptions::default(),
        ),
        (
            "468 pts, sans 2e passage",
            EstimateOptions {
                points: PointSet::All,
                inlier_ratio: 1.0,
                depth_points: None,
            },
        ),
        (
            "rigides, sans 2e passage",
            EstimateOptions {
                points: PointSet::Rigid,
                inlier_ratio: 1.0,
                depth_points: None,
            },
        ),
        (
            "rigides, 2e passage 90 %",
            EstimateOptions {
                points: PointSet::Rigid,
                inlier_ratio: 0.9,
                depth_points: None,
            },
        ),
        (
            "rigides, 2e passage 75 %",
            EstimateOptions {
                points: PointSet::Rigid,
                inlier_ratio: 0.75,
                depth_points: None,
            },
        ),
        (
            "avant : 468 pts, distance par les 468",
            EstimateOptions {
                depth_points: None,
                ..EstimateOptions::default()
            },
        ),
        ("rigides sans front", custom(no_forehead)),
        ("nez + pommettes + tempes", custom(nose_cheeks_temples)),
        ("nez + tempes", custom(nose_temples)),
        ("haut : front, arête, yeux, tempes", custom(upper)),
    ];

    let poses = |options: &EstimateOptions| -> Vec<[f64; 6]> {
        frames
            .iter()
            .filter_map(|f| {
                let camera = CameraModel {
                    width: f.width,
                    height: f.height,
                    hfov_deg: f.hfov_deg,
                };
                estimate_with(&f.landmarks, &camera, options).map(|p| p.absolute().to_array())
            })
            .collect()
    };
    let reference = poses(&EstimateOptions::default());

    for (title, metric) in [
        ("rugosité à l'arrêt (médiane)", 0usize),
        ("sauts (99e centile)", 1),
    ] {
        print!("{title:<36}");
        for name in AXES {
            print!("{name:>9}");
        }
        println!();
        for (label, options) in &variants {
            let series = poses(options);
            print!("{label:<36}");
            for axis in 0..6 {
                let v: Vec<f64> = series.iter().map(|p| p[axis]).collect();
                let unit = if axis < 3 { "°" } else { "cm" };
                let value = if metric == 0 {
                    still_roughness(&v, 15)
                } else {
                    spikes(&v, 0.99)
                };
                print!("{:>7.3}{unit}", value.unwrap_or(f64::NAN));
            }
            println!();
        }
        println!();
    }
    // Tête supposée immobile : écart à la pose de repos (médiane des 5 premières
    // secondes). Mesure directe de la sensibilité aux expressions.
    println!("écart à la pose de repos (RMS), si la tête n'a pas bougé :");
    for (label, options) in &variants {
        let series = poses(options);
        let t0 = frames.first().map_or(0.0, |f| f.t);
        let rest: Vec<&[f64; 6]> = series
            .iter()
            .zip(&frames)
            .filter(|(_, f)| f.t - t0 < 5.0)
            .map(|(p, _)| p)
            .collect();
        let median = |axis: usize| {
            let mut v: Vec<f64> = rest.iter().map(|p| p[axis]).collect();
            v.sort_by(f64::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(0.0)
        };
        print!("{label:<36}");
        for axis in [0, 1, 2, 5] {
            let m = median(axis);
            let rms = (series.iter().map(|p| (p[axis] - m).powi(2)).sum::<f64>()
                / series.len().max(1) as f64)
                .sqrt();
            let unit = if axis < 3 { "°" } else { "cm" };
            print!("  {} {rms:.2}{unit}", AXES[axis]);
        }
        println!();
    }
    println!();
    for (label, options) in &variants[1..] {
        let series = poses(options);
        let bias = |axis: usize| {
            series
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a[axis] - b[axis]).abs())
                .sum::<f64>()
                / series.len().max(1) as f64
        };
        println!(
            "écart moyen avec l'actuel, {label:<34} lacet {:.2}°  tangage {:.2}°",
            bias(0),
            bias(1)
        );
    }
    Ok(())
}
