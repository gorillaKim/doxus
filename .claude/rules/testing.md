# 테스트 전략

## 테스트 피라미드

```
        /‾‾‾‾‾‾‾‾‾‾‾‾‾\
       /   E2E (소수)   \
      /‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾\
     /  통합 (중간 비중)   \
    /‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾\
   /    단위 (대다수)       \
  /‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾\
```

## 레이어별 테스트 전략

### 단위 테스트 (`crates/*/src/`)
- DocSource trait 구현체 (mock 사용)
- 개별 Host Function 동작
- 검색 랭킹 로직 (RRF 계산)
- 마이그레이션 SQL 파싱
- `#[cfg(test)]` 모듈 또는 `tests/unit/`

### 통합 테스트 (`crates/*/tests/`)
- PluginManager + WasmDocSourceAdapter (실제 WASM 로드)
- DB 마이그레이션 순서 (V1→V8 전체 체인)
- FTS5 + sqlite-vec 하이브리드 검색 결과
- ONNX 임베딩 배치 처리

### E2E 테스트
- Desktop IPC 커맨드 → SearchEngine → DB 전체 흐름
- Agent sidecar 세션 (JSONL 프로토콜)
- MCP 도구 호출 시나리오

## 테스트 헬퍼

### TestDb
```rust
pub struct TestDb {
    pub conn: Connection,
    _dir: TempDir,
}

impl TestDb {
    pub fn new() -> Self {
        // in-memory SQLite + 전체 마이그레이션 적용
        // sqlite-vec 익스텐션 로드 포함
    }
}
```

### TestVault
```rust
pub struct TestVault {
    pub path: PathBuf,
    _dir: TempDir,
}

impl TestVault {
    pub fn with_files(files: &[(&str, &str)]) -> Self {
        // TempDir에 마크다운 파일 생성
    }
}
```

### TestWasmPlugin
```rust
pub struct TestWasmPlugin {
    // 테스트용 WASM 플러그인 로더
    // fixtures/plugins/*.wasm 사용
}
```

### MockHttpServer (wiremock)
```rust
// http_request Host Function 테스트용
// 실제 외부 HTTP 요청 없이 응답 시뮬레이션
let server = MockServer::start().await;
Mock::given(method("GET"))
    .and(path("/wiki/rest/api/content"))
    .respond_with(ResponseTemplate::new(200).set_body_json(&fixture))
    .mount(&server)
    .await;
```

## CI 매트릭스

```yaml
strategy:
  matrix:
    os: [macos-latest]          # Linux는 Phase 후기 추가, Windows 없음
    rust: [stable, nightly]
    node: [18, 20, 22]          # Agent sidecar 호환성
```

- Rust stable: 기본 테스트
- Rust nightly: lint (`clippy --all-features`), `cargo +nightly fmt --check`
- Node 매트릭스: Agent sidecar (`crates/agent/`) 에서만 실행

## 규칙

- `unwrap()` / `expect()` 는 테스트 코드에서만 허용
- 외부 네트워크 요청 금지 — MockHttpServer 또는 `#[cfg(not(test))]`로 분기
- WASM 플러그인 테스트용 fixture는 `crates/plugins/tests/fixtures/` 에 위치
- 통합 테스트는 `TestDb::new()` 사용 — 실제 `~/.doxus/db/doxus.db` 건드리지 않음
- 마이그레이션 테스트: 각 버전에서 롤포워드 후 데이터 무결성 확인
- 통합 테스트에 **대용량 시나리오(100+ 문서)** 포함 필요 — 소규모 테스트에서 재현 안 되는 버그가 존재

## Vitest + Tauri 플러그인 mock 패턴

`@tauri-apps/plugin-*` 같은 Tauri 플러그인을 mock할 때는 반드시 `vi.hoisted()` 사용.
일반 `vi.fn()`은 ES module hoisting 이후 평가되므로 mock factory 진입 전에 `undefined`로 참조된다.

```typescript
// 올바른 예 — vi.hoisted()로 hoisting 단계에서 먼저 실행
const { mockCheckUpdate } = vi.hoisted(() => ({
  mockCheckUpdate: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheckUpdate,
}));

// 잘못된 예 — hoisting 전에 평가되어 mock이 undefined
const mockCheckUpdate = vi.fn();
vi.mock('@tauri-apps/plugin-updater', () => ({ check: mockCheckUpdate }));
```

> **근거**: 2026-04-29 devlog — `updateManager.test.ts`, `migrationListener.test.ts` 2개 파일 연속 재현.

## Tauri 플러그인 timeout 테스트

`vi.useFakeTimers()` 환경에서 `Promise.race` 기반 timeout이 정상 동작하지 않는다.
real timer + 짧은 `timeoutMs`(예: 50ms)로 실제 timeout 동작을 검증할 것.

```typescript
// 권장 — real timer
await updateManager.checkForUpdates({ timeoutMs: 50 });

// 사용 금지 — fake timer + Promise.race 조합 불안정
vi.useFakeTimers();
vi.advanceTimersByTime(5000);
```

## React + Tauri 이벤트 리스너 — StrictMode cleanup

`useEffect`에서 Tauri 이벤트를 구독할 때, async로 등록되는 `unlisten` 함수에 대한
cleanup race를 `cancelled` 플래그로 방어해야 StrictMode 이중 실행에 안전하다.

```typescript
useEffect(() => {
  let unlisten: (() => void) | undefined;
  let cancelled = false;

  listen('migration:progress', handleProgress).then(fn => {
    if (cancelled) {
      fn();  // 이미 unmount됐으면 즉시 해제
      return;
    }
    unlisten = fn;
  });

  return () => {
    cancelled = true;
    unlisten?.();
  };
}, []);
```

> **근거**: 2026-04-29 devlog — App.tsx StrictMode cleanup race로 이벤트 리스너 중복 등록 (Opus 리뷰 지적).
