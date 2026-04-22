# doxus-registry 설정 가이드

## GitHub repo 생성
1. `YOUR_ORG/doxus-registry` GitHub repo 생성
2. Settings → Pages → Branch: main, folder: / (root) 활성화
3. `plugins.json` 푸시
4. `https://YOUR_ORG.github.io/doxus-registry/plugins.json` 접근 확인

## plugins.json 형식
[{
  "plugin_id": "com.example.plugin",
  "version": "1.0.0",
  "display_name": "Example Plugin",
  "download_url": "https://github.com/YOUR_ORG/doxus-registry/releases/download/v1.0.0/example-1.0.0.wasm",
  "checksum_sha256": "...",
  "public_key_hex": "...",
  "auth_type": "none",
  "guide_url": ""
}]

## 기본 URL 변경
apps/desktop/src-tauri/src/commands/market.rs 의 YOUR_ORG를 실제 조직명으로 교체
