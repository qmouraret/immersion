//! Thread de traitement : capture → visage → UI.
//!
//! Le thread possède la `CaptureHandle` et le `FaceTracker`. L'UI ne fait
//! qu'afficher : elle reçoit l'image *et* son résultat ensemble, donc les
//! points dessinés correspondent toujours exactement à l'image affichée.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use capture::{CaptureEvent, CaptureHandle, FormatTarget};
use tracker_core::Frame;
use vision::{FaceResult, FaceTracker};

pub enum PipelineEvent {
    Started(String),
    Frame {
        frame: Frame,
        decode_ms: f64,
        /// `None` si les modèles n'ont pas pu être chargés.
        face: Option<FaceResult>,
    },
    /// Erreur affichée dans l'UI. `fatal` : le flux est arrêté.
    Error {
        message: String,
        fatal: bool,
    },
}

pub struct Pipeline {
    events: Receiver<PipelineEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub fn start(camera: u32, models_dir: PathBuf) -> Self {
        let (tx, events) = mpsc::sync_channel(2);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("pipeline".into())
            .spawn(move || run(camera, models_dir, &tx, &stop_thread))
            .expect("impossible de créer le thread pipeline");
        Self {
            events,
            stop,
            thread: Some(thread),
        }
    }

    pub fn drain(&self) -> impl Iterator<Item = PipelineEvent> + '_ {
        std::iter::from_fn(|| self.events.try_recv().ok())
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(camera: u32, models_dir: PathBuf, tx: &SyncSender<PipelineEvent>, stop: &AtomicBool) {
    // Les modèles se chargent pendant que la caméra démarre.
    let capture = CaptureHandle::start(camera, FormatTarget::default());
    let mut tracker = match FaceTracker::new(&models_dir) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::error!("modèles non chargés : {e}");
            let _ = tx.send(PipelineEvent::Error {
                message: e.to_string(),
                fatal: false,
            });
            None // on continue quand même à afficher la vidéo
        }
    };

    while !stop.load(Ordering::Relaxed) {
        // Attente bornée : on revérifie `stop` au moins 10 fois par seconde.
        let Some(first) = capture.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };

        // Si plusieurs images attendent, on ne traite que la plus récente :
        // mieux vaut sauter une image que d'accumuler du retard.
        let mut latest = None;
        for event in std::iter::once(first).chain(capture.drain()) {
            match event {
                CaptureEvent::Frame { frame, decode_ms } => latest = Some((frame, decode_ms)),
                CaptureEvent::Started {
                    width,
                    height,
                    fps,
                    pixel_format,
                } => {
                    let text = format!("{width}×{height} @ {fps} fps ({pixel_format})");
                    if tx.send(PipelineEvent::Started(text)).is_err() {
                        return;
                    }
                }
                CaptureEvent::Error(message) => {
                    let _ = tx.send(PipelineEvent::Error {
                        message,
                        fatal: true,
                    });
                    return;
                }
            }
        }
        let Some((frame, decode_ms)) = latest else {
            continue;
        };

        let face = match tracker.as_mut().map(|t| t.process(&frame)) {
            Some(Ok(result)) => Some(result),
            Some(Err(e)) => {
                tracing::warn!("inférence : {e}");
                None
            }
            None => None,
        };

        match tx.try_send(PipelineEvent::Frame {
            frame,
            decode_ms,
            face,
        }) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => return,
        }
    }
}
