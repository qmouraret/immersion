//! Profils d'étalonnage, un par webcam, en TOML.
//!
//! Emplacement : dossier de configuration de l'OS (`~/Library/Application Support/
//! immersion/profiles` sur macOS, `%APPDATA%\immersion\profiles` sur Windows), ou
//! `IMMERSION_CONFIG_DIR` s'il est défini. Le fichier est lisible et modifiable
//! à la main.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use output::OutputSettings;
use serde::{Deserialize, Serialize};
use tracker_core::expression::ExpressionModel;
use tracker_core::processing::TrackingSettings;

/// Tout ce que l'étalonnage d'une webcam produit.
///
/// `#[serde(default)]` : un champ absent (fichier ancien ou édité à la main)
/// prend sa valeur par défaut au lieu de faire échouer la lecture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// Nom de la webcam (sert aussi à nommer le fichier).
    pub camera: String,
    /// FOV horizontal de la caméra, en degrés.
    pub hfov_deg: f32,
    /// Filtres et courbes.
    pub settings: TrackingSettings,
    /// Correction des expressions, si elle a été apprise.
    pub expression: Option<ExpressionModel>,
    /// Envoi vers le simulateur.
    pub output: OutputSettings,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            camera: String::new(),
            hfov_deg: 66.0,
            settings: TrackingSettings::default(),
            expression: None,
            output: OutputSettings::default(),
        }
    }
}

/// Dossier des profils.
pub fn profiles_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("IMMERSION_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("profiles"));
    }
    dirs::config_dir().map(|d| d.join("immersion").join("profiles"))
}

/// Nom de fichier sûr à partir du nom de la webcam (« FaceTime HD Camera » →
/// `FaceTime_HD_Camera.toml`).
fn file_name(camera: &str) -> String {
    let safe: String = camera
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.toml", if safe.is_empty() { "camera" } else { &safe })
}

/// Chemin du profil de `camera` dans `dir`.
pub fn path_for(dir: &Path, camera: &str) -> PathBuf {
    dir.join(file_name(camera))
}

/// Charge le profil de `camera` depuis `dir` ; `Ok(None)` s'il n'existe pas.
pub fn load_from(dir: &Path, camera: &str) -> anyhow::Result<Option<Profile>> {
    let path = path_for(dir, camera);
    if !path.exists() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("lecture de {}", path.display()))?;
    let profile = toml::from_str(&text).with_context(|| format!("format de {}", path.display()))?;
    Ok(Some(profile))
}

/// Enregistre le profil dans `dir` ; renvoie le chemin du fichier.
pub fn save_to(dir: &Path, profile: &Profile) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    let path = path_for(dir, &profile.camera);
    let text = toml::to_string_pretty(profile).context("sérialisation du profil")?;
    fs::write(&path, text).with_context(|| format!("écriture de {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("immersion-profile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn nom_de_fichier_sur() {
        assert_eq!(file_name("FaceTime HD Camera"), "FaceTime_HD_Camera.toml");
        assert_eq!(file_name("../../etc"), "______etc.toml");
        assert_eq!(file_name(""), "camera.toml");
    }

    #[test]
    fn aller_retour() {
        let dir = temp_dir("rt");
        let mut profile = Profile {
            camera: "Webcam C920".into(),
            hfov_deg: 70.5,
            ..Default::default()
        };
        profile.settings.curves[0].input = 33.0;
        profile.expression = Some(ExpressionModel {
            neutral: [0.01, 0.1, 0.05],
            coeffs: [[1.0, 2.0, 3.0]; 6],
            max_correction: [4.0; 6],
        });
        let path = save_to(&dir, &profile).unwrap();
        assert!(path.ends_with("Webcam_C920.toml"));
        assert_eq!(load_from(&dir, "Webcam C920").unwrap(), Some(profile));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn profil_absent() {
        assert_eq!(load_from(&temp_dir("absent"), "rien").unwrap(), None);
    }

    #[test]
    fn champs_manquants_completes_par_defaut() {
        let dir = temp_dir("partial");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cam.toml"), "camera = \"cam\"\nhfov_deg = 72.0\n").unwrap();
        let p = load_from(&dir, "cam").unwrap().unwrap();
        assert_eq!(p.hfov_deg, 72.0);
        assert_eq!(p.settings, TrackingSettings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fichier_invalide_donne_une_erreur_claire() {
        let dir = temp_dir("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cam.toml"), "hfov_deg = \"soixante\"").unwrap();
        let err = load_from(&dir, "cam").unwrap_err();
        assert!(format!("{err:#}").contains("cam.toml"));
        let _ = fs::remove_dir_all(&dir);
    }
}
