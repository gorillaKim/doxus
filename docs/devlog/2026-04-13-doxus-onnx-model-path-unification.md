---
title: "ONNX 모델 경로 통일 — 바이너리별 중복 제거"
aliases:
  - onnx-model-path-unification
  - ONNX 모델 경로 통일
tags:
  - devlog
  - refactor
  - onnx
  - embedding
  - tdd
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

# ONNX 모델 경로 통일 — 바이너리별 중복 제거

## 배경

doxus의 세 바이너리(desktop, mcp-server, cli)가 ONNX 모델 경로를 각자 다르게 처리하고 있었다.

- **Desktop**: 로컬 `find_model_path()` 함수, tokenizer.json 검증 없음
- **MCP server**: `~/.doxus/models/multilingual-e5-small/model.onnx` 하드코딩 (서브디렉토리)
- **CLI**: 모델 로딩 자체 없음, FTS-only 고정

모델 파일 중복, 경로 불일치, CLI의 하이브리드 검색 미지원 문제가 동시에 발생했다.

## 변경 내용

Planner 에이전트로 구현 계획을 수립한 후, Critic 에이전트 리뷰에서 CRITICAL 1개(tokenizer.json 누락)와 MAJOR 2개(CLI connection 타입 과도한 변경, MCP legacy path)를 구현 전에 발견했다. TDD 방식(RED → GREEN)으로 구현했다.

### 주요 변경사항

**`crates/core/src/embedding.rs`**
- `resolve_model_path()` 공개 함수 추가
  - 우선순위: `DOXUS_MODEL_PATH` env → macOS 번들 → `~/.doxus/models/multilingual-e5-small.onnx` → legacy MCP 서브디렉토리 path → dev `crates/core/models/`
  - tokenizer.json 동시 존재 검증 필수 (Critic CRITICAL 반영)
  - legacy path 사용 시 `tracing::warn!` 출력
- `OnnxEmbedder::from_default_path()` convenience constructor 추가
- `NoOpEmbedder::model_info()`의 `Box::leak` → `LazyLock` 으로 메모리 릭 수정
- env var 테스트에 `#[serial_test::serial(doxus_model_path_env)]` 추가 (경쟁 조건 방지)
- candidates Vec에서 `unwrap_or_default()` → `.into_iter().flatten()` 패턴으로 변경

**`apps/desktop/src-tauri/src/main.rs`**
- 로컬 `find_model_path()` 함수 완전 제거
- `OnnxEmbedder::from_default_path()` 사용

**`crates/mcp-server/src/main.rs`**
- 하드코딩 `~/.doxus/models/multilingual-e5-small/model.onnx` 제거
- `OnnxEmbedder::from_default_path()` 사용

**`crates/cli/src/main.rs`**
- ONNX 로딩 추가 (`from_default_path` 시도, 실패 시 FTS-only 폴백)
- `handle_search` fn → async fn 변환
- ONNX 있으면 두 번째 connection으로 `SearchEngine::with_embedder()` (hybrid)
- ONNX 없으면 `SearchEngine::new(conn)` (FTS-only, connection 타입 변경 없음)
- Critic MAJOR 반영: `Arc<Mutex<Connection>>` 타입 전파를 `SearchEngine::new_fts_only()` 패턴으로 회피

### 영향 범위

- 세 바이너리 모두 단일 `resolve_model_path()` 경로 결정 로직 공유
- CLI가 처음으로 하이브리드 검색(FTS5 + 벡터) 지원
- 모델 설치 경로: `~/.doxus/models/multilingual-e5-small.onnx` + `tokenizer.json` (평탄 구조)

## 결과

- `cargo build --workspace` 성공
- 테스트: 236 passed, 0 failed
- `find_model_path` grep → 없음 (제거 완료)
- `multilingual-e5-small/model.onnx` (서브디렉토리 형태) grep → 없음 (하드코딩 제거)
- `from_default_path` → 세 바이너리 모두 사용 확인
- OrtHardwareDevice Apple Silicon 감지 로그 확인
- 재인덱싱 후 113개 문서 벡터 임베딩 완료

## 교훈

- Critic 에이전트 리뷰가 실제로 유효했다. tokenizer.json 누락(CRITICAL)과 CLI connection 타입 과도한 변경(MAJOR) 두 가지를 구현 전에 잡아냈다.
- `unwrap_or_default()`는 빈 PathBuf를 candidate에 넣는 코드 스멜이다. `Option` + `.into_iter().flatten()` 패턴이 의도를 더 명확히 표현한다.
- TDD 흐름(RED → GREEN): 컴파일 에러로 RED 확인, 함수 구현 후 GREEN 확인이 자연스럽게 동작했다.
- `Box::leak`은 함수 호출마다 새 할당이 발생하므로 항상 `static LazyLock`으로 대체해야 한다.

## 관련 문서

- [[2026-04-12-doxus-ux-cache-toast-emoji]]
- [[2026-04-13-confluence-search-score-chunking]]
