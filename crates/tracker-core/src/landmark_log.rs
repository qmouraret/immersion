//! Enregistrement des points du visage, image par image (format binaire).
//!
//! Sert à comparer hors ligne différentes façons de calculer la pose
//! (`examples/eval_pose`) sur exactement les mêmes points.
//!
//! Format (petit-boutiste) :
//! - en-tête : `b"IMLM"`, version (`u32` = 1), nombre de points par image (`u32`) ;
//! - puis, par image : `t` (`f64`, s), largeur, hauteur, FOV horizontal (°),
//!   présence (4 × `f32`), puis `n × (x, y, z)` en `f32`.

use std::fmt;

use crate::FaceLandmarks;

const MAGIC: &[u8; 4] = b"IMLM";
const VERSION: u32 = 1;

/// Les points d'une image et ce qu'il faut pour en recalculer la pose.
#[derive(Debug, Clone, PartialEq)]
pub struct LandmarkFrame {
    /// Instant, en secondes depuis le début de la session.
    pub t: f64,
    /// Largeur de l'image, en pixels.
    pub width: f32,
    /// Hauteur de l'image, en pixels.
    pub height: f32,
    /// FOV horizontal de la caméra utilisé à l'enregistrement, en degrés.
    pub hfov_deg: f32,
    /// Les points, comme renvoyés par le modèle (voir [`FaceLandmarks`]).
    pub landmarks: FaceLandmarks,
}

/// Erreur de lecture d'un fichier de points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogError(pub String);

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fichier de points invalide : {}", self.0)
    }
}

impl std::error::Error for LogError {}

/// Sérialise une session de points. Toutes les images doivent avoir le même
/// nombre de points (sinon la fonction panique : erreur de programmation).
pub fn to_bytes(frames: &[LandmarkFrame]) -> Vec<u8> {
    let n = frames.first().map_or(0, |f| f.landmarks.points.len());
    let mut out = Vec::with_capacity(12 + frames.len() * (24 + n * 12));
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for f in frames {
        assert_eq!(f.landmarks.points.len(), n, "nombre de points variable");
        out.extend_from_slice(&f.t.to_le_bytes());
        for v in [f.width, f.height, f.hfov_deg, f.landmarks.presence] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for p in &f.landmarks.points {
            for v in p {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

/// Lit une session de points.
pub fn from_bytes(bytes: &[u8]) -> Result<Vec<LandmarkFrame>, LogError> {
    let mut r = Reader { bytes, pos: 0 };
    if r.take(4)? != MAGIC {
        return Err(LogError("en-tête IMLM absent".into()));
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(LogError(format!("version {version} non gérée")));
    }
    let n = r.u32()? as usize;
    let mut frames = Vec::new();
    while r.pos < bytes.len() {
        let t = r.f64()?;
        let (width, height, hfov_deg, presence) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let points = (0..n)
            .map(|_| Ok([r.f32()?, r.f32()?, r.f32()?]))
            .collect::<Result<Vec<_>, LogError>>()?;
        frames.push(LandmarkFrame {
            t,
            width,
            height,
            hfov_deg,
            landmarks: FaceLandmarks { points, presence },
        });
    }
    Ok(frames)
}

/// Lecteur séquentiel : chaque lecture avance et vérifie qu'il reste assez d'octets.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], LogError> {
        let end = self.pos + n;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| LogError(format!("tronqué à l'octet {}", self.pos)))?;
        self.pos = end;
        Ok(slice)
    }

    /// `try_into` convertit la tranche en tableau de taille fixe, attendu par `from_le_bytes`.
    fn u32(&mut self) -> Result<u32, LogError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 octets"),
        ))
    }

    fn f32(&mut self) -> Result<f32, LogError> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("4 octets"),
        ))
    }

    fn f64(&mut self) -> Result<f64, LogError> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("8 octets"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: f64) -> LandmarkFrame {
        LandmarkFrame {
            t,
            width: 1920.0,
            height: 1080.0,
            hfov_deg: 66.0,
            landmarks: FaceLandmarks {
                points: vec![[1.5, -2.0, 0.25]; 478],
                presence: 0.98,
            },
        }
    }

    #[test]
    fn aller_retour() {
        let frames = vec![frame(0.0), frame(0.033)];
        assert_eq!(from_bytes(&to_bytes(&frames)).unwrap(), frames);
    }

    #[test]
    fn taille_attendue() {
        // 12 octets d'en-tête + par image 8 + 16 + 478 × 12.
        assert_eq!(to_bytes(&[frame(0.0)]).len(), 12 + 24 + 478 * 12);
    }

    #[test]
    fn fichier_tronque_detecte() {
        let bytes = to_bytes(&[frame(0.0)]);
        let err = from_bytes(&bytes[..bytes.len() - 5]).unwrap_err();
        assert!(err.to_string().contains("tronqué"));
    }

    #[test]
    fn mauvais_en_tete_detecte() {
        assert!(from_bytes(b"XXXX\x01\0\0\0").is_err());
    }
}
