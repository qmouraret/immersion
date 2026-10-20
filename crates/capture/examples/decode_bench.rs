//! Mesure le coût du décodage YUYV → RGB de nokhwa, sans caméra.
//! `cargo run -p capture --example decode_bench`

use std::time::Instant;

use nokhwa::Buffer;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{FrameFormat, Resolution};

fn main() {
    for (w, h) in [(1920, 1080), (1280, 720), (640, 480)] {
        // YUYV : 2 octets par pixel.
        let raw = vec![128u8; w * h * 2];
        let buffer = Buffer::new(Resolution::new(w as u32, h as u32), &raw, FrameFormat::YUYV);
        let start = Instant::now();
        let n = 10;
        for _ in 0..n {
            let img = buffer.decode_image::<RgbFormat>().unwrap();
            std::hint::black_box(img);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("{w}x{h} : {ms:.1} ms / image");
    }
}
