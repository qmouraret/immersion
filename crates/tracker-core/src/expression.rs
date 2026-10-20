//! Correction de la pose selon l'expression du visage.
//!
//! Le modèle de points décale la pose quand l'expression change, tête immobile
//! (mesuré : tangage +1,5° yeux fermés, +4° bouche ouverte, +3° sourcils levés).
//! On mesure l'expression par trois rapports de distances, on apprend pendant
//! l'étalonnage comment ils décalent chaque axe (régression linéaire), puis on
//! retranche ce décalage.

use nalgebra::{Matrix4, Vector4};

use crate::FaceLandmarks;

/// Nombre de mesures d'expression.
pub const NUM_FEATURES: usize = 3;
/// Noms des mesures, dans l'ordre de [`features`].
pub const FEATURE_NAMES: [&str; NUM_FEATURES] = ["bouche", "sourcils", "yeux"];

/// Un axe n'est corrigé que si la correction réduit son écart d'au moins 10 %.
const MIN_GAIN: f64 = 0.9;
/// La correction est bornée à 1,5 × la plus grande vue pendant l'étalonnage.
const MAX_CORRECTION_FACTOR: f64 = 1.5;

/// Mesure l'expression : [ouverture de la bouche, hauteur des sourcils,
/// ouverture des yeux], chacune rapportée à la hauteur du visage (front → menton)
/// pour ne pas dépendre de la distance à la caméra.
pub fn features(lm: &FaceLandmarks) -> Option<[f64; NUM_FEATURES]> {
    let p = &lm.points;
    if p.len() < 468 {
        return None;
    }
    let d = |a: usize, b: usize| f64::from(p[a][0] - p[b][0]).hypot(f64::from(p[a][1] - p[b][1]));
    let height = d(10, 152);
    if height < 1.0 {
        return None;
    }
    Some([
        d(13, 14) / height,                         // lèvres intérieures haut / bas
        (d(105, 159) + d(334, 386)) / 2.0 / height, // sourcil → paupière supérieure
        (d(159, 145) + d(386, 374)) / 2.0 / height, // paupière supérieure → inférieure
    ])
}

/// Correction apprise : `pose corrigée = pose − c · (expression − expression neutre)`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExpressionModel {
    /// Expression du visage au repos (médiane du début de la capture).
    pub neutral: [f64; NUM_FEATURES],
    /// Coefficients par axe ([lacet, tangage, roulis, x, y, z]) ; une ligne nulle = axe non corrigé.
    pub coeffs: [[f64; NUM_FEATURES]; 6],
    /// Correction maximale par axe (° ou cm).
    pub max_correction: [f64; 6],
}

impl ExpressionModel {
    /// Décalage estimé de chaque axe pour cette expression.
    pub fn offset(&self, features: &[f64; NUM_FEATURES]) -> [f64; 6] {
        std::array::from_fn(|axis| {
            let raw: f64 = (0..NUM_FEATURES)
                .map(|k| self.coeffs[axis][k] * (features[k] - self.neutral[k]))
                .sum();
            raw.clamp(-self.max_correction[axis], self.max_correction[axis])
        })
    }

    /// Pose corrigée.
    pub fn correct(&self, pose: [f64; 6], features: &[f64; NUM_FEATURES]) -> [f64; 6] {
        let off = self.offset(features);
        std::array::from_fn(|i| pose[i] - off[i])
    }
}

/// Résultat d'un apprentissage.
#[derive(Debug, Clone, PartialEq)]
pub struct FitReport {
    /// Le modèle appris.
    pub model: ExpressionModel,
    /// Écart (RMS) de chaque axe autour de sa moyenne, avant correction.
    pub before: [f64; 6],
    /// Même mesure après correction.
    pub after: [f64; 6],
}

/// Apprend la correction à partir d'une capture **tête immobile** pendant
/// laquelle l'utilisateur change d'expression.
///
/// `samples` : (expression, pose relative) par image. Les `neutral_count`
/// premières images servent de référence « visage au repos ».
pub fn fit(samples: &[([f64; NUM_FEATURES], [f64; 6])], neutral_count: usize) -> Option<FitReport> {
    if samples.len() < 30 {
        return None;
    }
    let neutral_count = neutral_count.clamp(1, samples.len());
    let neutral: [f64; NUM_FEATURES] = std::array::from_fn(|k| {
        let mut v: Vec<f64> = samples[..neutral_count].iter().map(|(f, _)| f[k]).collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    });

    // Écart à l'expression neutre, pour chaque image.
    let centered: Vec<[f64; NUM_FEATURES]> = samples
        .iter()
        .map(|(f, _)| std::array::from_fn(|k| f[k] - neutral[k]))
        .collect();

    // Équations normales (constante + 3 mesures), avec une petite régularisation
    // sur les 3 coefficients : AᵀA·c = Aᵀ·y.
    let mut ata = Matrix4::<f64>::zeros();
    for f in &centered {
        let a = Vector4::new(1.0, f[0], f[1], f[2]);
        ata += a * a.transpose();
    }
    let scale = ata.diagonal().iter().skip(1).sum::<f64>() / 3.0;
    for k in 1..4 {
        ata[(k, k)] += 1e-3 * scale.max(f64::EPSILON);
    }
    let lu = ata.lu();

    let mut coeffs = [[0.0; NUM_FEATURES]; 6];
    let mut max_correction = [0.0; 6];
    let mut before = [0.0; 6];
    let mut after = [0.0; 6];
    for axis in 0..6 {
        let y: Vec<f64> = samples.iter().map(|(_, p)| p[axis]).collect();
        let mut aty = Vector4::zeros();
        for (f, yi) in centered.iter().zip(&y) {
            aty += Vector4::new(1.0, f[0], f[1], f[2]) * *yi;
        }
        let Some(c) = lu.solve(&aty) else { continue };
        let c_axis = [c[1], c[2], c[3]];

        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let corrections: Vec<f64> = centered
            .iter()
            .map(|f| (0..NUM_FEATURES).map(|k| c_axis[k] * f[k]).sum())
            .collect();
        let residual_mean =
            y.iter().zip(&corrections).map(|(v, k)| v - k).sum::<f64>() / y.len() as f64;
        before[axis] = rms(y.iter().map(|v| v - mean));
        let corrected = rms(y
            .iter()
            .zip(&corrections)
            .map(|(v, k)| v - k - residual_mean));

        if corrected < MIN_GAIN * before[axis] {
            coeffs[axis] = c_axis;
            max_correction[axis] =
                MAX_CORRECTION_FACTOR * corrections.iter().fold(0.0f64, |m, k| m.max(k.abs()));
            after[axis] = corrected;
        } else {
            after[axis] = before[axis]; // pas de correction pour cet axe
        }
    }
    Some(FitReport {
        model: ExpressionModel {
            neutral,
            coeffs,
            max_correction,
        },
        before,
        after,
    })
}

fn rms(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v * v, n + 1));
    if n == 0 { 0.0 } else { (sum / n as f64).sqrt() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Capture synthétique : 3 s de repos, puis bouche (décale le tangage de
    /// +20°/unité) et sourcils (+30°/unité), avec un peu de bruit.
    fn capture() -> Vec<([f64; 3], [f64; 6])> {
        (0..450)
            .map(|i| {
                let t = i as f64 / 30.0;
                let mouth = if (5.0..8.0).contains(&t) { 0.15 } else { 0.0 };
                let brow = if (10.0..12.0).contains(&t) { 0.1 } else { 0.0 };
                let noise = if i % 2 == 0 { 0.1 } else { -0.1 };
                let features = [0.01 + mouth, 0.11 + brow, 0.05];
                let mut pose = [noise; 6];
                pose[1] += 20.0 * mouth + 30.0 * brow;
                (features, pose)
            })
            .collect()
    }

    #[test]
    fn apprend_les_decalages_connus() {
        let report = fit(&capture(), 90).unwrap();
        let c = report.model.coeffs[1];
        assert!((c[0] - 20.0).abs() < 0.5, "bouche → tangage : {}", c[0]);
        assert!((c[1] - 30.0).abs() < 0.5, "sourcils → tangage : {}", c[1]);
        assert!(report.after[1] < report.before[1] / 5.0);
    }

    #[test]
    fn n_ajoute_rien_aux_axes_non_concernes() {
        let report = fit(&capture(), 90).unwrap();
        assert_eq!(
            report.model.coeffs[0], [0.0; 3],
            "le lacet n'est pas influencé"
        );
        assert_eq!(report.after[0], report.before[0]);
    }

    #[test]
    fn la_correction_annule_le_decalage() {
        let model = fit(&capture(), 90).unwrap().model;
        let open_mouth = [0.16, 0.11, 0.05];
        let mut pose = [0.0; 6];
        pose[1] = 3.0; // 0,15 × 20°
        let corrected = model.correct(pose, &open_mouth);
        assert!(
            corrected[1].abs() < 0.1,
            "tangage corrigé : {}",
            corrected[1]
        );
    }

    #[test]
    fn la_correction_est_bornee() {
        let model = fit(&capture(), 90).unwrap().model;
        let absurd = [5.0, 0.11, 0.05]; // bouche « ouverte » 30× plus que pendant l'étalonnage
        let off = model.offset(&absurd);
        assert!(off[1] <= model.max_correction[1] + 1e-12);
        assert!(model.max_correction[1] < 10.0);
    }

    #[test]
    fn trop_peu_d_images() {
        assert!(fit(&capture()[..10], 5).is_none());
    }
}
