#!/usr/bin/env bash
set -euo pipefail

# doxus DMG 빌드 스크립트
# 사용법: ./scripts/build-dmg.sh [--universal]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/apps/desktop"
TAURI_SRC="$DESKTOP_DIR/src-tauri"

# Confluence WASM 플러그인 빌드 (tauri resources에 번들됨)
echo "▶ Building Confluence WASM plugin..."
cargo build --release --manifest-path "$ROOT_DIR/crates/plugins/confluence/Cargo.toml" --target wasm32-unknown-unknown
cp "$ROOT_DIR/crates/plugins/confluence/target/wasm32-unknown-unknown/release/doxus_plugin_confluence.wasm" \
   "$ROOT_DIR/crates/plugins/confluence/confluence.wasm"

# doxus-mcp 바이너리 빌드 (externalBin 필요)
echo "▶ Building doxus-mcp binary..."
cargo build --release -p doxus-mcp --manifest-path "$ROOT_DIR/Cargo.toml"

# binaries/ 디렉토리에 복사 (tauri externalBin 경로)
mkdir -p "$TAURI_SRC/binaries"
cp "$ROOT_DIR/target/release/doxus-mcp" "$TAURI_SRC/binaries/doxus-mcp-aarch64-apple-darwin"

echo "▶ Building frontend..."
cd "$DESKTOP_DIR"
npm install --prefer-offline
npm run build

echo "▶ Building Tauri app + DMG..."
cd "$TAURI_SRC"
cargo tauri build

DMG_PATH=$(find "$ROOT_DIR/target/release/bundle/dmg" -name "*.dmg" 2>/dev/null | head -1)
if [ -n "$DMG_PATH" ]; then
    echo "✅ DMG 생성 완료: $DMG_PATH"
    echo "   크기: $(du -sh "$DMG_PATH" | cut -f1)"
else
    echo "❌ DMG 파일을 찾을 수 없습니다"
    exit 1
fi
