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
    └── app/              # binaire `immersion` : interface egui
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
