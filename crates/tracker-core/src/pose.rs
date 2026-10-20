//! Pose de la tête en 6DOF à partir des points du visage.
//!
//! # Repères
//!
//! - **Caméra (C)** : origine sur l'objectif, x vers la droite *de l'image*,
//!   y vers le haut, z vers l'utilisateur (la caméra regarde vers -z, comme
//!   en OpenGL). La tête est donc à z < 0.
//! - **Modèle** : visage canonique MediaPipe, en cm (voir `canonical_face`).
//!   Tête de face devant une caméra non mirroir ⇒ rotation identité.
//! - **Sortie ([`Pose6`])** : du point de vue de l'utilisateur, face à l'écran.
//!   x+ = vers sa droite, y+ = vers le haut, z+ = s'éloigner de l'écran ;
//!   lacet+ = tourner la tête à droite, tangage+ = lever la tête,
//!   roulis+ = pencher la tête vers l'épaule droite.

use nalgebra::{Matrix3, Rotation3, Unit, Vector3};

use crate::FaceLandmarks;
use crate::canonical_face::CANONICAL_FACE;

/// Modèle de caméra sténopé (pinhole), centre optique au centre de l'image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraModel {
    pub width: f32,
    pub height: f32,
    /// Champ de vision horizontal, en degrés.
    pub hfov_deg: f32,
}

impl CameraModel {
    /// Focale en pixels : f = (largeur / 2) / tan(FOV / 2).
    pub fn focal_px(&self) -> f64 {
        f64::from(self.width) / 2.0 / (f64::from(self.hfov_deg).to_radians() / 2.0).tan()
    }

    fn center(&self) -> (f64, f64) {
        (f64::from(self.width) / 2.0, f64::from(self.height) / 2.0)
    }
}

/// Similitude `dst ≈ scale · R · src + t` (rotation + échelle + translation).
#[derive(Debug, Clone, Copy)]
pub struct Similarity {
    pub scale: f64,
    pub rotation: Rotation3<f64>,
    pub translation: Vector3<f64>,
}

impl Similarity {
    pub fn apply(&self, p: &Vector3<f64>) -> Vector3<f64> {
        self.scale * (self.rotation * p) + self.translation
    }
}

/// Alignement de Umeyama (1991) : la similitude qui minimise
/// `Σ wᵢ |dstᵢ - (s R srcᵢ + t)|²`, en forme close via une SVD 3×3.
///
/// Renvoie `None` si les points sont dégénérés (tous confondus, poids nuls).
pub fn umeyama(src: &[Vector3<f64>], dst: &[Vector3<f64>], weights: &[f64]) -> Option<Similarity> {
    let wsum: f64 = weights.iter().sum();
    if wsum <= 0.0 || src.len() != dst.len() || src.len() != weights.len() {
        return None;
    }
    let mean = |pts: &[Vector3<f64>]| {
        pts.iter()
            .zip(weights)
            .map(|(p, w)| p * *w)
            .sum::<Vector3<f64>>()
            / wsum
    };
    let (mu_s, mu_d) = (mean(src), mean(dst));

    // Variance de la source et covariance croisée dst × srcᵀ.
    let mut var_s = 0.0;
    let mut cov = Matrix3::zeros();
    for ((s, d), w) in src.iter().zip(dst).zip(weights) {
        let (cs, cd) = (s - mu_s, d - mu_d);
        var_s += w * cs.norm_squared();
        cov += *w * cd * cs.transpose();
    }
    var_s /= wsum;
    cov /= wsum;
    if var_s <= f64::EPSILON {
        return None;
    }

    let svd = cov.svd(true, true);
    let (u, v_t) = (svd.u?, svd.v_t?);
    // Si U·Vᵀ est une réflexion (déterminant -1), on retourne le dernier axe :
    // on veut une vraie rotation, pas un miroir.
    let sign = if (u * v_t).determinant() < 0.0 {
        -1.0
    } else {
        1.0
    };
    let d = Matrix3::from_diagonal(&Vector3::new(1.0, 1.0, sign));
    let rotation = Rotation3::from_matrix_unchecked(u * d * v_t);
    let scale =
        (svd.singular_values[0] + svd.singular_values[1] + sign * svd.singular_values[2]) / var_s;
    let translation = mu_d - scale * (rotation * mu_s);
    Some(Similarity {
        scale,
        rotation,
        translation,
    })
}

/// Angles de la tête en degrés (convention de [`Pose6`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Angles {
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
}

impl Angles {
    /// Décompose une rotation du repère caméra en lacet / tangage / roulis.
    ///
    /// On écrit `R = Ry(a) · Rx(b) · Rz(c)` (d'abord le roulis, puis le
    /// tangage, puis le lacet), ce qui correspond à l'intuition « je tourne la
    /// tête, je la lève, je la penche ». Puis on passe aux signes utilisateur.
    pub fn from_rotation(r: &Rotation3<f64>) -> Self {
        let m = r.matrix();
        let b = (-m[(1, 2)]).clamp(-1.0, 1.0).asin();
        let a = m[(0, 2)].atan2(m[(2, 2)]);
        let c = m[(1, 0)].atan2(m[(1, 1)]);
        Self {
            // a > 0 : le nez part vers +x caméra = vers la gauche de la personne.
            yaw: -a.to_degrees(),
            // b > 0 : le nez part vers le bas.
            pitch: -b.to_degrees(),
            // c > 0 : le côté gauche de la tête monte = penché vers la droite.
            roll: c.to_degrees(),
        }
    }

    /// Opération inverse (pour les tests et l'affichage).
    pub fn to_rotation(self) -> Rotation3<f64> {
        let (a, b, c) = (
            -self.yaw.to_radians(),
            -self.pitch.to_radians(),
            self.roll.to_radians(),
        );
        Rotation3::from_axis_angle(&Vector3::y_axis(), a)
            * Rotation3::from_axis_angle(&Vector3::x_axis(), b)
            * Rotation3::from_axis_angle(&Vector3::z_axis(), c)
    }
}

/// Pose 6DOF dans le repère utilisateur : angles en degrés, positions en cm.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose6 {
    pub angles: Angles,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Résultat de l'estimation de pose pour une image.
#[derive(Debug, Clone, Copy)]
pub struct HeadPose {
    /// Rotation de la tête dans le repère caméra, perspective corrigée.
    pub rotation: Rotation3<f64>,
    /// Position du centre de la tête dans le repère caméra, en cm.
    pub position: Vector3<f64>,
    /// Ajustement dans le plan image (pour dessiner les axes sur la vidéo).
    pub image_fit: Similarity,
    /// Erreur moyenne de reprojection des points retenus, en pixels.
    pub residual_px: f64,
}

impl HeadPose {
    /// Pose absolue, dans le repère utilisateur.
    pub fn absolute(&self) -> Pose6 {
        to_user(&self.rotation, &self.position)
    }

    /// Pose relative à une pose neutre (« recentrage »).
    pub fn relative_to(&self, neutral: &HeadPose) -> Pose6 {
        // Rotation effectuée depuis la pose neutre, exprimée dans le repère caméra.
        let delta = self.rotation * neutral.rotation.inverse();
        to_user(&delta, &(self.position - neutral.position))
    }

    /// Position à l'écran (pixels image) d'un point du modèle, en cm.
    pub fn project_model_point(&self, p: Vector3<f64>) -> (f32, f32) {
        let q = self.image_fit.apply(&p);
        (q.x as f32, -q.y as f32)
    }
}

/// Repère caméra → repère utilisateur : la caméra fait face à l'utilisateur,
/// donc la droite de l'image est sa gauche, et « reculer » est vers -z caméra.
fn to_user(rotation: &Rotation3<f64>, position: &Vector3<f64>) -> Pose6 {
    Pose6 {
        angles: Angles::from_rotation(rotation),
        x: -position.x,
        y: position.y,
        z: -position.z,
    }
}

/// Part des points conservés au second passage (les autres sont considérés
/// comme aberrants : bouche, sourcils, points mal estimés).
const INLIER_RATIO: f64 = 0.75;

/// Estime la pose de la tête à partir des points du visage.
pub fn estimate(landmarks: &FaceLandmarks, camera: &CameraModel) -> Option<HeadPose> {
    let n = CANONICAL_FACE.len();
    if landmarks.points.len() < n {
        return None;
    }
    let src: Vec<Vector3<f64>> = CANONICAL_FACE
        .iter()
        .map(|p| Vector3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
        .collect();
    // Points image → repère C : y vers le haut, z vers la caméra
    // (MediaPipe donne z < 0 pour ce qui est proche de la caméra).
    let dst: Vec<Vector3<f64>> = landmarks.points[..n]
        .iter()
        .map(|p| Vector3::new(f64::from(p[0]), -f64::from(p[1]), -f64::from(p[2])))
        .collect();

    // 1er passage : tous les points.
    let mut weights = vec![1.0; n];
    let first = umeyama(&src, &dst, &weights)?;

    // 2e passage : on ne garde que les points les mieux alignés.
    let residuals: Vec<f64> = residuals_2d(&first, &src, &dst);
    let mut sorted = residuals.clone();
    sorted.sort_by(f64::total_cmp);
    let cutoff = sorted[((n as f64 * INLIER_RATIO) as usize).min(n - 1)];
    for (w, r) in weights.iter_mut().zip(&residuals) {
        *w = if *r <= cutoff { 1.0 } else { 0.0 };
    }
    let fit = umeyama(&src, &dst, &weights)?;
    let kept = residuals_2d(&fit, &src, &dst)
        .iter()
        .zip(&weights)
        .filter(|(_, w)| **w > 0.0)
        .map(|(r, _)| *r)
        .collect::<Vec<_>>();
    let residual_px = kept.iter().sum::<f64>() / kept.len().max(1) as f64;

    // Distance : à la profondeur Z, 1 cm mesure f / Z pixels ⇒ Z = f / échelle.
    let f = camera.focal_px();
    let (cx, cy) = camera.center();
    let depth = f / fit.scale;
    // Position du centre de la tête : on « remonte » le pixel de l'origine du modèle.
    let (u, v) = (fit.translation.x, -fit.translation.y);
    let position = Vector3::new((u - cx) * depth / f, (cy - v) * depth / f, -depth);

    // Correction de perspective : l'alignement donne l'orientation *par rapport
    // à la ligne de visée*. On la ramène au repère caméra en tournant l'axe z
    // vers la direction tête → caméra.
    let to_camera = Unit::new_normalize(-position);
    let line_of_sight =
        Rotation3::rotation_between(&Vector3::z(), &to_camera).unwrap_or_else(Rotation3::identity);

    Some(HeadPose {
        rotation: line_of_sight * fit.rotation,
        position,
        image_fit: fit,
        residual_px,
    })
}

/// Écart dans le plan image (x, y) entre chaque point mesuré et le modèle ajusté.
/// On ignore z : la profondeur estimée par le réseau est bien moins fiable.
fn residuals_2d(fit: &Similarity, src: &[Vector3<f64>], dst: &[Vector3<f64>]) -> Vec<f64> {
    src.iter()
        .zip(dst)
        .map(|(s, d)| (fit.apply(s) - d).xy().norm())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMERA: CameraModel = CameraModel {
        width: 1920.0,
        height: 1080.0,
        hfov_deg: 70.0,
    };

    /// Visage synthétique : modèle canonique placé à une pose connue, puis
    /// « vu » par la caméra comme le ferait MediaPipe (projection faible perspective).
    fn synthetic(angles: Angles, position: Vector3<f64>) -> FaceLandmarks {
        let f = CAMERA.focal_px();
        let depth = -position.z;
        let scale = f / depth;
        // Rotation apparente = rotation réelle vue depuis la ligne de visée.
        let los = Rotation3::rotation_between(
            &Vector3::z(),
            &Unit::new_normalize(-position).into_inner(),
        )
        .unwrap();
        let apparent = los.inverse() * angles.to_rotation();
        let u0 = 960.0 + position.x * f / depth;
        let v0 = 540.0 - position.y * f / depth;
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

    fn assert_close(a: f64, b: f64, tol: f64, what: &str) {
        assert!(
            (a - b).abs() < tol,
            "{what} : obtenu {a:.3}, attendu {b:.3}"
        );
    }

    #[test]
    fn angles_aller_retour() {
        let a = Angles {
            yaw: 30.0,
            pitch: -15.0,
            roll: 10.0,
        };
        let b = Angles::from_rotation(&a.to_rotation());
        assert_close(b.yaw, 30.0, 1e-9, "lacet");
        assert_close(b.pitch, -15.0, 1e-9, "tangage");
        assert_close(b.roll, 10.0, 1e-9, "roulis");
    }

    #[test]
    fn signes_intuitifs() {
        // Tête tournée vers sa droite : le nez part vers la gauche de l'image (−x caméra).
        let r = Angles {
            yaw: 20.0,
            pitch: 0.0,
            roll: 0.0,
        }
        .to_rotation();
        assert!((r * Vector3::z()).x < 0.0);
        // Tête levée : le nez monte (+y).
        let r = Angles {
            yaw: 0.0,
            pitch: 20.0,
            roll: 0.0,
        }
        .to_rotation();
        assert!((r * Vector3::z()).y > 0.0);
    }

    #[test]
    fn umeyama_retrouve_une_similitude() {
        let src: Vec<_> = CANONICAL_FACE
            .iter()
            .map(|p| Vector3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
            .collect();
        let truth = Similarity {
            scale: 12.5,
            rotation: Angles {
                yaw: 25.0,
                pitch: 10.0,
                roll: -5.0,
            }
            .to_rotation(),
            translation: Vector3::new(300.0, -200.0, 40.0),
        };
        let dst: Vec<_> = src.iter().map(|p| truth.apply(p)).collect();
        let fit = umeyama(&src, &dst, &vec![1.0; src.len()]).unwrap();
        assert_close(fit.scale, 12.5, 1e-9, "échelle");
        // Pas `angle_to` : pour deux rotations quasi identiques, le cosinus
        // calculé dépasse 1 d'un cheveu et `acos` renvoie NaN.
        let err = (fit.rotation.matrix() - truth.rotation.matrix()).norm();
        assert!(err < 1e-9, "écart de rotation {err:e}");
        assert!((fit.translation - truth.translation).norm() < 1e-6);
    }

    #[test]
    fn pose_de_face_au_centre() {
        let lm = synthetic(Angles::default(), Vector3::new(0.0, 0.0, -60.0));
        let pose = estimate(&lm, &CAMERA).unwrap().absolute();
        assert_close(pose.angles.yaw, 0.0, 1e-6, "lacet");
        assert_close(pose.z, 60.0, 1e-6, "distance");
        assert_close(pose.x, 0.0, 1e-6, "x");
    }

    #[test]
    fn pose_quelconque_decalee() {
        // Tête à 15 cm sur la gauche de l'image, 5 cm plus haut, à 70 cm.
        let angles = Angles {
            yaw: 25.0,
            pitch: -10.0,
            roll: 8.0,
        };
        let lm = synthetic(angles, Vector3::new(-15.0, 5.0, -70.0));
        let pose = estimate(&lm, &CAMERA).unwrap().absolute();
        assert_close(pose.angles.yaw, 25.0, 1e-4, "lacet");
        assert_close(pose.angles.pitch, -10.0, 1e-4, "tangage");
        assert_close(pose.angles.roll, 8.0, 1e-4, "roulis");
        // Gauche de l'image = droite de l'utilisateur.
        assert_close(pose.x, 15.0, 1e-4, "x");
        assert_close(pose.y, 5.0, 1e-4, "y");
        assert_close(pose.z, 70.0, 1e-4, "z");
    }

    #[test]
    fn robuste_a_une_bouche_qui_s_ouvre() {
        let angles = Angles {
            yaw: -12.0,
            pitch: 6.0,
            roll: 0.0,
        };
        let mut lm = synthetic(angles, Vector3::new(0.0, 0.0, -60.0));
        // On déplace de 30 px vers le bas 15 % des points (≈ bouche / menton).
        for p in lm.points.iter_mut().filter(|p| p[1] > 600.0).take(70) {
            p[1] += 30.0;
        }
        let pose = estimate(&lm, &CAMERA).unwrap().absolute();
        assert_close(pose.angles.yaw, -12.0, 0.5, "lacet");
        assert_close(pose.angles.pitch, 6.0, 0.5, "tangage");
    }

    #[test]
    fn recentrage() {
        let neutral = estimate(
            &synthetic(
                Angles {
                    yaw: 5.0,
                    ..Default::default()
                },
                Vector3::new(0.0, 0.0, -60.0),
            ),
            &CAMERA,
        )
        .unwrap();
        let now = estimate(
            &synthetic(
                Angles {
                    yaw: 35.0,
                    ..Default::default()
                },
                Vector3::new(0.0, 0.0, -50.0),
            ),
            &CAMERA,
        )
        .unwrap();
        let rel = now.relative_to(&neutral);
        assert_close(rel.angles.yaw, 30.0, 1e-4, "lacet relatif");
        assert_close(rel.z, -10.0, 1e-4, "s'approcher = z négatif");
    }
}
