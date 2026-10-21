# Développer sur Windows 11

Objectif : le même confort que sur le Mac. **Développer directement sous Windows, pas dans WSL** :
le projet a besoin de la webcam, d'une fenêtre graphique, puis de MSFS et d'OpenTrack, qui sont
tous natifs Windows. WSL compliquerait chacun de ces points.

## Outils

| Rôle | Outil | Pourquoi |
|---|---|---|
| Gestionnaire de paquets | **winget** (déjà installé) | L'équivalent de Homebrew. |
| Terminal | **Windows Terminal** (déjà installé sur Windows 11) | Onglets, panneaux, thèmes. |
| Shell | **PowerShell 7** (`pwsh`), plus **Git Bash** (livré avec Git) | PowerShell pour Windows ; Git Bash pour les commandes Unix et les scripts `.sh`. |
| Invite | **Starship** (facultatif) | La même invite sur Mac et Windows (branche git, version de Rust). |
| Git | **Git for Windows** | Inclut Git Credential Manager : connexion à GitHub par le navigateur, sans jeton. |
| GitHub | **gh** | Workflows, artefacts, PR depuis le terminal. |
| Compilateur C/C++ | **Build Tools de Visual Studio 2022**, charge « Développement Desktop en C++ » | **Indispensable** : éditeur de liens MSVC et SDK Windows, nécessaires à Rust et à `mozjpeg-sys`. |
| Rust | **rustup** (chaîne MSVC) | Le même outil que sur le Mac. |
| Éditeur | **VS Code** + rust-analyzer (ou RustRover, gratuit pour un usage non commercial) | |

## Installation (PowerShell en administrateur)

Build Tools avec la bonne charge de travail :
```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```
Le reste :
```powershell
winget install Microsoft.PowerShell Git.Git GitHub.cli Rustlang.Rustup Microsoft.VisualStudioCode Starship.Starship
```
Rouvrir le terminal, puis vérifier :
```powershell
rustc --version; cargo --version; git --version; gh --version
```

## Extensions VS Code

- **rust-analyzer** : complétion, erreurs en direct, navigation ;
- **Even Better TOML** : `Cargo.toml` et profils ;
- **Error Lens** : erreurs affichées sur la ligne ;
- **C/C++** (Microsoft) : le débogueur adapté aux programmes compilés avec MSVC.

## Réglages qui comptent

1. **Dev Drive** (*Paramètres → Système → Stockage → Disques de développement*) : un volume pour les
   projets, par exemple `D:\dev`. Windows Defender y analyse les fichiers de façon allégée, et `cargo build`
   y est nettement plus rapide.
2. **Autoriser les scripts PowerShell** (sinon `fetch_models.ps1` est bloqué) :
   ```powershell
   Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
   ```
3. **Chemins longs dans git** (l'arborescence de `target/` dépasse parfois 260 caractères) :
   ```powershell
   git config --global core.longpaths true
   ```
4. **Identité git**, avec l'email personnel (règle 27 d'`AGENTS.md`) :
   ```powershell
   git config --global user.name "Quentin Mouraret"; git config --global user.email "qmouraret@gmail.com"
   ```
5. **Caméra** : *Paramètres → Confidentialité et sécurité → Caméra* → autoriser les applications de bureau.

## Premier lancement

```powershell
gh auth login
```
```powershell
gh repo clone qmouraret/immersion; cd immersion; .\scripts\fetch_models.ps1; cargo run --release --bin immersion
```
La première compilation prend une dizaine de minutes ; les suivantes, quelques secondes.

## Récupérer l'exécutable produit par GitHub

Chrome bloque souvent le `.zip` (exécutable non signé, sans « réputation »). Le plus simple :
```powershell
gh run download --repo qmouraret/immersion --name immersion-windows-x64
```
Au lancement, SmartScreen avertit : *Informations complémentaires* → *Exécuter quand même*.
Ces avertissements ne disparaîtraient qu'avec une signature de code payante (inutile pour ce projet).

## Ensuite

Voir [REPRISE.md](REPRISE.md) : ce qui reste à valider et les prochaines étapes.
