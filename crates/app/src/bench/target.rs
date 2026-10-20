//! Mire : un réticule piloté par la sortie, et des cibles à atteindre.
//!
//! Mesure ce qui compte en vol : combien de temps pour poser le regard sur
//! un point (un instrument, un avion) et l'y *maintenir* stable.

use eframe::egui::{self, Color32, Sense, Stroke, vec2};
use tracker_core::processing::TrackingSettings;

use super::Sample;

/// Étendue représentée par la mire, en degrés de sortie.
const SPAN_YAW: f64 = 90.0;
const SPAN_PITCH: f64 = 50.0;
/// Rayon d'une cible, en degrés de sortie.
const TARGET_RADIUS: f64 = 5.0;
/// Temps à rester sur la cible pour la valider.
const HOLD_S: f64 = 0.5;

#[derive(Default)]
pub struct TargetGame {
    running: bool,
    target: Option<(f64, f64)>,
    appeared_at: f64,
    on_target_since: Option<f64>,
    /// Temps d'acquisition de chaque cible validée.
    times: Vec<f64>,
    rng: u64,
}

impl TargetGame {
    /// Générateur xorshift : suffisant pour placer des cibles, sans dépendance.
    fn random(&mut self) -> f64 {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9_7F4A_7C15;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn new_target(&mut self, t: f64) {
        let yaw = (self.random() * 2.0 - 1.0) * SPAN_YAW * 0.8;
        let pitch = (self.random() * 2.0 - 1.0) * SPAN_PITCH * 0.7;
        self.target = Some((yaw, pitch));
        self.appeared_at = t;
        self.on_target_since = None;
    }

    pub fn update(&mut self, s: &Sample) {
        if !self.running {
            return;
        }
        let Some((ty, tp)) = self.target else {
            self.new_target(s.t);
            return;
        };
        let dist = (s.output[0] - ty).hypot(s.output[1] - tp);
        if dist > TARGET_RADIUS {
            self.on_target_since = None;
            return;
        }
        let since = *self.on_target_since.get_or_insert(s.t);
        if s.t - since >= HOLD_S {
            // Temps d'acquisition : jusqu'à l'entrée dans la cible (pas la fin du maintien).
            self.times.push(since - self.appeared_at);
            self.new_target(s.t);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, latest: Option<&Sample>, settings: &TrackingSettings) {
        ui.horizontal(|ui| {
            let label = if self.running {
                "Arrêter"
            } else {
                "Démarrer les cibles"
            };
            if ui.button(label).clicked() {
                self.running = !self.running;
                self.target = None;
            }
            if ui.button("Effacer les scores").clicked() {
                self.times.clear();
            }
            if let Some(last) = self.times.last() {
                let mean = self.times.iter().sum::<f64>() / self.times.len() as f64;
                ui.label(format!(
                    "Dernière : {last:.2} s   moyenne : {mean:.2} s   ({} cibles)",
                    self.times.len()
                ));
            }
        });
        ui.small(format!(
            "Poser le réticule sur la cible et l'y maintenir {HOLD_S} s. \
             Point gris : la même tête sans filtre (courbe seule)."
        ));

        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);

        let scale =
            (rect.width() as f64 / (2.0 * SPAN_YAW)).min(rect.height() as f64 / (2.0 * SPAN_PITCH));
        let to_screen = |yaw: f64, pitch: f64| {
            rect.center() + vec2((yaw * scale) as f32, (-pitch * scale) as f32)
        };
        let grid = ui.visuals().weak_text_color().gamma_multiply(0.4);

        // Graduations tous les 15°.
        for k in -6..=6 {
            let a = f64::from(k) * 15.0;
            if a.abs() <= SPAN_YAW {
                painter.line_segment(
                    [to_screen(a, -SPAN_PITCH), to_screen(a, SPAN_PITCH)],
                    Stroke::new(1.0, grid),
                );
            }
            if a.abs() <= SPAN_PITCH {
                painter.line_segment(
                    [to_screen(-SPAN_YAW, a), to_screen(SPAN_YAW, a)],
                    Stroke::new(1.0, grid),
                );
            }
        }

        if let Some((ty, tp)) = self.target {
            let color = if self.on_target_since.is_some() {
                Color32::from_rgb(60, 220, 120)
            } else {
                Color32::from_rgb(255, 170, 0)
            };
            painter.circle_stroke(
                to_screen(ty, tp),
                (TARGET_RADIUS * scale) as f32,
                Stroke::new(2.0, color),
            );
        }

        let Some(s) = latest else { return };

        // Pose brute passée dans la courbe, sans filtre : montre ce que le filtre retire.
        let unfiltered: [f64; 6] = std::array::from_fn(|i| {
            if settings.curves_enabled {
                settings.curves[i].apply(s.raw[i])
            } else {
                s.raw[i]
            }
        });
        painter.circle_filled(to_screen(unfiltered[0], unfiltered[1]), 3.0, Color32::GRAY);

        // Réticule : tourne avec le roulis de sortie.
        let center = to_screen(s.output[0], s.output[1]);
        // Écran : y vers le bas, donc un angle positif tourne dans le sens horaire,
        // comme la tête penchée à droite (roulis +).
        let (sin, cos) = s.output[2].to_radians().sin_cos();
        let rot = |dx: f32, dy: f32| {
            center
                + vec2(
                    dx * cos as f32 - dy * sin as f32,
                    dx * sin as f32 + dy * cos as f32,
                )
        };
        let stroke = Stroke::new(2.0, Color32::from_rgb(80, 160, 255));
        let r = 18.0;
        painter.line_segment([rot(-r, 0.0), rot(-6.0, 0.0)], stroke);
        painter.line_segment([rot(6.0, 0.0), rot(r, 0.0)], stroke);
        painter.line_segment([rot(0.0, -r), rot(0.0, -6.0)], stroke);
        painter.line_segment([rot(0.0, 6.0), rot(0.0, r)], stroke);
        painter.circle_stroke(center, 4.0, stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: f64, yaw: f64, pitch: f64) -> Sample {
        let mut output = [0.0; 6];
        output[0] = yaw;
        output[1] = pitch;
        Sample {
            t,
            raw: output,
            filtered: output,
            output,
        }
    }

    fn game_with_target(yaw: f64, pitch: f64) -> TargetGame {
        TargetGame {
            running: true,
            target: Some((yaw, pitch)),
            ..Default::default()
        }
    }

    #[test]
    fn cible_validee_apres_le_maintien() {
        let mut g = game_with_target(20.0, 10.0);
        g.update(&at(0.0, 0.0, 0.0)); // loin de la cible
        g.update(&at(0.8, 21.0, 10.0)); // entre dans la cible à 0,8 s
        g.update(&at(1.1, 20.5, 9.5));
        assert!(g.times.is_empty(), "0,3 s seulement");
        g.update(&at(1.35, 20.0, 10.0));
        assert_eq!(
            g.times,
            vec![0.8],
            "temps d'acquisition = entrée dans la cible"
        );
        assert_ne!(g.target, Some((20.0, 10.0)), "une nouvelle cible est tirée");
    }

    #[test]
    fn sortir_de_la_cible_remet_le_chrono_a_zero() {
        let mut g = game_with_target(0.0, 0.0);
        g.update(&at(0.0, 0.0, 0.0));
        g.update(&at(0.4, 8.0, 0.0)); // sortie (rayon 5°)
        g.update(&at(0.5, 0.0, 0.0));
        g.update(&at(0.9, 0.0, 0.0));
        assert!(g.times.is_empty());
        g.update(&at(1.0, 0.0, 0.0));
        assert_eq!(g.times.len(), 1);
    }

    #[test]
    fn rien_ne_se_passe_a_l_arret() {
        let mut g = TargetGame::default();
        g.update(&at(0.0, 0.0, 0.0));
        assert!(g.target.is_none());
    }

    #[test]
    fn les_cibles_restent_dans_la_mire() {
        let mut g = TargetGame::default();
        for _ in 0..1000 {
            g.new_target(0.0);
            let (y, p) = g.target.unwrap();
            assert!(y.abs() <= SPAN_YAW && p.abs() <= SPAN_PITCH);
        }
    }
}
