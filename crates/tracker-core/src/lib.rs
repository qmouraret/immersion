//! Cœur du tracker : types partagés et (bientôt) pipeline image → pose.
//!
//! Cette crate ne fait aucune E/S (pas de caméra, pas de réseau, pas d'UI) :
//! elle reste testable unitairement et identique sur macOS et Windows.

mod frame;

pub use frame::Frame;
