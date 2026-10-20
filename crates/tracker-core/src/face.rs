//! Points clés du visage (MediaPipe Face Mesh V2 : 468 points + 10 pour les iris).

use crate::Roi;
use crate::blazeface::{Detection, keypoint};

/// Nombre de points renvoyés par le modèle.
pub const NUM_LANDMARKS: usize = 478;
/// Taille d'entrée du modèle de points.
pub const LANDMARK_INPUT_SIZE: usize = 256;
/// Agrandissement de la boîte autour du visage (valeur MediaPipe).
const ROI_SCALE: f32 = 1.5;

/// Indices utiles dans le maillage MediaPipe.
pub mod index {
    /// Coin externe de l'œil droit de la personne.
    pub const RIGHT_EYE_OUTER: usize = 33;
    /// Coin externe de l'œil gauche de la personne.
    pub const LEFT_EYE_OUTER: usize = 263;
    /// Bout du nez.
    pub const NOSE_TIP: usize = 1;
    /// Bas du menton.
    pub const CHIN: usize = 152;
    /// Centres des iris (raffinement, indices 468 à 477).
    pub const RIGHT_IRIS: usize = 468;
    /// Centre de l'iris gauche de la personne.
    pub const LEFT_IRIS: usize = 473;
}

/// Les points du visage dans une image.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceLandmarks {
    /// (x, y, z) en pixels de l'image source. `z` est la profondeur relative
    /// (même échelle que x ; négatif = plus près de la caméra).
    pub points: Vec<[f32; 3]>,
    /// Probabilité qu'un visage soit bien présent dans le recadrage.
    pub presence: f32,
}

impl FaceLandmarks {
    /// Convertit la sortie brute du modèle (coordonnées en pixels du
    /// recadrage 256×256) en points de l'image source.
    pub fn from_model_output(raw: &[f32], roi: &Roi, presence: f32) -> Option<Self> {
        if raw.len() != NUM_LANDMARKS * 3 {
            return None;
        }
        let s = LANDMARK_INPUT_SIZE as f32;
        let z_scale = roi.size / s;
        let points = raw
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                let (x, y) = roi.to_image(p[0] / s, p[1] / s);
                [x, y, p[2] * z_scale]
            })
            .collect();
        Some(Self { points, presence })
    }

    fn xy(&self, i: usize) -> (f32, f32) {
        (self.points[i][0], self.points[i][1])
    }

    /// Recadrage pour l'image suivante, calculé à partir de ces points : c'est
    /// ce qui permet de *suivre* le visage sans relancer le détecteur.
    pub fn next_roi(&self) -> Option<Roi> {
        Roi::from_points(
            self.points.iter().take(468).map(|p| (p[0], p[1])),
            self.xy(index::RIGHT_EYE_OUTER),
            self.xy(index::LEFT_EYE_OUTER),
            ROI_SCALE,
        )
    }
}

/// Recadrage du modèle de points à partir d'une détection BlazeFace.
///
/// `detector_roi` est la région de l'image qui a été donnée au détecteur :
/// elle sert à ramener la détection (normalisée) en pixels image.
pub fn roi_from_detection(det: &Detection, detector_roi: &Roi) -> Option<Roi> {
    let to_img = |(u, v): (f32, f32)| detector_roi.to_image(u, v);
    let (hw, hh) = (det.w / 2.0, det.h / 2.0);
    let corners = [
        (det.cx - hw, det.cy - hh),
        (det.cx + hw, det.cy - hh),
        (det.cx + hw, det.cy + hh),
        (det.cx - hw, det.cy + hh),
    ]
    .map(to_img);
    Roi::from_points(
        corners,
        to_img(det.keypoints[keypoint::RIGHT_EYE]),
        to_img(det.keypoints[keypoint::LEFT_EYE]),
        ROI_SCALE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sortie_du_modele_ramenee_dans_l_image() {
        let roi = Roi {
            cx: 500.0,
            cy: 300.0,
            size: 512.0,
            rotation: 0.0,
        };
        let mut raw = vec![0.0; NUM_LANDMARKS * 3];
        raw[0..3].copy_from_slice(&[128.0, 128.0, 10.0]); // centre du recadrage
        let lm = FaceLandmarks::from_model_output(&raw, &roi, 0.9).unwrap();
        assert_eq!(lm.points[0], [500.0, 300.0, 20.0]); // z × (512 / 256)
    }

    #[test]
    fn detection_vers_roi_agrandie_et_carree() {
        let det = Detection {
            score: 0.9,
            cx: 0.5,
            cy: 0.5,
            w: 0.2,
            h: 0.25,
            keypoints: [
                (0.45, 0.45),
                (0.55, 0.45),
                (0.5, 0.5),
                (0.5, 0.55),
                (0.4, 0.45),
                (0.6, 0.45),
            ],
        };
        let detector_roi = Roi::full_image(1000, 1000);
        let roi = roi_from_detection(&det, &detector_roi).unwrap();
        assert!(roi.rotation.abs() < 1e-5); // yeux à l'horizontale
        assert!((roi.size - 0.25 * 1000.0 * 1.5).abs() < 1e-2); // côté le plus long × 1.5
        assert!((roi.cx - 500.0).abs() < 1e-3);
    }
}
