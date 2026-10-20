//! Temps d'inférence et sorties sur des images sans visage (gris, bruit).
//! `cargo run -p vision --example bench_models`

use std::path::Path;
use std::time::Instant;

use tracker_core::{Frame, Roi};
use vision::FaceTracker;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tracker = FaceTracker::new(Path::new("models"))?;
    let (w, h) = (1920u32, 1080u32);

    // Bruit pseudo-aléatoire (générateur linéaire congruentiel : pas besoin de crate).
    let mut seed = 12345u32;
    let noise: Vec<u8> = (0..w * h * 3)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 24) as u8
        })
        .collect();

    for (name, rgb) in [
        ("gris", vec![128u8; (w * h * 3) as usize]),
        ("bruit", noise),
    ] {
        let frame = Frame::new(w, h, rgb, 0, Instant::now()).unwrap();
        // Détection : 10 passages (le premier inclut l'initialisation).
        let mut detect = Vec::new();
        for _ in 0..10 {
            tracker.reset();
            let r = tracker.process(&frame)?;
            detect.push(r.detect_ms);
        }
        // Points : on force un recadrage fixe au centre de l'image.
        let roi = Roi {
            cx: 960.0,
            cy: 540.0,
            size: 600.0,
            rotation: 0.0,
        };
        let mut lm_ms = Vec::new();
        let mut presence = 0.0;
        for _ in 0..10 {
            let (ms, p) = tracker.landmarks_only(&frame, &roi)?;
            lm_ms.push(ms);
            presence = p;
        }
        let avg = |v: &[f64]| v[1..].iter().sum::<f64>() / (v.len() - 1) as f64;
        println!(
            "{name:>5} : détection {:.1} ms (1er {:.1}), points {:.1} ms, présence {presence:.3}",
            avg(&detect),
            detect[0],
            avg(&lm_ms),
        );
    }
    Ok(())
}
