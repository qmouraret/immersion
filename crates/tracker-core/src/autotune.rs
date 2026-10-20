//! Réglage automatique d'un filtre (rotations ou translations) sur un enregistrement.
//!
//! On rejoue la session pour une grille de (coupure min., β, coupure de
//! vitesse) et on garde le réglage qui **tremble le moins en sortie** (après
//! les courbes : c'est ce que l'on voit) tout en restant sous un **retard** maximal.

use crate::filter::OneEuroParams;
use crate::processing::TrackingSettings;
use crate::recording::{RawSample, replay};
use crate::stats::{active_still_roughness, estimate_lag, still_roughness};

/// Fenêtre de la rugosité « à l'arrêt » : 15 images ≈ 0,5 s.
const STILL_WINDOW: usize = 15;

/// Groupe d'axes dont on règle le filtre (chacun a le sien dans [`TrackingSettings`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisGroup {
    /// Lacet, tangage, roulis (degrés).
    Rotation,
    /// X, Y, Z (cm).
    Translation,
}

impl AxisGroup {
    fn axes(self) -> [usize; 3] {
        match self {
            Self::Rotation => [0, 1, 2],
            Self::Translation => [3, 4, 5],
        }
    }

    /// Amplitude minimale pour qu'un axe serve à mesurer le retard (° ou cm).
    fn min_motion(self) -> f64 {
        match self {
            Self::Rotation => 5.0,
            Self::Translation => 2.0,
        }
    }

    /// Plage de β explorée : une vitesse de tête typique est ~100 °/s mais ~20 cm/s.
    fn beta_range(self) -> (f64, f64) {
        match self {
            Self::Rotation => (0.002, 0.5),
            Self::Translation => (0.01, 3.0),
        }
    }

    /// Le filtre de ce groupe dans les réglages.
    pub fn filter(self, settings: &TrackingSettings) -> OneEuroParams {
        match self {
            Self::Rotation => settings.rotation_filter,
            Self::Translation => settings.position_filter,
        }
    }

    fn with_filter(self, base: &TrackingSettings, params: OneEuroParams) -> TrackingSettings {
        let mut s = TrackingSettings {
            filter_enabled: true,
            ..base.clone()
        };
        match self {
            Self::Rotation => s.rotation_filter = params,
            Self::Translation => s.position_filter = params,
        }
        s
    }
}

/// Résultat pour un réglage essayé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// Réglage essayé.
    pub params: OneEuroParams,
    /// Rugosité à l'arrêt de la sortie, hors zone morte, moyenne sur les
    /// axes du groupe qui ont des moments hors zone morte (° ou cm).
    pub roughness: f64,
    /// `false` si aucun axe du groupe n'est jamais sorti de la zone morte : la
    /// rugosité est alors celle de la pose filtrée (avant les courbes).
    pub measured_on_output: bool,
    /// Plus grand retard mesuré sur les axes qui ont bougé (s), `None` si
    /// l'enregistrement ne contient aucun mouvement suffisant.
    pub lag_s: Option<f64>,
}

/// Évalue un réglage du filtre de `group` sur l'enregistrement.
pub fn evaluate(
    samples: &[RawSample],
    base: &TrackingSettings,
    group: AxisGroup,
    params: OneEuroParams,
) -> Candidate {
    let processed = replay(samples, &group.with_filter(base, params));
    let dt = mean_dt(samples);
    let max_shift = dt.map_or(9, |dt| (0.3 / dt).round() as usize);

    let (mut rough_out, mut n_out) = (0.0, 0);
    let mut rough_filtered = 0.0;
    let mut lag: Option<f64> = None;
    for axis in group.axes() {
        let raw: Vec<f64> = samples.iter().map(|s| s.pose[axis]).collect();
        let filtered: Vec<f64> = processed
            .iter()
            .map(|p| p.filtered.to_array()[axis])
            .collect();
        let output: Vec<f64> = processed
            .iter()
            .map(|p| p.output.to_array()[axis])
            .collect();
        if let Some(r) = active_still_roughness(&output, STILL_WINDOW) {
            rough_out += r;
            n_out += 1;
        }
        rough_filtered += still_roughness(&filtered, STILL_WINDOW).unwrap_or(0.0);
        if let Some(k) = estimate_lag(&raw, &filtered, max_shift, group.min_motion()) {
            lag = Some(lag.map_or(k, |l: f64| l.max(k)));
        }
    }
    Candidate {
        params,
        roughness: if n_out > 0 {
            rough_out / f64::from(n_out)
        } else {
            rough_filtered / 3.0
        },
        measured_on_output: n_out > 0,
        lag_s: lag.zip(dt).map(|(k, dt)| k * dt),
    }
}

/// Essaie une grille de réglages pour `group` ; renvoie ceux qui respectent
/// `max_lag_s`, du moins rugueux au plus rugueux.
///
/// Si l'enregistrement ne contient aucun mouvement (retard non mesurable),
/// la contrainte de retard est ignorée : le résultat favorisera alors un
/// lissage excessif, à ne pas appliquer tel quel.
pub fn search(
    samples: &[RawSample],
    base: &TrackingSettings,
    group: AxisGroup,
    max_lag_s: f64,
) -> Vec<Candidate> {
    // Sur des données réelles, β et la coupure de vitesse comptent plus que la
    // coupure min. : la gigue fait croire à une vitesse et β relève la coupure ;
    // une coupure de vitesse haute repère plus tôt les vrais mouvements.
    let cutoffs = log_grid(0.05, 2.0, 10);
    let (b_lo, b_hi) = group.beta_range();
    let betas: Vec<f64> = std::iter::once(0.0)
        .chain(log_grid(b_lo, b_hi, 11))
        .collect();
    let d_cutoffs = log_grid(0.2, 3.0, 6);

    let mut out: Vec<Candidate> = cutoffs
        .iter()
        .flat_map(|&min_cutoff| betas.iter().map(move |&beta| (min_cutoff, beta)))
        .flat_map(|(min_cutoff, beta)| {
            d_cutoffs.iter().map(move |&d_cutoff| OneEuroParams {
                min_cutoff,
                beta,
                d_cutoff,
            })
        })
        .map(|p| evaluate(samples, base, group, p))
        .filter(|c| c.lag_s.is_none_or(|l| l <= max_lag_s))
        .collect();
    // Ordre total obligatoire : une comparaison « égaux à 1 % près » n'est pas
    // transitive (A ≈ B, B ≈ C, mais A ≠ C), et `sort_by` panique depuis
    // Rust 1.81 quand il le détecte. On trie donc par rugosité, puis par retard.
    let lag = |c: &Candidate| c.lag_s.unwrap_or(0.0);
    out.sort_by(|a, b| {
        a.roughness
            .total_cmp(&b.roughness)
            .then(lag(a).total_cmp(&lag(b)))
    });
    out
}

/// `n` valeurs réparties géométriquement entre `lo` et `hi` (inclus).
fn log_grid(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    let ratio = (hi / lo).powf(1.0 / (n - 1) as f64);
    (0..n).map(|i| lo * ratio.powi(i as i32)).collect()
}

fn mean_dt(samples: &[RawSample]) -> Option<f64> {
    let n = samples.len();
    (n >= 2)
        .then(|| (samples[n - 1].t - samples[0].t) / (n - 1) as f64)
        .filter(|dt| *dt > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4 s immobile (bruit ±0,3°), une rotation de 30° en 0,5 s, 4 s immobile.
    fn session() -> Vec<RawSample> {
        let noise = |i: usize| if (i * 7919) % 13 < 6 { 0.3 } else { -0.3 };
        (0..300)
            .map(|i| {
                let t = i as f64 / 30.0;
                let base = ((t - 4.0) / 0.5).clamp(0.0, 1.0) * 30.0;
                RawSample {
                    t,
                    pose: [base + noise(i), noise(i + 3), noise(i + 7), 0.0, 0.0, 0.0],
                }
            })
            .collect()
    }

    #[test]
    fn le_tri_ne_panique_pas_sur_des_valeurs_proches() {
        // Rugosités en escalier de 0,5 % : l'ancienne comparaison « à 1 % près »
        // n'était pas un ordre total et faisait paniquer `sort_by`.
        let mut v: Vec<f64> = (0..200).map(|i| 1.0 + 0.005 * f64::from(i % 37)).collect();
        v.reverse();
        let params = OneEuroParams {
            min_cutoff: 1.0,
            beta: 0.0,
            d_cutoff: 1.0,
        };
        let mut c: Vec<Candidate> = v
            .iter()
            .enumerate()
            .map(|(i, r)| Candidate {
                params,
                roughness: *r,
                measured_on_output: true,
                lag_s: Some(i as f64 * 1e-3),
            })
            .collect();
        let lag = |c: &Candidate| c.lag_s.unwrap_or(0.0);
        c.sort_by(|a, b| {
            a.roughness
                .total_cmp(&b.roughness)
                .then(lag(a).total_cmp(&lag(b)))
        });
        assert!(c.windows(2).all(|w| w[0].roughness <= w[1].roughness));
    }

    #[test]
    fn grille_logarithmique() {
        let g = log_grid(0.1, 10.0, 3);
        assert!(
            (g[0] - 0.1).abs() < 1e-12 && (g[1] - 1.0).abs() < 1e-9 && (g[2] - 10.0).abs() < 1e-9
        );
    }

    #[test]
    fn le_meilleur_reglage_respecte_le_retard() {
        let best = search(
            &session(),
            &TrackingSettings::default(),
            AxisGroup::Rotation,
            0.08,
        )[0];
        assert!(best.lag_s.unwrap() <= 0.08);
    }

    #[test]
    fn le_meilleur_tremble_moins_qu_un_filtre_presque_inactif() {
        let samples = session();
        let base = TrackingSettings::default();
        let best = search(&samples, &base, AxisGroup::Rotation, 0.1)[0];
        let loose = evaluate(
            &samples,
            &base,
            AxisGroup::Rotation,
            OneEuroParams {
                min_cutoff: 50.0,
                beta: 0.0,
                d_cutoff: 1.0,
            },
        );
        assert!(
            best.roughness < loose.roughness / 2.0,
            "{best:?} vs {loose:?}"
        );
    }

    #[test]
    fn un_retard_plus_tolere_permet_moins_de_tremblement() {
        let samples = session();
        let base = TrackingSettings::default();
        let strict = search(&samples, &base, AxisGroup::Rotation, 0.04)[0];
        let relaxed = search(&samples, &base, AxisGroup::Rotation, 0.15)[0];
        assert!(relaxed.roughness <= strict.roughness);
    }
}
