#!/usr/bin/env bash
# Télécharge les modèles MediaPipe Face Landmarker convertis en ONNX (Apache-2.0).
# Source : https://huggingface.co/fernandotonon/QtMeshEditor-models (dossier mocap/face)
set -euo pipefail
cd "$(dirname "$0")/../models"
BASE="https://huggingface.co/fernandotonon/QtMeshEditor-models/resolve/main/mocap"
for f in face/face_detector.onnx face/face_landmarks.onnx NOTICE.txt; do
  echo "→ $f"
  curl -fL --proto '=https' -o "$(basename "$f")" "$BASE/$f"
done
ls -l
