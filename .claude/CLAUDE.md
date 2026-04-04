# doxus — Claude Code 컨텍스트

## 프로젝트 개요

doxus는 **obsidian-nexus의 차세대 진화판**으로, WASM 플러그인 기반 다중 소스 통합 문서 검색 허브다.
로컬 퍼스트 + 에이전트 친화적 설계를 핵심으로 한다.

> 설계 문서: `/Users/madup/gorillaProject/brain/Ideas/doxus/` (15개, 800KB+)

## Rules

상세 규칙은 `.claude/rules/` 에 분리되어 있다:

| 파일 | 내용 |
|------|------|
| [architecture.md](rules/architecture.md) | 전체 아키텍처 원칙, 모노레포 레이아웃, Phase 로드맵 |
| [plugin-system.md](rules/plugin-system.md) | DocSource trait, WASM/Extism, Host Function |
| [rust-conventions.md](rules/rust-conventions.md) | 크레이트 역할, 에러 처리, 의존성 |
| [frontend.md](rules/frontend.md) | Tauri v2, React 19, Zustand, IPC |
| [database.md](rules/database.md) | SQLite 스키마, 마이그레이션 V1~V8, 경로 |
| [testing.md](rules/testing.md) | 테스트 피라미드, 헬퍼, CI |
| [agent-mcp.md](rules/agent-mcp.md) | JSONL 프로토콜, MCP 도구 명명, 에이전트 |
| [git-workflow.md](rules/git-workflow.md) | 커밋 컨벤션, Phase 태그, 브랜치 |

## 현재 상태

- **설계 완료** — brain 프로젝트에 15개 설계 문서 존재
- **구현 미시작** — Phase 0 (ONNX PoC + Extism PoC) 준비 단계

## 즉시 시작 체크리스트

### Phase 0 - Track A (ONNX 임베딩)
- [ ] `crates/core/src/embedding.rs` — EmbeddingProvider trait
- [ ] `ort` 크레이트 통합
- [ ] `all-MiniLM-L6-v2.onnx` 모델 번들
- [ ] 배치 인퍼런스 테스트

### Phase 0 - Track B (Extism PoC) — Track A와 병렬
- [ ] Extism 바이너리 PoC (간단한 WASM 로드)
- [ ] `http_request` Host Function 동작 확인
- [ ] `Plugin: Send + Sync` 여부 확정 → Phase 2b 아키텍처 결정

## 핵심 제약사항

- `remove_project` 는 인덱스 데이터만 삭제 — 원본 파일 절대 변경 없음
- 외부 플러그인은 반드시 WASM (Extism) 샌드박스
- DB 접근은 `crates/core/src/db/` 모듈만 — 직접 SQL 금지
- `main` 브랜치 직접 push 금지
