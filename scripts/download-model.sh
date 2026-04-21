#!/usr/bin/env bash
# Downloads multilingual-e5-small ONNX model for doxus vector search
set -e

MODEL_NAME="multilingual-e5-small.onnx"
EXPECTED_SHA256="ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665"
MODEL_DIR="${HOME}/.doxus/models"
mkdir -p "$MODEL_DIR"

# Download from HuggingFace (intfloat/multilingual-e5-small)
BASE_URL="https://huggingface.co/intfloat/multilingual-e5-small/resolve/main"

# Check if already exists and is valid
if [ -f "$MODEL_DIR/$MODEL_NAME" ]; then
  ACTUAL_SHA256=$(shasum -a 256 "$MODEL_DIR/$MODEL_NAME" | awk '{print $1}')
  if [ "$ACTUAL_SHA256" = "$EXPECTED_SHA256" ]; then
    echo "Model already exists and is valid at $MODEL_DIR/$MODEL_NAME"
    exit 0
  else
    echo "Existing model checksum mismatch. Re-downloading..."
    rm -f "$MODEL_DIR/$MODEL_NAME"
  fi
fi

echo "Downloading multilingual-e5-small to $MODEL_DIR..."
trap 'echo "Download failed, cleaning up..."; rm -f "$MODEL_DIR/$MODEL_NAME" "$MODEL_DIR/tokenizer.json"' ERR

curl -L --progress-bar "$BASE_URL/onnx/model.onnx" -o "$MODEL_DIR/$MODEL_NAME"
curl -L --progress-bar "$BASE_URL/tokenizer.json" -o "$MODEL_DIR/tokenizer.json"

# Final verification
ACTUAL_SHA256=$(shasum -a 256 "$MODEL_DIR/$MODEL_NAME" | awk '{print $1}')
if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
  echo "CRITICAL: Downloaded model checksum mismatch!"
  echo "Expected: $EXPECTED_SHA256"
  echo "Actual:   $ACTUAL_SHA256"
  exit 1
fi

echo "Done! Model verified."
