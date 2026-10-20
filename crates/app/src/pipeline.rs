//! Thread de traitement : capture → visage → pose → UI.
//!
//! Le thread possède la `CaptureHandle`, le `FaceTracker` et l'état de la pose
//! (pose neutre, modèle de caméra). L'UI ne fait qu'afficher : elle reçoit
//! l'image *et* ses résultats ensemble, et envoie des commandes en retour.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use capture::{CaptureEvent, CaptureHandle, FormatTarget};
use tracker_core::{CameraModel, Frame, HeadPose, Pose6, pose};
use vision::{FaceResult, FaceTracker};

/// Tout ce qui a été calculé pour une image.
pub struct FrameOutput {
    pub frame: Frame,
    pub decode_ms: f64,
    /// `None` si les modèles n'ont pas pu être chargés.
    pub face: Option<FaceResult>,
    /// Pose de la tête, si un visage a été trouvé.
    pub head: Option<HeadPose>,
    /// Pose relative à la pose neutre (ou absolue si aucun recentrage).
    pub pose: Option<Pose6>,
}

pub enum PipelineEvent {
    Started(String),
    /// En `Box` : une variante d'enum occupe la taille de la plus grosse ;
    /// on garde ainsi les événements légers (~ 32 octets).
    Frame(Box<FrameOutput>),
    /// Erreur affichée dans l'UI. `fatal` : le flux est arrêté.
    Error {
        message: String,
        fatal: bool,
    },
}

/// Ordres envoyés par l'UI au pipeline.
pub enum PipelineCommand {
    /// La pose courante devient la pose neutre.
    Recenter,
    /// Revenir à la pose absolue (par rapport à la caméra).
    ClearNeutral,
    /// Champ de vision horizontal de la caméra, en degrés.
    SetHfov(f32),
}

pub struct Pipeline {
    events: Receiver<PipelineEvent>,
    commands: Sender<PipelineCommand>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub fn start(camera: u32, models_dir: PathBuf, hfov_deg: f32) -> Self {
        let (tx, events) = mpsc::sync_channel(2);
        // Canal non borné pour les commandes : il y en a peu, et l'UI ne doit
        // jamais bloquer en les envoyant.
        let (commands, rx_commands) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("pipeline".into())
            .spawn(move || {
                let state = State {
                    hfov_deg,
                    neutral: None,
                    recenter_requested: false,
                };
                run(camera, models_dir, state, &tx, &rx_commands, &stop_thread);
            })
            .expect("impossible de créer le thread pipeline");
        Self {
            events,
            commands,
            stop,
            thread: Some(thread),
        }
    }

    pub fn drain(&self) -> impl Iterator<Item = PipelineEvent> + '_ {
        std::iter::from_fn(|| self.events.try_recv().ok())
    }

    pub fn send(&self, command: PipelineCommand) {
        // Seule erreur possible : le thread est arrêté, la commande n'a plus d'objet.
        let _ = self.commands.send(command);
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

/// État de la pose, propre au thread pipeline.
struct State {
    hfov_deg: f32,
    neutral: Option<HeadPose>,
    /// Le recentrage s'applique à la prochaine image où la tête est trouvée.
    recenter_requested: bool,
}

impl State {
    fn apply(&mut self, command: PipelineCommand) {
        match command {
            PipelineCommand::Recenter => self.recenter_requested = true,
            PipelineCommand::ClearNeutral => self.neutral = None,
            PipelineCommand::SetHfov(deg) => {
                self.hfov_deg = deg;
                // Changer de focale change les distances : l'ancienne pose neutre n'a plus de sens.
                self.neutral = None;
            }
        }
    }

    fn pose(
        &mut self,
        frame: &Frame,
        face: Option<&FaceResult>,
    ) -> (Option<HeadPose>, Option<Pose6>) {
        let camera = CameraModel {
            width: frame.width as f32,
            height: frame.height as f32,
            hfov_deg: self.hfov_deg,
        };
        let Some(head) = face
            .and_then(|f| f.landmarks.as_ref())
            .and_then(|lm| pose::estimate(lm, &camera))
        else {
            return (None, None);
        };
        if std::mem::take(&mut self.recenter_requested) {
            self.neutral = Some(head);
        }
        let pose = match &self.neutral {
            Some(neutral) => head.relative_to(neutral),
            None => head.absolute(),
        };
        (Some(head), Some(pose))
    }
}

fn run(
    camera: u32,
    models_dir: PathBuf,
    mut state: State,
    tx: &SyncSender<PipelineEvent>,
    commands: &Receiver<PipelineCommand>,
    stop: &AtomicBool,
) {
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
        for command in commands.try_iter() {
            state.apply(command);
        }

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
        let (head, pose) = state.pose(&frame, face.as_ref());

        match tx.try_send(PipelineEvent::Frame(Box::new(FrameOutput {
            frame,
            decode_ms,
            face,
            head,
            pose,
        }))) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => return,
        }
    }
}
