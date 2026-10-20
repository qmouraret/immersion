//! Décodage des sorties du détecteur BlazeFace (MediaPipe, « short range », 128×128).
//!
//! Le modèle prédit, pour chacune des 896 ancres (positions de référence
//! réparties en grille), un décalage de boîte + 6 points clés, et un score.
//! Ici, on reconstruit les boîtes et on fusionne celles qui se chevauchent.

/// Taille d'entrée du détecteur.
pub const INPUT_SIZE: usize = 128;
/// Nombre de valeurs par ancre : boîte (4) + 6 points clés × (x, y).
pub const NUM_COORDS: usize = 16;
const NUM_KEYPOINTS: usize = 6;
const MIN_SCORE: f32 = 0.5;
const MIN_SUPPRESSION_IOU: f32 = 0.3;

/// Indices des points clés de BlazeFace.
pub mod keypoint {
    /// Œil droit de la personne (à gauche dans une image non mirroir).
    pub const RIGHT_EYE: usize = 0;
    /// Œil gauche de la personne.
    pub const LEFT_EYE: usize = 1;
    /// Bout du nez.
    pub const NOSE_TIP: usize = 2;
    /// Centre de la bouche.
    pub const MOUTH: usize = 3;
}

/// Un visage détecté, en coordonnées normalisées [0, 1] de l'entrée du modèle.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// Probabilité (sigmoïde du logit), entre 0 et 1.
    pub score: f32,
    /// Boîte : centre et taille.
    pub cx: f32,
    /// Ordonnée du centre de la boîte.
    pub cy: f32,
    /// Largeur de la boîte.
    pub w: f32,
    /// Hauteur de la boîte.
    pub h: f32,
    /// Points clés (x, y), dans l'ordre du module [`keypoint`].
    pub keypoints: [(f32, f32); NUM_KEYPOINTS],
}

impl Detection {
    fn iou(&self, other: &Self) -> f32 {
        let (ax0, ax1, ay0, ay1) = self.edges();
        let (bx0, bx1, by0, by1) = other.edges();
        let iw = (ax1.min(bx1) - ax0.max(bx0)).max(0.0);
        let ih = (ay1.min(by1) - ay0.max(by0)).max(0.0);
        let inter = iw * ih;
        let union = self.w * self.h + other.w * other.h - inter;
        if union <= 0.0 { 0.0 } else { inter / union }
    }

    fn edges(&self) -> (f32, f32, f32, f32) {
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        (self.cx - hw, self.cx + hw, self.cy - hh, self.cy + hh)
    }
}

/// Centres des ancres, dans l'ordre des sorties du modèle.
///
/// Grille 16×16 avec 2 ancres par case (pas de 8 px), puis 8×8 avec 6 ancres
/// (les 3 couches de pas 16 sont regroupées) : 512 + 384 = 896.
pub fn anchors() -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(896);
    for (grid, per_cell) in [(16usize, 2usize), (8, 6)] {
        for y in 0..grid {
            for x in 0..grid {
                let c = (
                    (x as f32 + 0.5) / grid as f32,
                    (y as f32 + 0.5) / grid as f32,
                );
                out.extend(std::iter::repeat_n(c, per_cell));
            }
        }
    }
    out
}

/// Décode les sorties brutes et renvoie le visage le plus probable.
///
/// - `regressors` : `[896 × 16]` décalages en pixels de l'entrée 128×128
/// - `scores` : `[896]` logits (avant sigmoïde)
///
/// On ne garde qu'un visage (celui du pilote), mais on applique quand même
/// la suppression pondérée de MediaPipe : les détections voisines du meilleur
/// candidat sont moyennées, ce qui stabilise nettement la boîte.
pub fn decode(regressors: &[f32], scores: &[f32], anchors: &[(f32, f32)]) -> Option<Detection> {
    let scale = INPUT_SIZE as f32;
    let mut candidates: Vec<Detection> = anchors
        .iter()
        .zip(scores)
        // `as_chunks::<16>` : des `&[f32; 16]` de taille connue à la compilation,
        // ce qui supprime les vérifications de bornes sur `r[i]`.
        .zip(regressors.as_chunks::<NUM_COORDS>().0)
        .filter_map(|((&(ax, ay), &logit), r)| {
            let score = sigmoid(logit.clamp(-100.0, 100.0));
            (score >= MIN_SCORE).then(|| Detection {
                score,
                cx: r[0] / scale + ax,
                cy: r[1] / scale + ay,
                w: r[2] / scale,
                h: r[3] / scale,
                keypoints: std::array::from_fn(|k| {
                    (r[4 + 2 * k] / scale + ax, r[5 + 2 * k] / scale + ay)
                }),
            })
        })
        .collect();

    // Le plus haut score d'abord. `total_cmp` : ordre total sur les f32 (gère NaN).
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let best = candidates.first()?.clone();

    // Moyenne pondérée par le score de toutes les détections qui recouvrent la meilleure.
    let cluster: Vec<&Detection> = candidates
        .iter()
        .filter(|d| best.iou(d) >= MIN_SUPPRESSION_IOU)
        .collect();
    let total: f32 = cluster.iter().map(|d| d.score).sum();
    let avg =
        |f: &dyn Fn(&Detection) -> f32| cluster.iter().map(|d| f(d) * d.score).sum::<f32>() / total;

    Some(Detection {
        score: best.score,
        cx: avg(&|d| d.cx),
        cy: avg(&|d| d.cy),
        w: avg(&|d| d.w),
        h: avg(&|d| d.h),
        keypoints: std::array::from_fn(|k| {
            (avg(&|d| d.keypoints[k].0), avg(&|d| d.keypoints[k].1))
        }),
    })
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_y_a_896_ancres() {
        let a = anchors();
        assert_eq!(a.len(), 896);
        assert_eq!(a[0], (0.5 / 16.0, 0.5 / 16.0));
        assert_eq!(a[512], (0.5 / 8.0, 0.5 / 8.0));
    }

    #[test]
    fn decode_une_detection_isolee() {
        let anchors = anchors();
        let mut regs = vec![0.0; 896 * NUM_COORDS];
        let mut scores = vec![-10.0; 896];
        // Ancre 600, boîte de 64×64 px décalée de +8 px en x.
        let i = 600;
        scores[i] = 5.0;
        regs[i * NUM_COORDS..i * NUM_COORDS + 4].copy_from_slice(&[8.0, 0.0, 64.0, 64.0]);

        let d = decode(&regs, &scores, &anchors).unwrap();
        assert!((d.cx - (anchors[i].0 + 8.0 / 128.0)).abs() < 1e-6);
        assert!((d.w - 0.5).abs() < 1e-6);
        assert!(d.score > 0.99);
    }

    #[test]
    fn rien_sous_le_seuil() {
        let anchors = anchors();
        let regs = vec![0.0; 896 * NUM_COORDS];
        let scores = vec![-1.0; 896];
        assert!(decode(&regs, &scores, &anchors).is_none());
    }

    #[test]
    fn les_detections_voisines_sont_moyennees() {
        let anchors = anchors();
        let mut regs = vec![0.0; 896 * NUM_COORDS];
        let mut scores = vec![-10.0; 896];
        // Deux ancres identiques (même case), boîtes de 64 px, l'une décalée de 4 px.
        for (i, dx) in [(512, 0.0), (513, 4.0)] {
            scores[i] = 3.0;
            regs[i * NUM_COORDS..i * NUM_COORDS + 4].copy_from_slice(&[dx, 0.0, 64.0, 64.0]);
        }
        let d = decode(&regs, &scores, &anchors).unwrap();
        // Scores égaux → moyenne simple : décalage de 2 px.
        assert!((d.cx - (anchors[512].0 + 2.0 / 128.0)).abs() < 1e-6);
    }
}
