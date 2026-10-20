//! Cœur du tracker : types partagés et pipeline image → points → pose.
//!
//! Cette crate ne fait aucune E/S (pas de caméra, pas de réseau, pas d'UI,
//! pas de moteur ML) : elle reste testable unitairement et identique sur
//! macOS et Windows.

pub mod blazeface;
pub mod canonical_face;
pub mod face;
mod frame;
pub mod pose;
mod roi;
pub mod stats;
pub mod tensor;

pub use face::FaceLandmarks;
pub use frame::Frame;
pub use pose::{CameraModel, HeadPose, Pose6};
pub use roi::Roi;
