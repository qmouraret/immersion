# Tests

Règles : voir [AGENTS.md](../AGENTS.md), § 6 (règles 16 à 19).

## Lancer

```bash
cargo test                          # tous les tests automatiques
cargo test -p tracker-core          # une seule crate
cargo test pose                     # tests dont le nom contient « pose »
cargo test -- --nocapture           # afficher les println!/eprintln!
cargo test -p capture -- --ignored  # tests matériels (caméra branchée, permission accordée)
```

Les tests de `vision` utilisent les vrais modèles. S'ils sont absents, ils affichent un message et
s'arrêtent sans échouer (lancer `scripts/fetch_models.sh`).

## Ce qui est testé automatiquement (120 tests, dont 2 ignorés)

| Crate / module | Tests | Ce qui est vérifié |
|---|---|---|
| **tracker-core** | **76** | |
| `frame` | 2 | cohérence taille / buffer |
| `roi` | 7 | aller-retour image ↔ recadrage, letterbox, orientation déduite de deux points, stabilisation (petites variations ignorées, vrais mouvements suivis, angles autour de ±π) |
| `tensor` | 2 | recadrage identité, noir hors de l'image |
| `blazeface` | 4 | 896 ancres, décodage d'une détection, seuil, moyenne pondérée des détections voisines |
| `face` | 2 | sortie du modèle ramenée dans l'image, détection → ROI agrandie et carrée |
| `canonical_face` | 3 | symétrie des yeux, nez le plus en avant, menton en bas |
| `pose` | 13 | Umeyama exact, angles aller-retour, signes intuitifs, pose centrée et décalée (perspective), robustesse (bouche ouverte), recentrage dans le repère de la tête neutre, **webcam posée de côté : mêmes axes qu'en face**, moyenne de poses, ensembles de points (rigides, haut du visage : symétriques, loin de la bouche), variantes de calcul exactes |
| `filter` | 4 | première mesure, réduction du bruit ≥ ×5, retard < 50 ms à 120 °/s, convergence |
| `curve` | 4 | zone morte, symétrie, plafond, forme expo, continuité |
| `processing` | 5 | contournement, courbe après filtre, lissage, `reset`, filtres rotation / translation séparés |
| `stats` | 9 | écart-type, fenêtre glissante, rugosité (rampe = 0), rugosité à l'arrêt, hors zone morte, sauts (quantile), retard entier et fractionnaire, pas de retard sans mouvement |
| `recording` | 3 | aller-retour CSV, erreur avec numéro de ligne, rejeu identité |
| `landmark_log` | 4 | aller-retour binaire, taille attendue, fichier tronqué et mauvais en-tête détectés |
| `autotune` | 5 | tri sans panique (ordre total), grille logarithmique, contrainte de retard, meilleur réglage nettement moins rugueux qu'un filtre inactif, plus de retard toléré → moins de tremblement |
| `expression` | 5 | décalages connus appris, axes non concernés laissés tels quels, correction qui annule le décalage, correction bornée, trop peu d'images |
| `calibration` | 4 | profondeur de la racine du nez, **FOV de 70° retrouvé alors que l'application tourne à 50°**, amplitudes aux centiles (pic parasite ignoré), rien sans mesures |
| **capture** | **4 (+2 ignorés)** | |
| `format` | 4 | 720p préféré au 1080p, fluidité prioritaire, MJPEG évité, formats non décodables ignorés |
| `lib` | 2 ignorés | caméra inexistante → erreur et arrêt propre ; 10 images reçues de la caméra 0 |
| **vision** (`tests/models.rs`) | **4** | erreur claire si un modèle manque ; pas de visage ni de ROI sur une image unie ; présence < 0,5 sans visage ; détecteur relancé tant que rien n'est trouvé |
| **app** | **34** | |
| `pipeline` | 8 | recentrage automatique, recentrage moyenné sur 15 images, mode absolu, recentrage après changement de FOV, réglages appliqués, correction des expressions appliquée, pas de pose sans visage, dt réel du filtre |
| `calibration` | 4 | FOV mesuré en 2 s, amplitudes → entrées des courbes, rien sans visage, rapport d'expressions (non appliqué sans validation) |
| `profile` | 5 | nom de fichier sûr, aller-retour TOML, profil absent, champs manquants complétés par défaut, fichier invalide → erreur claire |
| `bench::scene` | 7 | signes de la caméra virtuelle (lacet, tangage, roulis, translation), rien derrière la tête, découpe au plan proche |
| `bench::target` | 4 | validation après le maintien, chrono remis à zéro en sortant, arrêt, cibles dans la mire |
| `bench::recordings` | 3 | enregistrer puis recharger (dossier temporaire), rien hors enregistrement, fichier invalide signalé |
| `bench` | 1 | l'historique ne garde que les 20 dernières secondes |
| `camera_view` | 2 | `fit_aspect` (fenêtre trop haute, trop large) |

### Données synthétiques

`tracker_core::testing` fournit :
- `synthetic_landmarks(camera, angles, position)` : les points qu'aurait donnés MediaPipe pour une tête
  à une pose **connue**. Utilisé pour tester la pose et le pipeline sans caméra ni modèle.
- `gray_frame(w, h, seq, t)` : une image unie.

## Ce qui n'est pas testé automatiquement, et pourquoi

| Partie | Raison | Couverture à la place |
|---|---|---|
| Thread de capture (`capture::lib`) | matériel et permission macOS | 2 tests `#[ignore]` + tests manuels |
| Détection et points sur un **vrai visage** | aucune image de visage dans le dépôt (vie privée, licences) | tests manuels ; une idée : enregistrer une courte vidéo de l'utilisateur, avec son accord, hors dépôt |
| Rendu egui (panneaux, superposition, mire, scène, assistant) | rendu graphique | la logique est sortie dans des fonctions testées (`fit_aspect`, `Camera`, `clip_near`, `TargetGame::update`, `Calibration::push`) ; le reste est couvert par les tests manuels |
| Performances | dépendent de la machine | exemples de mesure (`examples/`), voir AGENTS.md § 8 |

## Tests manuels (à refaire à chaque phase qui touche l'interface)

Lancer `cargo run --bin immersion` **depuis un terminal ayant la permission caméra**.

### Capture (phase 0)
- [ ] La vidéo s'affiche ; « Reçu » ≈ 30 img/s ; « Images sautées » reste à 0.
- [ ] Changer de caméra dans la liste : l'ancien flux s'arrête, le nouveau démarre.
- [ ] La case « Effet miroir » inverse l'image **et** les points.

### Visage (phase 1)
- [ ] 478 points collés au visage, iris en cyan, confiance proche de 100 %.
- [ ] Sortir du champ puis revenir : le cadre passe à l'orange (détection), puis au bleu (suivi).
- [ ] Avec des lunettes, tourner la tête vers une source de lumière : la confiance reste > 80 %.
- [ ] **Redimensionner la fenêtre dans tous les sens : les points restent collés au visage.**

### Pose (phase 2)
- [ ] À 60 cm mesurés (objectif → racine du nez), la distance affichée vaut ≈ 65 cm (centre de la tête). Sinon, ajuster le FOV.
- [ ] Après un recentrage, les signes sont corrects : lacet + à droite, tangage + vers le haut, roulis + vers l'épaule droite, x + à droite, y + vers le haut, z + en reculant.
- [ ] Un quart de tour donne un lacet de 80 à 90°.

### Filtre et courbes (phase 3)
- [ ] Au démarrage, les sorties partent de 0 (recentrage automatique).
- [ ] Immobile : sorties stables (zone morte) ; « ± filtré » < « ± brut ».
- [ ] **Tête tenue à 20° sur le côté : la Mire et la Scène 3D ne tremblent pas** avec les réglages par défaut.
- [ ] Tête immobile au centre : cligner des yeux, ouvrir la bouche, lever les sourcils. La vue bouge peu, nettement moins qu'avant le calcul hybride et la zone morte du tangage.
- [ ] `IMMERSION_ROI_STABILIZE=0 cargo run --bin immersion` : le tremblement est plus fort qu'avec la stabilisation.
- [ ] Rotation vive puis retour : la sortie suit sans retard gênant et revient à 0 sans dépasser.
- [ ] Les curseurs agissent immédiatement ; « Valeurs par défaut » restaure les réglages.

### Banc de test (phase 4)
- [ ] **Mire** : le réticule suit la tête et penche dans le même sens qu'elle ; une cible tenue 0,5 s est validée ; le point gris tremble plus que le réticule.
- [ ] **Scène 3D** : tourner à droite fait apparaître la portière droite ; lever la tête montre le toit ; pencher à droite fait monter l'horizon à droite ; se pencher en avant rapproche le tableau de bord.
- [ ] **Courbes** : le brut est plus rugueux que le filtré ; le retard s'affiche après un mouvement de plus de 5°.
- [ ] **Enregistrements** : enregistrer, arrêter, puis le fichier se recharge ; changer un réglage met à jour les courbes rejouées.
- [ ] « Proposer un réglage des rotations » applique un réglage (panneau de droite mis à jour) et affiche sa rugosité et son retard.

### Étalonnage (phase 5)
- [ ] Onglet « Étalonnage » : 5 étapes avec leur coche ; un compte à rebours s'affiche pendant une mesure ; « Annuler » l'interrompt.
- [ ] **FOV** : à une distance mesurée, « Mesurer » donne un FOV proche de celui trouvé à la main (66° pour la webcam du MacBook) ; « Distance caméra » devient juste.
- [ ] **Posture neutre** : après le recentrage, les sorties sont à 0 en regardant le centre de l'écran. Pas de décalage du tangage comme avec un recentrage sur une seule image.
- [ ] **Amplitudes** : après 10 s de balayage, la vue maximale est atteinte en regardant les bords de l'écran.
- [ ] **Expressions** : le tableau « sans → avec » s'affiche ; « Appliquer » réduit les sauts quand on ouvre la bouche ou lève les sourcils.
- [ ] **Profil** : « Enregistrer », puis relancer l'application : FOV, courbes et correction sont repris (message « profil chargé » dans le terminal).
- [ ] Barre d'espace ignorée pendant une mesure.
- [ ] **Webcam déportée** (dès qu'elle est disponible) : posée de côté, après recentrage face à l'écran, tourner la tête à droite donne un lacet positif sans roulis ni tangage parasites.
