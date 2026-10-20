//! Affiche les entrées / sorties des modèles ONNX.
//! `cargo run -p vision --example inspect_models`

use ort::session::Session;

fn main() -> ort::Result<()> {
    // Configuration globale d'ONNX Runtime, partagée par toutes les sessions.
    let env = ort::init().with_name("immersion").build()?;
    for path in ["models/face_detector.onnx", "models/face_landmarks.onnx"] {
        let session = Session::builder(&env)?.commit_from_file(path)?;
        println!("== {path}");
        for i in session.inputs() {
            println!("  entrée  {} : {:?}", i.name(), i.dtype());
        }
        for o in session.outputs() {
            println!("  sortie  {} : {:?}", o.name(), o.dtype());
        }
    }
    Ok(())
}
