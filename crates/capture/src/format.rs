//! Choix du format vidéo parmi ceux que propose la caméra.

use nokhwa::utils::{CameraFormat, FrameFormat};

/// Ce que l'on souhaite obtenir de la caméra.
#[derive(Debug, Clone, Copy)]
pub struct FormatTarget {
    /// Largeur souhaitée, en pixels.
    pub width: u32,
    /// Hauteur souhaitée, en pixels.
    pub height: u32,
    /// Fréquence d'images minimale souhaitée.
    pub min_fps: u32,
}

impl Default for FormatTarget {
    /// 720p à 30 fps minimum : assez de détails pour une webcam éloignée,
    /// sans payer le coût de conversion du 1080p.
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            min_fps: 30,
        }
    }
}

/// Classe les formats de `available` du meilleur au moins bon, en ne gardant
/// que ceux que l'on sait décoder (`decodable`).
///
/// On renvoie une liste plutôt qu'un seul gagnant : certains backends
/// annoncent des formats qu'ils refusent ensuite d'appliquer (AVFoundation
/// n'accepte que la fréquence *maximale* de chaque plage). L'appelant essaie
/// donc les formats dans l'ordre jusqu'à ce que la caméra en accepte un.
///
/// Ordre de priorité :
/// 1. atteindre `min_fps` (la fluidité prime sur la définition) ;
/// 2. résolution la plus proche de la cible, en nombre de pixels ;
/// 3. fréquence d'images la plus haute ;
/// 4. éviter le MJPEG, qui demande une décompression coûteuse.
pub fn rank_formats(
    available: &[CameraFormat],
    decodable: &[FrameFormat],
    target: FormatTarget,
) -> Vec<CameraFormat> {
    let target_px = i64::from(target.width) * i64::from(target.height);

    let mut ranked: Vec<CameraFormat> = available
        .iter()
        .filter(|f| decodable.contains(&f.format()))
        .copied()
        .collect();
    // `sort_by_key` avec un tuple : comparaison champ par champ, dans l'ordre.
    // Le tri est stable : à égalité, l'ordre d'origine est conservé.
    ranked.sort_by_key(|f| {
        let px = i64::from(f.width()) * i64::from(f.height());
        (
            f.frame_rate() < target.min_fps, // false < true : les formats rapides d'abord
            (px - target_px).abs(),          // puis le plus proche de la cible
            std::cmp::Reverse(f.frame_rate()), // puis le plus rapide
            f.format() == FrameFormat::MJPEG, // puis les formats non compressés
        )
    });
    ranked.dedup();
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use nokhwa::utils::Resolution;

    fn fmt(w: u32, h: u32, fps: u32, ff: FrameFormat) -> CameraFormat {
        CameraFormat::new(Resolution::new(w, h), ff, fps)
    }

    const DECODABLE: &[FrameFormat] = &[FrameFormat::NV12, FrameFormat::YUYV, FrameFormat::MJPEG];

    #[test]
    fn prefere_720p_a_1080p() {
        let formats = [
            fmt(1920, 1080, 30, FrameFormat::NV12),
            fmt(1280, 720, 30, FrameFormat::NV12),
        ];
        let chosen = rank_formats(&formats, DECODABLE, FormatTarget::default())[0];
        assert_eq!(chosen.resolution(), Resolution::new(1280, 720));
    }

    #[test]
    fn la_fluidite_prime_sur_la_resolution() {
        let formats = [
            fmt(1280, 720, 10, FrameFormat::NV12),
            fmt(640, 480, 30, FrameFormat::NV12),
        ];
        let chosen = rank_formats(&formats, DECODABLE, FormatTarget::default())[0];
        assert_eq!(chosen.frame_rate(), 30);
    }

    #[test]
    fn evite_le_mjpeg_a_egalite() {
        let formats = [
            fmt(1280, 720, 30, FrameFormat::MJPEG),
            fmt(1280, 720, 30, FrameFormat::YUYV),
        ];
        let chosen = rank_formats(&formats, DECODABLE, FormatTarget::default())[0];
        assert_eq!(chosen.format(), FrameFormat::YUYV);
    }

    #[test]
    fn ignore_les_formats_non_decodables() {
        let formats = [fmt(1280, 720, 30, FrameFormat::RAWRGB)];
        assert!(rank_formats(&formats, &[FrameFormat::NV12], FormatTarget::default()).is_empty());
    }
}
