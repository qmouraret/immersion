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
use std::time::{Duration, Instant};

use capture::{CaptureEvent, CaptureHandle, FormatTarget};
use output::{OutputSettings, PoseSink};
use tracker_core::expression::{self, ExpressionModel, NUM_FEATURES};
use tracker_core::processing::{PoseProcessor, Processed, TrackingSettings};
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
    /// Pose filtrée puis passée dans les courbes : ce qui ira au simulateur.
    pub processed: Option<Processed>,
    /// Mesure de l'expression du visage (pour l'étalonnage).
    pub expression: Option<[f64; NUM_FEATURES]>,
    /// État de l'envoi vers le simulateur pour cette image.
    pub sink: SinkStatus,
}

/// État de la sortie (envoi vers OpenTrack), pour l'affichage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SinkStatus {
    /// Destination active (ex. « OpenTrack UDP → 127.0.0.1:4242 »).
    pub target: Option<String>,
    /// Un paquet a été envoyé pour cette image.
    pub sent: bool,
    /// Dernière erreur (connexion ou envoi).
    pub error: Option<String>,
    /// Connexion en cours (résolution de l'adresse).
    pub connecting: bool,
}

/// Nombre d'images moyennées pour un recentrage (≈ 0,5 s).
const RECENTER_FRAMES: usize = 15;

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
    /// La pose moyenne des prochaines images (≈ 0,5 s) devient la pose neutre.
    Recenter,
    /// Revenir à la pose absolue (par rapport à la caméra).
    ClearNeutral,
    /// Champ de vision horizontal de la caméra, en degrés.
    SetHfov(f32),
    /// Nouveaux réglages de filtre et de courbes. En `Box` : la structure
    /// est bien plus grosse que les autres variantes.
    SetSettings(Box<TrackingSettings>),
    /// Correction des expressions apprise à l'étalonnage (`None` : désactivée).
    SetExpressionModel(Option<Box<ExpressionModel>>),
    /// Nouvelle configuration de la sortie (reconnexion si nécessaire).
    SetOutput(OutputSettings),
}

pub struct Pipeline {
    events: Receiver<PipelineEvent>,
    commands: Sender<PipelineCommand>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub fn start(
        camera: u32,
        models_dir: PathBuf,
        hfov_deg: f32,
        settings: TrackingSettings,
        output: OutputSettings,
    ) -> Self {
        let (tx, events) = mpsc::sync_channel(2);
        // Canal non borné pour les commandes : il y en a peu, et l'UI ne doit
        // jamais bloquer en les envoyant.
        let (commands, rx_commands) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("pipeline".into())
            .spawn(move || {
                let mut state = State {
                    hfov_deg,
                    neutral: None,
                    // Recentrage automatique sur la première pose : une pose absolue
                    // (≈ 60 cm de la caméra) n'a pas de sens dans les courbes.
                    recenter: Some(Vec::new()),
                    expression: None,
                    settings,
                    processor: PoseProcessor::default(),
                    last_pose_at: None,
                    sink: None,
                    connecting: None,
                    sink_status: SinkStatus::default(),
                };
                state.apply(PipelineCommand::SetOutput(output));
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
    /// Recentrage en cours : poses accumulées, moyennées une fois au complet.
    recenter: Option<Vec<HeadPose>>,
    settings: TrackingSettings,
    processor: PoseProcessor,
    /// Instant de capture de la dernière image avec une pose (pour le `dt` du filtre).
    last_pose_at: Option<Instant>,
    expression: Option<ExpressionModel>,
    /// Destination de la pose, une fois connectée.
    sink: Option<Box<dyn PoseSink>>,
    /// Connexion en cours sur un thread à part (une résolution DNS peut prendre
    /// plusieurs secondes : le suivi ne doit pas s'arrêter pendant ce temps).
    connecting: Option<Receiver<Result<Box<dyn PoseSink>, String>>>,
    sink_status: SinkStatus,
}

/// Résultats de [`State::pose`] pour une image.
#[derive(Default)]
struct PoseOutput {
    head: Option<HeadPose>,
    pose: Option<Pose6>,
    processed: Option<Processed>,
    expression: Option<[f64; NUM_FEATURES]>,
}

impl State {
    fn apply(&mut self, command: PipelineCommand) {
        match command {
            PipelineCommand::Recenter => self.recenter = Some(Vec::new()),
            PipelineCommand::ClearNeutral => {
                self.neutral = None;
                self.recenter = None;
                self.processor.reset();
            }
            PipelineCommand::SetHfov(deg) => {
                self.hfov_deg = deg;
                // Changer de focale change les distances : l'ancienne pose neutre
                // n'a plus de sens, on en reprend une sur les prochaines images.
                self.neutral = None;
                self.recenter = Some(Vec::new());
            }
            PipelineCommand::SetSettings(settings) => self.settings = *settings,
            PipelineCommand::SetExpressionModel(model) => self.expression = model.map(|m| *m),
            PipelineCommand::SetOutput(settings) => {
                self.sink = None;
                self.sink_status = SinkStatus::default();
                self.connecting = settings.enabled.then(|| {
                    let (tx, rx) = mpsc::channel();
                    // Thread éphémère : il se termine dès la connexion faite.
                    thread::spawn(move || {
                        let _ = tx.send(settings.connect().map_err(|e| e.to_string()));
                    });
                    self.sink_status.connecting = true;
                    rx
                });
            }
        }
    }

    /// Envoie la pose de sortie, après avoir récupéré une connexion terminée.
    fn send_output(&mut self, processed: Option<&Processed>) -> SinkStatus {
        if let Some(rx) = &self.connecting
            && let Ok(result) = rx.try_recv()
        {
            self.connecting = None;
            self.sink_status.connecting = false;
            match result {
                Ok(sink) => {
                    self.sink_status.target = Some(sink.describe());
                    self.sink = Some(sink);
                }
                Err(e) => self.sink_status.error = Some(e),
            }
        }
        let mut status = self.sink_status.clone();
        if let (Some(sink), Some(p)) = (&mut self.sink, processed) {
            match sink.send(&p.output) {
                Ok(()) => status.sent = true,
                Err(e) => {
                    self.sink_status.error = Some(e.to_string());
                    status.error = self.sink_status.error.clone();
                }
            }
        }
        status
    }

    /// Accumule les poses d'un recentrage en cours ; à la dernière, la moyenne
    /// devient la pose neutre. Sans pose neutre (démarrage), la première pose
    /// sert de neutre provisoire pour que la sortie parte de 0 tout de suite.
    fn collect_recenter(&mut self, head: HeadPose) {
        let Some(poses) = &mut self.recenter else {
            return;
        };
        poses.push(head);
        if self.neutral.is_none() {
            self.neutral = Some(head);
        }
        if poses.len() >= RECENTER_FRAMES {
            self.neutral = HeadPose::average(poses);
            self.recenter = None;
            self.processor.reset();
        }
    }

    fn pose(&mut self, frame: &Frame, face: Option<&FaceResult>) -> PoseOutput {
        let camera = CameraModel {
            width: frame.width as f32,
            height: frame.height as f32,
            hfov_deg: self.hfov_deg,
        };
        let landmarks = face.and_then(|f| f.landmarks.as_ref());
        let features = landmarks.and_then(expression::features);
        let Some(head) = landmarks.and_then(|lm| pose::estimate(lm, &camera)) else {
            return PoseOutput {
                expression: features,
                ..Default::default()
            };
        };
        self.collect_recenter(head);
        let mut pose = match &self.neutral {
            Some(neutral) => head.relative_to(neutral),
            None => head.absolute(),
        };
        // Correction des expressions : seulement en relatif (elle a été apprise ainsi).
        if let (Some(model), Some(f), Some(_)) = (&self.expression, &features, &self.neutral) {
            pose = Pose6::from_array(model.correct(pose.to_array(), f));
        }

        // dt réel entre deux poses (et non 1/30 s supposé) : si une image est
        // sautée ou si le visage a été perdu un instant, le filtre en tient compte.
        let dt = self.last_pose_at.map_or(0.0, |t| {
            frame.captured_at.saturating_duration_since(t).as_secs_f64()
        });
        self.last_pose_at = Some(frame.captured_at);
        let processed = self.processor.process(&pose, dt, &self.settings);
        PoseOutput {
            head: Some(head),
            pose: Some(pose),
            processed: Some(processed),
            expression: features,
        }
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
        let pose_output = state.pose(&frame, face.as_ref());
        let sink = state.send_output(pose_output.processed.as_ref());
        let PoseOutput {
            head,
            pose,
            processed,
            expression,
        } = pose_output;

        match tx.try_send(PipelineEvent::Frame(Box::new(FrameOutput {
            frame,
            decode_ms,
            face,
            head,
            pose,
            processed,
            expression,
            sink,
        }))) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use nalgebra::Vector3;
    use tracker_core::pose::Angles;
    use tracker_core::testing::{gray_frame, synthetic_landmarks};
    use vision::TrackMode;

    use super::*;

    const HFOV: f32 = 66.0;

    fn state() -> State {
        State {
            hfov_deg: HFOV,
            neutral: None,
            recenter: Some(Vec::new()),
            settings: TrackingSettings::default(),
            processor: PoseProcessor::default(),
            last_pose_at: None,
            expression: None,
            sink: None,
            connecting: None,
            sink_status: SinkStatus::default(),
        }
    }

    /// Image + résultat de visage pour une tête au lacet donné, à 60 cm.
    fn observe(state: &mut State, yaw: f64, at: Instant) -> (Option<Pose6>, Option<Processed>) {
        let frame = gray_frame(1920, 1080, 0, at);
        let camera = CameraModel {
            width: 1920.0,
            height: 1080.0,
            hfov_deg: state.hfov_deg,
        };
        let angles = Angles {
            yaw,
            pitch: 0.0,
            roll: 0.0,
        };
        let face = FaceResult {
            landmarks: Some(synthetic_landmarks(
                &camera,
                angles,
                Vector3::new(0.0, 0.0, -60.0),
            )),
            mode: TrackMode::Tracked,
            roi: None,
            detection_keypoints: None,
            detect_ms: 0.0,
            landmarks_ms: 0.0,
        };
        let out = state.pose(&frame, Some(&face));
        (out.pose, out.processed)
    }

    #[test]
    fn recentre_automatiquement_sur_la_premiere_pose() {
        let mut s = state();
        let t0 = Instant::now();
        let (first, _) = observe(&mut s, 20.0, t0);
        assert!(
            first.unwrap().angles.yaw.abs() < 1e-6,
            "la première pose devient le zéro"
        );
        let (next, _) = observe(&mut s, 30.0, t0 + Duration::from_millis(33));
        assert!((next.unwrap().angles.yaw - 10.0).abs() < 1e-4);
    }

    #[test]
    fn le_recentrage_fait_la_moyenne_de_plusieurs_images() {
        let mut s = state();
        let t0 = Instant::now();
        // Lacet qui oscille entre 18° et 22° pendant le recentrage : le zéro doit
        // être leur moyenne (20°), pas la première image (18°).
        for i in 0..RECENTER_FRAMES {
            let yaw = if i % 2 == 0 { 18.0 } else { 22.0 };
            observe(&mut s, yaw, t0 + Duration::from_millis(33 * i as u64));
        }
        assert!(s.recenter.is_none(), "recentrage terminé");
        let (pose, _) = observe(&mut s, 30.0, t0 + Duration::from_secs(1));
        let yaw = pose.unwrap().angles.yaw;
        // 15 images : 8 à 18°, 7 à 22° → moyenne 19,87°.
        assert!((yaw - (30.0 - 19.87)).abs() < 0.05, "lacet relatif {yaw}");
    }

    #[test]
    fn la_correction_des_expressions_est_appliquee() {
        let mut s = state();
        let t0 = Instant::now();
        observe(&mut s, 0.0, t0);
        // Modèle factice : décale le lacet de 100°/unité d'ouverture de bouche, et
        // l'expression mesurée sur ce visage synthétique sert de neutre → 0 décalage.
        let camera = CameraModel {
            width: 1920.0,
            height: 1080.0,
            hfov_deg: HFOV,
        };
        let lm = synthetic_landmarks(&camera, Angles::default(), Vector3::new(0.0, 0.0, -60.0));
        let neutral = expression::features(&lm).unwrap();
        let mut coeffs = [[0.0; NUM_FEATURES]; 6];
        coeffs[0][0] = 100.0;
        let model = ExpressionModel {
            neutral: [neutral[0] - 0.01, neutral[1], neutral[2]],
            coeffs,
            max_correction: [5.0; 6],
        };
        s.apply(PipelineCommand::SetExpressionModel(Some(Box::new(model))));
        let (pose, _) = observe(&mut s, 0.0, t0 + Duration::from_millis(33));
        // Expression « bouche ouverte de 0,01 » par rapport au neutre → −1° de correction.
        assert!((pose.unwrap().angles.yaw + 1.0).abs() < 1e-3);
    }

    #[test]
    fn la_pose_de_sortie_part_vers_opentrack() {
        use std::net::UdpSocket;
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();

        let mut s = state();
        s.apply(PipelineCommand::SetOutput(OutputSettings {
            enabled: true,
            host: "127.0.0.1".into(),
            port,
            invert: [false; 6],
        }));
        // La connexion se fait sur un autre thread : on attend qu'elle aboutisse.
        let t0 = Instant::now();
        let mut status = SinkStatus::default();
        for i in 0u32..200 {
            let (_, processed) = observe(
                &mut s,
                5.0 * f64::from(i % 2),
                t0 + Duration::from_millis(33 * u64::from(i)),
            );
            status = s.send_output(processed.as_ref());
            if status.sent {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(status.sent, "aucun envoi : {status:?}");
        assert!(status.target.unwrap().contains(&port.to_string()));
        let mut buf = [0u8; 64];
        let (n, _) = receiver.recv_from(&mut buf).unwrap();
        assert!(output::decode(&buf[..n]).is_some());
    }

    #[test]
    fn sortie_desactivee_rien_n_est_envoye() {
        let mut s = state();
        s.apply(PipelineCommand::SetOutput(OutputSettings::default()));
        let (_, processed) = observe(&mut s, 0.0, Instant::now());
        let status = s.send_output(processed.as_ref());
        assert!(!status.sent && status.target.is_none() && !status.connecting);
    }

    #[test]
    fn absolu_apres_clear_neutral() {
        let mut s = state();
        let t0 = Instant::now();
        observe(&mut s, 20.0, t0);
        s.apply(PipelineCommand::ClearNeutral);
        let (pose, _) = observe(&mut s, 20.0, t0 + Duration::from_millis(33));
        assert!((pose.unwrap().angles.yaw - 20.0).abs() < 1e-4);
    }

    #[test]
    fn changer_le_fov_redemande_un_recentrage() {
        let mut s = state();
        let t0 = Instant::now();
        observe(&mut s, 0.0, t0);
        s.apply(PipelineCommand::SetHfov(80.0));
        assert!(s.recenter.is_some());
        let (pose, _) = observe(&mut s, 15.0, t0 + Duration::from_millis(33));
        assert!(
            pose.unwrap().angles.yaw.abs() < 1e-6,
            "nouveau zéro après le changement de FOV"
        );
    }

    #[test]
    fn les_reglages_recus_sont_appliques() {
        let mut s = state();
        let settings = TrackingSettings {
            filter_enabled: false,
            curves_enabled: false,
            ..Default::default()
        };
        s.apply(PipelineCommand::SetSettings(Box::new(settings)));
        let t0 = Instant::now();
        observe(&mut s, 0.0, t0);
        let (pose, processed) = observe(&mut s, 12.0, t0 + Duration::from_millis(33));
        // Sans filtre ni courbe, la sortie est la pose brute.
        assert!((processed.unwrap().output.angles.yaw - pose.unwrap().angles.yaw).abs() < 1e-9);
    }

    #[test]
    fn pas_de_pose_sans_visage() {
        let mut s = state();
        let frame = gray_frame(640, 480, 0, Instant::now());
        let out = s.pose(&frame, None);
        assert!(out.head.is_none() && out.pose.is_none() && out.processed.is_none());
        assert!(s.recenter.is_some(), "le recentrage attend un vrai visage");
    }

    #[test]
    fn le_dt_du_filtre_suit_l_horloge_des_images() {
        // Même saut de 10°, une fois 33 ms après l'image précédente, une fois 1 s
        // après (visage perdu un moment). Réglages fixés ici (0,5 Hz, β = 0,05) :
        // - 33 ms : vitesse ≈ 300 °/s → coupure ≈ 3 Hz → α ≈ 0,39 → ≈ 3,9° ;
        // - 1 s   : vitesse ≈ 10 °/s → coupure ≈ 0,9 Hz → α ≈ 0,85 → ≈ 8,5°.
        // Si le dt était supposé constant, les deux cas donneraient la même valeur.
        let after = |delay: Duration| {
            let mut s = state();
            s.settings.curves_enabled = false;
            s.settings.rotation_filter = tracker_core::filter::OneEuroParams {
                min_cutoff: 0.5,
                beta: 0.05,
                d_cutoff: 1.0,
            };
            let t0 = Instant::now();
            observe(&mut s, 0.0, t0);
            observe(&mut s, 10.0, t0 + delay)
                .1
                .unwrap()
                .filtered
                .angles
                .yaw
        };
        let (fast, slow) = (
            after(Duration::from_millis(33)),
            after(Duration::from_secs(1)),
        );
        assert!((3.0..5.0).contains(&fast), "après 33 ms : {fast:.2}°");
        assert!((7.5..9.5).contains(&slow), "après 1 s : {slow:.2}°");
    }
}
