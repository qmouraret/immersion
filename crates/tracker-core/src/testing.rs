//! Données synthétiques pour les tests (de cette crate et des autres).
//!
//! Module public, mais sans rôle dans le pipeline réel : il permet de tester
//! la pose, le pipeline ou l'UI avec une tête dont on connaît la pose exacte,
//! sans caméra ni modèle ML.

use nalgebra::{Rotation3, Unit, Vector3};

use crate::canonical_face::CANONICAL_FACE;
use crate::pose::{Angles, CameraModel};
use crate::{FaceLandmarks, Frame};

/// Points du visage tels que MediaPipe les donnerait pour une tête à la pose
/// `angles` (convention utilisateur) et à la `position` donnée dans le repère
/// caméra (cm, z < 0 devant la caméra).
///
/// Projection « faible perspective », comme le modèle : x, y en pixels image,
/// z relatif à la même échelle que x.
pub fn synthetic_landmarks(
    camera: &CameraModel,
    angles: Angles,
    position: Vector3<f64>,
) -> FaceLandmarks {
    let f = camera.focal_px();
    let depth = -position.z;
    let scale = f / depth;
    // Rotation apparente = rotation réelle vue depuis la ligne de visée.
    let line_of_sight =
        Rotation3::rotation_between(&Vector3::z(), &Unit::new_normalize(-position).into_inner())
            .unwrap_or_else(Rotation3::identity);
    let apparent = line_of_sight.inverse() * angles.to_rotation();
    let u0 = f64::from(camera.width) / 2.0 + position.x * f / depth;
    let v0 = f64::from(camera.height) / 2.0 - position.y * f / depth;
    let points = CANONICAL_FACE
        .iter()
        .map(|p| {
            let m = Vector3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            let q = scale * (apparent * m);
            [(u0 + q.x) as f32, (v0 - q.y) as f32, -q.z as f32]
        })
        .collect();
    FaceLandmarks {
        points,
        presence: 1.0,
    }
}

/// Image unie (gris moyen) de la taille donnée.
pub fn gray_frame(width: u32, height: u32, seq: u64, captured_at: std::time::Instant) -> Frame {
    Frame::new(
        width,
        height,
        vec![128; (width * height * 3) as usize],
        seq,
        captured_at,
    )
    .expect("taille cohérente par construction")
}
