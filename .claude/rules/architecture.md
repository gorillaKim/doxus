# doxus 아키텍처 원칙

## 핵심 가치

doxus는 **로컬 퍼스트 + WASM 플러그인 기반 다중 소스 통합 문서 검색 허브**다.
obsidian-nexus의 검색 엔진을 계승하되, 플러그인 시스템으로 다양한 문서 소스를 통합한다.

## 설계 원칙

1. **로컬 퍼스트**
   - 모든 데이터는 `~/.doxus/` 아래 SQLite에 저장
   - 클라우드 동기화 없음 (플러그인이 외부 소스를 가져오는 것은 OK)
   - 오프라인에서도 검색 가능해야 함

2. **플러그인 기반 분리**
   - 문서 소스 = 플러그인 (가져오기만 담당)
   - 인덱싱 / 검색 / 랭킹 = core (통합 엔진, 플러그인 로직 없음)
   - 새 소스를 추가해도 core 코드는 변경하지 않음

3. **프로젝트 관리 원칙**
   - doxus는 프로젝트를 "관리만" 함 — 소유하지 않음
   - `remove_project` = 인덱스 데이터만 삭제, **원본 파일 절대 무변경**
   - 검색은 `Active` 프로젝트 범위에서만 동작
   - `Disabled` 상태에서는 인덱스 유지, 검색에서만 제외

4. **하이브리드 검색 (obsidian-nexus 계승)**
   - FTS5 (전문 검색) + sqlite-vec (벡터 유사도) 병렬 실행
   - ONNX Runtime 내장 임베딩 (Ollama는 선택적 fallback)
   - RRF(Reciprocal Rank Fusion)로 최종 랭킹 합산

5. **에이전트 친화성**
   - doxus-mcp: 33개 `doxus_*` 도구를 MCP 프로토콜로 노출
   - Claude Code / Gemini CLI 자동 감지 후 적절한 브릿지 사용
   - Node.js sidecar로 AI 에이전트 실행 (JSONL 프로토콜)

## 모노레포 레이아웃

```
doxus/
├── Cargo.toml              # workspace 루트
├── crates/
│   ├── core/               # 핵심 엔진 (search, index, db, plugin, embedding, workspace)
│   │                       # 플러그인 비즈니스 로직 없음
│   ├── plugin-sdk/         # DocSource trait + 공유 타입 (RawDocument, PluginError 등)
│   ├── plugins/
│   │   ├── obsidian/       # Obsidian 볼트 (in-process 빌트인)
│   │   ├── confluence/     # Confluence Cloud/Server (WASM)
│   │   └── github/         # GitHub Issues/Wiki/Discussions (WASM)
│   ├── cli/                # doxus-cli (단일 바이너리)
│   ├── mcp-server/         # doxus-mcp (MCP 프로토콜)
│   └── agent/              # 사서 에이전트 (Node.js sidecar 관리)
├── apps/
│   └── desktop/            # doxus-desktop (Tauri v2 + React 19)
│       ├── src-tauri/      # Rust 백엔드 (AppState, IPC 커맨드)
│       └── src/            # React 프론트엔드
└── packages/
    └── types/              # 공유 TypeScript 타입
```

## 구현 Phase 로드맵

| Phase | 내용 | 상태 |
|-------|------|------|
| 0 | ONNX 임베딩 PoC + Extism PoC (병렬 2트랙) | 준비 중 |
| 1 | Cargo workspace 초기화, Core 포팅, CLI/MCP/Desktop 스캐폴드 | 대기 |
| 2a | DocSource trait + Obsidian 플러그인 (in-process) | 대기 |
| 2b | WASM MVP (WasmDocSourceAdapter + http_request) | 대기 |
| 2c | Host Function 전체 + 보안/매니페스트 | 대기 |
| 2d | OAuth 인증 플로우 | 대기 |
| 3 | Confluence 플러그인 + Agent sidecar 기본 | 대기 |
| 4 | 플러그인 마켓 (UI + 레지스트리 + 코드 서명) | 대기 |
| 5 | GitHub 플러그인 + 마켓 배포 | 대기 |
| 6 | 동기화 스케줄러 + 관측성/디버깅 | 대기 |
| 7 | 워크스페이스 + 템플릿 관리 | 대기 |
| 8 | Desktop UI 고도화 | 대기 |

## 데이터 경로 규칙

```
~/.doxus/
├── db/nexus.db         # SQLite 메인 DB
├── config.toml         # 전역 설정
├── agents/             # 프롬프트 하네스 (librarian 등)
│   └── librarian/
└── plugins/            # 사용자 설치 플러그인 (.wasm)
```
