//! Envoi de la pose vers un simulateur.
//!
//! Seule crate qui fait du réseau. Les conventions internes ([`Pose6`]) sont
//! converties **ici**, à la frontière, vers celles de chaque destination.

// Tout élément public doit être documenté (`cargo doc` sert de référence).
#![warn(missing_docs)]

mod opentrack;

pub use opentrack::{OpenTrackUdp, decode, encode};
use tracker_core::Pose6;

/// Erreurs d'envoi.
#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    /// Adresse de destination invalide ou introuvable.
    #[error("adresse « {0} » invalide : {1}")]
    Address(String, String),
    /// Erreur réseau (création du socket, envoi).
    #[error("réseau : {0}")]
    Io(#[from] std::io::Error),
}

/// Une destination de la pose.
///
/// `Send` : la destination vit dans le thread pipeline, et c'est l'UI qui la crée.
pub trait PoseSink: Send {
    /// Envoie une pose (convention [`Pose6`] : utilisateur, degrés, cm).
    fn send(&mut self, pose: &Pose6) -> Result<(), OutputError>;
    /// Description lisible (ex. « OpenTrack UDP → 127.0.0.1:4242 »).
    fn describe(&self) -> String;
}

/// Réglages de la sortie, enregistrés dans le profil.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    /// Envoi activé.
    pub enabled: bool,
    /// Machine qui fait tourner OpenTrack (`127.0.0.1` si c'est la même). Préférer
    /// une adresse IP : un nom passe par le DNS, ce qui peut prendre plusieurs secondes.
    pub host: String,
    /// Port UDP d'OpenTrack (« UDP over network », 4242 par défaut).
    pub port: u16,
    /// Inversion par axe, dans l'ordre [lacet, tangage, roulis, x, y, z].
    pub invert: [bool; 6],
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 4242,
            invert: [false; 6],
        }
    }
}

impl OutputSettings {
    /// Crée la destination correspondante. Peut bloquer (résolution DNS d'un nom) :
    /// à appeler hors des threads temps réel.
    pub fn connect(&self) -> Result<Box<dyn PoseSink>, OutputError> {
        Ok(Box::new(OpenTrackUdp::new(
            &self.host,
            self.port,
            self.invert,
        )?))
    }
}
