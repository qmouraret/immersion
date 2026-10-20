//! Coût de la préparation des entrées (`sample_roi`).
use std::time::Instant;
use tracker_core::tensor::{Normalization, sample_roi};
use tracker_core::{Frame, Roi};

fn main() {
    let frame = Frame::new(1920, 1080, vec![100; 1920 * 1080 * 3], 0, Instant::now()).unwrap();
    let mut dst = Vec::new();
    for (name, roi, out) in [
        ("détecteur 128", Roi::full_image(1920, 1080), 128),
        (
            "points 256 (visage 600 px)",
            Roi {
                cx: 960.0,
                cy: 540.0,
                size: 600.0,
                rotation: 0.3,
            },
            256,
        ),
    ] {
        let start = Instant::now();
        for _ in 0..20 {
            sample_roi(&frame, &roi, out, Normalization::ZeroToOne, &mut dst);
        }
        println!(
            "{name} : {:.2} ms",
            start.elapsed().as_secs_f64() * 1000.0 / 20.0
        );
    }
}
