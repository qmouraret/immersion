//! Phase 0 : sélection de la caméra et affichage du flux.

use std::time::Instant;

use capture::{CameraInfo, CaptureEvent, CaptureHandle, FormatTarget};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

/// Mesures du flux, affichées dans le panneau latéral.
#[derive(Default)]
struct Stats {
    /// Images par seconde reçues, lissées (moyenne mobile exponentielle).
    fps: f64,
    /// Temps entre la capture et l'affichage, en ms.
    latency_ms: f64,
    /// Conversion du format caméra en RGB (thread de capture), en ms.
    decode_ms: f64,
    /// Conversion RGB → RGBA et envoi de la texture au GPU (thread UI), en ms.
    upload_ms: f64,
    /// Images sautées (trous dans les numéros de séquence).
    dropped: u64,
    last_frame_at: Option<Instant>,
    last_seq: Option<u64>,
}

impl Stats {
    fn on_frame(&mut self, seq: u64, latency_ms: f64) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame_at {
            let instant_fps = 1.0 / now.duration_since(prev).as_secs_f64().max(1e-6);
            self.fps = if self.fps == 0.0 {
                instant_fps
            } else {
                0.9 * self.fps + 0.1 * instant_fps
            };
        }
        if let Some(prev) = self.last_seq {
            self.dropped += seq.saturating_sub(prev + 1);
        }
        self.latency_ms = latency_ms;
        self.last_frame_at = Some(now);
        self.last_seq = Some(seq);
    }
}

pub struct CameraView {
    cameras: Vec<CameraInfo>,
    selected: Option<u32>,
    capture: Option<CaptureHandle>,
    texture: Option<TextureHandle>,
    /// Format négocié, déjà mis en forme pour l'affichage.
    format: Option<String>,
    error: Option<String>,
    stats: Stats,
    mirror: bool,
}

impl CameraView {
    pub fn new() -> Self {
        let mut view = Self {
            cameras: Vec::new(),
            selected: None,
            capture: None,
            texture: None,
            format: None,
            error: None,
            stats: Stats::default(),
            mirror: true,
        };
        view.refresh_cameras();
        if let Some(first) = view.cameras.first().map(|c| c.index) {
            view.open(first);
        }
        view
    }

    fn refresh_cameras(&mut self) {
        match capture::list_cameras() {
            Ok(cams) => self.cameras = cams,
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn open(&mut self, index: u32) {
        // Remplacer l'Option détruit l'ancienne poignée → son `Drop` arrête
        // l'ancien thread avant que le nouveau n'ouvre la caméra.
        self.capture = None;
        self.capture = Some(CaptureHandle::start(index, FormatTarget::default()));
        self.selected = Some(index);
        self.format = None;
        self.error = None;
        self.stats = Stats::default();
    }

    /// Traite les événements du thread de capture ; ne garde que la dernière image.
    fn poll_capture(&mut self, ctx: &egui::Context) {
        let Some(capture) = &self.capture else { return };

        let mut latest = None;
        for event in capture.drain() {
            match event {
                CaptureEvent::Started {
                    width,
                    height,
                    fps,
                    pixel_format,
                } => self.format = Some(format!("{width}×{height} @ {fps} fps ({pixel_format})")),
                CaptureEvent::Frame { frame, decode_ms } => latest = Some((frame, decode_ms)),
                CaptureEvent::Error(e) => self.error = Some(e),
            }
        }

        if let Some((frame, decode_ms)) = latest {
            let upload_start = Instant::now();
            let size = [frame.width as usize, frame.height as usize];
            let image = ColorImage::from_rgb(size, &frame.rgb);
            match &mut self.texture {
                // On réutilise la même texture GPU plutôt que d'en créer une par image.
                Some(tex) => tex.set(image, TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture("camera", image, TextureOptions::LINEAR))
                }
            }
            self.stats.upload_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
            self.stats.decode_ms = decode_ms;
            self.stats.on_frame(frame.seq, frame.age_ms());
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Caméra");

        let current = self
            .cameras
            .iter()
            .find(|c| Some(c.index) == self.selected)
            .map_or("aucune", |c| c.name.as_str())
            .to_owned();
        let mut choice = self.selected;
        egui::ComboBox::from_id_salt("camera")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for cam in &self.cameras {
                    ui.selectable_value(&mut choice, Some(cam.index), &cam.name);
                }
            });
        // `if ... && let` (« let chains », Rust 2024) : condition + déstructuration.
        if choice != self.selected
            && let Some(index) = choice
        {
            self.open(index);
        }
        if ui.button("Actualiser la liste").clicked() {
            self.refresh_cameras();
        }
        ui.checkbox(&mut self.mirror, "Effet miroir");

        ui.separator();
        ui.heading("Flux");
        ui.label(format!(
            "Format : {}",
            self.format.as_deref().unwrap_or("ouverture…")
        ));
        ui.label(format!("Reçu : {:.1} img/s", self.stats.fps));
        ui.label(format!(
            "Latence capture → écran : {:.1} ms",
            self.stats.latency_ms
        ));
        ui.label(format!("  dont décodage : {:.1} ms", self.stats.decode_ms));
        ui.label(format!("  dont envoi GPU : {:.1} ms", self.stats.upload_ms));
        ui.label(format!("Images sautées : {}", self.stats.dropped));

        if let Some(e) = &self.error {
            ui.separator();
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    fn camera_image(&self, ui: &mut egui::Ui) {
        let Some(tex) = &self.texture else {
            ui.centered_and_justified(|ui| ui.label("En attente de la caméra…"));
            return;
        };
        // Miroir : on inverse les coordonnées de texture horizontalement.
        let uv = if self.mirror {
            egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
        } else {
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        };
        ui.centered_and_justified(|ui| {
            ui.add(egui::Image::new(tex).uv(uv).shrink_to_fit());
        });
    }
}

impl eframe::App for CameraView {
    /// Logique (sans dessin) : appelée avant chaque `ui`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_capture(ctx);

        // Tant que la caméra tourne, on redessine en continu.
        if self.capture.is_some() {
            ctx.request_repaint();
        }
    }

    /// Dessin de l'interface.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("controls")
            .resizable(false)
            .min_size(240.0)
            .show(ui, |ui| {
                self.side_panel(ui);
            });
        egui::CentralPanel::default().show(ui, |ui| self.camera_image(ui));
    }
}
