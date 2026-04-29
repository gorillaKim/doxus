# Auto Updater & Post-Update Migration Plan

## 1. 개요 (Overview)
Doxus 데스크톱 애플리케이션의 버전을 관리하고, 사용자에게 자동 업데이트 기능을 제공하기 위한 설계 문서입니다. 추가적으로 애플리케이션이 업데이트되었을 때 필요한 후속 조치(DB 마이그레이션, 강제 재인덱싱 등)를 처리하는 로직을 분리하여 안정적인 업데이트 환경을 구축하는 것을 목표로 합니다.

Apple Developer Program을 통한 공식 코드 사이닝(Code Signing) 및 공증(Notarization)은 일시적으로 보류하며, Tauri의 자체 업데이터(ED25519 서명)를 통한 릴리즈 파일 위변조 검증만 진행합니다. 현재 macOS-only 빌드만 지원하며, Windows/Linux 지원은 추후 Phase에서 추가합니다.

## 2. GitHub 릴리즈 파이프라인 구성 (CI/CD)
Tauri Updater 플러그인을 활용하여 GitHub Releases를 업데이트 저장소로 사용합니다.

### 2.1 Tauri Updater 및 키 설정
- **의존성 추가**:
  - `apps/desktop/package.json`에 `@tauri-apps/plugin-updater` 추가
  - `apps/desktop/src-tauri/Cargo.toml`에 `tauri-plugin-updater` 추가
  - `apps/desktop/src-tauri/src/main.rs`에 `.plugin(tauri_plugin_updater::Builder::new().build())` 등록
- **키 생성**: Tauri CLI를 이용해 자체 업데이트 검증용 ED25519 키 페어 생성.
  ```bash
  cargo tauri signer generate -w ~/.tauri/doxus.key
  ```
- **설정**:
  - 생성된 퍼블릭 키를 `tauri.conf.json` 내 updater 설정에 등록.
  - 엔드포인트는 GitHub Releases의 `latest.json`을 바라보게 설정:
    ```json
    "plugins": {
      "updater": {
        "pubkey": "YOUR_ED25519_PUBLIC_KEY",
        "endpoints": [
          "https://github.com/OWNER/doxus/releases/latest/download/latest.json"
        ]
      }
    }
    ```
  - 프라이빗 키와 비밀번호는 GitHub Secrets (`TAURI_PRIVATE_KEY`, `TAURI_KEY_PASSWORD`)에 안전하게 저장.

### 2.2 GitHub Actions 워크플로우
- 릴리즈 태그(`v0.1.x`)가 푸시될 때 트리거.
- **Mac 코드 사이닝 스킵**: Gatekeeper 공증을 진행하지 않으므로 애플 개발자 인증서 설정 과정을 제거하여 CI 구성을 경량화.
- **아티팩트 업로드**: 빌드된 실행 파일(`.dmg`, `.app.tar.gz` 등)과 `latest.json` (업데이트 서명 및 메타데이터 포함) 파일을 GitHub Release 자산(Assets)에 자동 업로드.
- **릴리즈 노트 안내문**: Gatekeeper 관련 경고를 우회하기 위한 안내 문구를 릴리즈 노트에 명시하도록 템플릿 구성.
  > *"Mac 사용자는 최초 실행 시 '확인되지 않은 개발자' 경고가 발생할 수 있습니다. 다운로드 후 Doxus 앱 아이콘을 우클릭하고 '열기'를 선택하여 실행해주세요. 시스템 설정 → 개인정보 보호 및 보안에서 명시적 허용이 필요할 수 있습니다 (macOS 15+)."*

## 3. 프론트엔드: 업데이트 다운로드 및 UI 제어 (`UpdateManager.ts`)
순수하게 업데이트 확인, 다운로드, 설치 및 재시작 UI 로직을 담당하는 래퍼(Wrapper) 계층을 둡니다.

- **위치**: `apps/desktop/src/services/updateManager.ts` (신규 디렉토리 `services/` 생성 필요)
- **주요 기능**:
  - `checkForUpdates()`: 새 버전 감지. 네트워크 오류 시 10초 timeout 후 silent fail (설정 페이지에서 수동 실행 시에만 에러 표시).
  - `downloadAndInstall(onProgress)`: 릴리즈 에셋 다운로드 및 진행률 반환.
  - `relaunchApp()`: 설치 후 앱 재시작.
- **이벤트 채널 단일화** (기존 `index:progress`, `sync:status` 패턴 준수):
  - 프론트엔드는 `UpdateManager.ts`가 emit하는 `update:*` 이벤트만 구독.
  - `@tauri-apps/plugin-updater`의 raw 콜백(`Started`/`Progress`/`Finished`)은 매니저 내부에서만 소비하고 아래 이벤트로 변환하여 emit:
    - `update:check` — 업데이트 확인 시작
    - `update:available` — 새 버전 감지됨
    - `update:progress` — 다운로드 진행률 (0~100)
    - `update:installed` — 설치 완료
- **UI 연동**: 설정 페이지(`SettingsPage.tsx`)에 "업데이트" 영역을 추가하고 이 매니저를 호출하여 렌더링. 새 버전 발견 후 다운로드 시 프로그레스 바(Progress Bar)를 통해 진행 현황을 시각적으로 표시.
- **로컬 테스트**: dev 빌드용 별도 ED25519 키로 서명한 mock `latest.json`을 `json-server`로 서빙. 프로덕션 서명 키와 분리하여 관리.

## 4. 백엔드(Rust): 마이그레이션 및 후속 조치 제어 (`update_manager.rs`)
앱 실행 시 메인 윈도우 로드 직전에 버전 변경을 감지하고, 기존 데이터 스키마 병합, 캐시 초기화, 강제 재인덱싱 등을 수행하는 코어 로직입니다.

- **위치**: `apps/desktop/src-tauri/src/update_manager.rs`
  > ⚠️ `crates/core`가 아닌 desktop 크레이트에 위치해야 함. 현재 앱 버전 조회(`app.config().version`)는 Tauri 컨텍스트에서만 가능하며, `crates/core`는 Tauri 의존성을 갖지 않음.

- **실행 순서** (startup 내 선후관계 엄수):
  1. DB 연결 및 스키마 마이그레이션(V1~V40) 실행 ← `system_config` 테이블 생성 포함, 반드시 먼저
  2. `system_config`에서 `last_run_version` 조회
  3. 버전 비교 후 post-update 마이그레이션 분기
  4. `last_run_version` 갱신

- **로직 흐름**:
  1. **버전 감지**: `system_config` 테이블에서 `last_run_version`을 조회하고, Tauri API(`app.config().version`)로 현재 버전(`current_version`)을 획득.
  2. **3-way 버전 비교**: **`semver` crate 필수** (문자열 비교 시 `"0.10.0" < "0.9.0"` 오작동 위험). prerelease 식별자(`-beta`, `-rc`)는 무시하고 `MAJOR.MINOR.PATCH` 코어만 비교.
     ```rust
     use semver::Version;
     use std::cmp::Ordering;

     let last = Version::parse(&last_run_version)
         .unwrap_or(Version::new(0, 0, 0));
     let current = Version::new(
         current_version.major,
         current_version.minor,
         current_version.patch,
     );

     match last.cmp(&current) {
         Ordering::Less => {
             // 업그레이드: post-update 마이그레이션 실행
             run_post_update_migrations(&last, &current, &db).await?;
         }
         Ordering::Equal => {
             // 정상 재시작: no-op
         }
         Ordering::Greater => {
             // 다운그레이드 감지: last_run_version 갱신하지 않음
             // (재업그레이드 시 마이그레이션 재실행 가능하도록)
             log::warn!("Downgrade detected: {} → {}", last, current);
             emit_warning("downgrade_detected", &last, &current).await;
         }
     }
     ```
  3. **버전별 마이그레이션 분기**: 특정 버전 구간에 필요한 로직을 배열로 정의.
     ```rust
     if last < Version::new(0, 2, 0) {
         core::indexing::force_reindex_all_projects(&db).await?;
     }
     ```
  4. **상태 업데이트**: post-update 마이그레이션의 **큐 등록이 완료된 시점**에 `last_run_version`을 `current_version`으로 갱신. 큐 처리 완료를 기다리지 않음 (앱 종료 후 재시작 시 idempotent하게 skip 가능).
  5. **실패 처리**: 마이그레이션 실패 시 `last_run_version`을 갱신하지 않고 앱을 계속 실행. 오류 내용은 `audit_log` 테이블에 기록하고 프론트엔드에 경고 알림 emit.

### 4.1 DB 스키마: `system_config` 테이블 (V40 마이그레이션)
`last_run_version`을 영속화하기 위해 새 마이그레이션 파일 추가 (현재 V39까지 존재):

```sql
-- crates/core/src/db/migrations/V40__add_system_config.sql
CREATE TABLE IF NOT EXISTS system_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 부트스트랩: V40 첫 적용 시 기존 사용자는 row가 없으므로 '0.0.0'으로 초기화.
-- 이렇게 하면 기존 사용자도 신규 설치와 동일하게 full migration path를 밟음.
-- 특정 버전 이전 사용자에게만 필요한 마이그레이션은 조건 분기로 제어.
INSERT OR IGNORE INTO system_config (key, value) VALUES ('last_run_version', '0.0.0');
```

사용 예:
```sql
INSERT OR REPLACE INTO system_config (key, value) VALUES ('last_run_version', '0.1.0');
SELECT value FROM system_config WHERE key = 'last_run_version';
```

### 4.2 `audit_log` 사용 패턴
마이그레이션 관련 이벤트 기록 시 `project_id`는 NULL (시스템 이벤트):

```rust
// event_type 목록
// 'migration_started'  — 버전 업그레이드 감지, 마이그레이션 시작
// 'migration_completed' — last_run_version 갱신 완료
// 'migration_failed'   — 마이그레이션 오류 (payload에 error 포함)
// 'downgrade_detected' — 다운그레이드 감지
```

payload 예시:
```json
{ "from_version": "0.1.0", "to_version": "0.2.0", "error": "force_reindex failed: ..." }
```

### 4.3 `force_reindex_all_projects()` 구현 계획
Phase 4 마이그레이션 인프라 구현 시 함께 추가:
- **위치**: `crates/core/src/index_engine.rs`
- **기능**: 모든 `active` 프로젝트의 인덱스를 초기화하고 전체 재인덱싱 작업을 큐에 등록.
- **idempotency**: 이미 큐에 등록된 작업은 content hash 비교로 skip하여 중단 후 재시작 시 안전하게 재개.

### 4.4 강제 재인덱싱(Forced Re-indexing) UX 고려
백그라운드 마이그레이션으로 인해 장시간 인덱싱이 다시 일어나는 경우 사용자에게 명확히 알려야 합니다.
- 백엔드에서 재인덱싱 시작 이벤트를 프론트엔드로 `emit` 처리. (이벤트명: `migration:reindex_started`, `migration:reindex_completed`)
- 프론트엔드는 이를 감지해 글로벌 토스트(Toast) 또는 상단 배너 알림 표시:
  > *"앱 업데이트로 인해 검색 성능 향상을 위한 데이터 재구성이 진행 중입니다. 이 기간 동안 검색 결과가 일부 누락될 수 있습니다."*

## 5. 단계별 구현 계획 (Roadmap)

> Phase 순서 주의: CI/CD 파이프라인(Phase 2)을 먼저 구성해야 프론트엔드 업데이터(Phase 3) E2E 테스트 가능.

- **Phase 1**: Tauri CLI를 이용한 ED25519 서명 키 생성 및 의존성 추가.
  - `@tauri-apps/plugin-updater` 설치 (package.json + Cargo.toml)
  - `tauri.conf.json`에 updater 섹션 추가 (pubkey + endpoint)
  - `main.rs`에 plugin 등록
- **Phase 2**: GitHub Actions 워크플로우 작성 (빌드, 서명 생성, Release 자동 업로드).
- **Phase 3**: `UpdateManager.ts` 프론트엔드 모듈 구현 및 `SettingsPage.tsx`에 업데이트 확인/다운로드 UI 구축.
- **Phase 4 (인프라)**: 백엔드 마이그레이션 기반 구현.
  - V40 마이그레이션 파일 (`system_config` 테이블 + 부트스트랩 INSERT)
  - `update_manager.rs` 모듈 구현 (버전 감지, 3-way 비교, audit_log 기록)
  - `force_reindex_all_projects()` core 함수 구현
- **Phase 5 (라우팅 + UX)**: 마이그레이션 라우팅 + 프론트엔드 연동.
  - semver 비교 기반 버전별 분기 (`< 0.2.0` → force_reindex 등)
  - 강제 재인덱싱 emit 처리 및 프론트엔드 토스트 알림 구현
