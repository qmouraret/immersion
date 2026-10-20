//! Profil ONNX Runtime du modèle de points : temps par opération.
//! `cargo run -p vision --example profile_landmarks` → fichier JSON dans target/
use ort::session::Session;
use ort::value::TensorRef;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env = ort::init().with_name("profil").build()?;
    let builder = Session::builder(&env)?
        .with_profiling("target/profile_landmarks")
        .map_err(ort::Error::<()>::from)?;
    let mut builder = builder
        .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)
        .map_err(ort::Error::<()>::from)?;
    let mut session = builder.commit_from_file("models/face_landmarks.onnx")?;
    let input = vec![0.5f32; 256 * 256 * 3];
    for _ in 0..20 {
        let t = TensorRef::from_array_view(([1usize, 256, 256, 3], input.as_slice()))?;
        session.run(ort::inputs!["input_12" => t])?;
    }
    println!("{}", session.end_profiling()?);
    Ok(())
}
