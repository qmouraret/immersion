# Reprise du projet — état au 21 octobre 2026

Ce document permet de reprendre le travail sur une autre machine (le PC Windows) ou dans une
nouvelle session. À lire **après** [AGENTS.md](../AGENTS.md), qui donne les règles et les conventions.

> Pour une nouvelle session Claude Code : « Lis `AGENTS.md` et `docs/REPRISE.md`, puis continuons. »

---

## 1. Où en est le projet

| Phase | Contenu | État |
|---|---|---|
| 0 | Capture webcam, affichage | ✅ validée |
| 1 | Détection du visage, 478 points | ✅ validée |
| 2 | Pose 6DOF (Umeyama, perspective, distance) | ✅ validée |
| 3 | Filtre One-Euro, courbes, recentrage | ✅ validée |
| 4 | Banc de test, réglage automatique, analyse des sessions | ✅ validée |
| 5 | Étalonnage (FOV, posture neutre, amplitudes, expressions), profils | ✅ validée |
| 6 | Sortie UDP OpenTrack, compilation Windows, GitHub | ✅ validée sur le PC Windows |
| 7 | Intégration MSFS 2024 (7a via OpenTrack, 7b émulation TrackIR native) | à faire, **demande le PC** |
| 8 | Suivi du regard (iris), épaules | à faire, faisable sur Mac |

Dépôt : <https://github.com/qmouraret/immersion> (public, licence MIT).
Le workflow `.github/workflows/windows.yml` compile, teste (128 tests, plus 2 tests matériels ignorés)
et produit l'exécutable `immersion-windows-x64` à chaque push sur `main`.

## 2. Validé sur le PC Windows (21 octobre 2026)

- `cargo test --workspace` : 128 tests verts (2 tests matériels ignorés), modèles chargés.
- L'application démarre et voit la webcam. Décodage 0,7 ms, points 3,7 ms (Ryzen 7 9700X).
- Phase 6 : les paquets arrivent au récepteur ; le débit tombe à 0 quand le visage est masqué ; une
  adresse invalide affiche une erreur sans figer la vidéo.

Constats à garder en tête :
- **Webcam actuelle limitée** : au mieux 640×480 à 30 img/s (960×720 à 15), et en pratique **9 img/s**
  (exposition automatique de la caméra, probablement, par manque de lumière). Le calcul n'est pas en cause. À remplacer
  par une webcam 720p/1080p à 30 img/s ou plus.
- **nokhwa (backend Windows)** affiche « @ 1 fps » pour le format en cours : il ne garde que le
  dénominateur de la fréquence. Il jette aussi les formats à fréquence fractionnaire (29,97 img/s).
  Sans effet sur cette webcam, mais à surveiller avec la prochaine.
- **Masquer le visage en partie** (main devant) envoie quelques poses fantaisistes avant la perte du
  visage : tangage +43°, Z en butée. À corriger avant de jouer (seuil de confiance, rejet des sauts).
- **Smart App Control** bloque `cargo build --release` (script de build de `mozjpeg-sys`) ; le debug
  passe. **Norton SafeCam** bloquait la caméra jusqu'à ce que `immersion.exe` soit autorisé.

## 3. Prochaines étapes possibles

- **Phase 7a (PC)** : Immersion → OpenTrack (entrée « UDP over network », port 4242 ; sortie
  « freetrack 2.0 Enhanced ») → MSFS 2024 (activer le suivi de tête dans les options de caméra).
  À vérifier en vrai : le nom exact des options de MSFS 2024, les signes des axes dans le cockpit
  (cases « Inverser » du panneau Sortie), la répartition entre nos courbes et les sensibilités de MSFS,
  la latence ressentie.
- **Phase 7b (PC)** : émulation TrackIR native en Rust : DLL `NPClient64.dll` (`cdylib`,
  `extern "C"`), mémoire partagée avec Immersion, clé de registre NaturalPoint. Référence : le code
  source d'OpenTrack. Piste alternative à évaluer : l'API caméra de SimConnect (SDK MSFS).
- **Phase 8 (Mac ou PC)** : suivi du regard à partir des 10 points d'iris (468 à 477), pour amplifier la
  vue quand on regarde sur le côté sans tourner la tête (comme Beam). Les lunettes de l'utilisateur
  (reflets) sont un point d'attention.

Petites tâches en attente, proposées mais pas faites :
- attestation de provenance des exécutables (`actions/attest-build-provenance`) ;
- publication de versions dans *Releases* (les artefacts expirent après 90 jours) ;
- vérifier si `Swatinem/rust-cache` et `actions/upload-artifact` affichent aussi l'avertissement Node.js 20
  (seul `actions/checkout` a été passé en v5).

## 4. Décisions et mesures à connaître

Le détail, avec les chiffres, est dans le [README](../README.md) (une section par phase) et dans les
pièges d'`AGENTS.md` § 7. L'essentiel :

- **Inférence sur CPU** (`PreferCPU`) : CoreML, choisi par défaut par `ort`, était 3 fois plus lent.
- **Bruit** : il vient des points du modèle, pas du calcul de pose. Les remèdes en place sont la
  stabilisation du recadrage, des filtres mesurés sur de vraies sessions (rotations 0,1 Hz / β 0,03 /
  `d_cutoff` 3 Hz, translations 0,17 Hz / β 0,17), et des zones mortes plus larges que le bruit.
- **Expressions** (yeux, bouche, sourcils) : elles décalent la pose (tangage +1,5 à +4,4°). Remèdes : distance
  calculée sur le haut du visage, zone morte de 2° pour le tangage, et correction facultative apprise à
  l'étalonnage.
- **Repère de sortie** = la tête au moment du recentrage, c'est-à-dire l'écran : une webcam déportée,
  décalée ou inclinée, est corrigée d'elle-même.
- **FOV de la webcam du MacBook Pro M1 Pro : 66°**, mesuré. À remesurer pour chaque webcam (étape 1 de
  l'assistant).

## 5. Ce qui ne passe pas d'une machine à l'autre

| Élément | Où il est | Sur le PC |
|---|---|---|
| Modèles ONNX | `models/` (non versionnés) | `.\scripts\fetch_models.ps1` |
| Profils d'étalonnage | dossier de configuration de l'OS (`~/Library/Application Support/immersion/profiles` sur Mac, `%APPDATA%\immersion\profiles` sur Windows) | refaire l'étalonnage (autre webcam de toute façon) ; ou copier le `.toml` |
| Sessions enregistrées (`recordings/*.csv`, `*.lmk`) | non versionnées : données personnelles (points du visage) | les copier à la main si on veut relancer les outils d'analyse dessus |
| Conversation d'origine | export dans `~/Downloads/session-export-1792598934343.zip` sur le Mac | pour relecture seulement : une session ne se recharge pas |

## 6. Reprendre sur le PC, étape par étape

1. Installer les outils : voir [WINDOWS.md](WINDOWS.md).
2. Cloner et lancer :
   ```powershell
   gh repo clone qmouraret/immersion; cd immersion
   .\scripts\fetch_models.ps1
   cargo test --workspace
   cargo run --release --bin immersion
   ```
3. Attaquer la phase 7a (§ 3). OpenTrack 2026.1.0 est à installer ; MSFS 2024 est installé (Steam).

## 7. Façon de travailler (rappel)

- L'utilisateur débute en Rust : mode **« tu codes, tu m'expliques »**, en français.
- **Une phase à la fois**, validée par l'utilisateur avant le commit. Tout téléchargement se fait avec
  son accord.
- Ce qui touche à la caméra, à l'interface ou au simulateur ne se vérifie qu'à travers ses retours :
  lui donner la commande à lancer, et lui dire quoi observer et quoi rapporter.
- **Mesurer avant de décider** : plusieurs intuitions se sont révélées fausses (points rigides, sens de
  `d_cutoff`, gel de la pose). Les outils `examples/` et le banc de test existent pour ça.
