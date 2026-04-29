# Auto-Updater & Post-Update Migration

## 개요

Doxus 데스크톱 앱은 Tauri `plugin-updater`를 통해 자동 업데이트를 지원하고,
버전 변경 시 필요한 DB 마이그레이션과 재인덱싱을 앱 시작 시점에 자동으로 처리한다.

---

## 전체 흐름

```
앱 시작
  │
  ├─ 1. DB SQL 마이그레이션 (V1~V40+) ─────── 항상 실행, 멱등
  │
  ├─ 2. detect_and_migrate() ──────────────── 버전 비교
  │       ├─ FirstRun / UpToDate → 버전 기록만
  │       ├─ Upgraded → 버전별 훅 실행 → last_run_version 갱신
  │       └─ Downgraded → 경고 로그 + 버전 기록만
  │
  └─ 3. (업그레이드 시) PostUpdateHook
          └─ force_reindex_all() → migration:reindex_started 이벤트
                                 → 비동기 재인덱싱
                                 → migration:reindex_completed 이벤트

SettingsPage (백그라운드)
  └─ 업데이트 확인 (plugin-updater)
       └─ 다운로드 → 설치 → 재시작 → 위 흐름 반복
```

---

## DB SQL 마이그레이션 (`V1~V40+`)

- 위치: `crates/core/src/db/migrations/`
- 앱 시작 시 **항상** 실행 (멱등 — `CREATE TABLE IF NOT EXISTS` 등)
- V40: `system_config` 테이블 추가 (`last_run_version` 저장용)

```sql
-- V40__add_system_config.sql
CREATE TABLE IF NOT EXISTS system_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO system_config(key, value) VALUES ('last_run_version', '0.0.0');
```

SQL 마이그레이션은 스키마 변경에 사용하고, 데이터 재처리(재인덱싱 등)는 아래의 PostUpdateHook으로 처리한다.

---

## 버전 감지: `detect_and_migrate()`

위치: `apps/desktop/src-tauri/src/update_manager.rs`

`system_config` 테이블의 `last_run_version`과 현재 앱 버전을 semver로 비교한다.

| 결과 | 조건 | 동작 |
|------|------|------|
| `FirstRun` | 저장값 없음 또는 `"0.0.0"` | 버전 기록, audit_log 작성 |
| `UpToDate` | 현재 == 저장 | 아무것도 안 함 |
| `Upgraded` | 현재 > 저장 | 버전별 훅 실행 → 버전 갱신 |
| `Downgraded` | 현재 < 저장 | 경고 로그 → 버전 갱신 (훅 없음) |

**semver 비교** — `semver` crate 사용. 문자열 비교가 아니므로 `0.10.0 > 0.9.0` 올바르게 처리됨.

---

## 재인덱싱 트리거 규칙

버전별 재인덱싱 조건은 `detect_and_migrate()` 내부에 명시적으로 정의된다.

```rust
// apps/desktop/src-tauri/src/update_manager.rs

// v0.2.0: 임베딩 포맷 변경 — 전체 재인덱싱 필요
if current >= Version::new(0, 2, 0) && from_ver < Version::new(0, 2, 0) {
    hook.force_reindex_all()?;
}
```

**새 버전에서 재인덱싱이 필요한 경우** 이 블록에 조건을 추가한다:

```rust
// 예시: v0.3.0에서 재인덱싱 필요
if current >= Version::new(0, 3, 0) && from_ver < Version::new(0, 3, 0) {
    hook.force_reindex_all()?;
}
```

### 버전별 재인덱싱 이력

| 버전 | 재인덱싱 여부 | 사유 |
|------|-------------|------|
| `0.1.x → 0.1.y` | ❌ | 마이너 패치, 스키마 호환 |
| `< 0.2.0 → 0.2.0` | ✅ | 임베딩 포맷 변경 |

---

## 프론트엔드 이벤트

재인덱싱 진행 중 사용자에게 알림을 표시한다.

| 이벤트 | 발생 시점 | 페이로드 |
|--------|---------|---------|
| `migration:reindex_started` | 재인덱싱 시작 직전 | `{}` |
| `migration:reindex_completed` | 재인덱싱 완료 | `{ count: number }` |

- 리스너: `apps/desktop/src/services/migrationListener.ts`
- UI: `App.tsx`에서 toast로 표시

재인덱싱 실패 시 앱은 계속 실행되며 (non-fatal), `last_run_version`은 정상 갱신된다.
사용자가 Settings에서 수동으로 재인덱싱할 수 있다.

---

## 릴리즈 파이프라인

위치: `.github/workflows/release.yml`

```
태그 푸시 (v*)
  │
  ├─ pubkey 설정 확인 (tauri.conf.json)
  ├─ 서명 키 사전 검증 (npx @tauri-apps/cli signer sign)
  ├─ doxus-mcp 사이드카 빌드 (aarch64-apple-darwin)
  ├─ tauri-action 빌드 + Draft 릴리즈 생성
  ├─ 재서명 (* 참고)
  └─ latest.json + .app.tar.gz 업로드
```

> **재서명 필요 이유**: `tauri-action@v0.5.17`이 `.app`을 재패키징하면서
> `cargo tauri build`가 생성한 `.app.tar.gz.sig`를 빈 파일로 덮어쓴다.
> 재패키징 후 `npx @tauri-apps/cli signer sign`으로 재서명한다.

### 필요한 GitHub Secrets

| 시크릿 | 설명 |
|--------|------|
| `TAURI_SIGNING_PRIVATE_KEY` | base64 인코딩된 rsign 암호화 개인키 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 개인키 생성 시 사용한 패스프레이즈 |

### 업데이터 엔드포인트

```
https://github.com/gorillaKim/doxus/releases/latest/download/latest.json
```

`latest.json` 형식 (Tauri v2):

```json
{
  "version": "0.1.0",
  "notes": "...",
  "pub_date": "2026-04-29T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://github.com/.../doxus.app.tar.gz",
      "signature": "<minisign signature>"
    }
  }
}
```

---

## 관련 파일

| 파일 | 역할 |
|------|------|
| `apps/desktop/src-tauri/src/update_manager.rs` | 버전 감지, PostUpdateHook, IPC 커맨드 |
| `apps/desktop/src-tauri/src/main.rs` | 앱 시작 시 detect_and_migrate 호출 |
| `apps/desktop/src/services/migrationListener.ts` | 프론트엔드 이벤트 리스너 |
| `apps/desktop/src/services/updateManager.ts` | plugin-updater 래퍼 |
| `apps/desktop/src/pages/SettingsPage.tsx` | 업데이트 UI |
| `crates/core/src/db/migrations/V40__add_system_config.sql` | system_config 테이블 |
| `.github/workflows/release.yml` | CI/CD 릴리즈 파이프라인 |
| `apps/desktop/src-tauri/tauri.conf.json` | updater pubkey, 엔드포인트 설정 |
