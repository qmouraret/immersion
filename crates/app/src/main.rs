//! Immersion — head tracking webcam pour MSFS 2024.

mod bench;
mod calibration;
mod camera_view;
mod output_panel;
mod pipeline;
mod profile;
mod tuning;

use std::path::PathBuf;

use anyhow::Context as _;

fn main() -> anyhow::Result<()> {
    // Niveau de log réglable : RUST_LOG=debug cargo run
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu=warn,eframe=warn".into()),
        )
        .init();

    capture::ensure_permission().context("autorisation caméra")?;

    let models_dir = models_dir();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Immersion")
            .with_inner_size([1100.0, 700.0]),
        ..Default::default()
    };

    // `eframe::Error` ne contient pas que des types `Send + Sync`, on le
    // convertit en texte pour pouvoir l'emballer dans `anyhow::Error`.
    eframe::run_native(
        "Immersion",
        options,
        Box::new(|_cc| Ok(Box::new(camera_view::CameraView::new(models_dir)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Dossier des modèles ONNX (`scripts/fetch_models.sh`) : `IMMERSION_MODELS`,
/// sinon `models/` dans le dossier courant (développement avec `cargo run`),
/// sinon `models/` à côté de l'exécutable (application installée, Windows).
fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("IMMERSION_MODELS") {
        return PathBuf::from(dir);
    }
    let local = PathBuf::from("models");
    if local.exists() {
        return local;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("models")))
        .filter(|dir| dir.exists())
        .unwrap_or(local)
}
