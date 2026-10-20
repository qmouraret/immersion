//! Immersion — head tracking webcam pour MSFS 2024.

mod camera_view;

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
        Box::new(|_cc| Ok(Box::new(camera_view::CameraView::new()))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
