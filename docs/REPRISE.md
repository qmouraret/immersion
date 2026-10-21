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
| 6 | Sortie UDP OpenTrack, compilation Windows, GitHub | 🧪 committée, **CI Windows au vert**, test manuel à faire |
| 7 | Intégration MSFS 2024 (7a via OpenTrack, 7b émulation TrackIR native) | à faire, **demande le PC** |
| 8 | Suivi du regard (iris), épaules | à faire, faisable sur Mac |

Dépôt : <https://github.com/qmouraret/immersion> (public, licence MIT).
Le workflow `.github/workflows/windows.yml` compile, teste (128 tests, plus 2 tests matériels ignorés)
et produit l'exécutable `immersion-windows-x64` à chaque push sur `main`.

## 2. Ce qui reste à valider

1. **Phase 6, sur le Mac ou le PC** : lancer le récepteur de diagnostic puis l'application, et cocher
   « Envoyer la pose » dans le panneau Sortie :
   ```bash
   cargo run -p output --example receive
   cargo run --bin immersion
   ```
   Attendu : environ 30 paquets/s, valeurs identiques à la colonne « Sortie » ; le débit tombe à 0 quand le
   visage est masqué ; une adresse invalide affiche une erreur sans figer la vidéo.
   → Une fois validé, marquer la phase 6 ✅ dans `AGENTS.md`.
2. **Sur le PC** : `immersion.exe` démarre, la webcam est vue, et les tests passent en local
   (`cargo test --workspace`).

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
3. Valider la phase 6 (§ 2), puis attaquer la phase 7a (§ 3).

## 7. Façon de travailler (rappel)

- L'utilisateur débute en Rust : mode **« tu codes, tu m'expliques »**, en français.
- **Une phase à la fois**, validée par l'utilisateur avant le commit. Tout téléchargement se fait avec
  son accord.
- Ce qui touche à la caméra, à l'interface ou au simulateur ne se vérifie qu'à travers ses retours :
  lui donner la commande à lancer, et lui dire quoi observer et quoi rapporter.
- **Mesurer avant de décider** : plusieurs intuitions se sont révélées fausses (points rigides, sens de
  `d_cutoff`, gel de la pose). Les outils `examples/` et le banc de test existent pour ça.
