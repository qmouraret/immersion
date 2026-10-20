//! Région d'intérêt (ROI) : un carré orienté dans l'image source.
//!
//! Les modèles travaillent sur des images carrées de taille fixe (128 ou 256 px).
//! Une `Roi` décrit quelle partie de l'image source on y projette, et permet
//! de convertir les coordonnées dans les deux sens.

/// Carré orienté, en pixels de l'image source.
///
/// Convention : l'axe horizontal du recadrage pointe dans la direction
/// `(cos rotation, sin rotation)` de l'image. `rotation = 0` : pas de rotation.
/// En coordonnées image, y descend : un angle positif tourne dans le sens horaire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roi {
    /// Abscisse du centre, en pixels.
    pub cx: f32,
    /// Ordonnée du centre, en pixels.
    pub cy: f32,
    /// Côté du carré, en pixels.
    pub size: f32,
    /// Orientation, en radians (voir la convention ci-dessus).
    pub rotation: f32,
}

impl Roi {
    /// Carré centré qui couvre toute l'image (la partie hors image est noire) :
    /// équivalent d'un redimensionnement avec bandes noires (« letterbox »).
    pub fn full_image(width: u32, height: u32) -> Self {
        Self {
            cx: width as f32 / 2.0,
            cy: height as f32 / 2.0,
            size: width.max(height) as f32,
            rotation: 0.0,
        }
    }

    /// Coordonnées normalisées du recadrage (`u`, `v` dans [0, 1]) → pixels image.
    pub fn to_image(&self, u: f32, v: f32) -> (f32, f32) {
        let (sin, cos) = self.rotation.sin_cos();
        let dx = (u - 0.5) * self.size;
        let dy = (v - 0.5) * self.size;
        (self.cx + dx * cos - dy * sin, self.cy + dx * sin + dy * cos)
    }

    /// Pixels image → coordonnées normalisées du recadrage (inverse de [`Self::to_image`]).
    pub fn to_local(&self, x: f32, y: f32) -> (f32, f32) {
        let (sin, cos) = self.rotation.sin_cos();
        let (ex, ey) = (x - self.cx, y - self.cy);
        let dx = ex * cos + ey * sin;
        let dy = -ex * sin + ey * cos;
        (dx / self.size + 0.5, dy / self.size + 0.5)
    }

    /// Stabilisation du suivi : garde `self` (le recadrage courant) tant que
    /// `next` n'en diffère pas assez, sinon adopte `next`.
    ///
    /// Sans cela, la gigue des points déplace le recadrage, ce qui change
    /// l'image donnée au modèle, donc ses points : le bruit s'entretient
    /// d'image en image. Seuils relatifs à la taille du recadrage : `shift`
    /// (déplacement du centre et variation de taille, en fraction du côté) et
    /// `angle` (radians).
    pub fn stabilized(&self, next: &Roi, shift: f32, angle: f32) -> Roi {
        let moved = (next.cx - self.cx).hypot(next.cy - self.cy) > shift * self.size;
        let resized = (next.size - self.size).abs() > shift * self.size;
        // Écart d'angle ramené dans ]-π, π] (sinon 359° et 1° sembleraient loin).
        let turn = (next.rotation - self.rotation + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        if moved || resized || turn.abs() > angle {
            *next
        } else {
            *self
        }
    }

    /// Les 4 coins dans l'image (pour l'affichage), dans l'ordre du contour.
    pub fn corners(&self) -> [(f32, f32); 4] {
        [
            self.to_image(0.0, 0.0),
            self.to_image(1.0, 0.0),
            self.to_image(1.0, 1.0),
            self.to_image(0.0, 1.0),
        ]
    }

    /// Construit la ROI d'un ensemble de points : orientation donnée par deux
    /// points de référence (les yeux), boîte englobante calculée *dans ce repère
    /// tourné*, puis agrandie de `scale` et rendue carrée (côté le plus long).
    ///
    /// C'est la transformation utilisée par MediaPipe (`scale` = 1.5) pour passer
    /// d'une détection, ou des points de l'image précédente, au recadrage suivant.
    pub fn from_points(
        points: impl IntoIterator<Item = (f32, f32)>,
        eye_a: (f32, f32),
        eye_b: (f32, f32),
        scale: f32,
    ) -> Option<Self> {
        let rotation = (eye_b.1 - eye_a.1).atan2(eye_b.0 - eye_a.0);
        let (sin, cos) = rotation.sin_cos();

        // On projette chaque point sur les axes tournés et on garde les extrêmes.
        let (mut min_a, mut max_a) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_b, mut max_b) = (f32::INFINITY, f32::NEG_INFINITY);
        for (x, y) in points {
            let a = x * cos + y * sin;
            let b = -x * sin + y * cos;
            min_a = min_a.min(a);
            max_a = max_a.max(a);
            min_b = min_b.min(b);
            max_b = max_b.max(b);
        }
        if !min_a.is_finite() {
            return None; // aucun point
        }

        // Centre de la boîte, ramené dans le repère image (rotation inverse).
        let (ca, cb) = ((min_a + max_a) / 2.0, (min_b + max_b) / 2.0);
        let size = (max_a - min_a).max(max_b - min_b) * scale;
        Some(Self {
            cx: ca * cos - cb * sin,
            cy: ca * sin + cb * cos,
            size,
            rotation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_6;

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn stabilisation_ignore_les_petites_variations() {
        let roi = Roi {
            cx: 500.0,
            cy: 300.0,
            size: 400.0,
            rotation: 0.1,
        };
        let jitter = Roi {
            cx: 503.0,
            cy: 298.0,
            size: 404.0,
            rotation: 0.11,
        };
        assert_eq!(roi.stabilized(&jitter, 0.02, 0.035), roi);
    }

    #[test]
    fn stabilisation_suit_un_vrai_mouvement() {
        let roi = Roi {
            cx: 500.0,
            cy: 300.0,
            size: 400.0,
            rotation: 0.0,
        };
        let moved = Roi { cx: 520.0, ..roi }; // 5 % du côté
        let turned = Roi {
            rotation: 0.1,
            ..roi
        }; // ≈ 6°
        let zoomed = Roi { size: 430.0, ..roi };
        for next in [moved, turned, zoomed] {
            assert_eq!(roi.stabilized(&next, 0.02, 0.035), next);
        }
    }

    #[test]
    fn stabilisation_angles_autour_de_pi() {
        let roi = Roi {
            cx: 0.0,
            cy: 0.0,
            size: 100.0,
            rotation: std::f32::consts::PI - 0.01,
        };
        let next = Roi {
            rotation: -std::f32::consts::PI + 0.01,
            ..roi
        }; // 0,02 rad d'écart réel
        assert_eq!(roi.stabilized(&next, 0.02, 0.035), roi);
    }

    #[test]
    fn aller_retour_image_local() {
        let roi = Roi {
            cx: 300.0,
            cy: 200.0,
            size: 128.0,
            rotation: FRAC_PI_6,
        };
        for (u, v) in [(0.0, 0.0), (0.25, 0.8), (1.0, 1.0)] {
            let (x, y) = roi.to_image(u, v);
            assert!(close(roi.to_local(x, y), (u, v)));
        }
    }

    #[test]
    fn le_centre_du_recadrage_est_le_centre_de_la_roi() {
        let roi = Roi {
            cx: 10.0,
            cy: 20.0,
            size: 50.0,
            rotation: 1.0,
        };
        assert!(close(roi.to_image(0.5, 0.5), (10.0, 20.0)));
    }

    #[test]
    fn letterbox_paysage() {
        // Image 1920×1080 : le carré fait 1920 de côté, des bandes noires en haut et en bas.
        let roi = Roi::full_image(1920, 1080);
        assert!(close(roi.to_image(0.0, 0.0), (0.0, -420.0)));
    }

    #[test]
    fn from_points_suit_l_inclinaison_des_yeux() {
        // Carré de 100 px incliné de 30°, les « yeux » sur son bord supérieur.
        let truth = Roi {
            cx: 500.0,
            cy: 400.0,
            size: 100.0,
            rotation: FRAC_PI_6,
        };
        let pts = truth.corners();
        let roi = Roi::from_points(pts, pts[0], pts[1], 1.0).unwrap();
        assert!((roi.rotation - FRAC_PI_6).abs() < 1e-4);
        assert!(close((roi.cx, roi.cy), (500.0, 400.0)));
        assert!((roi.size - 100.0).abs() < 1e-3);
    }
}
