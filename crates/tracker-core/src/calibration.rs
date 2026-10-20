//! Calculs de l'assistant d'étalonnage (sans E/S : l'interface collecte les
//! mesures, ces fonctions en tirent les réglages).

use crate::canonical_face::CANONICAL_FACE;

/// Racine du nez (point 168), entre les yeux : là où l'on pose le mètre.
const NOSE_ROOT: usize = 168;

/// Profondeur, en cm, entre la racine du nez et l'origine du modèle (≈ centre
/// de la tête) : la distance calculée par la pose est celle de cette origine.
pub fn nose_root_depth_cm() -> f64 {
    f64::from(CANONICAL_FACE[NOSE_ROOT][2])
}

/// FOV horizontal de la caméra déduit d'une distance mesurée.
///
/// - `depth_scales` : échelles (px/cm) de plusieurs images ([`crate::HeadPose::depth_scale`]),
///   l'utilisateur faisant face à la caméra à la distance mesurée ;
/// - `measured_cm` : distance objectif → racine du nez, mesurée au mètre ;
/// - `image_width` : largeur de l'image, en pixels.
///
/// À la profondeur `Z`, 1 cm mesure `f / Z` pixels, donc `f = échelle × Z`,
/// puis `FOV = 2·atan(largeur / 2 / f)`. On prend la médiane des échelles.
pub fn hfov_from_distance(depth_scales: &[f64], measured_cm: f64, image_width: f32) -> Option<f32> {
    if depth_scales.is_empty() || measured_cm <= 0.0 {
        return None;
    }
    let mut v = depth_scales.to_vec();
    v.sort_by(f64::total_cmp);
    let scale = v[v.len() / 2];
    let focal = scale * (measured_cm + nose_root_depth_cm());
    Some((2.0 * (f64::from(image_width) / 2.0 / focal).atan()).to_degrees() as f32)
}

/// Amplitudes confortables d'un axe : (côté négatif, côté positif), en valeur
/// absolue, aux 5ᵉ et 95ᵉ centiles des valeurs mesurées.
pub fn comfortable_range(values: &[f64]) -> Option<(f64, f64)> {
    if values.len() < 10 {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    Some((at(0.05).min(0.0).abs(), at(0.95).max(0.0)))
}

/// « Entrée max. » d'une courbe symétrique à partir des deux amplitudes :
/// la plus petite des deux (pour atteindre la sortie max. des deux côtés),
/// au moins `min`.
pub fn curve_input(range: (f64, f64), min: f64) -> f64 {
    range.0.min(range.1).max(min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose::{Angles, CameraModel, estimate};
    use crate::testing::synthetic_landmarks;
    use nalgebra::Vector3;

    #[test]
    fn racine_du_nez_devant_le_centre_de_la_tete() {
        let d = nose_root_depth_cm();
        assert!((4.0..7.0).contains(&d), "{d} cm");
    }

    #[test]
    fn retrouve_le_fov_d_une_camera_synthetique() {
        // Caméra de 70° ; tête de face à 60 cm (racine du nez) de l'objectif.
        let camera = CameraModel {
            width: 1920.0,
            height: 1080.0,
            hfov_deg: 70.0,
        };
        let depth = 60.0 + nose_root_depth_cm();
        let lm = synthetic_landmarks(&camera, Angles::default(), Vector3::new(0.0, 0.0, -depth));
        // L'application tourne avec un FOV faux (50°) : l'échelle, elle, ne dépend pas du FOV.
        let wrong = CameraModel {
            hfov_deg: 50.0,
            ..camera
        };
        let scale = estimate(&lm, &wrong).unwrap().depth_scale;
        let hfov = hfov_from_distance(&[scale, scale, scale], 60.0, 1920.0).unwrap();
        assert!((hfov - 70.0).abs() < 0.01, "FOV retrouvé : {hfov}");
    }

    #[test]
    fn amplitudes_aux_centiles() {
        // Balayage de −30° à +40°, plus un pic parasite à +90°.
        let mut v: Vec<f64> = (0..=70).map(|i| f64::from(i) - 30.0).collect();
        v.push(90.0);
        let (neg, pos) = comfortable_range(&v).unwrap();
        assert!(
            (neg - 26.0).abs() < 1.5 && (pos - 36.0).abs() < 1.5,
            "{neg} {pos}"
        );
        assert_eq!(curve_input((neg, pos), 10.0), neg);
        assert_eq!(
            curve_input((2.0, 3.0), 10.0),
            10.0,
            "jamais sous le minimum"
        );
    }

    #[test]
    fn rien_sans_mesures() {
        assert!(hfov_from_distance(&[], 60.0, 1920.0).is_none());
        assert!(comfortable_range(&[1.0; 3]).is_none());
    }
}
