# Dépendances, composants natifs et données

Règle (voir [AGENTS.md](../AGENTS.md), § 6) : toute dépendance est justifiée ici, avec sa licence
et ce qu'elle apporte de natif ou de téléchargé. Versions relevées dans `Cargo.lock` ; environ
540 paquets au total, dépendances transitives comprises.

## Crates directes

| Crate | Version | Licence | Utilisée par | Rôle | Pourquoi elle / alternatives écartées |
|---|---|---|---|---|---|
| `nokhwa` | 0.10.11 | Apache-2.0 | `capture` | Capture webcam multiplateforme (AVFoundation sur macOS, Media Foundation sur Windows) | La plus complète en Rust. Alternatives : `opencv` (bindings C++ lourds), API natives (une implémentation par OS). Limite connue : décodeurs génériques lents sans optimisation, et formats macOS partiellement applicables (voir AGENTS.md § 7). |
| `ort` | 2.0.0-rc.15 | MIT OR Apache-2.0 | `vision` | Bindings ONNX Runtime 1.31 (inférence des modèles) | Choisi par l'utilisateur pour sa compatibilité totale avec ONNX. Alternative proposée : `tract` (pur Rust, plus simple à compiler pour Windows, mais couverture d'opérations moindre). Version « release candidate » : l'API change entre les rc (`Session::builder(&env)`, `Error<SessionBuilder>`). |
| `nalgebra` | 0.35.0 | Apache-2.0 | `tracker-core`, `app` | Algèbre linéaire : vecteurs, `Rotation3`, SVD 3×3 | Pur Rust, référence de l'écosystème. Alternative : `glam` (plus rapide pour le jeu vidéo, mais sans SVD). |
| `eframe` / `egui` | 0.37.0 | MIT OR Apache-2.0 | `app` | Interface graphique en mode immédiat, rendu wgpu | Simple et multiplateforme, idéal pour des outils de mesure. Changement d'API en 0.37 : `App::logic` + `App::ui`, `Panel::left/right`. |
| `egui_plot` | 0.38.0 | MIT OR Apache-2.0 | `app` | Courbes temporelles du banc de test | Compatible avec egui 0.37 (vérifié avec `cargo tree -i egui`). |
| `thiserror` | 2.0 | MIT OR Apache-2.0 | `capture`, `vision` | Erreurs typées des bibliothèques | Standard de fait. La version 1.0 présente dans le lock vient de `nokhwa`. |
| `anyhow` | 1.0 | MIT OR Apache-2.0 | `app` | Erreurs du binaire, avec contexte | Standard de fait pour les applications. |
| `tracing` | 0.1 | MIT | `app`, `capture`, `vision` | Journalisation structurée | Standard de l'écosystème ; `ort` l'utilise aussi. |
| `tracing-subscriber` | 0.3 | MIT | `app` | Affichage des logs, filtre `RUST_LOG` | Va avec `tracing`. |

Aucune autre crate n'est utilisée : pas de `rand` (petits générateurs écrits à la main), pas de
`csv` (format trivial écrit à la main), pas de `ndarray` (`ort` accepte des tranches).

## Composants natifs et téléchargés

| Composant | Provenance | Comment il arrive | Remarques |
|---|---|---|---|
| **ONNX Runtime 1.31** | Microsoft, via les binaires précompilés de pyke (auteur d'`ort`) | Téléchargé par `ort-sys` à la **première compilation** (fonction `download-binaries`), puis **lié statiquement** | Le binaire `immersion` fait ≈ 50 Mo en debug. Le framework système **CoreML** est lié dynamiquement sur macOS (utilisé seulement si `IMMERSION_DEVICE` le choisit). |
| AVFoundation (macOS) / Media Foundation (Windows) | système | via `nokhwa` | Permission caméra macOS : `capture::ensure_permission`. |
| wgpu → Metal (macOS) / DirectX 12 ou Vulkan (Windows) | système | via `eframe` | Rendu de l'interface. |

## Modèles et données

| Fichier | Taille | Source | Licence | Arrivée |
|---|---|---|---|---|
| `models/face_detector.onnx` | 418 Ko | Google MediaPipe BlazeFace (« short range », 128×128), converti en ONNX par le projet QtMeshEditor : [huggingface.co/fernandotonon/QtMeshEditor-models](https://huggingface.co/fernandotonon/QtMeshEditor-models) (`mocap/face/`) | Apache-2.0 | `scripts/fetch_models.sh` (non versionné) |
| `models/face_landmarks.onnx` | 4,9 Mo | Google MediaPipe Face Mesh V2 (478 points, 256×256), même source | Apache-2.0 | idem |
| `models/NOTICE.txt` | < 1 Ko | même source | — | idem (versionné) |
| `crates/tracker-core/src/canonical_face.rs` | 468 sommets | `canonical_face_model.obj` du dépôt officiel [google-ai-edge/mediapipe](https://github.com/google-ai-edge/mediapipe) (`mediapipe/modules/face_geometry/data/`) | Apache-2.0, © Google LLC | généré une fois et **intégré au code**, avec la mention de licence en tête du fichier |

Entrées et sorties des modèles (vérifiables avec `cargo run -p vision --example inspect_models`) :

| Modèle | Entrée | Sorties |
|---|---|---|
| détecteur | `input` [1, 128, 128, 3], RGB dans [−1, 1] | `regressors` [1, 896, 16], `classificators` [1, 896, 1] (logits) |
| points | `input_12` [N, 256, 256, 3], RGB dans [0, 1] | `Identity` [N, 1, 1, 1434] (478 × xyz en px du recadrage), `Identity_1` (logit de présence), `Identity_2` (non utilisé) |

## Outils

| Outil | Rôle |
|---|---|
| `rustup` | installation de la toolchain stable ; ajout plus tard de la cible `x86_64-pc-windows-msvc` |
| `cargo-xwin` *(phase 6, à installer)* | compiler pour Windows depuis macOS, ou utiliser un runner Windows en CI |
