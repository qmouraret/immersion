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

## Ce qui est testé automatiquement (98 tests, dont 2 ignorés)

| Crate / module | Tests | Ce qui est vérifié |
|---|---|---|
| **tracker-core** | **65** | |
| `frame` | 2 | cohérence taille / buffer |
| `roi` | 7 | aller-retour image ↔ recadrage, letterbox, orientation déduite de deux points, stabilisation (petites variations ignorées, vrais mouvements suivis, angles autour de ±π) |
| `tensor` | 2 | recadrage identité, noir hors de l'image |
| `blazeface` | 4 | 896 ancres, décodage d'une détection, seuil, moyenne pondérée des détections voisines |
| `face` | 2 | sortie du modèle ramenée dans l'image, détection → ROI agrandie et carrée |
| `canonical_face` | 3 | symétrie des yeux, nez le plus en avant, menton en bas |
| `pose` | 11 | points du haut du visage (symétriques, loin de la bouche), Umeyama exact, angles aller-retour, signes intuitifs, pose centrée, pose décalée (perspective), robustesse (bouche ouverte), recentrage, points rigides (symétriques, sans bouche), variantes de calcul exactes, points rigides insensibles à la bouche |
| `landmark_log` | 4 | aller-retour binaire, taille attendue, fichier tronqué et mauvais en-tête détectés |
| `filter` | 4 | première mesure, réduction du bruit ≥ ×5, retard < 50 ms à 120 °/s, convergence |
| `curve` | 4 | zone morte, symétrie, plafond, forme expo, continuité |
| `processing` | 5 | contournement, courbe après filtre, lissage, `reset`, filtres rotation / translation séparés |
| `stats` | 9 | sauts rares vus par le quantile, écart-type, fenêtre glissante, rugosité (rampe = 0), rugosité à l'arrêt (mouvements écartés), rugosité hors zone morte, retard entier et fractionnaire retrouvé, pas de retard sans mouvement |
| `autotune` | 5 | tri sans panique sur des valeurs proches (ordre total), grille logarithmique, contrainte de retard respectée, meilleur réglage nettement moins rugueux qu'un filtre inactif, plus de retard toléré → moins de tremblement |
| `recording` | 3 | aller-retour CSV, erreur avec numéro de ligne, rejeu identité |
| **capture** | **4 (+2 ignorés)** | |
| `format` | 4 | 720p préféré au 1080p, fluidité prioritaire, MJPEG évité, formats non décodables ignorés |
| `lib` | 2 ignorés | caméra inexistante → erreur et arrêt propre ; 10 images reçues de la caméra 0 |
| **vision** (`tests/models.rs`) | **4** | erreur claire si un modèle manque ; pas de visage ni de ROI sur une image unie ; présence < 0,5 sans visage ; détecteur relancé tant que rien n'est trouvé |
| **app** | **23** | |
| `pipeline` | 6 | recentrage automatique, mode absolu, recentrage après changement de FOV, réglages appliqués, pas de pose sans visage, dt réel du filtre |
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
| Rendu egui (panneaux, superposition, mire, scène) | rendu graphique | la logique est sortie dans des fonctions testées (`fit_aspect`, `Camera`, `clip_near`, `TargetGame::update`) ; le reste est couvert par les tests manuels |
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
