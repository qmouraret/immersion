# Immersion

Head tracking par webcam pour Microsoft Flight Simulator 2024, écrit en Rust.
Le cahier des charges complet est dans [PROMPT.md](PROMPT.md).

## Structure

```
immersion/
├── Cargo.toml            # workspace : membres + versions de dépendances partagées
└── crates/
    ├── tracker-core/     # pipeline pur, sans E/S (types, maths de pose, filtres)
    ├── capture/          # webcam sur un thread dédié (seule crate qui connaît nokhwa)
    ├── vision/           # inférence ONNX Runtime (seule crate qui connaît ort)
    └── app/              # binaire `immersion` : interface egui + thread pipeline
```

Flux de données en phase 0 :

```
[thread capture-0]  nokhwa → décodage RGB → Frame ──canal borné (2)──▶ [thread UI]  texture GPU → écran
```

## Phase 0 — Capture et affichage

### Lancer

```bash
cargo run --bin immersion
```

Au premier lancement, macOS demande l'accès à la caméra **pour l'application qui lance le
programme** (Terminal, iTerm, VS Code…). En cas de refus : Réglages Système → Confidentialité
et sécurité → Caméra, puis activer ce terminal et relancer.

Pour plus de logs : `RUST_LOG=debug cargo run --bin immersion`.
Pour des mesures représentatives des performances : `cargo run --release --bin immersion`.

### Quoi observer

- Le flux de la webcam s'affiche, en miroir par défaut (case à cocher).
- **Format** : la résolution et la fréquence négociées avec la caméra.
- **Reçu** : environ 30 img/s pour la caméra d'un Mac.
- **Latence capture → écran** : quelques ms seulement. Elle ne mesure que notre pipeline, pas le capteur.
  - **dont décodage** : conversion du format de la caméra (NV12, YUYV…) en RGB.
  - **dont envoi GPU** : conversion en RGBA puis envoi de la texture.
  Si la somme dépasse 33 ms, on ne peut plus tenir 30 img/s.
- Pour voir tous les formats proposés par la caméra : `RUST_LOG=capture=debug cargo run --bin immersion`.
  On vise du 720p à 30 img/s au moins (`FormatTarget`) : le 1080p coûte deux fois plus à convertir,
  sans aider le tracking.
- **Images sautées** : doit rester à 0 ou presque. Si le nombre grimpe, l'interface n'arrive pas à suivre (mode debug, fenêtre en arrière-plan).
- Changer de caméra dans la liste, par exemple un iPhone en Continuity Camera, arrête proprement l'ancien flux.

### Concepts Rust de cette phase

| Concept | Où le voir |
|---|---|
| Workspace et `workspace.dependencies` | `Cargo.toml`, `crates/*/Cargo.toml` |
| Modules, `pub use` (réexport) | `tracker-core/src/lib.rs` |
| `Option`, `bool::then` | `Frame::new` |
| Erreurs typées avec `thiserror`, conversion `#[from]` et `?` | `capture::CaptureError`, `run` |
| `anyhow` et `.context()` dans un binaire | `app/src/main.rs` |
| Threads, `move`, `Arc<AtomicBool>` | `CaptureHandle::start` |
| Canal borné `sync_channel` et `try_send` (contre-pression) | `capture::run` |
| RAII : `impl Drop` pour arrêter le thread | `CaptureHandle` |
| Pourquoi `Camera` est créée *dans* le thread (`Send`) | commentaire dans `start` |
| `let ... else`, chaînes `if ... && let` (Rust 2024) | `capture::run`, `side_panel` |
| `#[cfg(target_os = "macos")]` : code spécifique à un OS | `ensure_permission` |
| Monomorphisation : le code générique est compilé dans la crate qui l'appelle | `Cargo.toml` (`profile.dev.package`), `examples/decode_bench.rs` |
| Mesurer avant d'optimiser : un exemple cargo comme micro-benchmark | `cargo run -p capture --example decode_bench` |
| Tests unitaires dans le même fichier (`#[cfg(test)]`) | `tracker-core/src/frame.rs` |

### Exercices facultatifs

1. Ajouter dans le panneau la **largeur de bande** reçue en Mo/s (largeur × hauteur × 3 × fps).
2. Faire en sorte que la capture appelle `ctx.request_repaint()` à chaque image, au lieu de redessiner
   en continu. Indice : `egui::Context` est `Clone + Send`. Mais `capture` ne doit pas dépendre d'egui :
   comment découpler les deux ? (closure `Box<dyn Fn() + Send>`)

## Phase 1 — Points du visage

### Préparer

```bash
./scripts/fetch_models.sh   # 2 modèles MediaPipe convertis en ONNX (≈ 5 Mo, Apache-2.0)
cargo run --bin immersion
```

La première compilation télécharge ONNX Runtime (bibliothèque native, via la crate `ort`).

### Pipeline

```
[capture] ─Frame─▶ [pipeline]                                         ─▶ [UI]
                    ├ visage inconnu : BlazeFace 128×128 → boîte + yeux  (≈ 2 ms)
                    ├ recadrage orienté 256×256 → 478 points + présence  (≈ 10 ms)
                    └ image suivante : recadrage déduit des points (suivi)
```

Le détecteur ne tourne que si le visage est perdu (présence < 50 %). Le reste du
temps, on *suit* le visage à partir des points de l'image précédente.

### Quoi observer

- **Points verts** : confiance > 80 %. Oranges : en dessous. Points cyan : les iris.
- **Cadre bleu** : recadrage en mode suivi. Orange : le détecteur vient de tourner.
- Tourner, incliner la tête : le cadre doit pivoter avec elle, et les points rester collés au visage.
- **Redétections** doit rester stable quand on bouge normalement. Il augmente si on sort du champ ou si
  on masque le visage. C'est un bon test pour les lunettes : reflets, monture.
- **Points** doit rester autour de 10 ms. Comparer les moteurs d'exécution :
  `IMMERSION_DEVICE=efficiency cargo run --bin immersion` (CoreML) est environ 3 fois plus lent ici.

### Outils de mesure

```bash
cargo run -p vision --example inspect_models     # entrées / sorties des modèles
cargo run -p vision --example bench_models       # temps d'inférence
cargo run -p vision --example profile_landmarks  # profil ORT par opération (JSON)
cargo run -p tracker-core --example bench_sample # coût du recadrage
```

### Concepts Rust de cette phase

| Concept | Où le voir |
|---|---|
| Garder le cœur pur : maths testables, aucune dépendance native | `tracker-core` (roi, tensor, blazeface, face) |
| Emprunt sans copie vers une bibliothèque C (`TensorRef`) | `vision::FaceTracker::detect` |
| Tampon réutilisé (`Vec::clear` + `reserve`) : aucune allocation par image | `tensor::sample_roi` |
| `as_chunks::<N>()` : tableaux de taille fixe, sans vérifications de bornes | `blazeface::decode`, `FaceLandmarks::from_model_output` |
| Clé de tri `total_cmp` sur des `f32` (ordre total) | `blazeface::decode` |
| Erreurs qui transportent une valeur à récupérer (`ort::Error<SessionBuilder>`) | `FaceTracker::new` |
| Chaîne de `Drop` : pipeline → capture, arrêt en cascade | `app/src/pipeline.rs` |
| Mesurer plutôt que supposer : le « bon » accélérateur était le plus lent | `device_policy` dans `vision/src/lib.rs` |

### Exercices facultatifs

1. Afficher les 6 points clés de BlazeFace (`detection_keypoints`) quand le détecteur tourne.
2. `sample_roi` prend environ 4,5 ms pour le visage. Le paralléliser par lignes avec `std::thread::scope`
   (sans crate externe) et mesurer avec `bench_sample`.
