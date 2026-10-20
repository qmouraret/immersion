//! Courbes temporelles : brut / filtré / sortie, avec rugosité et retard.

use std::collections::VecDeque;

use eframe::egui::{self, Color32};
use egui_plot::{Legend, Line, Plot, PlotPoints};
use tracker_core::processing::AXES;
use tracker_core::stats;

use super::Sample;

/// Fenêtre de la rugosité « à l'arrêt » : 15 images ≈ 0,5 s.
const STILL_WINDOW: usize = 15;

pub const RAW_COLOR: Color32 = Color32::from_rgb(150, 150, 150);
pub const FILTERED_COLOR: Color32 = Color32::from_rgb(80, 160, 255);
pub const OUTPUT_COLOR: Color32 = Color32::from_rgb(255, 170, 0);

pub struct Plots {
    axis: usize,
    window_s: f64,
}

impl Default for Plots {
    fn default() -> Self {
        Self {
            axis: 0,
            window_s: 10.0,
        }
    }
}

impl Plots {
    pub fn ui(&mut self, ui: &mut egui::Ui, history: &VecDeque<Sample>) {
        ui.horizontal(|ui| {
            for (i, name) in AXES.iter().enumerate() {
                ui.selectable_value(&mut self.axis, i, *name);
            }
            ui.separator();
            ui.add(
                egui::Slider::new(&mut self.window_s, 2.0..=20.0)
                    .suffix(" s")
                    .text("Fenêtre"),
            );
        });

        let now = history.back().map_or(0.0, |s| s.t);
        let visible: Vec<&Sample> = history
            .iter()
            .filter(|s| now - s.t <= self.window_s)
            .collect();
        let samples: Vec<Sample> = visible.iter().map(|s| **s).collect();
        metrics_ui(ui, &samples, self.axis);

        let height = (ui.available_height() - 20.0) / 2.0;
        series_plot(ui, "live_in", &samples, self.axis, now, height, false);
        series_plot(ui, "live_out", &samples, self.axis, now, height, true);
    }
}

/// Rugosité et retard sur les échantillons donnés, pour un axe.
pub fn metrics_ui(ui: &mut egui::Ui, samples: &[Sample], axis: usize) {
    let raw: Vec<f64> = samples.iter().map(|s| s.raw[axis]).collect();
    let filtered: Vec<f64> = samples.iter().map(|s| s.filtered[axis]).collect();
    let unit = if axis < 3 { "°" } else { " cm" };
    let dt = mean_dt(samples);

    ui.horizontal_wrapped(|ui| {
        let r = stats::still_roughness(&raw, STILL_WINDOW).unwrap_or(0.0);
        let f = stats::still_roughness(&filtered, STILL_WINDOW).unwrap_or(0.0);
        ui.label(format!("Rugosité à l'arrêt — brute : {r:.3}{unit}"));
        ui.label(format!("filtrée : {f:.4}{unit}"));
        if f > 0.0 {
            ui.label(format!("(÷{:.0})", r / f));
        }
        // Ce que l'on voit vraiment : après la courbe, qui amplifie aussi la gigue.
        let output: Vec<f64> = samples.iter().map(|s| s.output[axis]).collect();
        match stats::active_still_roughness(&output, STILL_WINDOW) {
            Some(o) => ui.strong(format!("sortie (hors zone morte) : {o:.3}{unit}")),
            None => ui.label("sortie : dans la zone morte"),
        };
        ui.separator();
        // Jusqu'à 300 ms de retard cherché ; il faut au moins 5° (ou 5 cm) d'amplitude.
        let max_shift = dt.map_or(9, |dt| (0.3 / dt).round() as usize);
        match (stats::estimate_lag(&raw, &filtered, max_shift, 5.0), dt) {
            (Some(k), Some(dt)) => {
                ui.label(format!("Retard du filtre : {:.0} ms", k * dt * 1000.0))
            }
            _ => ui.label("Retard : bouger la tête (≥ 5) pour le mesurer"),
        };
    });
    ui.small("Rugosité à l'arrêt : tremblement image à image (médiane sur des fenêtres de 0,5 s, les mouvements sont écartés).");
}

fn mean_dt(samples: &[Sample]) -> Option<f64> {
    let n = samples.len();
    (n >= 2)
        .then(|| (samples[n - 1].t - samples[0].t) / (n - 1) as f64)
        .filter(|dt| *dt > 0.0)
}

/// Trace brut + filtré (`output = false`) ou la sortie (`output = true`).
pub fn series_plot(
    ui: &mut egui::Ui,
    id: &str,
    samples: &[Sample],
    axis: usize,
    t_ref: f64,
    height: f32,
    output: bool,
) {
    let points = |f: &dyn Fn(&Sample) -> f64| -> PlotPoints<'static> {
        samples
            .iter()
            .map(|s| [s.t - t_ref, f(s)])
            .collect::<Vec<_>>()
            .into()
    };
    Plot::new(id)
        .height(height.max(80.0))
        .legend(Legend::default())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .x_axis_label("s")
        .show(ui, |plot| {
            if output {
                plot.line(Line::new("sortie", points(&|s| s.output[axis])).color(OUTPUT_COLOR));
            } else {
                plot.line(Line::new("brut", points(&|s| s.raw[axis])).color(RAW_COLOR));
                plot.line(
                    Line::new("filtré", points(&|s| s.filtered[axis]))
                        .color(FILTERED_COLOR)
                        .width(2.0),
                );
            }
        });
}
