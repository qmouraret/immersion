//! Zone morte et courbe de réponse d'un axe.
//!
//! Avec un seul écran, on ne peut pas tourner la tête de 90° en continuant à
//! regarder l'image : on amplifie donc les mouvements. Une courbe convexe
//! (`expo` > 1) garde de la précision près du centre (petits mouvements pour
//! lire les instruments) et accélère vers les bords (regarder derrière l'aile).
//!
//! ```text
//!  sortie
//!  out ┤                   ●━━━━  (plafonnée au-delà de `input`)
//!      │                 ╱
//!      │              ╱
//!      │          ╱          expo > 1 : lent au centre, rapide au bord
//!    0 ┼━━━━━━●
//!      └──────┴───────────┴────── entrée (|x|)
//!         deadzone       input
//! ```

/// Réglages d'un axe. Les unités sont celles de l'axe (degrés ou cm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisCurve {
    /// En dessous de cette amplitude, la sortie vaut 0.
    pub deadzone: f64,
    /// Amplitude d'entrée qui donne la sortie maximale.
    pub input: f64,
    /// Sortie maximale.
    pub output: f64,
    /// Forme : 1 = linéaire, 2 = quadratique…
    pub expo: f64,
}

impl AxisCurve {
    pub fn apply(&self, x: f64) -> f64 {
        let span = (self.input - self.deadzone).max(f64::EPSILON);
        let t = ((x.abs() - self.deadzone) / span).clamp(0.0, 1.0);
        // `copysign` : on garde le signe de l'entrée (courbe symétrique).
        (self.output * t.powf(self.expo.max(0.1))).copysign(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAW: AxisCurve = AxisCurve {
        deadzone: 1.0,
        input: 41.0,
        output: 180.0,
        expo: 2.0,
    };

    #[test]
    fn zone_morte() {
        assert_eq!(YAW.apply(0.8), 0.0);
        assert_eq!(YAW.apply(-1.0), 0.0);
    }

    #[test]
    fn symetrique_et_plafonnee() {
        assert_eq!(YAW.apply(41.0), 180.0);
        assert_eq!(YAW.apply(-90.0), -180.0);
    }

    #[test]
    fn expo_garde_la_precision_au_centre() {
        // À mi-course (21°), une courbe quadratique ne donne qu'un quart de la sortie.
        assert!((YAW.apply(21.0) - 45.0).abs() < 1e-9);
    }

    #[test]
    fn continue_a_la_sortie_de_la_zone_morte() {
        // Pas de saut : juste après la zone morte, la sortie est quasi nulle.
        assert!(YAW.apply(1.01) < 0.001);
    }
}
