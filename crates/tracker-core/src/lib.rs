//! Cœur du tracker : types partagés et pipeline image → points → pose.
//!
//! Cette crate ne fait aucune E/S (pas de caméra, pas de réseau, pas d'UI,
//! pas de moteur ML) : elle reste testable unitairement et identique sur
//! macOS et Windows.

// Tout élément public doit être documenté (`cargo doc` sert de référence).
#![warn(missing_docs)]

pub mod autotune;
pub mod blazeface;
pub mod calibration;
pub mod canonical_face;
pub mod curve;
pub mod expression;
pub mod face;
pub mod filter;
mod frame;
pub mod landmark_log;
pub mod pose;
pub mod processing;
pub mod recording;
mod roi;
pub mod stats;
pub mod tensor;
pub mod testing;

pub use face::FaceLandmarks;
pub use frame::Frame;
pub use pose::{CameraModel, EstimateOptions, HeadPose, Pose6};
pub use roi::Roi;
