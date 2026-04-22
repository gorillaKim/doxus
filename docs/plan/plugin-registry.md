---
title: 플러그인 레지스트리 구축 계획 (doxus-registry)
date: 2026-04-22
description: GitHub Pages 기반 정적 플러그인 레지스트리 저장소 구축 및 앱 연동 계획
---

# 🗂 플러그인 레지스트리 구축 계획 (doxus-registry)

Doxus 마켓플레이스의 플러그인 목록/메타데이터를 제공하는 정적 레지스트리를 GitHub Pages로 운영하기 위한 계획 문서.

---

## 개요

### 구조

앱이 플러그인 목록을 불러올 때 GitHub Pages를 정적 API 서버처럼 활용한다. 별도 서버 없이 GitHub 저장소 파일만으로 레지스트리를 운영할 수 있다.

```
Doxus 앱
  │
  ├─ GET https://{ORG}.github.io/doxus-registry/index.json
  │       → 전체 플러그인 목록
  │
  └─ GET https://{ORG}.github.io/doxus-registry/plugins/{plugin-id}/latest.json
          → 특정 플러그인 최신 메타데이터
```

### 관련 코드

- `apps/desktop/src-tauri/src/commands/market.rs` — `YOUR_ORG` 플레이스홀더가 실제 조직명으로 교체되어야 함
- `apps/desktop/src/pages/SettingsPage.tsx` — 마켓 탭이 레지스트리 URL에서 목록을 불러옴

---

## Step 1: GitHub 저장소 생성

- 저장소명: `doxus-registry`
- 공개(public) 저장소로 생성 (GitHub Pages 무료 플랜 요건)
- 조직 또는 개인 계정 선택 후 `market.rs`의 `YOUR_ORG`와 맞춰야 함

---

## Step 2: GitHub Pages 활성화

저장소 생성 후:

1. `Settings` → `Pages`
2. Source: `Deploy from a branch`
3. Branch: `main`, 폴더: `/ (root)`
4. 저장 후 `https://{ORG}.github.io/doxus-registry/` 접근 확인

---

## Step 3: 레지스트리 파일 구조

저장소 루트에 아래 파일들을 추가한다.

### `index.json` — 전체 플러그인 목록

```json
{
  "version": 1,
  "updated_at": "2026-04-22",
  "plugins": [
    {
      "id": "com.doxus.confluence",
      "name": "Confluence",
      "description": "Confluence Cloud/Server 문서를 인덱싱합니다.",
      "author": "Doxus Team",
      "version": "1.0.0",
      "icon": "https://{ORG}.github.io/doxus-registry/plugins/com.doxus.confluence/icon.png",
      "category": "productivity",
      "tags": ["confluence", "atlassian", "wiki"]
    }
  ]
}
```

### `plugins/{plugin-id}/latest.json` — 플러그인 상세

```json
{
  "id": "com.doxus.confluence",
  "version": "1.0.0",
  "min_doxus_version": "0.1.0",
  "download_url": "https://github.com/{ORG}/doxus-plugins/releases/download/v1.0.0/confluence.wasm",
  "sha256": "...",
  "changelog": "최초 릴리즈",
  "permissions": {
    "http_domains": ["*.atlassian.net"],
    "secrets": ["api_token", "base_url"]
  }
}
```

---

## Step 4: 앱 코드 연동

`market.rs`의 플레이스홀더를 실제 값으로 교체한다.

```rust
// Before
const REGISTRY_BASE: &str = "https://YOUR_ORG.github.io/doxus-registry";

// After
const REGISTRY_BASE: &str = "https://madup.github.io/doxus-registry";
```

---

## Step 5: 플러그인 WASM 파일 배포

플러그인 `.wasm` 바이너리는 레지스트리 저장소가 아닌 **별도 GitHub Releases**로 관리한다.

- 저장소: `doxus-plugins` (또는 각 플러그인별 저장소)
- 릴리즈 태그: `com.doxus.confluence@1.0.0`
- `latest.json`의 `download_url`이 해당 Release Asset을 가리킴

---

## 우선순위 및 타이밍

| 작업 | 우선순위 | 타이밍 |
|------|----------|--------|
| 저장소 생성 + Pages 활성화 | 낮음 | 첫 플러그인 배포 전 |
| `index.json` 초기 파일 추가 | 낮음 | 첫 플러그인 배포 전 |
| `market.rs` `YOUR_ORG` 교체 | 중간 | 팀 내부 테스트 시작 전 |
| WASM 빌드 자동화 (CI) | 낮음 | 플러그인 안정화 후 |

> 현재는 플러그인 마켓 UI가 빈 목록으로 표시될 뿐 앱 동작에는 영향 없음. 실제 플러그인 배포 준비가 됐을 때 진행하면 된다.
