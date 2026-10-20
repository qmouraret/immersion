//! Inférence ML : détection du visage et points clés, avec ONNX Runtime.
//!
//! [`FaceTracker`] enchaîne les deux modèles comme MediaPipe :
//! - **détection** (BlazeFace) quand on ne sait pas où est le visage ;
//! - **suivi** ensuite : le recadrage de l'image suivante est déduit des
//!   points de l'image courante, et le détecteur n'est plus appelé.

use std::path::Path;
use std::time::Instant;

use ort::session::Session;
use ort::session::builder::{AutoDevicePolicy, GraphOptimizationLevel};
use ort::value::TensorRef;
use tracker_core::blazeface::{self, Detection};
use tracker_core::face::{self, FaceLandmarks, LANDMARK_INPUT_SIZE};
use tracker_core::tensor::{Normalization, sample_roi};
use tracker_core::{Frame, Roi};

/// Seuil de présence sous lequel on considère le visage perdu.
const MIN_PRESENCE: f32 = 0.5;

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("modèle introuvable : {0} (lancer scripts/fetch_models.sh)")]
    MissingModel(String),
    #[error("ONNX Runtime : {0}")]
    Ort(#[from] ort::Error),
    #[error("sortie inattendue du modèle : {0}")]
    BadOutput(&'static str),
}

/// Comment le visage a été localisé sur cette image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMode {
    /// Le détecteur a été lancé (premier passage ou visage perdu).
    Detected,
    /// Recadrage déduit de l'image précédente.
    Tracked,
}

/// Résultat pour une image.
#[derive(Debug, Clone)]
pub struct FaceResult {
    pub landmarks: Option<FaceLandmarks>,
    pub mode: TrackMode,
    /// Recadrage utilisé par le modèle de points (pour l'affichage).
    pub roi: Option<Roi>,
    /// Points clés BlazeFace en pixels image, si le détecteur a tourné.
    pub detection_keypoints: Option<Vec<(f32, f32)>>,
    pub detect_ms: f64,
    pub landmarks_ms: f64,
}

pub struct FaceTracker {
    detector: Session,
    landmarker: Session,
    anchors: Vec<(f32, f32)>,
    /// Recadrage prévu pour la prochaine image (`None` = visage perdu).
    next_roi: Option<Roi>,
    /// Tampon d'entrée réutilisé (évite une allocation par image).
    input: Vec<f32>,
}

impl FaceTracker {
    /// Charge les deux modèles depuis `models_dir`.
    pub fn new(models_dir: &Path) -> Result<Self, VisionError> {
        // L'environnement ORT est global au processus ; on le crée ici, les
        // sessions en gardent une référence interne.
        let env = ort::init().with_name("immersion").build()?;
        let load = |name: &str| -> Result<Session, VisionError> {
            let path = models_dir.join(name);
            if !path.exists() {
                return Err(VisionError::MissingModel(path.display().to_string()));
            }
            // Les erreurs du builder transportent le builder (pour le récupérer) :
            // `map_err(ort::Error::from)` les ramène à une erreur ORT ordinaire.
            let builder = Session::builder(&env)?;
            let builder = builder
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(ort::Error::from)?;
            let builder = builder
                .with_intra_threads(intra_threads())
                .map_err(ort::Error::from)?;
            let mut builder = builder
                .with_auto_device(device_policy())
                .map_err(ort::Error::from)?;
            Ok(builder.commit_from_file(path)?)
        };
        Ok(Self {
            detector: load("face_detector.onnx")?,
            landmarker: load("face_landmarks.onnx")?,
            anchors: blazeface::anchors(),
            next_roi: None,
            input: Vec::new(),
        })
    }

    /// Traite une image : localise le visage puis calcule ses points.
    pub fn process(&mut self, frame: &Frame) -> Result<FaceResult, VisionError> {
        let mut result = FaceResult {
            landmarks: None,
            mode: TrackMode::Tracked,
            roi: None,
            detection_keypoints: None,
            detect_ms: 0.0,
            landmarks_ms: 0.0,
        };

        // 1) Suivi si possible, sinon détection.
        let roi = match self.next_roi {
            Some(roi) => roi,
            None => {
                result.mode = TrackMode::Detected;
                let start = Instant::now();
                let found = self.detect(frame)?;
                result.detect_ms = ms_since(start);
                let Some((det, det_roi)) = found else {
                    return Ok(result); // pas de visage dans l'image
                };
                result.detection_keypoints = Some(
                    det.keypoints
                        .iter()
                        .map(|&(u, v)| det_roi.to_image(u, v))
                        .collect(),
                );
                match face::roi_from_detection(&det, &det_roi) {
                    Some(roi) => roi,
                    None => return Ok(result),
                }
            }
        };

        // 2) Points du visage dans ce recadrage.
        let start = Instant::now();
        let landmarks = self.landmarks(frame, &roi)?;
        result.landmarks_ms = ms_since(start);
        result.roi = Some(roi);

        // 3) Préparer l'image suivante : suivi si le visage est toujours là.
        self.next_roi = landmarks
            .as_ref()
            .filter(|lm| lm.presence >= MIN_PRESENCE)
            .and_then(FaceLandmarks::next_roi);
        result.landmarks = landmarks;
        Ok(result)
    }

    /// Lance uniquement le modèle de points sur `roi` (diagnostic, benchmarks).
    /// Renvoie le temps d'inférence et la présence.
    pub fn landmarks_only(&mut self, frame: &Frame, roi: &Roi) -> Result<(f64, f32), VisionError> {
        let start = Instant::now();
        let lm = self.landmarks(frame, roi)?;
        Ok((ms_since(start), lm.map_or(0.0, |l| l.presence)))
    }

    /// Force une nouvelle détection à l'image suivante.
    pub fn reset(&mut self) {
        self.next_roi = None;
    }

    fn detect(&mut self, frame: &Frame) -> Result<Option<(Detection, Roi)>, VisionError> {
        let size = blazeface::INPUT_SIZE;
        let roi = Roi::full_image(frame.width, frame.height);
        sample_roi(
            frame,
            &roi,
            size,
            Normalization::MinusOneToOne,
            &mut self.input,
        );

        // `TensorRef` : le tenseur *emprunte* notre tampon, pas de copie.
        let input = TensorRef::from_array_view(([1usize, size, size, 3], self.input.as_slice()))?;
        let outputs = self.detector.run(ort::inputs!["input" => input])?;
        let (_, regressors) = outputs["regressors"].try_extract_tensor::<f32>()?;
        let (_, scores) = outputs["classificators"].try_extract_tensor::<f32>()?;
        if regressors.len() != self.anchors.len() * blazeface::NUM_COORDS {
            return Err(VisionError::BadOutput("regressors"));
        }

        Ok(blazeface::decode(regressors, scores, &self.anchors).map(|d| (d, roi)))
    }

    fn landmarks(
        &mut self,
        frame: &Frame,
        roi: &Roi,
    ) -> Result<Option<FaceLandmarks>, VisionError> {
        let size = LANDMARK_INPUT_SIZE;
        sample_roi(frame, roi, size, Normalization::ZeroToOne, &mut self.input);

        let input = TensorRef::from_array_view(([1usize, size, size, 3], self.input.as_slice()))?;
        let outputs = self.landmarker.run(ort::inputs!["input_12" => input])?;
        let (_, raw) = outputs["Identity"].try_extract_tensor::<f32>()?;
        let (_, presence_logit) = outputs["Identity_1"].try_extract_tensor::<f32>()?;
        let logit = *presence_logit
            .first()
            .ok_or(VisionError::BadOutput("Identity_1"))?;
        let presence = 1.0 / (1.0 + (-logit).exp());

        Ok(FaceLandmarks::from_model_output(raw, roi, presence))
    }
}

/// Choix du matériel d'inférence, laissé à ONNX Runtime (sélection automatique).
///
/// ort active par défaut `MaxEfficiency`, qui sur Mac choisit CoreML : or ce
/// modèle y est découpé en ~20 morceaux entrecoupés d'opérations `PRelu`
/// renvoyées au CPU, et les allers-retours coûtent plus qu'ils ne rapportent.
/// `IMMERSION_DEVICE=cpu|gpu|npu|efficiency` permet de comparer.
fn device_policy() -> AutoDevicePolicy {
    match std::env::var("IMMERSION_DEVICE").as_deref() {
        Ok("gpu") => AutoDevicePolicy::PreferGPU,
        Ok("npu") => AutoDevicePolicy::PreferNPU,
        Ok("efficiency") => AutoDevicePolicy::MaxEfficiency,
        _ => AutoDevicePolicy::PreferCPU,
    }
}

/// Threads ONNX Runtime par modèle (variable `IMMERSION_THREADS`, 4 par défaut).
fn intra_threads() -> usize {
    std::env::var("IMMERSION_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
}

fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
