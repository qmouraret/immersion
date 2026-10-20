//! Vue caméra : flux vidéo, points du visage superposés, mesures.

use std::path::PathBuf;
use std::time::Instant;

use capture::CameraInfo;
use eframe::egui::{self, Color32, ColorImage, Pos2, Stroke, TextureHandle, TextureOptions};
use tracker_core::face::index;
use vision::{FaceResult, TrackMode};

use crate::pipeline::{Pipeline, PipelineEvent};

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
    /// Nombre de fois où le détecteur a dû être relancé (visage perdu).
    detections: u64,
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

/// Options d'affichage.
struct Overlay {
    mirror: bool,
    points: bool,
    roi: bool,
}

pub struct CameraView {
    cameras: Vec<CameraInfo>,
    selected: Option<u32>,
    models_dir: PathBuf,
    pipeline: Option<Pipeline>,
    texture: Option<TextureHandle>,
    /// Taille de l'image affichée, pour placer les points.
    frame_size: (f32, f32),
    face: Option<FaceResult>,
    /// Format négocié, déjà mis en forme pour l'affichage.
    format: Option<String>,
    error: Option<String>,
    stats: Stats,
    overlay: Overlay,
}

impl CameraView {
    pub fn new(models_dir: PathBuf) -> Self {
        let mut view = Self {
            cameras: Vec::new(),
            selected: None,
            models_dir,
            pipeline: None,
            texture: None,
            frame_size: (1.0, 1.0),
            face: None,
            format: None,
            error: None,
            stats: Stats::default(),
            overlay: Overlay {
                mirror: true,
                points: true,
                roi: true,
            },
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
        // Remplacer l'Option détruit l'ancien pipeline → son `Drop` arrête
        // l'ancien thread (et donc la caméra) avant d'ouvrir la nouvelle.
        self.pipeline = None;
        self.pipeline = Some(Pipeline::start(index, self.models_dir.clone()));
        self.selected = Some(index);
        self.format = None;
        self.error = None;
        self.face = None;
        self.stats = Stats::default();
    }

    /// Traite les événements du pipeline ; ne garde que la dernière image.
    fn poll_pipeline(&mut self, ctx: &egui::Context) {
        let Some(pipeline) = &self.pipeline else {
            return;
        };

        let mut latest = None;
        let mut stopped = false;
        for event in pipeline.drain() {
            match event {
                PipelineEvent::Started(format) => self.format = Some(format),
                PipelineEvent::Frame {
                    frame,
                    decode_ms,
                    face,
                } => latest = Some((frame, decode_ms, face)),
                PipelineEvent::Error { message, fatal } => {
                    self.error = Some(message);
                    stopped |= fatal;
                }
            }
        }

        if stopped {
            // Le thread s'est arrêté : on libère le pipeline (et on cesse de
            // redessiner en continu). Choisir une caméra le relance.
            self.pipeline = None;
        }

        if let Some((frame, decode_ms, face)) = latest {
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
            if face.as_ref().is_some_and(|f| f.mode == TrackMode::Detected) {
                self.stats.detections += 1;
            }
            self.frame_size = (frame.width as f32, frame.height as f32);
            self.face = face;
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

        ui.separator();
        ui.heading("Affichage");
        ui.checkbox(&mut self.overlay.mirror, "Effet miroir");
        ui.checkbox(&mut self.overlay.points, "Points du visage");
        ui.checkbox(&mut self.overlay.roi, "Zone de recadrage");

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

        ui.separator();
        ui.heading("Visage");
        match &self.face {
            None => {
                ui.label("Modèles non chargés");
            }
            Some(face) => {
                let presence = face.landmarks.as_ref().map_or(0.0, |l| l.presence);
                ui.label(format!("Confiance : {:.0} %", presence * 100.0));
                ui.add(egui::ProgressBar::new(presence).desired_height(6.0));
                let mode = match (face.mode, face.landmarks.is_some()) {
                    (_, false) => "aucun visage",
                    (TrackMode::Detected, true) => "détection",
                    (TrackMode::Tracked, true) => "suivi",
                };
                ui.label(format!("Mode : {mode}"));
                ui.label(format!("Détecteur : {:.1} ms", face.detect_ms));
                ui.label(format!("Points : {:.1} ms", face.landmarks_ms));
                ui.label(format!("Redétections : {}", self.stats.detections));
            }
        }

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
        let uv = if self.overlay.mirror {
            egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
        } else {
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        };
        let response = ui
            .centered_and_justified(|ui| ui.add(egui::Image::new(tex).uv(uv).shrink_to_fit()))
            .inner;

        if let Some(face) = &self.face {
            self.draw_face(ui.painter_at(response.rect), response.rect, face);
        }
    }

    /// Dessine les points et la zone de recadrage par-dessus l'image.
    fn draw_face(&self, painter: egui::Painter, rect: egui::Rect, face: &FaceResult) {
        let (w, h) = self.frame_size;
        let mirror = self.overlay.mirror;
        // Pixels de l'image source → position à l'écran.
        let to_screen = |x: f32, y: f32| -> Pos2 {
            let u = if mirror { 1.0 - x / w } else { x / w };
            rect.min + egui::vec2(u * rect.width(), y / h * rect.height())
        };

        if self.overlay.roi
            && let Some(roi) = &face.roi
        {
            let color = match face.mode {
                TrackMode::Detected => Color32::from_rgb(255, 170, 0),
                TrackMode::Tracked => Color32::from_rgb(80, 160, 255),
            };
            let corners = roi.corners().map(|(x, y)| to_screen(x, y)).to_vec();
            painter.add(egui::Shape::closed_line(corners, Stroke::new(1.5, color)));
        }

        if self.overlay.points
            && let Some(lm) = &face.landmarks
        {
            // Vert si confiant, orange sinon : utile pour repérer les reflets des lunettes.
            let color = if lm.presence > 0.8 {
                Color32::from_rgb(60, 220, 120)
            } else {
                Color32::from_rgb(255, 150, 60)
            };
            for (i, p) in lm.points.iter().enumerate() {
                let iris = i >= index::RIGHT_IRIS;
                let (radius, c) = if iris {
                    (1.8, Color32::from_rgb(0, 220, 255))
                } else {
                    (1.1, color)
                };
                painter.circle_filled(to_screen(p[0], p[1]), radius, c);
            }
        }
    }
}

impl eframe::App for CameraView {
    /// Logique (sans dessin) : appelée avant chaque `ui`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_pipeline(ctx);

        // Tant que la caméra tourne, on redessine en continu.
        if self.pipeline.is_some() {
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
