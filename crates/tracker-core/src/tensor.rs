//! Préparation des entrées des modèles : recadrage orienté + normalisation.

use crate::{Frame, Roi};

/// Plage de valeurs attendue par un modèle pour ses pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Normalization {
    /// 0..255 → 0..1
    ZeroToOne,
    /// 0..255 → -1..1
    MinusOneToOne,
}

impl Normalization {
    #[inline]
    fn apply(self, v: f32) -> f32 {
        match self {
            Self::ZeroToOne => v / 255.0,
            Self::MinusOneToOne => v / 127.5 - 1.0,
        }
    }
}

/// Remplit `dst` avec le contenu de `roi`, rééchantillonné en `out × out`
/// pixels RGB, au format NHWC (`[1, out, out, 3]`) attendu par les modèles.
///
/// - Hors de l'image, les pixels sont noirs (bandes du letterbox).
/// - Quand on réduit beaucoup (le visage fait 400 px, le modèle en veut 128),
///   un seul échantillon par pixel crée du crénelage : on moyenne alors
///   plusieurs échantillons bilinéaires par pixel de sortie (sur-échantillonnage).
///
/// `dst` est réutilisé d'un appel à l'autre : aucune allocation par image.
pub fn sample_roi(frame: &Frame, roi: &Roi, out: usize, norm: Normalization, dst: &mut Vec<f32>) {
    dst.clear();
    dst.reserve(out * out * 3);

    // Nombre d'échantillons par axe : environ un par pixel source couvert.
    let n = ((roi.size / out as f32).ceil() as usize).clamp(1, 3);
    let weight = 1.0 / (n * n) as f32;

    // `roi.to_image` est affine : image = origine + i·pas_i + j·pas_j, où i, j
    // comptent les sous-pixels. On précalcule ces vecteurs une fois pour toutes
    // plutôt que de refaire un sin/cos pour chacun des ~600 000 échantillons.
    let step = 1.0 / (out * n) as f32;
    let origin = roi.to_image(0.5 * step, 0.5 * step);
    let right = sub(roi.to_image(1.5 * step, 0.5 * step), origin);
    let down = sub(roi.to_image(0.5 * step, 1.5 * step), origin);

    for row in 0..out {
        for col in 0..out {
            let mut acc = [0.0f32; 3];
            for sy in 0..n {
                for sx in 0..n {
                    let (i, j) = ((col * n + sx) as f32, (row * n + sy) as f32);
                    let x = origin.0 + i * right.0 + j * down.0;
                    let y = origin.1 + i * right.1 + j * down.1;
                    let px = bilinear(frame, x, y);
                    acc[0] += px[0];
                    acc[1] += px[1];
                    acc[2] += px[2];
                }
            }
            dst.extend(acc.map(|c| norm.apply(c * weight)));
        }
    }
}

fn sub(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 - b.0, a.1 - b.1)
}

/// Échantillon bilinéaire au point (x, y) en pixels (centre du pixel i = i + 0.5).
#[inline]
fn bilinear(frame: &Frame, x: f32, y: f32) -> [f32; 3] {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let (tx, ty) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);

    let p00 = pixel(frame, x0, y0);
    let p10 = pixel(frame, x0 + 1, y0);
    let p01 = pixel(frame, x0, y0 + 1);
    let p11 = pixel(frame, x0 + 1, y0 + 1);

    std::array::from_fn(|c| {
        let top = p00[c] + (p10[c] - p00[c]) * tx;
        let bottom = p01[c] + (p11[c] - p01[c]) * tx;
        top + (bottom - top) * ty
    })
}

/// Pixel (x, y), noir s'il est hors de l'image.
#[inline]
fn pixel(frame: &Frame, x: i64, y: i64) -> [f32; 3] {
    if x < 0 || y < 0 || x >= i64::from(frame.width) || y >= i64::from(frame.height) {
        return [0.0; 3];
    }
    let i = (y as usize * frame.width as usize + x as usize) * 3;
    let p = &frame.rgb[i..i + 3];
    [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Image 4×4 où chaque pixel vaut (x*10, y*10, 0).
    fn gradient() -> Frame {
        let mut rgb = Vec::new();
        for y in 0..4u8 {
            for x in 0..4u8 {
                rgb.extend([x * 10, y * 10, 0]);
            }
        }
        Frame::new(4, 4, rgb, 0, Instant::now()).unwrap()
    }

    #[test]
    fn recadrage_identite_restitue_les_pixels() {
        let frame = gradient();
        let mut dst = Vec::new();
        sample_roi(
            &frame,
            &Roi::full_image(4, 4),
            4,
            Normalization::ZeroToOne,
            &mut dst,
        );
        assert_eq!(dst.len(), 4 * 4 * 3);
        // Pixel (x=2, y=1) → R = 20/255, G = 10/255.
        let i = (4 + 2) * 3;
        assert!((dst[i] - 20.0 / 255.0).abs() < 1e-5);
        assert!((dst[i + 1] - 10.0 / 255.0).abs() < 1e-5);
    }

    #[test]
    fn hors_image_c_est_noir() {
        let frame = gradient();
        let roi = Roi {
            cx: 100.0,
            cy: 100.0,
            size: 4.0,
            rotation: 0.0,
        };
        let mut dst = Vec::new();
        sample_roi(&frame, &roi, 2, Normalization::MinusOneToOne, &mut dst);
        assert!(dst.iter().all(|&v| (v + 1.0).abs() < 1e-6));
    }
}
