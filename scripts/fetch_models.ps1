# Télécharge les modèles MediaPipe Face Landmarker convertis en ONNX (Apache-2.0).
# Source : https://huggingface.co/fernandotonon/QtMeshEditor-models (dossier mocap/face)
# Usage (PowerShell, depuis la racine du dépôt) : .\scripts\fetch_models.ps1
$ErrorActionPreference = "Stop"
$models = Join-Path $PSScriptRoot "..\models"
New-Item -ItemType Directory -Force -Path $models | Out-Null
$base = "https://huggingface.co/fernandotonon/QtMeshEditor-models/resolve/main/mocap"
foreach ($f in @("face/face_detector.onnx", "face/face_landmarks.onnx", "NOTICE.txt")) {
    $name = Split-Path $f -Leaf
    Write-Host "-> $f"
    Invoke-WebRequest -Uri "$base/$f" -OutFile (Join-Path $models $name)
}
Get-ChildItem $models
