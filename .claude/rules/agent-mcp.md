# 에이전트 & MCP 규칙

## 에이전트 아키텍처 (사서 에이전트)

```
┌─────────────────────────────────────────┐
│  doxus (Rust)                           │
│  ┌──────────────┐    JSONL (stdio)      │
│  │  AgentManager│◄──────────────────────┤
│  │  (agent 크레) │                      │
│  └──────┬───────┘                      │
│         │ spawn                         │
│  ┌──────▼───────┐                      │
│  │  Node.js     │  @anthropic-ai/       │
│  │  Sidecar     │  claude-agent-sdk     │
│  └─────────────┘                       │
└─────────────────────────────────────────┘
```

## JSONL 프로토콜

### Rust → Node.js (stdin)

| 메시지 | 설명 |
|--------|------|
| `{"type":"start","session_id":"...","prompt":"..."}` | 세션 시작 |
| `{"type":"message","content":"..."}` | 사용자 메시지 |
| `{"type":"cancel"}` | 취소 요청 |
| `{"type":"close"}` | 세션 종료 |

### Node.js → Rust (stdout)

| 메시지 | 설명 |
|--------|------|
| `{"type":"init","model":"...","tools":[...]}` | 초기화 완료 |
| `{"type":"thought","content":"..."}` | 에이전트 사고 과정 |
| `{"type":"tool_use","name":"...","input":{...}}` | 도구 호출 |
| `{"type":"text","content":"..."}` | 텍스트 응답 (스트리밍) |
| `{"type":"result","content":"..."}` | 최종 결과 |
| `{"type":"error","message":"..."}` | 에러 |
| `{"type":"cancelled"}` | 취소 확인 |

- 각 메시지는 개행(`\n`)으로 구분
- stderr는 로그 전용 (프로토콜 메시지 금지)

## CLI 자동 감지

```rust
// crates/agent/src/cli_detector.rs
pub enum CliKind { ClaudeCode, GeminiCli, None }

pub fn detect_cli() -> CliKind {
    // 1. CLAUDE_CODE_ENTRYPOINT 환경변수
    // 2. `claude` 바이너리 PATH 확인
    // 3. GEMINI_CLI_* 환경변수
    // 4. `gemini` 바이너리 PATH 확인
}
```

## 프롬프트 하네스

- 위치: `~/.doxus/agents/librarian/`
- 파일 구조:
  ```
  ~/.doxus/agents/librarian/
  ├── system.md        # 시스템 프롬프트
  ├── tools.json       # 허용 도구 목록
  └── examples/        # few-shot 예시
  ```
- 프롬프트는 런타임에 로드 (재시작 없이 수정 가능)
- 기본 프롬프트는 `crates/agent/src/prompts/` 에 번들 (하네스 없을 때 폴백)

## MCP 서버 규칙 (doxus-mcp)

### 도구 명명 규칙

모든 도구는 `doxus_` prefix:

```
doxus_search            # 하이브리드 검색
doxus_get_document      # 문서 전문 조회
doxus_get_section       # 특정 섹션만 조회 (토큰 절약)
doxus_list_projects     # 프로젝트 목록
doxus_add_project       # 프로젝트 추가
doxus_remove_project    # 프로젝트 제거
doxus_get_backlinks     # 역방향 링크
doxus_get_links         # 정방향 링크
doxus_find_path         # 문서 간 최단 경로
doxus_get_cluster       # 멀티홉 그래프 탐색 (추천)
doxus_index_project     # 프로젝트 인덱싱 시작
doxus_sync_project      # 변경분 동기화
doxus_status            # 서버 상태
# ... 총 33개
```

### MCP 응답 규칙

- 에러는 MCP `error` 타입으로 반환 (예외 throw 금지)
- 대용량 결과는 페이지네이션 (`cursor` 파라미터)
- 문서 내용은 `content_type: "text/markdown"` 명시

## 도구 허용 목록 (에이전트)

사서 에이전트가 사용 가능한 doxus-mcp 도구:

```json
{
  "allowed_tools": [
    "doxus_search",
    "doxus_get_document",
    "doxus_get_section",
    "doxus_list_projects",
    "doxus_get_backlinks",
    "doxus_get_links",
    "doxus_get_cluster",
    "doxus_find_path"
  ]
}
```

쓰기/수정 도구(`doxus_add_project`, `doxus_index_project` 등)는 기본 허용 목록에서 제외.
