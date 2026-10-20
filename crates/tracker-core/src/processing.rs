//! Chaîne de traitement de la pose : filtre One-Euro puis courbes de réponse.

use crate::Pose6;
use crate::curve::AxisCurve;
use crate::filter::{OneEuro, OneEuroParams};

/// Noms des 6 axes, dans l'ordre de [`Pose6::to_array`].
pub const AXES: [&str; 6] = ["Lacet", "Tangage", "Roulis", "X", "Y", "Z"];

/// Tous les réglages du traitement.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackingSettings {
    pub filter_enabled: bool,
    /// Filtre des rotations (degrés).
    pub rotation_filter: OneEuroParams,
    /// Filtre des translations (cm).
    pub position_filter: OneEuroParams,
    pub curves_enabled: bool,
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
            rotation_filter: OneEuroParams {
                min_cutoff: 0.5,
                beta: 0.05,
                d_cutoff: 1.0,
            },
            position_filter: OneEuroParams {
                min_cutoff: 0.5,
                beta: 0.5,
                d_cutoff: 1.0,
            },
            curves_enabled: true,
            curves: [
                rot(0.5, 40.0, 180.0, 1.6), // lacet : 40° de tête → regarder derrière soi
                rot(0.5, 25.0, 90.0, 1.4),  // tangage
                rot(0.5, 30.0, 30.0, 1.0),  // roulis : 1:1, l'amplifier donne le mal de mer
                rot(0.2, 15.0, 30.0, 1.0),  // x (cm)
                rot(0.2, 15.0, 20.0, 1.0),  // y (cm)
                rot(0.2, 15.0, 30.0, 1.0),  // z (cm)
            ],
        }
    }
}

/// Résultat des étapes intermédiaires (pour le banc de test).
#[derive(Debug, Clone, Copy, Default)]
pub struct Processed {
    pub filtered: Pose6,
    pub output: Pose6,
}

/// État des filtres, un par axe.
#[derive(Debug, Clone, Default)]
pub struct PoseProcessor {
    filters: [OneEuro; 6],
}

impl PoseProcessor {
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
