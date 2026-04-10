#!/usr/bin/env bash
# Downloads all-MiniLM-L6-v2 ONNX model for doxus vector search
set -e

MODEL_DIR="${HOME}/.doxus/models/all-MiniLM-L6-v2"
mkdir -p "$MODEL_DIR"

# Download from HuggingFace (optimum ONNX export)
BASE_URL="https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main"

# Check if already downloaded
if [ -f "$MODEL_DIR/model.onnx" ] && [ -f "$MODEL_DIR/tokenizer.json" ] && [ -f "$MODEL_DIR/tokenizer_config.json" ]; then
  echo "Model already downloaded at $MODEL_DIR"
  exit 0
fi

echo "Downloading all-MiniLM-L6-v2 to $MODEL_DIR..."
trap 'echo "Download failed, cleaning up..."; rm -f "$MODEL_DIR/model.onnx" "$MODEL_DIR/tokenizer.json" "$MODEL_DIR/tokenizer_config.json"' ERR
curl -L --progress-bar "$BASE_URL/onnx/model.onnx" -o "$MODEL_DIR/model.onnx"
curl -L --progress-bar "$BASE_URL/tokenizer.json" -o "$MODEL_DIR/tokenizer.json"
curl -L --progress-bar "$BASE_URL/tokenizer_config.json" -o "$MODEL_DIR/tokenizer_config.json"
echo "Done!"
