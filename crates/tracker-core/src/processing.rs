//! Chaîne de traitement de la pose : filtre One-Euro puis courbes de réponse.

use crate::Pose6;
use crate::curve::AxisCurve;
use crate::filter::{OneEuro, OneEuroParams};

/// Noms des 6 axes, dans l'ordre de [`Pose6::to_array`].
pub const AXES: [&str; 6] = ["Lacet", "Tangage", "Roulis", "X", "Y", "Z"];

/// Tous les réglages du traitement.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackingSettings {
    /// Si `false`, le filtre est contourné (pose brute).
    pub filter_enabled: bool,
    /// Filtre des rotations (degrés).
    pub rotation_filter: OneEuroParams,
    /// Filtre des translations (cm).
    pub position_filter: OneEuroParams,
    /// Si `false`, les courbes sont contournées (sortie = pose filtrée).
    pub curves_enabled: bool,
    /// Une courbe par axe, dans l'ordre de [`AXES`].
    pub curves: [AxisCurve; 6],
}

impl Default for TrackingSettings {
    /// Point de départ raisonnable pour un seul écran, à affiner au banc de test.
    fn default() -> Self {
        let rot = |deadzone, input, output, expo| AxisCurve {
            deadzone,
            input,
            output,
            expo,
        };
        Self {
            filter_enabled: true,
            // Choisi avec `examples/tune_filter` et `compare_filters` sur une vraie
            // session (35 s) : tremblement de sortie hors zone morte 0,091° pour 70 ms
            // de retard, contre 0,144° / 57 ms avec 0,5 Hz / β 0,05 / 1 Hz. Un
            // `d_cutoff` élevé détecte plus tôt le début des mouvements, ce qui
            // permet un β faible. Plus bas que 0,1 Hz, les mouvements lents deviennent « mous ».
            rotation_filter: OneEuroParams {
                min_cutoff: 0.1,
                beta: 0.03,
                d_cutoff: 3.0,
            },
            // Translations : bruit lent (Z vient de la taille apparente du visage) et
            // mouvements peu urgents. Mesuré sur 2 sessions : tremblement ÷ 2 pour
            // 100 à 130 ms de retard (contre 50 à 67 ms avec 0,5 Hz / β 0,5).
            position_filter: OneEuroParams {
                min_cutoff: 0.17,
                beta: 0.17,
                d_cutoff: 1.0,
            },
            curves_enabled: true,
            curves: [
                rot(0.5, 40.0, 180.0, 1.6), // lacet : 40° de tête → regarder derrière soi
                rot(0.5, 25.0, 90.0, 1.4),  // tangage
                rot(0.5, 30.0, 30.0, 1.0),  // roulis : 1:1, l'amplifier donne le mal de mer
                rot(0.2, 15.0, 30.0, 1.0),  // x (cm)
                rot(0.2, 15.0, 20.0, 1.0),  // y (cm)
                rot(0.4, 15.0, 20.0, 1.0), // z (cm) : zone morte ≈ 2,5 × le bruit brut, gain modéré
            ],
        }
    }
}

/// Résultat des étapes intermédiaires (pour le banc de test).
#[derive(Debug, Clone, Copy, Default)]
pub struct Processed {
    /// Pose après le filtre One-Euro.
    pub filtered: Pose6,
    /// Pose après les courbes : ce qui part vers le simulateur.
    pub output: Pose6,
}

/// État des filtres, un par axe.
#[derive(Debug, Clone, Default)]
pub struct PoseProcessor {
    filters: [OneEuro; 6],
}

impl PoseProcessor {
    /// Filtre puis applique les courbes. `dt` : secondes écoulées depuis la pose précédente.
    pub fn process(&mut self, pose: &Pose6, dt: f64, settings: &TrackingSettings) -> Processed {
        let raw = pose.to_array();
        let filtered: [f64; 6] = std::array::from_fn(|i| {
            if !settings.filter_enabled {
                return raw[i];
            }
            let params = if i < 3 {
                &settings.rotation_filter
            } else {
                &settings.position_filter
            };
            self.filters[i].filter(raw[i], dt, params)
        });
        let output: [f64; 6] = std::array::from_fn(|i| {
            if settings.curves_enabled {
                settings.curves[i].apply(filtered[i])
            } else {
                filtered[i]
            }
        });
        Processed {
            filtered: Pose6::from_array(filtered),
            output: Pose6::from_array(output),
        }
    }

    /// À appeler quand la pose « saute » (recentrage) : sinon le filtre
    /// glisserait lentement de l'ancienne valeur vers la nouvelle.
    pub fn reset(&mut self) {
        self.filters.iter_mut().for_each(OneEuro::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose::Angles;

    const DT: f64 = 1.0 / 30.0;

    fn pose(yaw: f64) -> Pose6 {
        Pose6 {
            angles: Angles {
                yaw,
                pitch: 0.0,
                roll: 0.0,
            },
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    #[test]
    fn sans_filtre_ni_courbe_la_pose_passe_telle_quelle() {
        let settings = TrackingSettings {
            filter_enabled: false,
            curves_enabled: false,
            ..Default::default()
        };
        let out = PoseProcessor::default().process(&pose(12.5), DT, &settings);
        assert_eq!(out.filtered.angles.yaw, 12.5);
        assert_eq!(out.output.angles.yaw, 12.5);
    }

    #[test]
    fn la_courbe_s_applique_apres_le_filtre() {
        let settings = TrackingSettings {
            filter_enabled: false,
            ..Default::default()
        };
        let out = PoseProcessor::default().process(&pose(40.0), DT, &settings);
        // Entrée max. du lacet par défaut : 40° → sortie max. 180°.
        assert_eq!(out.output.angles.yaw, settings.curves[0].output);
    }

    #[test]
    fn le_filtre_lisse_un_saut() {
        let settings = TrackingSettings {
            curves_enabled: false,
            ..Default::default()
        };
        let mut p = PoseProcessor::default();
        p.process(&pose(0.0), DT, &settings);
        let out = p.process(&pose(10.0), DT, &settings);
        assert!(out.filtered.angles.yaw > 0.0 && out.filtered.angles.yaw < 10.0);
    }

    #[test]
    fn reset_reprend_la_mesure_telle_quelle() {
        let settings = TrackingSettings {
            curves_enabled: false,
            ..Default::default()
        };
        let mut p = PoseProcessor::default();
        p.process(&pose(0.0), DT, &settings);
        p.reset();
        assert_eq!(
            p.process(&pose(10.0), DT, &settings).filtered.angles.yaw,
            10.0
        );
    }

    #[test]
    fn rotations_et_translations_ont_chacune_leur_filtre() {
        // Translation figée par un filtre extrêmement lent ; rotation sans lissage notable.
        let mut settings = TrackingSettings {
            curves_enabled: false,
            ..Default::default()
        };
        settings.position_filter.min_cutoff = 1e-6;
        settings.position_filter.beta = 0.0;
        settings.rotation_filter.min_cutoff = 1000.0;
        let mut p = PoseProcessor::default();
        p.process(&Pose6::default(), DT, &settings);
        let moved = Pose6 {
            x: 10.0,
            ..pose(10.0)
        };
        let out = p.process(&moved, DT, &settings);
        assert!(out.filtered.x < 0.01);
        assert!(out.filtered.angles.yaw > 9.9);
    }
}
