//! Filtre One-Euro (Casiez, Roussel & Vogel, CHI 2012).
//!
//! Un passe-bas dont la fréquence de coupure dépend de la vitesse :
//! - à l'arrêt, coupure basse (`min_cutoff`) → fort lissage, la gigue disparaît ;
//! - en mouvement, la coupure monte (`beta` × vitesse) → presque pas de retard.
//!
//! Réglage : augmenter `min_cutoff` si la vue « traîne » à l'arrêt, augmenter
//! `beta` si elle est en retard pendant les mouvements rapides.

use std::f64::consts::TAU;

/// Paramètres du filtre.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OneEuroParams {
    /// Fréquence de coupure à l'arrêt, en Hz.
    pub min_cutoff: f64,
    /// Augmentation de la coupure par unité de vitesse (Hz par unité/s).
    pub beta: f64,
    /// Coupure du filtre appliqué à la vitesse elle-même, en Hz.
    pub d_cutoff: f64,
}

/// Passe-bas du premier ordre, coefficient calculé selon `dt`.
fn alpha(cutoff: f64, dt: f64) -> f64 {
    let tau = 1.0 / (TAU * cutoff);
    1.0 / (1.0 + tau / dt)
}

/// État du filtre pour une seule grandeur.
#[derive(Debug, Clone, Default)]
pub struct OneEuro {
    /// (valeur filtrée, vitesse filtrée) après la dernière mesure.
    state: Option<(f64, f64)>,
}

impl OneEuro {
    /// Filtre la mesure `x` arrivée `dt` secondes après la précédente.
    pub fn filter(&mut self, x: f64, dt: f64, p: &OneEuroParams) -> f64 {
        let Some((prev, prev_speed)) = self.state else {
            self.state = Some((x, 0.0));
            return x;
        };
        if dt <= 0.0 {
            return prev; // même instant (ou horloge incohérente) : on ne bouge pas
        }
        let speed = (x - prev) / dt;
        let speed = prev_speed + alpha(p.d_cutoff, dt) * (speed - prev_speed);
        let cutoff = p.min_cutoff + p.beta * speed.abs();
        let value = prev + alpha(cutoff, dt) * (x - prev);
        self.state = Some((value, speed));
        value
    }

    /// Oublie l'historique : la prochaine mesure est reprise telle quelle.
    pub fn reset(&mut self) {
        self.state = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: OneEuroParams = OneEuroParams {
        min_cutoff: 0.5,
        beta: 0.05,
        d_cutoff: 1.0,
    };
    const DT: f64 = 1.0 / 30.0;

    /// Bruit déterministe ±1 (pas de crate `rand` nécessaire).
    fn noise(i: usize) -> f64 {
        if (i * 7919) % 13 < 6 { 1.0 } else { -1.0 }
    }

    fn std_dev(v: &[f64]) -> f64 {
        let m = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn la_premiere_mesure_passe_telle_quelle() {
        let mut f = OneEuro::default();
        assert_eq!(f.filter(42.0, DT, &P), 42.0);
    }

    #[test]
    fn reduit_fortement_le_bruit_a_l_arret() {
        let mut f = OneEuro::default();
        let out: Vec<f64> = (0..300).map(|i| f.filter(0.3 * noise(i), DT, &P)).collect();
        let (raw, filtered) = (0.3, std_dev(&out[60..]));
        assert!(
            filtered < raw / 5.0,
            "gigue filtrée {filtered:.3} (brute {raw})"
        );
    }

    #[test]
    fn suit_un_mouvement_rapide_sans_trop_de_retard() {
        // Rotation de la tête à 120 °/s pendant 0,5 s.
        let mut f = OneEuro::default();
        let mut last = 0.0;
        for i in 0..=15 {
            last = f.filter(120.0 * i as f64 * DT, DT, &P);
        }
        let truth = 120.0 * 15.0 * DT; // 60°
        let lag = truth - last;
        assert!(
            lag < 6.0,
            "retard {lag:.1}° (soit {:.0} ms)",
            lag / 120.0 * 1000.0
        );
    }

    #[test]
    fn converge_vers_une_nouvelle_position() {
        let mut f = OneEuro::default();
        f.filter(0.0, DT, &P);
        let mut v = 0.0;
        for _ in 0..90 {
            v = f.filter(10.0, DT, &P);
        }
        assert!((v - 10.0).abs() < 0.1);
    }
}
