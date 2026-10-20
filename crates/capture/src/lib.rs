//! Capture webcam sur un thread dédié.
//!
//! Le reste de l'application ne voit jamais `nokhwa` : il reçoit des
//! [`CaptureEvent`] via un canal. Changer de bibliothèque caméra plus tard
//! (ou ajouter une source « vidéo enregistrée » pour le rejeu) ne touchera
//! que cette crate.

// Tout élément public doit être documenté (`cargo doc` sert de référence).
#![warn(missing_docs)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nokhwa::Camera;
use nokhwa::pixel_format::{FormatDecoder, RgbFormat};
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use tracker_core::Frame;

mod format;
pub use format::FormatTarget;

/// Erreurs remontées par la capture.
///
/// `thiserror` génère l'implémentation de `std::error::Error` et de
/// `Display` à partir des attributs `#[error(...)]`.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("accès caméra refusé (Réglages Système → Confidentialité → Caméra)")]
    /// macOS a refusé l'accès à la caméra pour l'application qui a lancé le programme.
    PermissionDenied,
    #[error("erreur caméra : {0}")]
    /// Erreur remontée par nokhwa (ouverture, format, lecture d'image…).
    Backend(#[from] nokhwa::NokhwaError),
}

/// Description d'une caméra disponible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraInfo {
    /// Index à passer à [`CaptureHandle::start`].
    pub index: u32,
    /// Nom lisible (ex. « FaceTime HD Camera »).
    pub name: String,
}

/// Ce que le thread de capture envoie à l'application.
#[derive(Debug)]
pub enum CaptureEvent {
    /// Le flux est ouvert ; format réellement négocié avec la caméra.
    Started {
        /// Largeur négociée, en pixels.
        width: u32,
        /// Hauteur négociée, en pixels.
        height: u32,
        /// Fréquence d'images négociée.
        fps: u32,
        /// Format des pixels envoyé par la caméra (YUYV, NV12, MJPEG…).
        pixel_format: String,
    },
    /// Une image, et le temps passé à la convertir en RGB.
    Frame {
        /// L'image, en RGB.
        frame: Frame,
        /// Temps de conversion du format caméra en RGB, en ms.
        decode_ms: f64,
    },
    /// Erreur fatale : le thread s'arrête après l'avoir envoyée.
    Error(String),
}

/// Demande l'autorisation caméra à macOS (no-op sur les autres OS).
///
/// Bloque jusqu'à la réponse de l'utilisateur à la première exécution.
pub fn ensure_permission() -> Result<(), CaptureError> {
    #[cfg(target_os = "macos")]
    {
        if !nokhwa::nokhwa_check() {
            let (tx, rx) = mpsc::channel();
            nokhwa::nokhwa_initialize(move |granted| {
                let _ = tx.send(granted);
            });
            if !rx.recv().unwrap_or(false) {
                return Err(CaptureError::PermissionDenied);
            }
        }
    }
    Ok(())
}

/// Liste les caméras connues du système.
pub fn list_cameras() -> Result<Vec<CameraInfo>, CaptureError> {
    let cams = nokhwa::query(ApiBackend::Auto)?;
    Ok(cams
        .iter()
        .enumerate()
        .map(|(i, c)| CameraInfo {
            index: match c.index() {
                CameraIndex::Index(n) => *n,
                CameraIndex::String(_) => i as u32,
            },
            name: c.human_name(),
        })
        .collect())
}

/// Poignée sur un thread de capture en cours.
///
/// Quand la poignée est détruite (`Drop`), le thread est arrêté proprement :
/// c'est le motif RAII, la durée de vie de l'objet *est* celle de la capture.
pub struct CaptureHandle {
    events: Receiver<CaptureEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    /// Ouvre la caméra `index` au format le plus proche de `target`, et commence à capturer.
    pub fn start(index: u32, target: FormatTarget) -> Self {
        // Canal borné à 2 images : si l'UI prend du retard, on jette les
        // images en trop plutôt que d'accumuler de la latence.
        let (tx, events) = mpsc::sync_channel(2);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);

        // `move` transfère la propriété de `tx` et `stop_thread` au thread.
        // La caméra elle-même est créée *dans* le thread : le type `Camera`
        // n'est pas `Send` sur tous les backends, on ne peut donc pas la
        // construire ici puis l'envoyer.
        let thread = thread::Builder::new()
            .name(format!("capture-{index}"))
            .spawn(move || {
                if let Err(e) = run(index, target, &tx, &stop_thread) {
                    tracing::error!("capture arrêtée : {e}");
                    let _ = tx.send(CaptureEvent::Error(e.to_string()));
                }
            })
            .expect("impossible de créer le thread de capture");

        Self {
            events,
            stop,
            thread: Some(thread),
        }
    }

    /// Attend le prochain événement, au plus `timeout`.
    ///
    /// Renvoie `None` si rien n'est arrivé à temps *ou* si le thread de
    /// capture s'est arrêté (après avoir envoyé son `Error`).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<CaptureEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Récupère tous les événements en attente sans bloquer.
    pub fn drain(&self) -> impl Iterator<Item = CaptureEvent> + '_ {
        // `try_recv` échoue si le canal est vide ou fermé : dans les deux
        // cas, l'itération s'arrête.
        std::iter::from_fn(|| self.events.try_recv().ok())
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Boucle du thread de capture.
fn run(
    index: u32,
    target: FormatTarget,
    tx: &SyncSender<CaptureEvent>,
    stop: &AtomicBool,
) -> Result<(), CaptureError> {
    // On ouvre d'abord avec un format que la caméra accepte à coup sûr, pour
    // pouvoir l'interroger, puis on applique notre propre choix.
    let initial = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = Camera::new(CameraIndex::Index(index), initial)?;
    apply_best_format(&mut camera, target)?;
    camera.open_stream()?;

    let current = camera.camera_format();
    tracing::info!("caméra {index} ouverte : {current}");
    let started = CaptureEvent::Started {
        width: current.width(),
        height: current.height(),
        fps: current.frame_rate(),
        pixel_format: current.format().to_string(),
    };
    if tx.send(started).is_err() {
        return Ok(()); // l'application a déjà fermé le canal
    }

    let mut seq = 0u64;
    while !stop.load(Ordering::Relaxed) {
        // `frame()` bloque jusqu'à la prochaine image du capteur : c'est elle
        // qui cadence la boucle. On horodate juste après, *avant* la conversion,
        // pour que la latence affichée inclue bien le coût de décodage.
        // `?` : en cas d'erreur, on sort de la fonction en la propageant
        // (convertie automatiquement grâce à `#[from]`).
        let buffer = camera.frame()?;
        let captured_at = Instant::now();
        let image = buffer.decode_image::<RgbFormat>()?;
        let decode_ms = captured_at.elapsed().as_secs_f64() * 1000.0;

        let (w, h) = image.dimensions();
        let Some(frame) = Frame::new(w, h, image.into_raw(), seq, captured_at) else {
            tracing::warn!("image {seq} ignorée : taille de buffer incohérente");
            continue;
        };
        seq += 1;

        match tx.try_send(CaptureEvent::Frame { frame, decode_ms }) {
            Ok(()) | Err(TrySendError::Full(_)) => {} // image jetée si l'UI est en retard
            Err(TrySendError::Disconnected(_)) => break,
        }
    }

    camera.stop_stream()?;
    Ok(())
}

/// Essaie les formats du meilleur au moins bon ; garde le premier accepté.
///
/// Si aucun n'est accepté, la caméra reste dans son format d'ouverture :
/// ce n'est pas une erreur fatale, juste un avertissement.
fn apply_best_format(camera: &mut Camera, target: FormatTarget) -> Result<(), CaptureError> {
    let available = camera.compatible_camera_formats()?;
    for f in &available {
        tracing::debug!("format proposé : {f}");
    }

    for candidate in format::rank_formats(&available, RgbFormat::FORMATS, target) {
        let exact = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(candidate));
        match camera.set_camera_requset(exact) {
            Ok(applied) => {
                tracing::debug!("format accepté : {applied}");
                return Ok(());
            }
            Err(e) => tracing::debug!("format refusé : {candidate} ({e})"),
        }
    }

    tracing::warn!(
        "aucun format préféré accepté, on garde {}",
        camera.camera_format()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touche au matériel (et peut déclencher la demande d'accès caméra de macOS) :
    /// lancé seulement à la demande, `cargo test -p capture -- --ignored`.
    #[test]
    #[ignore = "matériel : nécessite l'accès caméra"]
    fn camera_inexistante_erreur_puis_arret_propre() {
        let handle = CaptureHandle::start(9_999, FormatTarget::default());
        let event = handle.recv_timeout(Duration::from_secs(5));
        assert!(
            matches!(event, Some(CaptureEvent::Error(_))),
            "attendu une erreur, reçu {event:?}"
        );
        drop(handle); // ne doit pas bloquer : le thread s'est déjà arrêté
    }

    #[test]
    #[ignore = "matériel : nécessite l'accès caméra"]
    fn la_camera_par_defaut_envoie_des_images() {
        let handle = CaptureHandle::start(0, FormatTarget::default());
        let mut frames = 0;
        let start = Instant::now();
        while frames < 10 && start.elapsed() < Duration::from_secs(10) {
            if let Some(CaptureEvent::Frame { frame, .. }) =
                handle.recv_timeout(Duration::from_secs(1))
            {
                assert_eq!(frame.rgb.len(), (frame.width * frame.height * 3) as usize);
                frames += 1;
            }
        }
        assert_eq!(frames, 10);
    }
}
