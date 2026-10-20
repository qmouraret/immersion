//! Enregistrement de sessions (poses brutes horodatées) et rejeu hors ligne.
//!
//! Format CSV volontairement simple, lisible dans un tableur :
//! `t,yaw,pitch,roll,x,y,z` (secondes, degrés, cm).

use std::fmt;

use crate::Pose6;
use crate::processing::{PoseProcessor, Processed, TrackingSettings};

const HEADER: &str = "t,yaw,pitch,roll,x,y,z";

/// Une pose brute (recentrée, avant filtre) et son instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawSample {
    /// Instant, en secondes depuis le début de la session.
    pub t: f64,
    /// [lacet, tangage, roulis, x, y, z] (voir [`Pose6::to_array`]).
    pub pose: [f64; 6],
}

/// Erreur de lecture d'un fichier d'enregistrement.
///
/// Écrite à la main (sans `thiserror`) pour voir ce que la macro génère :
/// une implémentation de `Display`, et `std::error::Error` (vide ici).
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    /// Numéro de ligne (à partir de 1).
    pub line: usize,
    /// Description du problème.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ligne {} : {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Sérialise une session au format CSV (avec en-tête).
pub fn to_csv(samples: &[RawSample]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for s in samples {
        // `fmt::Write` permettrait `write!(out, …)` ; ici `format!` suffit.
        let p = s.pose;
        out.push_str(&format!(
            "{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
            s.t, p[0], p[1], p[2], p[3], p[4], p[5]
        ));
    }
    out
}

/// Lit une session au format CSV ; l'erreur indique la ligne fautive.
pub fn from_csv(text: &str) -> Result<Vec<RawSample>, ParseError> {
    let mut lines = text.lines().enumerate();
    match lines.next() {
        Some((_, h)) if h.trim() == HEADER => {}
        _ => {
            return Err(ParseError {
                line: 1,
                message: format!("en-tête attendu : {HEADER}"),
            });
        }
    }
    lines
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            let err = |message: String| ParseError {
                line: i + 1,
                message,
            };
            let values = l
                .split(',')
                .map(|v| {
                    v.trim()
                        .parse::<f64>()
                        .map_err(|e| err(format!("« {v} » : {e}")))
                })
                .collect::<Result<Vec<f64>, _>>()?;
            // Motif de tranche : exactement 7 valeurs, sinon erreur.
            let [t, a, b, c, d, e, f] = values[..] else {
                return Err(err(format!(
                    "7 valeurs attendues, {} trouvées",
                    values.len()
                )));
            };
            Ok(RawSample {
                t,
                pose: [a, b, c, d, e, f],
            })
        })
        .collect()
}

/// Rejoue un enregistrement avec des réglages donnés (filtres neufs).
pub fn replay(samples: &[RawSample], settings: &TrackingSettings) -> Vec<Processed> {
    let mut processor = PoseProcessor::default();
    let mut prev_t = None;
    samples
        .iter()
        .map(|s| {
            let dt = prev_t.map_or(0.0, |p| s.t - p);
            prev_t = Some(s.t);
            processor.process(&Pose6::from_array(s.pose), dt, settings)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_csv() {
        let samples = vec![
            RawSample {
                t: 0.0,
                pose: [1.0, -2.5, 0.25, 0.0, 1.5, -3.0],
            },
            RawSample {
                t: 0.0333,
                pose: [1.1, -2.4, 0.2, 0.01, 1.4, -2.9],
            },
        ];
        assert_eq!(from_csv(&to_csv(&samples)).unwrap(), samples);
    }

    #[test]
    fn erreur_avec_numero_de_ligne() {
        let text = format!("{HEADER}\n0,1,2,3,4,5,6\n0.1,1,deux,3,4,5,6\n");
        let e = from_csv(&text).unwrap_err();
        assert_eq!(e.line, 3);
        assert!(e.to_string().contains("deux"));
    }

    #[test]
    fn rejeu_sans_filtre_ni_courbe_est_l_identite() {
        let samples: Vec<RawSample> = (0..10)
            .map(|i| RawSample {
                t: i as f64 / 30.0,
                pose: [i as f64; 6],
            })
            .collect();
        let settings = TrackingSettings {
            filter_enabled: false,
            curves_enabled: false,
            ..Default::default()
        };
        let out = replay(&samples, &settings);
        assert_eq!(out[7].output.to_array(), [7.0; 6]);
    }
}
