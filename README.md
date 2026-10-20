# Immersion

Head tracking par webcam pour Microsoft Flight Simulator 2024, écrit en Rust.
Le cahier des charges complet est dans [PROMPT.md](PROMPT.md).

Pour contribuer ou reprendre le code (architecture, conventions, règles d'implémentation) :
**[AGENTS.md](AGENTS.md)**. Voir aussi [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) et
[docs/TESTS.md](docs/TESTS.md).

## Structure

```
immersion/
├── Cargo.toml            # workspace : membres + versions de dépendances partagées
└── crates/
    ├── tracker-core/     # pipeline pur, sans E/S (types, maths de pose, filtres)
    ├── capture/          # webcam sur un thread dédié (seule crate qui connaît nokhwa)
    ├── vision/           # inférence ONNX Runtime (seule crate qui connaît ort)
    └── app/              # binaire `immersion` : interface egui, pipeline, banc de test
```

Le détail (threads, conventions de repères, règles) est dans [AGENTS.md](AGENTS.md).

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

## Phase 2 — Pose 6DOF de la tête

### Méthode

1. **Alignement 3D** (Umeyama, SVD 3×3) du visage canonique MediaPipe (468 sommets, en cm)
   sur les points mesurés : on obtient une rotation, une échelle (px/cm) et une translation.
2. **Second passage robuste** : on écarte les 25 % de points les plus mal alignés
   (bouche, sourcils…), puis on refait l'alignement.
3. **Distance** : `Z = focale / échelle`. La focale vient du FOV horizontal (curseur, 66° par défaut : mesuré pour la caméra du MacBook Pro M1 Pro).
4. **Correction de perspective** : l'alignement donne l'orientation *vue depuis la ligne de visée* ;
   on la recompose avec la direction tête → caméra.

### Conventions de sortie (point de vue de l'utilisateur)

| Axe | Positif quand… |
|---|---|
| Lacet | on tourne la tête vers la droite |
| Tangage | on lève la tête |
| Roulis | on penche la tête vers l'épaule droite |
| X / Y / Z | on se décale à droite / on monte / on recule |

### Quoi observer

- Les 3 axes dessinés sur le nez : rouge (gauche de la personne), vert (haut), bleu (avant).
- **Recentrer** (bouton ou barre d'espace) : la pose courante devient le zéro.
- **Vérifier la distance** avec un mètre : se placer à 60 cm de la caméra. Si la valeur affichée
  est fausse, ajuster le FOV jusqu'à ce qu'elle soit juste. C'est un étalonnage manuel, que la
  phase 5 automatisera.
- **Vérifier les angles** : recentrer, puis tourner la tête de 90° (regarder un mur sur le côté).
  Le lacet doit approcher 90°, même si le suivi devient difficile à ces angles.
- **Erreur d'ajustement** : quelques pixels. Elle augmente avec les grimaces et les reflets.

### Concepts Rust de cette phase

| Concept | Où le voir |
|---|---|
| `nalgebra` : `Vector3`, `Matrix3`, `Rotation3`, SVD | `tracker-core/src/pose.rs` |
| Données générées dans une constante `[[f32; 3]; 468]` (incluse dans le binaire) | `canonical_face.rs` |
| Tests par données synthétiques (pose connue → estimation → comparaison) | `pose::tests` |
| Piège flottant : `acos` d'un cosinus calculé à 1,0000001 → `NaN` | `umeyama_retrouve_une_similitude` |
| Canal de commandes UI → thread (`mpsc::channel`, `try_iter`) | `app/src/pipeline.rs` |
| `std::mem::take` pour lire et réinitialiser un drapeau en une opération | `State::pose` |
| `Box` dans un enum pour limiter la taille de toutes les variantes | `PipelineEvent::Frame` |

### Exercices facultatifs

1. `estimate` alloue deux `Vec` de 468 points à chaque image. Les garder dans une structure
   réutilisée, comme le tampon de `FaceTracker`, et mesurer la différence (indice : elle sera faible ;
   pourquoi ?).
2. Afficher en rouge les points écartés par le second passage robuste.

## Phase 3 — Lissage et courbes de réponse

### Chaîne de traitement

```
pose brute ─▶ recentrage ─▶ One-Euro (par axe) ─▶ zone morte + courbe ─▶ sortie
```

- **One-Euro** (`tracker-core/src/filter.rs`) : un passe-bas dont la fréquence de coupure monte avec
  la vitesse. À l'arrêt, il lisse fortement ; en mouvement, il n'ajoute presque pas de retard.
  Il utilise le **dt réel** entre deux images, et non un 1/30 s supposé.
- **Courbe** (`tracker-core/src/curve.rs`) : zone morte, entrée max., sortie max. et expo, pour chaque axe.
  Avec un seul écran : 40° de tête → 180° de vue (expo 1,6), et le roulis reste à 1:1.
- Chaque **recentrage** remet les filtres à zéro, sinon la vue glisserait lentement vers le nouveau zéro.

### Régler

Panneau de droite :
- **± brut / ± filtré** : gigue sur 1 s. Objectif : moins de 0,05° à l'arrêt.
- **Coupure min.** : plus basse, l'image est plus stable à l'arrêt mais « traîne ». **β** : plus haut, le
  retard diminue dans les mouvements rapides, mais un peu de gigue revient.
- La courbe de l'axe sélectionné est tracée, avec la position actuelle de la tête en orange.

### Concepts Rust de cette phase

| Concept | Où le voir |
|---|---|
| `Option<(f64, f64)>` comme état « pas encore initialisé » | `OneEuro` |
| `std::array::from_fn` pour traiter les 6 axes sans boucle mutable | `PoseProcessor::process` |
| `f64::copysign`, `clamp`, `powf` | `AxisCurve::apply` |
| Déstructuration de tableau `let [a, b, ..] = v` | `Pose6::from_array` |
| Module d'UI séparé qui renvoie « a changé ? » (`bool`) | `app/src/tuning.rs` |
| Dessin libre avec `Painter` (courbe, repère) | `tuning::draw_curve` |

### Exercices facultatifs

1. Ajouter un réglage « temps de retour au centre » : si le visage est perdu plus d'une seconde,
   ramener progressivement la sortie à 0 plutôt que de la figer.
2. Écrire un test qui mesure le **retard en millisecondes** du filtre sur une rampe, pour plusieurs
   valeurs de β, et l'afficher avec `cargo test -- --nocapture`.

## Phase 4 — Banc de test

Des onglets au-dessus de la zone centrale ; les panneaux de réglage restent visibles.

| Onglet | Usage |
|---|---|
| **Mire** | « Démarrer les cibles » : poser le réticule sur la cible et l'y maintenir 0,5 s. Le temps d'acquisition moyen mesure le compromis réactivité / stabilité. Le point gris montre la même tête **sans filtre**. |
| **Scène 3D** | Cockpit d'avion léger (aile haute) et sol quadrillé, vus à travers la tête. Les lettres N/E/S/O aident à juger les grandes rotations. Translations désactivables. |
| **Courbes** | Brut / filtré, et la sortie, sur 2 à 20 s. **Rugosité** (tremblement image à image) et **retard** du filtre (dès 5° de mouvement). |
| **Enregistrements** | Enregistrer une session (pose brute en CSV dans `recordings/`), puis la **rejouer avec les réglages actuels** : chaque curseur met à jour les courbes et les mesures. |

### Mesures

- **Rugosité à l'arrêt** = moyenne quadratique de `x[i+1] − 2·x[i] + x[i−1]`, médiane sur des fenêtres
  de 0,5 s. Une rampe (mouvement régulier) donne 0, et la médiane écarte les débuts et fins de mouvements :
  il ne reste que le tremblement. Pour la **sortie**, on ne garde que les fenêtres hors zone morte : c'est
  là que le tremblement se voit, amplifié par la courbe.
- **Retard** = décalage (0 à 300 ms) qui superpose le mieux le filtré au brut.

### Méthode de réglage conseillée

1. Enregistrer environ 20 s : 5 s immobile, quelques rotations vives du lacet et du tangage, puis à nouveau immobile.
2. Inclure des moments **tête tenue sur le côté** (15 à 25°) : c'est là que le tremblement se voit.
3. Dans « Enregistrements », choisir un retard maximal (60 ms par défaut), puis **« Proposer un réglage
   des rotations »** : le meilleur réglage de la grille est appliqué. À la main : baisser la **coupure min.**
   pour réduire la rugosité de sortie, monter **β** pour réduire le retard.
4. Vérifier la sensation dans la **Mire** et la **Scène 3D**. Une coupure très basse (< 0,1 Hz) rend les
   mouvements *lents* « mous » : le retard mesuré ne concerne que les mouvements rapides.

### Concepts Rust de cette phase

| Concept | Où le voir |
|---|---|
| Module en dossier (`bench/mod.rs` + sous-modules privés) | `app/src/bench/` |
| `VecDeque` comme fenêtre glissante dans le temps | `Bench::push` |
| `slice::windows(3)` pour la différence seconde | `stats::roughness` |
| Motif de tranche `let [a, b, ..] = v[..] else { … }` | `recording::from_csv` |
| `impl std::error::Error` écrit à la main (sans `thiserror`) | `recording::ParseError` |
| `collect::<Result<Vec<_>, _>>()` : s'arrêter à la première erreur | `recording::from_csv` |
| `Option::take` puis remise en place (emprunt contourné proprement) | `Recordings::ui` |
| Projection perspective et découpe au plan proche | `bench/scene.rs` |

### Stabilisation et nouvelles valeurs par défaut (retour d'essai)

Même filtrée, la vue tremblait dès qu'on regardait sur le côté. Deux causes et deux corrections :

- **Le bruit s'entretenait dans la boucle de suivi** : le recadrage était recalculé à partir des points de
  l'image précédente, leur gigue le déplaçait, ce qui changeait les points suivants. Désormais, le
  recadrage ne bouge que si le visage s'est déplacé de plus de 2 % ou a tourné de plus de 2°
  (`Roi::stabilized`). Comparer avec `IMMERSION_ROI_STABILIZE=0`.
- **Le filtre par défaut était trop peu lissant, une fois la courbe appliquée.** Réglé sur une vraie session de
  35 s avec `tune_filter`, on passe de 0,5 Hz / β 0,05 / `d_cutoff` 1 Hz à **0,1 Hz / β 0,03 / `d_cutoff` 3 Hz**.
  Le tremblement de sortie hors zone morte passe de 0,144° à 0,091°, pour un retard de 70 ms au lieu de 57.
  Sans filtre, il serait de 1,04°.
- **Leçon de la recherche** : sur des données réelles, la coupure minimale compte peu, et ce sont β et
  `d_cutoff` qui décident. La gigue fait croire au filtre que la tête bouge, et β relève alors la coupure.
  Un `d_cutoff` élevé repère plus tôt le début des vrais mouvements, ce qui permet un β plus faible.

### Enregistrer les points du visage pour améliorer le calcul de la pose

Chaque enregistrement écrit maintenant **deux fichiers** : `session-XXX.csv` (la pose) et `session-XXX.lmk`
(les 478 points de chaque image, en binaire, environ 6 Mo pour 30 s). Ils permettent de comparer hors ligne
plusieurs façons de calculer la pose sur exactement les mêmes points :

```bash
cargo run --release -p tracker-core --example analyze_session -- recordings/session-XXX.csv
cargo run --release -p tracker-core --example eval_pose -- recordings/session-XXX.lmk
```

Variantes comparées : avec ou sans le second passage robuste, avec les 468 points ou seulement 59 points
rigides (front, nez, coins des yeux, pommettes, tempes).

**Résultat sur une vraie session** : les variantes du calcul de pose diffèrent de moins de 10 %. Le bruit vient
des points du modèle, pas du calcul. Le levier est ailleurs : **Z**, déduit de la taille apparente du visage,
est 10 fois plus bruité que X. Les translations ont donc maintenant leur propre filtre, plus lent
(0,17 Hz / β 0,17), et Z une zone morte de 0,4 cm et un gain de ×1,33. Le tremblement de Z en sortie est
divisé par 3, pour un retard de 100 à 130 ms sur les translations, contre environ 70 ms sur les rotations.

### Expressions du visage (clignements, bouche, sourcils)

Une session « grimaces », tête immobile, a montré que le **modèle de points décale la pose quand
l'expression change** :

| Expression | Tangage | Z |
|---|---|---|
| Yeux fermés | +1,5° | — |
| Bouche ouverte | +2 à +4,4° | jusqu'à −7 cm (le visage paraît plus grand) |
| Sourcils levés | +3,3° | — |

Ce qui a été essayé (outil `eval_pose`, sur les points enregistrés) :
- **Choisir les points** (9 ensembles) : aucun effet sur le tangage. Pour Z, en revanche, ne garder que le
  haut du visage divise l'écart par 2.
- **Figer la pose pendant que l'expression change** : pire, car la pose saute au relâchement.
- **Corriger selon une mesure de l'expression**, apprise par régression linéaire : Z divisé par 3, tangage
  par 1,3 seulement. Ce sera l'objet d'une étape de l'étalonnage (phase 5).

Ce qui est en place :
- **Calcul hybride** : l'orientation est calculée sur les 468 points, la distance sur les 33 points du haut
  du visage (`UPPER_POINTS`). L'écart de Z en sortie au repos passe de 2,66 à 0,68 cm.
- **Courbes** : zone morte de 2° pour le tangage et courbe plus douce (25° de tête → 60° de vue), zone morte
  de 1° pour le lacet. L'écart de sortie au repos passe de 2,93° à 0,58° pour le tangage, et de 0,95° à
  0,63° pour le lacet.
