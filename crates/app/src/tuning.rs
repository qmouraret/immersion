//! Panneau de réglage : filtre One-Euro et courbes de réponse.

use eframe::egui::{self, Color32, Pos2, Sense, Stroke, vec2};
use tracker_core::filter::OneEuroParams;
use tracker_core::processing::{AXES, TrackingSettings};

pub struct Tuning {
    pub settings: TrackingSettings,
    /// Axe dont la courbe est affichée.
    axis: usize,
}

impl Tuning {
    pub fn new(settings: TrackingSettings) -> Self {
        Self { settings, axis: 0 }
    }

    /// Dessine le panneau. Renvoie `true` si un réglage a changé.
    ///
    /// `filtered` : valeurs filtrées courantes, pour placer un repère sur la courbe.
    pub fn ui(&mut self, ui: &mut egui::Ui, filtered: Option<[f64; 6]>) -> bool {
        let mut changed = false;
        let s = &mut self.settings;

        egui::CollapsingHeader::new("Lissage (One-Euro)").default_open(true).show(ui, |ui| {
            changed |= ui.checkbox(&mut s.filter_enabled, "Activé").changed();
            ui.add_enabled_ui(s.filter_enabled, |ui| {
                ui.label("Rotations");
                changed |= filter_sliders(ui, &mut s.rotation_filter, 1.0);
                ui.label("Translations");
                changed |= filter_sliders(ui, &mut s.position_filter, 10.0);
            });
            ui.small("Coupure min. ↑ si la vue « traîne » à l'arrêt ; β ↑ si elle est en retard en mouvement.");
        });

        egui::CollapsingHeader::new("Courbes de réponse")
            .default_open(true)
            .show(ui, |ui| {
                changed |= ui.checkbox(&mut s.curves_enabled, "Activées").changed();
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in AXES.iter().enumerate() {
                        ui.selectable_value(&mut self.axis, i, *name);
                    }
                });
                let unit = if self.axis < 3 { "°" } else { " cm" };
                let c = &mut s.curves[self.axis];
                let max_in = if self.axis < 3 { 90.0 } else { 40.0 };
                let max_out = if self.axis < 3 { 180.0 } else { 60.0 };
                changed |= ui
                    .add(
                        egui::Slider::new(&mut c.deadzone, 0.0..=5.0)
                            .suffix(unit)
                            .text("Zone morte"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut c.input, 1.0..=max_in)
                            .suffix(unit)
                            .text("Entrée max."),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut c.output, 0.0..=max_out)
                            .suffix(unit)
                            .text("Sortie max."),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut c.expo, 0.5..=3.0).text("Expo"))
                    .changed();
                // L'entrée max. doit rester au-delà de la zone morte.
                c.input = c.input.max(c.deadzone + 0.1);

                let current = filtered.map(|v| v[self.axis]);
                draw_curve(ui, &s.curves[self.axis], current, s.curves_enabled);
            });

        if ui.button("Valeurs par défaut").clicked() {
            self.settings = TrackingSettings::default();
            changed = true;
        }
        changed
    }
}

/// Curseurs d'un filtre ; `beta_scale` adapte la plage aux unités (°/s ou cm/s).
fn filter_sliders(ui: &mut egui::Ui, p: &mut OneEuroParams, beta_scale: f64) -> bool {
    let a = ui.add(
        egui::Slider::new(&mut p.min_cutoff, 0.05..=5.0)
            .logarithmic(true)
            .suffix(" Hz")
            .text("Coupure min."),
    );
    let b = ui.add(
        egui::Slider::new(&mut p.beta, 0.0..=beta_scale)
            .logarithmic(true)
            .smallest_positive(0.001)
            .text("β (réactivité)"),
    );
    a.changed() || b.changed()
}

/// Trace la courbe (partie positive) et la position actuelle de la tête.
fn draw_curve(
    ui: &mut egui::Ui,
    curve: &tracker_core::curve::AxisCurve,
    current: Option<f64>,
    enabled: bool,
) {
    let width = ui.available_width().min(260.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 140.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);

    let max_x = curve.input * 1.25;
    let max_y = curve.output.max(1.0) * 1.05;
    let to_screen = |x: f64, y: f64| {
        Pos2::new(
            rect.left() + (x / max_x) as f32 * rect.width(),
            rect.bottom() - (y / max_y) as f32 * rect.height(),
        )
    };

    // Zone morte grisée.
    let dz = egui::Rect::from_min_max(to_screen(0.0, max_y), to_screen(curve.deadzone, 0.0));
    painter.rect_filled(dz, 0.0, visuals.faint_bg_color);

    let color = if enabled {
        Color32::from_rgb(80, 160, 255)
    } else {
        Color32::GRAY
    };
    let points: Vec<Pos2> = (0..=100)
        .map(|i| {
            let x = max_x * f64::from(i) / 100.0;
            to_screen(x, curve.apply(x))
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, color)));

    if let Some(v) = current {
        let x = v.abs().min(max_x);
        let p = to_screen(x, curve.apply(x));
        painter.circle_filled(p, 4.0, Color32::from_rgb(255, 170, 0));
    }
    painter.text(
        rect.left_top() + vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        format!("sortie max. {:.0}", curve.output),
        egui::FontId::proportional(11.0),
        visuals.weak_text_color(),
    );
}
