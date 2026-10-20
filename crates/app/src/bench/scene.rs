//! Scène 3D en fil de fer vue « à travers » la tête : cockpit simplifié,
//! sol quadrillé, horizon et points cardinaux.
//!
//! Repère monde (mètres) : x à droite, y en haut, z vers l'arrière ; l'œil du
//! pilote est à l'origine et regarde vers -z.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, vec2};
use nalgebra::{Rotation3, Vector3};

use super::Sample;

/// Plan proche : on coupe ce qui est plus près que ça (ou derrière l'œil).
const NEAR: f64 = 0.05;
const EYE_HEIGHT: f64 = 1.2;

pub struct SceneView {
    cockpit: bool,
    ground: bool,
    translation: bool,
    hfov_deg: f32,
}

impl Default for SceneView {
    fn default() -> Self {
        Self {
            cockpit: true,
            ground: true,
            translation: true,
            hfov_deg: 75.0,
        }
    }
}

type Segment = ([f64; 3], [f64; 3]);

impl SceneView {
    pub fn ui(&mut self, ui: &mut egui::Ui, latest: Option<&Sample>) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.cockpit, "Cockpit");
            ui.checkbox(&mut self.ground, "Sol et horizon");
            ui.checkbox(&mut self.translation, "Translations");
            ui.add(
                egui::Slider::new(&mut self.hfov_deg, 40.0..=110.0)
                    .suffix("°")
                    .text("FOV vue"),
            );
        });

        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 24, 36));

        let output = latest.map_or([0.0; 6], |s| s.output);
        let camera = Camera::new(rect, &output, self.hfov_deg, self.translation);

        if self.ground {
            let c = Color32::from_rgb(70, 110, 80);
            for (a, b) in ground() {
                camera.line(&painter, a, b, Stroke::new(1.0, c));
            }
            for (label, dir) in [
                ("N", [0.0, -1.0]),
                ("E", [1.0, 0.0]),
                ("S", [0.0, 1.0]),
                ("O", [-1.0, 0.0]),
            ] {
                let p = [dir[0] * 60.0, 2.0 - EYE_HEIGHT, dir[1] * 60.0];
                if let Some(pos) = camera.project(p) {
                    painter.text(
                        pos,
                        egui::Align2::CENTER_BOTTOM,
                        label,
                        egui::FontId::proportional(22.0),
                        Color32::from_rgb(240, 220, 120),
                    );
                }
            }
        }
        if self.cockpit {
            let c = Color32::from_rgb(170, 180, 200);
            for (a, b) in cockpit() {
                camera.line(&painter, a, b, Stroke::new(2.0, c));
            }
        }

        // Rappel de la pose de sortie.
        painter.text(
            rect.left_bottom() + vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            format!(
                "lacet {:+.0}°  tangage {:+.0}°  roulis {:+.0}°  x {:+.0}  y {:+.0}  z {:+.0} cm",
                output[0], output[1], output[2], output[3], output[4], output[5]
            ),
            egui::FontId::monospace(12.0),
            Color32::from_gray(200),
        );
    }
}

/// Caméra virtuelle à la place de la tête du pilote.
struct Camera {
    /// Monde → repère caméra.
    inv_rotation: Rotation3<f64>,
    position: Vector3<f64>,
    focal: f64,
    center: Pos2,
}

impl Camera {
    fn new(rect: Rect, output: &[f64; 6], hfov_deg: f32, translation: bool) -> Self {
        let [yaw, pitch, roll, x, y, z] = *output;
        // Lacet+ (droite) : l'avant (-z) part vers +x, soit une rotation de -lacet autour de y.
        // Tangage+ (haut) : rotation de +tangage autour de x. Roulis+ (droite) : -roulis autour de z.
        let rotation = Rotation3::from_axis_angle(&Vector3::y_axis(), -yaw.to_radians())
            * Rotation3::from_axis_angle(&Vector3::x_axis(), pitch.to_radians())
            * Rotation3::from_axis_angle(&Vector3::z_axis(), -roll.to_radians());
        let position = if translation {
            Vector3::new(x, y, z) / 100.0 // cm → m
        } else {
            Vector3::zeros()
        };
        Self {
            inv_rotation: rotation.inverse(),
            position,
            focal: f64::from(rect.width()) / 2.0 / (f64::from(hfov_deg).to_radians() / 2.0).tan(),
            center: rect.center(),
        }
    }

    fn to_camera(&self, p: [f64; 3]) -> Vector3<f64> {
        self.inv_rotation * (Vector3::from(p) - self.position)
    }

    fn screen(&self, c: &Vector3<f64>) -> Pos2 {
        let depth = -c.z;
        Pos2::new(
            self.center.x + (self.focal * c.x / depth) as f32,
            self.center.y - (self.focal * c.y / depth) as f32,
        )
    }

    fn project(&self, p: [f64; 3]) -> Option<Pos2> {
        let c = self.to_camera(p);
        (-c.z > NEAR).then(|| self.screen(&c))
    }

    /// Trace un segment, coupé au plan proche s'il passe derrière l'œil.
    fn line(&self, painter: &egui::Painter, a: [f64; 3], b: [f64; 3], stroke: Stroke) {
        if let Some((a, b)) = clip_near(self.to_camera(a), self.to_camera(b)) {
            painter.line_segment([self.screen(&a), self.screen(&b)], stroke);
        }
    }
}

/// Coupe un segment (repère caméra) au plan proche `z = -NEAR`.
///
/// `None` s'il est entièrement derrière ; sinon, l'extrémité de derrière est
/// remplacée par l'intersection avec le plan (interpolation linéaire).
fn clip_near(a: Vector3<f64>, b: Vector3<f64>) -> Option<(Vector3<f64>, Vector3<f64>)> {
    let (da, db) = (-a.z - NEAR, -b.z - NEAR);
    match (da >= 0.0, db >= 0.0) {
        (false, false) => None,
        (true, true) => Some((a, b)),
        (false, true) => Some((a + (b - a) * (da / (da - db)), b)),
        (true, false) => Some((a, b + (a - b) * (db / (db - da)))),
    }
}

/// Sol quadrillé tous les 5 m sur 200 m, plus un grand carré à l'horizon.
fn ground() -> Vec<Segment> {
    let y = -EYE_HEIGHT;
    let mut out = Vec::new();
    for k in -20..=20 {
        let v = f64::from(k) * 5.0;
        out.push(([v, y, -100.0], [v, y, 100.0]));
        out.push(([-100.0, y, v], [100.0, y, v]));
    }
    let far = 2000.0;
    for (a, b) in [([-far, -far]), ([far, -far]), ([far, far]), ([-far, far])]
        .iter()
        .zip([([far, -far]), ([far, far]), ([-far, far]), ([-far, -far])].iter())
    {
        out.push(([a[0], y, a[1]], [b[0], y, b[1]]));
    }
    out
}

/// Cockpit d'avion léger (aile haute), très simplifié.
fn cockpit() -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut poly = |pts: &[[f64; 3]], closed: bool| {
        for w in pts.windows(2) {
            out.push((w[0], w[1]));
        }
        if closed && pts.len() > 2 {
            out.push((pts[pts.len() - 1], pts[0]));
        }
    };

    // Tableau de bord : face avant et casquette.
    let (top, bottom, front) = (-0.18, -0.48, -0.75);
    poly(
        &[
            [-0.6, top, front],
            [0.6, top, front],
            [0.6, bottom, front],
            [-0.6, bottom, front],
        ],
        true,
    );
    poly(
        &[
            [-0.6, top, front],
            [-0.55, top + 0.03, front + 0.12],
            [0.55, top + 0.03, front + 0.12],
            [0.6, top, front],
        ],
        false,
    );

    // Six instruments ronds (2 rangées de 3) devant le pilote.
    for row in 0..2 {
        for col in 0..3 {
            let cx = -0.36 + f64::from(col) * 0.12;
            let cy = -0.26 - f64::from(row) * 0.12;
            let ring: Vec<[f64; 3]> = (0..16)
                .map(|i| {
                    let a = f64::from(i) / 16.0 * std::f64::consts::TAU;
                    [cx + 0.045 * a.cos(), cy + 0.045 * a.sin(), front + 0.001]
                })
                .collect();
            poly(&ring, true);
        }
    }

    // Montants du pare-brise, montant central et bord du toit.
    let roof = 0.32;
    poly(&[[-0.6, top, front], [-0.5, roof, -0.3]], false);
    poly(&[[0.6, top, front], [0.5, roof, -0.3]], false);
    poly(&[[0.0, top + 0.03, front + 0.12], [0.0, roof, -0.3]], false);
    poly(
        &[
            [-0.5, roof, -0.3],
            [0.5, roof, -0.3],
            [0.5, roof, 0.6],
            [-0.5, roof, 0.6],
        ],
        true,
    );

    // Portes : bas de vitre et montant arrière, de chaque côté.
    for side in [-1.0, 1.0] {
        let x = 0.6 * side;
        poly(
            &[[x, top, front], [x, -0.1, 0.5], [0.5 * side, roof, 0.6]],
            false,
        );
        poly(&[[0.5 * side, roof, -0.3], [0.5 * side, roof, 0.6]], false);
        // Aile haute : bord d'attaque, saumon, bord de fuite.
        let span = 5.5 * side;
        poly(
            &[
                [0.5 * side, roof, -0.2],
                [span, roof + 0.1, -0.15],
                [span, roof + 0.1, 0.9],
                [0.5 * side, roof, 0.9],
            ],
            false,
        );
        // Hauban.
        poly(
            &[[0.62 * side, -0.55, 0.1], [2.6 * side, roof + 0.05, 0.3]],
            false,
        );
    }

    // Capot moteur devant le pare-brise.
    poly(
        &[
            [-0.45, top, front - 0.05],
            [-0.3, -0.25, -2.0],
            [0.3, -0.25, -2.0],
            [0.45, top, front - 0.05],
        ],
        false,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Écran de 1000 × 600, FOV 90° : centre en (500, 300).
    fn camera(output: [f64; 6]) -> Camera {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        Camera::new(rect, &output, 90.0, true)
    }

    fn near(a: Pos2, b: Pos2) -> bool {
        (a - b).length() < 0.5
    }

    #[test]
    fn tout_droit_au_centre() {
        let p = camera([0.0; 6]).project([0.0, 0.0, -10.0]).unwrap();
        assert!(near(p, Pos2::new(500.0, 300.0)));
    }

    #[test]
    fn lacet_positif_regarde_a_droite() {
        // Un point à 45° sur la droite se retrouve au centre quand on tourne de 45°.
        let p = camera([45.0, 0.0, 0.0, 0.0, 0.0, 0.0])
            .project([10.0, 0.0, -10.0])
            .unwrap();
        assert!(near(p, Pos2::new(500.0, 300.0)));
    }

    #[test]
    fn tangage_positif_regarde_en_haut() {
        let up = 10.0 * 30f64.to_radians().tan();
        let p = camera([0.0, 30.0, 0.0, 0.0, 0.0, 0.0])
            .project([0.0, up, -10.0])
            .unwrap();
        assert!(near(p, Pos2::new(500.0, 300.0)));
    }

    #[test]
    fn roulis_positif_fait_monter_l_horizon_a_droite() {
        // Tête penchée à droite : le monde semble tourner dans le sens inverse
        // des aiguilles d'une montre, le côté droit de l'horizon monte.
        let p = camera([0.0, 0.0, 20.0, 0.0, 0.0, 0.0])
            .project([5.0, 0.0, -10.0])
            .unwrap();
        assert!(p.y < 300.0 && p.x > 500.0);
    }

    #[test]
    fn se_decaler_a_droite_fait_glisser_la_scene_a_gauche() {
        let p = camera([0.0, 0.0, 0.0, 10.0, 0.0, 0.0])
            .project([0.0, 0.0, -1.0])
            .unwrap();
        assert!(p.x < 500.0);
    }

    #[test]
    fn rien_derriere_la_tete() {
        assert!(camera([0.0; 6]).project([0.0, 0.0, 5.0]).is_none());
    }

    #[test]
    fn decoupe_au_plan_proche() {
        let front = Vector3::new(0.0, 0.0, -2.0);
        let behind = Vector3::new(0.0, 1.0, 2.0);
        let (a, b) = clip_near(behind, front).unwrap();
        assert!(
            (a.z + NEAR).abs() < 1e-12,
            "extrémité ramenée sur le plan proche"
        );
        assert_eq!(b, front);
        assert!(clip_near(behind, behind).is_none());
    }
}
