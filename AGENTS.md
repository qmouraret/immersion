# AGENTS.md — Guide du projet Immersion

Point d'entrée pour toute personne (ou tout agent) qui lit, modifie ou étend ce dépôt.
Le guide utilisateur, phase par phase, est dans [README.md](README.md). Le cahier des charges
d'origine est dans [PROMPT.md](PROMPT.md).

- Dépendances, licences et modèles : [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)
- Stratégie de test, couverture et tests manuels : [docs/TESTS.md](docs/TESTS.md)

---

## 1. Le projet

**Immersion** est un *head tracker* par webcam, inspiré de Beam Eye Tracker. Il estime la pose de
la tête en 6 degrés de liberté (lacet, tangage, roulis, x, y, z) et l'enverra à
**Microsoft Flight Simulator 2024**, pour déplacer la vue du cockpit.

C'est aussi un **projet d'apprentissage** : de Rust, de la vision par ordinateur et de l'intégration
avec un simulateur. Le code est donc écrit pour être lu : chaque choix est commenté, et chaque
phase du README liste les concepts Rust qu'elle introduit.

| | |
|---|---|
| Développement | macOS (Apple Silicon), sans PC Windows pour l'instant |
| Cible finale | Windows 10/11 x64 (MSFS 2024) |
| Langage | Rust, édition 2024 (toolchain stable, 1.99 au moment de l'écriture) |
| Langue | **Commentaires, documentation, interface et messages de commit en français** ; identifiants en anglais |

### Avancement

| Phase | Contenu | État |
|---|---|---|
| 0 | Capture webcam, affichage | ✅ |
| 1 | Détection du visage, 478 points | ✅ |
| 2 | Pose 6DOF | ✅ |
| 3 | Lissage One-Euro, courbes, recentrage | ✅ |
| 4 | Banc de test (mire, scène 3D, courbes, enregistrements), réglage automatique | ✅ |
| 5 | Assistant d'étalonnage (webcam déportée, écran, FOV) | à faire |
| 6 | Sortie UDP OpenTrack, configuration persistante, build Windows | à faire |
| 7 | Intégration MSFS 2024, puis émulation TrackIR native | à faire |
| 8 | Suivi du regard (iris), épaules | à faire |

On avance **une phase à la fois**. À la fin de chaque phase, on la fait valider par l'utilisateur
avant de la committer.

---

## 2. Démarrage rapide

```bash
./scripts/fetch_models.sh            # modèles ONNX (≈ 5 Mo), une fois
cargo run --bin immersion            # application
cargo test                           # tous les tests automatiques
cargo clippy --all-targets           # doit rester sans avertissement
cargo doc --no-deps --open           # documentation de référence des crates
```

- **Permission caméra (macOS)** : elle est accordée à l'*application qui lance* le programme
  (Terminal, iTerm…). Un agent qui lance le binaire depuis son propre shell n'aura pas la caméra :
  c'est l'utilisateur qui lance l'application et rapporte ce qu'il voit.
- La première compilation télécharge ONNX Runtime (via la crate `ort`). Il est ensuite lié statiquement.
- Variables d'environnement :

| Variable | Effet |
|---|---|
| `RUST_LOG` | niveau de log (`capture=debug`, `info`…) |
| `IMMERSION_MODELS` | dossier des modèles (`models` par défaut) |
| `IMMERSION_DEVICE` | matériel d'inférence : `cpu` (défaut), `gpu`, `npu`, `efficiency` |
| `IMMERSION_THREADS` | threads ONNX Runtime par modèle (4 par défaut) |
| `IMMERSION_ROI_STABILIZE` | `0` désactive la stabilisation du recadrage (pour comparer) |

---

## 3. Structure

```
immersion/
├── AGENTS.md  README.md  PROMPT.md
├── docs/                    DEPENDENCIES.md, TESTS.md
├── scripts/fetch_models.sh  téléchargement des modèles (non versionnés)
├── models/                  *.onnx (ignorés par git) + NOTICE.txt
├── recordings/              sessions enregistrées par le banc de test (ignoré par git)
└── crates/
    ├── tracker-core/        LOGIQUE PURE : géométrie, pose, filtres, courbes, stats, CSV
    ├── capture/             webcam (nokhwa) sur un thread dédié
    ├── vision/              inférence ONNX (ort) : détection + points du visage
    └── app/                 binaire `immersion` : egui, pipeline, banc de test
```

### Responsabilités et dépendances entre crates

```
app ──▶ vision ──▶ tracker-core
 │  └─▶ capture ──▶ tracker-core
 └────────────────▶ tracker-core
```

| Crate | Contient | Seule crate autorisée à utiliser |
|---|---|---|
| `tracker-core` | `Frame`, `Roi` (et sa stabilisation), recadrage → tenseur, décodage BlazeFace, `FaceLandmarks`, visage canonique, pose (Umeyama), One-Euro, courbes, `PoseProcessor`, stats (rugosité, retard), enregistrement CSV, réglage automatique (`autotune`), données synthétiques de test | — (aucune E/S) |
| `capture` | énumération des caméras, choix du format, thread de capture | `nokhwa` |
| `vision` | `FaceTracker` : enchaînement détection / suivi | `ort` |
| `app` | fenêtre, thread pipeline, réglages, banc de test | `eframe`, `egui`, `egui_plot` |

Modules principaux de `app` :

| Module | Rôle |
|---|---|
| `main.rs` | journalisation, permission caméra, lancement d'eframe |
| `pipeline.rs` | thread de traitement : capture → visage → pose → filtre → courbes ; commandes de l'UI |
| `camera_view.rs` | application egui : panneaux, vidéo, superposition des points, onglets |
| `tuning.rs` | panneau des réglages (filtre, courbes) |
| `bench/` | banc de test : `target` (mire), `scene` (3D), `plots` (courbes), `recordings` |

---

## 4. Architecture d'exécution

```
 [thread capture-N]                [thread pipeline]                              [thread UI (egui)]
 nokhwa.frame()                    FaceTracker.process()                           texture + overlay
 décodage RGB      ─ canal(2) ─▶   pose::estimate()             ─ canal(2) ─▶      Bench.push(Sample)
 Frame{captured_at}                recentrage → PoseProcessor                      panneaux
                                   ◀──────── PipelineCommand (canal non borné) ────
```

- **Propriété des threads** : chaque thread appartient à une poignée (`CaptureHandle`, `Pipeline`).
  Son `Drop` lève un drapeau d'arrêt (`Arc<AtomicBool>`) puis fait `join()`. Détruire le `Pipeline`
  arrête donc la capture, en cascade.
- **Contre-pression** : les canaux d'images sont bornés à 2 et alimentés par `try_send`. Si le
  consommateur est en retard, **on jette des images plutôt que d'accumuler de la latence**.
  Le pipeline ne traite que la plus récente des images en attente.
- **Cohérence** : l'image et ses résultats voyagent ensemble (`FrameOutput`). Les points affichés
  correspondent toujours à l'image affichée.
- **Temps** : chaque `Frame` est horodatée (`captured_at`) juste après la lecture du capteur, avant
  le décodage. Le filtre utilise le **dt réel** entre deux poses.
- **Commandes** : l'UI pilote le pipeline par messages (`Recenter`, `ClearNeutral`, `SetHfov`,
  `SetSettings`). L'état de la pose (pose neutre, filtres) appartient au thread pipeline. Un
  recentrage automatique a lieu sur la première pose, et après chaque changement de FOV.

### Budget de performance (MacBook Pro M1 Pro, 1080p)

| Étape | Temps | Où le mesurer |
|---|---|---|
| Décodage YUYV → RGB | ≈ 7 ms | panneau « Flux », `cargo run -p capture --example decode_bench` |
| Détecteur (seulement si le visage est perdu) | ≈ 2 ms | panneau « Visage », `bench_models` |
| Points du visage (recadrage + inférence) | ≈ 9,5 ms | panneau « Visage », `bench_models`, `profile_landmarks` |
| Pose + filtre + courbes | < 0,1 ms | — |
| Capture → écran | ≈ 20–25 ms | panneau « Flux » |

Objectif : **≥ 30 img/s** et **< 50 ms** de latence de bout en bout.

---

## 5. Conventions (à respecter partout)

### Repères

| Repère | Axes | Unités | Défini dans |
|---|---|---|---|
| **Image** | x → droite de l'image, y ↓ bas, origine en haut à gauche ; pixel *i* centré en *i + 0,5* | px | `tensor.rs` |
| **ROI** | carré orienté ; axe horizontal du recadrage = `(cos rotation, sin rotation)` dans l'image | px, rad | `roi.rs` |
| **Points MediaPipe** | x, y en px image ; z relatif (même échelle que x), **négatif = plus près** de la caméra | px | `face.rs` |
| **Visage canonique** | x → gauche *de la personne*, y ↑, z → avant du visage ; origine ≈ centre de la tête | cm | `canonical_face.rs` |
| **Caméra (C)** | x → droite de l'image, y ↑, z vers l'utilisateur (la caméra regarde vers −z) | cm | `pose.rs` |
| **Sortie (`Pose6`)** | point de vue de l'utilisateur face à l'écran (voir ci-dessous) | °, cm | `pose.rs` |
| **Scène 3D du banc** | x → droite, y ↑, z → arrière ; l'œil regarde vers −z | m | `bench/scene.rs` |

### Signes de la sortie (`Pose6`), point de vue de l'utilisateur

| Axe | Positif quand on… |
|---|---|
| lacet | tourne la tête vers la droite |
| tangage | lève la tête |
| roulis | penche la tête vers l'épaule droite |
| x / y / z | se décale à droite / monte / **recule** (s'éloigne de l'écran) |

Les tableaux d'axes suivent toujours l'ordre `[lacet, tangage, roulis, x, y, z]`
(`Pose6::to_array`, `processing::AXES`).

> Les conventions de sortie vers OpenTrack ou MSFS (phase 6) seront converties **à la frontière**,
> dans la crate de sortie. On ne change jamais les conventions internes.

### Indices de points utiles

- **BlazeFace** (`blazeface::keypoint`) : 0 = œil droit, 1 = œil gauche, 2 = nez, 3 = bouche.
- **Face Mesh** (`face::index`) : 33 / 263 = coins externes des yeux, 1 = bout du nez,
  152 = menton, 468 / 473 = centres des iris.

---

## 6. Règles d'implémentation

### Architecture

1. **`tracker-core` reste pur** : aucune E/S (fichiers, réseau, caméra, UI), aucun moteur ML,
   aucune dépendance native. Seules des dépendances en pur Rust y sont admises (aujourd'hui :
   `nalgebra`). Tout ce qui peut s'y écrire doit s'y écrire, car c'est là que tout est testable.
2. **Une dépendance lourde par crate** : `nokhwa` reste dans `capture`, `ort` dans `vision`,
   egui dans `app`. Changer de bibliothèque ne doit toucher qu'une crate.
3. **Rien de spécifique à un OS hors d'un `#[cfg(...)]`**. La cible est Windows : tout code propre
   à macOS (permission caméra…) est isolé et a un équivalent, même vide, sur les autres OS.
4. **Les conventions de la section 5 sont la référence.** Toute nouvelle grandeur précise son repère,
   son unité et son signe dans sa documentation.

### Temps réel

5. **Jamais de blocage dans le thread UI.** Le calcul se fait dans le pipeline ; l'UI affiche et
   envoie des commandes.
6. **Images les plus récentes d'abord** : canaux bornés + `try_send`. Jeter une image vaut mieux
   qu'accumuler du retard.
7. **Pas d'allocation par image sur le chemin critique** quand c'est mesurable (tampon d'entrée de
   `FaceTracker`, `Vec` réutilisés). On ne micro-optimise pas sans mesure.
8. **Mesurer avant d'optimiser.** Chaque optimisation s'appuie sur un exemple de mesure
   (`examples/`) ou sur le profileur d'ORT, et le gain est noté.
9. **Optimiser en debug les crates qui touchent aux pixels** (`[profile.dev.package.*]` dans
   `Cargo.toml`). Le code générique est compilé dans la crate qui l'appelle
   (monomorphisation) : une crate non optimisée qui appelle un décodeur générique le rend 60 fois plus lent.

### Erreurs et robustesse

10. **Bibliothèques : erreurs typées** avec `thiserror`. **Binaire : `anyhow`** et `.context()`.
11. **Pas de `unwrap()` ni d'`expect()` sur un chemin d'exécution**, sauf pour un invariant garanti
    par construction et documenté dans le message (`expect("taille cohérente par construction")`).
    Seule autre exception : l'échec de **création d'un thread** (`thread::Builder::spawn`). C'est un
    épuisement des ressources du système, comme un échec d'allocation, et `std::thread::spawn`
    panique d'ailleurs dans ce cas. Les tests peuvent utiliser `unwrap` et `expect` librement.
12. **Une erreur non fatale ne coupe pas le flux** : sans modèles, la vidéo s'affiche quand même,
    avec un message. Seules les erreurs de capture arrêtent le pipeline (`fatal: true`).
13. **Flottants** : borner avant `asin` et `acos` (`clamp(-1, 1)`), utiliser `total_cmp` pour trier,
    se méfier de `angle_to` (NaN pour deux rotations identiques).

### Threads

14. **Chaque thread a un propriétaire** dont le `Drop` l'arrête (drapeau + `join`). Pas de thread
    « détaché ».
15. **Un objet qui n'est pas `Send` se crée dans le thread qui l'utilise** (`nokhwa::Camera`).

### Qualité, tests et documentation

16. **`cargo fmt`, `cargo clippy --all-targets` sans avertissement et `cargo test` vert avant chaque commit.**
17. **Toute logique pure a des tests unitaires** dans le même fichier (`#[cfg(test)] mod tests`),
    nommés en français, en `snake_case`, sous forme de phrase (`recentre_automatiquement_sur_la_premiere_pose`).
18. **Tester avec des données synthétiques dont on connaît la vérité** :
    `tracker_core::testing::synthetic_landmarks` (tête à pose connue) et `gray_frame`.
19. **Les tests qui ont besoin du matériel** sont marqués `#[ignore = "raison"]`. **Ceux qui ont
    besoin des modèles** s'arrêtent sans échouer si les modèles manquent.
20. **Les bibliothèques ont `#![warn(missing_docs)]`** : tout élément public est documenté, et
    `cargo doc` ne doit signaler aucun lien cassé.
21. **Les commentaires expliquent le *pourquoi*** (choix, pièges, unités), pas ce que le code dit déjà.
    Comme le projet sert à apprendre, un concept Rust nouveau est expliqué brièvement à sa première
    apparition.
22. **Chaque phase met à jour** le README (lancer, quoi observer, concepts Rust, exercices),
    `docs/TESTS.md` (tests manuels) et ce fichier si une règle ou une convention change.

### Dépendances et données

23. **Toute nouvelle dépendance est justifiée** dans `docs/DEPENDENCIES.md` : rôle, alternative
    écartée, licence (MIT ou Apache-2.0 de préférence), composants natifs ou téléchargés.
24. **Les modèles et les gros binaires ne sont pas versionnés** : un script les télécharge depuis
    une source documentée. Une donnée intégrée au code (visage canonique) garde sa mention de licence.
25. **On demande l'accord de l'utilisateur avant tout téléchargement**, en indiquant le fichier,
    la source et la taille.

### Git

26. **Un commit par phase validée** (plus les correctifs). Message en français : un titre
    `Phase N : …`, puis une liste des changements.

---

## 7. Pièges connus (déjà rencontrés)

| Symptôme | Cause | Solution en place |
|---|---|---|
| 2 img/s, décodage à 400 ms | monomorphisation : décodeur générique compilé sans optimisation | `opt-level = 3` pour `capture`, `tracker-core`, `vision` en debug |
| Format caméra « refusé » sur macOS | nokhwa/AVFoundation n'applique que la fréquence **maximale** de chaque plage | `rank_formats` + essais successifs (`apply_best_format`) |
| Inférence 3 fois trop lente sur Mac | `ort` active par défaut `AutoDevicePolicy::MaxEfficiency` → CoreML, découpé en ~20 morceaux | `PreferCPU` par défaut (`IMMERSION_DEVICE`) |
| CoreML MLProgram : « pad missing » | opération non gérée par le format MLProgram | ne pas forcer CoreML (voir ligne précédente) |
| Points étirés quand on redimensionne | rectangle du widget ≠ rectangle de l'image | `fit_aspect` : un seul rectangle pour l'image et la superposition |
| Z bloqué à +30 cm | pose absolue (≈ 60 cm) passée dans les courbes | recentrage automatique |
| `NaN` dans un test de rotation | `acos` d'un cosinus calculé légèrement > 1 | comparer les matrices |
| API egui inattendue | egui 0.37 : `App::logic` + `App::ui`, `Panel::left`, `egui_wants_keyboard_input` | lire les sources de la crate dans `~/.cargo/registry` |
| L'interface tremble malgré le filtre | les courbes amplifient la gigue hors zone morte (≈ ×5) ; le recadrage recalculé à chaque image entretient le bruit | `Roi::stabilized` (seuils 2 % / 2°), filtre par défaut 0,1 Hz / β 0,03 / `d_cutoff` 3 Hz (choisi par `tune_filter` sur une vraie session), mesure « rugosité de sortie hors zone morte » |
| `sort_by` panique : « does not correctly implement a total order » | comparaison « égaux à 1 % près », qui n'est pas transitive | trier par une clé totale : `total_cmp(..).then(..)` |
| Rugosité de sortie toujours à 0 | la zone morte force 0 au centre | `stats::active_still_roughness` : fenêtres hors zone morte seulement |
| Mesure de tremblement faussée par les mouvements | la différence seconde est forte au début et à la fin d'un mouvement | `stats::still_roughness` : médiane sur des fenêtres de 0,5 s |
| La scène tremble même tête immobile | le bruit **brut** de la pose (tangage ≈ 0,7°, Z ≈ 0,35 cm) dépasse les zones mortes. Il vient des points du modèle : changer de calcul de pose (sans second passage, points rigides) ne gagne que 3 à 9 % (`eval_pose`). Z, déduit de la taille apparente du visage, est 10 × plus bruité que X | filtre des translations plus lent (0,17 Hz / β 0,17, retard 100 à 130 ms) ; Z : zone morte 0,4 cm, gain ×1,33. Tremblement de Z en sortie ÷ 3 |
| `cargo` introuvable depuis un shell d'agent | `~/.cargo/env` non chargé | `source "$HOME/.cargo/env"` |

---

## 8. Outils de mesure et de diagnostic

```bash
cargo run -p capture      --example decode_bench       # coût du décodage YUYV par résolution
cargo run -p vision       --example inspect_models     # entrées / sorties des modèles ONNX
cargo run -p vision       --example bench_models       # temps de détection / points
cargo run -p vision       --example profile_landmarks  # profil ORT par opération (JSON dans target/)
cargo run -p tracker-core --example bench_sample       # coût du recadrage
cargo run --release -p tracker-core --example analyze_session -- recordings/session-XXX.csv        # bruit et activité par axe
cargo run --release -p tracker-core --example tune_filter -- recordings/session-XXX.csv rot 60     # meilleur filtre (rot|pos) pour ≤ 60 ms
cargo run --release -p tracker-core --example compare_filters -- recordings/session-XXX.csv rot 0.5:0.05 0.1:0.03:3
cargo run --release -p tracker-core --example eval_pose -- recordings/session-XXX.lmk              # variantes du calcul de pose
cargo test -p capture -- --ignored                     # tests matériels (caméra)
```

Le **banc de test** intégré (onglets Mire, Scène 3D, Courbes, Enregistrements) sert à régler le
filtre : rugosité à l'arrêt (tremblement image à image, hors mouvements), retard du filtre, temps
d'acquisition de cibles, rejeu d'une session enregistrée avec d'autres réglages, et proposition
automatique d'un réglage (« Proposer un réglage des rotations »).

---

## 9. Travailler avec l'utilisateur

- Il est développeur expérimenté mais **débute en Rust** : expliquer les concepts propres à Rust
  quand ils apparaissent, sans expliquer la programmation en général.
- Mode choisi : **« tu codes, tu m'expliques »**. Écrire le code, puis résumer ce qui a été fait et
  ce qu'il y a à apprendre.
- Il n'a **pas encore de PC Windows** : tout doit pouvoir se valider sur Mac.
- Il porte des **lunettes** et n'a qu'**un seul écran**. Webcam du Mac pour l'instant, webcam déportée
  plus tard (l'étalonnage de la phase 5 en tiendra compte).
- Ce qui touche à la caméra ou à l'interface ne se vérifie qu'à travers ses retours : lui donner
  la commande à lancer et lui dire précisément quoi observer et quoi rapporter.
