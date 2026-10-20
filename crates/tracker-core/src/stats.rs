//! Statistiques glissantes, pour mesurer la gigue (bruit à l'arrêt).

use std::collections::VecDeque;

/// Moyenne et écart-type sur les `capacity` dernières valeurs.
#[derive(Debug, Clone)]
pub struct Rolling {
    capacity: usize,
    values: VecDeque<f64>,
}

impl Rolling {
    /// Fenêtre des `capacity` dernières valeurs (au moins 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            values: VecDeque::with_capacity(capacity),
        }
    }

    /// Ajoute une valeur ; la plus ancienne sort si la fenêtre est pleine.
    pub fn push(&mut self, v: f64) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(v);
    }

    /// Vide la fenêtre.
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Moyenne, ou `None` si la fenêtre est vide.
    pub fn mean(&self) -> Option<f64> {
        (!self.values.is_empty())
            .then(|| self.values.iter().sum::<f64>() / self.values.len() as f64)
    }

    /// Écart-type (population). `None` tant qu'il y a moins de 2 valeurs.
    pub fn std_dev(&self) -> Option<f64> {
        if self.values.len() < 2 {
            return None;
        }
        let mean = self.mean()?;
        let var =
            self.values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / self.values.len() as f64;
        Some(var.sqrt())
    }
}

/// Rugosité : moyenne quadratique de la différence seconde
/// `x[i+1] − 2·x[i] + x[i−1]`.
///
/// Contrairement à l'écart-type, elle ignore les mouvements réguliers (une
/// rampe a une différence seconde nulle) : il ne reste que le tremblement.
pub fn roughness(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let sum: f64 = values
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).powi(2))
        .sum();
    Some((sum / (values.len() - 2) as f64).sqrt())
}

/// Rugosité « à l'arrêt » : médiane des rugosités de fenêtres de `window`
/// échantillons.
///
/// Le tremblement se voit quand la tête est (presque) immobile. Sur un
/// enregistrement entier, les débuts et fins de mouvements rapides ont une
/// forte différence seconde et masqueraient le tremblement ; la médiane des
/// fenêtres les écarte tant qu'elles restent minoritaires.
pub fn still_roughness(values: &[f64], window: usize) -> Option<f64> {
    let window = window.max(3);
    let mut per_window: Vec<f64> = values.chunks(window).filter_map(roughness).collect();
    if per_window.is_empty() {
        return None;
    }
    per_window.sort_by(f64::total_cmp);
    Some(per_window[per_window.len() / 2])
}

/// Comme [`still_roughness`], mais seulement sur les fenêtres où le signal
/// n'est pas nul.
///
/// Pour la *sortie* (après les courbes) : au centre, la zone morte force 0 et
/// masque tout tremblement. Ce qui se voit, c'est le tremblement quand on
/// regarde sur le côté, hors de la zone morte. `None` s'il n'y a aucune
/// fenêtre de ce type.
pub fn active_still_roughness(values: &[f64], window: usize) -> Option<f64> {
    let window = window.max(3);
    let mut per_window: Vec<f64> = values
        .chunks(window)
        .filter(|w| w.iter().all(|v| *v != 0.0))
        .filter_map(roughness)
        .collect();
    if per_window.is_empty() {
        return None;
    }
    per_window.sort_by(f64::total_cmp);
    Some(per_window[per_window.len() / 2])
}

/// Sauts : quantile `q` (ex. 0,99) des différences secondes en valeur absolue.
///
/// Mesure ce que la médiane ignore : les événements rares et brefs (un
/// clignement, une bouche qui s'ouvre) qui font « sauter » la vue.
pub fn spikes(values: &[f64], q: f64) -> Option<f64> {
    let mut d: Vec<f64> = values
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .collect();
    if d.is_empty() {
        return None;
    }
    d.sort_by(f64::total_cmp);
    let i = ((d.len() - 1) as f64 * q.clamp(0.0, 1.0)).round() as usize;
    Some(d[i])
}

/// Estime de combien d'échantillons `delayed` est en retard sur `reference`
/// (décalage `k` qui minimise l'écart quadratique `reference[i] − delayed[i + k]`),
/// avec une précision inférieure à l'échantillon (interpolation parabolique
/// autour du minimum).
///
/// Renvoie `None` si le signal bouge trop peu pour qu'un retard ait un sens.
pub fn estimate_lag(
    reference: &[f64],
    delayed: &[f64],
    max_shift: usize,
    min_range: f64,
) -> Option<f64> {
    let n = reference.len().min(delayed.len());
    if n <= max_shift + 10 {
        return None;
    }
    let (lo, hi) = reference[..n]
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    if hi - lo < min_range {
        return None;
    }
    let err = |k: usize| {
        let m = n - k;
        (0..m)
            .map(|i| (reference[i] - delayed[i + k]).powi(2))
            .sum::<f64>()
            / m as f64
    };
    let errors: Vec<f64> = (0..=max_shift).map(err).collect();
    let k = (0..errors.len()).min_by(|&a, &b| errors[a].total_cmp(&errors[b]))?;
    // Parabole passant par les 3 points autour du minimum : son sommet donne
    // la fraction d'échantillon (entre -0,5 et +0,5).
    if k == 0 || k + 1 >= errors.len() {
        return Some(k as f64);
    }
    let (a, b, c) = (errors[k - 1], errors[k], errors[k + 1]);
    let denom = a - 2.0 * b + c;
    let offset = if denom > 0.0 {
        (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some(k as f64 + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rugosite_nulle_pour_un_mouvement_regulier() {
        let ramp: Vec<f64> = (0..50).map(|i| i as f64 * 0.7).collect();
        assert!(roughness(&ramp).unwrap() < 1e-12);
        let shaky: Vec<f64> = (0..50)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        assert!(roughness(&shaky).unwrap() > 0.3);
    }

    #[test]
    fn retrouve_un_retard_connu() {
        let reference: Vec<f64> = (0..200).map(|i| (i as f64 * 0.1).sin() * 20.0).collect();
        let mut delayed = vec![0.0; 4];
        delayed.extend_from_slice(&reference[..196]);
        let lag = estimate_lag(&reference, &delayed, 15, 1.0).unwrap();
        assert!((lag - 4.0).abs() < 0.05, "retard {lag}");
    }

    #[test]
    fn retard_fractionnaire() {
        // Sinus décalé de 2,5 échantillons exactement.
        let f = |x: f64| (x * 0.1).sin() * 20.0;
        let reference: Vec<f64> = (0..300).map(|i| f(i as f64)).collect();
        let delayed: Vec<f64> = (0..300).map(|i| f(i as f64 - 2.5)).collect();
        let lag = estimate_lag(&reference, &delayed, 15, 1.0).unwrap();
        assert!((lag - 2.5).abs() < 0.1, "retard {lag}");
    }

    #[test]
    fn rugosite_a_l_arret_ignore_les_mouvements() {
        // Tremblement constant ±0,1, plus un mouvement brusque au milieu.
        let mut v: Vec<f64> = (0..300)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        for (i, x) in v.iter_mut().enumerate().skip(140).take(10) {
            *x += (i - 140) as f64 * 5.0;
        }
        for x in v.iter_mut().skip(150) {
            *x += 50.0;
        }
        let still = still_roughness(&v, 15).unwrap();
        let global = roughness(&v).unwrap();
        assert!((still - 0.4).abs() < 0.01, "rugosité à l'arrêt {still}");
        assert!(global > still);
    }

    #[test]
    fn rugosite_active_ignore_la_zone_morte() {
        // 10 fenêtres à 0 (zone morte), 5 fenêtres hors zone morte qui tremblent.
        let mut v = vec![0.0; 150];
        v.extend((0..75).map(|i| 20.0 + if i % 2 == 0 { 0.1 } else { -0.1 }));
        assert_eq!(
            still_roughness(&v, 15),
            Some(0.0),
            "la médiane globale ne voit que la zone morte"
        );
        let active = active_still_roughness(&v, 15).unwrap();
        assert!((active - 0.4).abs() < 0.01);
        assert_eq!(active_still_roughness(&[0.0; 60], 15), None);
    }

    #[test]
    fn les_sauts_rares_sont_vus_par_le_quantile() {
        // Signal calme avec 2 sauts sur 200 images.
        let mut v = vec![0.0; 200];
        v[50] = 3.0;
        v[150] = -3.0;
        assert_eq!(
            still_roughness(&v, 15),
            Some(0.0),
            "la médiane ne les voit pas"
        );
        assert!(spikes(&v, 0.99).unwrap() >= 3.0);
        assert_eq!(spikes(&v, 0.5), Some(0.0));
    }

    #[test]
    fn pas_de_retard_sans_mouvement() {
        let still = vec![1.0; 200];
        assert_eq!(estimate_lag(&still, &still, 15, 1.0), None);
    }

    #[test]
    fn ecart_type_d_une_alternance() {
        let mut r = Rolling::new(4);
        for v in [1.0, -1.0, 1.0, -1.0] {
            r.push(v);
        }
        assert_eq!(r.mean(), Some(0.0));
        assert_eq!(r.std_dev(), Some(1.0));
    }

    #[test]
    fn fenetre_glissante() {
        let mut r = Rolling::new(2);
        for v in [100.0, 3.0, 5.0] {
            r.push(v);
        }
        assert_eq!(r.mean(), Some(4.0)); // 100 est sorti de la fenêtre
    }
}
