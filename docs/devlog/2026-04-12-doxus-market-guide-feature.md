---
title: "마켓 플러그인 가이드 기능 구현 및 UI 수정"
aliases:
  - market-plugin-guide-feature
  - 마켓 플러그인 가이드 기능
tags:
  - devlog
  - feature
  - frontend
  - troubleshooting
  - security
created: "2026-04-12"
updated: "2026-04-12"
---

<!-- docsmith: auto-generated 2026-04-12 -->

# 마켓 플러그인 가이드 기능 구현 및 UI 수정

## 배경

doxus 마켓 페이지에서 Confluence, GitHub 플러그인을 설치하려는 사용자가 인증 설정 방법을 알기 어렵다는 문제가 있었다. 또한 레지스트리 서버(`registry.doxus.io`)가 아직 운영되지 않아 네트워크 에러가 발생하고, `RegistryEntry` 구조가 프론트엔드 `Plugin` 타입과 맞지 않아 모든 플러그인이 "인증 불필요"로 표시되는 버그가 있었다.

이 세션에서는 다음 문제들을 한꺼번에 해결했다:

1. 레지스트리 fetch 실패 시 개발용 목 데이터 fallback 처리
2. `auth_type` 필드 누락으로 인한 인증 타입 오표시 수정
3. 플러그인 카드에 "가이드" 버튼 및 `PluginGuideModal` 컴포넌트 추가
4. Confluence, GitHub `GUIDE.md` 파일 작성

## 변경 내용

### 주요 변경사항

#### 레지스트리 목 데이터 fallback (`crates/core/src/marketplace/registry.rs`)

- `market_fetch_registry`에서 HTTP fetch 실패 시 개발용 Confluence, GitHub 목 데이터를 반환하도록 처리
- `eprintln!`으로 경고를 출력해 silent fail 방지
- 레지스트리 서버 운영 시작 시 코드 변경 없이 자동으로 실제 데이터를 사용하게 됨

#### `RegistryEntry` 구조 확장

```rust
// crates/core/src/marketplace/registry.rs
pub struct RegistryEntry {
    // 기존 필드 ...
    #[serde(default)]
    pub auth_type: String,   // "oauth" | "api_token" | ""
    #[serde(default)]
    pub guide_url: String,   // 로컬 절대경로 또는 HTTP URL
}
```

- `#[serde(default)]` 적용으로 기존 레지스트리 응답과 하위 호환성 유지

#### `MarketPage.tsx` 인증 타입 매핑 수정 (`apps/desktop/src/pages/MarketPage.tsx`)

- `registryEntryToPlugin` 함수에서 `auth_type` 필드를 `Plugin.auth_type`으로 올바르게 매핑
- 목 데이터에 `auth_type: "oauth"` (Confluence), `auth_type: "api_token"` (GitHub) 설정
- 수정 전에는 필드 누락으로 모든 플러그인이 "이 플러그인은 별도 인증이 필요하지 않습니다"로 표시됨

#### `PluginGuideModal` 컴포넌트 추가

- `react-markdown`으로 가이드 마크다운 렌더링
- 로딩 스피너 및 에러 상태 처리 포함
- 플러그인 카드에 초록 hover "가이드" 버튼 추가

#### `market_fetch_guide` Tauri 커맨드 추가 (`apps/desktop/src-tauri/src/commands/market.rs`)

```rust
#[tauri::command]
pub async fn market_fetch_guide(guide_url: String) -> Result<String, String> {
    if guide_url.starts_with('/') {
        // 로컬 파일 직접 읽기
        std::fs::read_to_string(&guide_url).map_err(|e| e.to_string())
    } else {
        // HTTP URL → reqwest fetch, 실패 시 기본 안내 반환
        // ...
    }
}
```

- `/`로 시작하는 경로는 `std::fs::read_to_string`으로 로컬 파일 읽기
- HTTP URL은 reqwest로 원격 fetch, 실패 시 기본 안내 문자열 반환
- `apps/desktop/src-tauri/src/main.rs`에 커맨드 등록

#### 플러그인 GUIDE.md 파일 작성

- `crates/plugins/confluence/GUIDE.md`: OAuth 앱 설정, Callback URL, 인증 절차, Space Key 설명, 지원 콘텐츠 표, 문제해결 섹션
- `crates/plugins/github/GUIDE.md`: PAT 발급, 권한 설정, Issues/Wiki/Discussions 지원 콘텐츠 표, 문제해결 섹션

`guide_url`은 컴파일 타임 절대 경로로 설정:

```rust
const GUIDE_URL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../crates/plugins/confluence/GUIDE.md"
);
```

### 영향 범위

- `crates/core/src/marketplace/registry.rs`: `RegistryEntry` 구조체, fetch 로직
- `apps/desktop/src/pages/MarketPage.tsx`: `registryEntryToPlugin`, 플러그인 카드 UI
- `apps/desktop/src/components/PluginGuideModal.tsx`: 신규 파일
- `apps/desktop/src-tauri/src/commands/market.rs`: `market_fetch_guide` 커맨드 추가
- `apps/desktop/src-tauri/src/main.rs`: 커맨드 등록
- `crates/plugins/confluence/GUIDE.md`: 신규 파일
- `crates/plugins/github/GUIDE.md`: 신규 파일

## 결과

- 마켓 페이지에서 레지스트리 서버 미운영 상태에서도 Confluence, GitHub 플러그인이 올바르게 표시됨
- Confluence 플러그인은 "OAuth 인증 필요", GitHub 플러그인은 "API 토큰 필요"로 정확하게 표시
- "가이드" 버튼 클릭 시 모달로 플러그인별 설정 안내 문서를 렌더링
- `guide_url`이 로컬 경로이므로 네트워크 없이도 가이드 조회 가능

## 교훈

### 트러블슈팅 기록

| 문제 | 원인 | 해결 |
|------|------|------|
| `npm run tauri dev` 실패 | 워크스페이스 루트에서 실행 | `cd apps/desktop` 후 실행 |
| 포트 1420 이미 사용 중 | 이전 Vite 프로세스 잔존 | `lsof -ti:1420 \| xargs kill -9` |
| DB 마이그레이션 V9 실패 | `duplicate column name: content` | `~/.doxus/db/doxus.db` 삭제 후 재기동 |
| `tracing::warn!` 컴파일 에러 | desktop crate에 tracing 미포함 | `eprintln!`으로 대체 |
| `description` 필드 없음 | `RegistryEntry` 구조 불일치 | 필드 제거 |
| 가이드 HTTP 404 | GitHub repo 미존재 URL 사용 | `env!("CARGO_MANIFEST_DIR")` 컴파일 타임 절대 경로로 변경 |

### 설계 인사이트

- **가이드 파일 배치**: 플러그인 크레이트 내부에 `GUIDE.md`로 배치하면 코드와 문서가 함께 버전 관리됨. 레지스트리 서버 운영 시작 시 HTTP URL로 전환만 하면 되어 점진적 마이그레이션이 용이함
- **`#[serde(default)]` 패턴**: 레지스트리 응답 스키마가 바뀌더라도 기존 클라이언트가 깨지지 않으려면 신규 필드에 반드시 `default` 적용
- **목 데이터 fallback**: 서버 미운영 구간에서 개발을 이어가려면 fallback 데이터가 필수. `eprintln!` 경고를 남겨야 프로덕션 전환 시 누락 방지

## 관련 문서

- [[docs/devlog/2026-04-04-phase2d-plugin-auth-ui]]
- [[docs/devlog/2026-04-04-confluence-oauth-troubleshooting]]
- [[docs/implementation-status]]

## 2026-04-12 추가 작업

---

세션 2차에서 추가로 수행한 작업들:

### 1. 키체인 반복 프롬프트 수정 (usePluginStore.ts)
- `fetchAuthStatus`가 매 페이지 접근마다 keychain 읽기를 반복하던 문제 수정
- 이미 캐시된 결과(`!loading` 상태)가 있으면 skip하는 로직 추가
- Zustand `set` 내부에서 현재 상태를 확인하는 패턴 사용

### 2. GitHub PAT 입력 UI 수정 (MarketPage.tsx)
- RegistryEntry를 Plugin으로 변환 시 `auth_schema`가 비어있어 GitHub 설정 모달에 입력 필드가 없던 버그 수정
- `PLUGIN_AUTH_SCHEMAS` 맵을 추가하여 plugin_id 기반으로 schema 주입
- `com.doxus.github` → PAT 비밀번호 필드 정의

### 3. 마크다운 가이드 렌더링 개선 (MarketPage.tsx)
- 기존 ReactMarkdown에 `remark-gfm`, `rehype-highlight` 플러그인 연결
- `highlight.js/styles/github-dark.css` 임포트로 코드 블록 다크 테마 적용
- Tailwind prose 클래스 확장: 테이블 헤더 배경, 코드블록 둥근 모서리, blockquote 스타일링

### 4. Confluence ancestor_id 필터 기능 추가 (crates/plugins/confluence/src/lib.rs)
- 특정 페이지 하위 트리만 연동하는 `ancestor_id` 선택적 필터 추가
- `~222368988` 같은 개인 스페이스 키의 `~` 문자 허용 버그 수정
- `fetch_all`: ancestor_id 있으면 CQL `ancestor = "ID"` 쿼리, 없으면 기존 spaceKey REST API
- `fetch_changes`: ancestor_id/space_key 분기 처리
- `fetch_all_ancestor_ids` helper 추가 (삭제 감지용)
- 테스트 33개 통과

### 5. V11 마이그레이션 + source_type 기반 플러그인 라우팅
- `V11__project_source.sql`: `projects` 테이블에 `source_type`, `config_json` 컬럼 추가
- `add_project` 커맨드에 `source_type`, `config` 파라미터 추가
- `index_project` 커맨드를 source_type 기반 분기로 교체 (confluence/github/obsidian)
- Confluence 인덱싱 시 keyring에서 access_token 자동 조회
- useProjectStore, ProjectsPage.tsx에서 pluginType과 config 필드 전달
- V11이 MIGRATIONS 배열에 누락된 것 발견 후 추가

### 6. Confluence 폴더 API 디버깅 (미해결)
- Confluence Cloud의 "폴더" 타입은 REST v1 API에서 `type=page`로 조회 불가
- `ancestor = "4667998225"` CQL로 하위 페이지 조회 시 0개 반환
- v2 `/wiki/api/v2/folders/{id}/children` API는 HTML 응답 반환 (미지원)
- `spaceKey=~222368988`로 전체 검색 시도 → 404 "No space with key: ~222368988"
- 실제 스페이스 목록도 0개 (OAuth 앱 권한 부족 가능성)
- **미결 사항**: Confluence Cloud 폴더 내 문서 인덱싱 방법 추가 조사 필요. PAT(API 토큰) 방식으로 전환하면 해결될 가능성 있음

### 트러블슈팅 기록 (2차)

| 문제 | 원인 | 해결 |
|------|------|------|
| 인덱싱 0개 | index_project가 항상 ObsidianPlugin 사용 | source_type 기반 분기로 교체 |
| V11 마이그레이션 미적용 | MIGRATIONS 배열에 V11 누락 | mod.rs에 추가 |
| Confluence 404 | Base URL에 /wiki 경로 누락 | config에 /wiki 포함하도록 수정 |
| access_token 만료 | 1시간 유효, 이미 만료 | 재인증 필요 |
| 폴더 ID CQL 0개 | Confluence 폴더 타입은 REST v1 미지원 | 미해결, 조사 중 |

## 2026-04-12 추가 작업

### Confluence folder 타입 버그 조사 및 수정

#### 배경

Confluence Cloud에서 "folder" 타입 콘텐츠를 ancestor로 지정하면 `AND type = page` CQL 필터가 결과를 0건으로 만드는 버그가 있었다. 사용자가 `https://madup.atlassian.net/wiki/spaces/~222368988/folder/4667998225`를 프로젝트로 추가했을 때 "0개 문서 인덱싱 완료"가 표시되는 문제.

#### 조사 과정

1. **Confluence v2 API 공식 문서 조사** — `/wiki/api/v2/folders/{id}/children` 엔드포인트 존재 확인
2. **실제 API 테스트 (curl + API Token)**:
   - v2 `/folders/{id}/children` → 404 (실제로는 미지원)
   - v1 `/rest/api/content/4667998225` → `type: folder` 확인
   - CQL `ancestor = "4667998225"` (type 필터 없음) → pages + folders 혼합 반환
   - CQL `ancestor = "4667998225" AND type = page` → 0건 (버그 원인 확인)

핵심 발견: Confluence Cloud의 folder 타입 ancestor에 `AND type = page` 조건을 붙이면 결과가 0이 된다. type 필터를 제거하면 pages와 folders가 함께 반환된다.

#### 수정 내용 (TDD)

**실패 테스트 먼저 작성 → 실패 확인 → 구현 → 통과 확인**

`crates/plugins/confluence/src/lib.rs`:

1. **`ConfluencePage` struct에 `content_type` 필드 추가**
```rust
#[serde(rename = "type", default = "default_page_type")]
content_type: String,
```
`#[serde(default)]`로 `type` 필드가 없는 구버전 응답도 안전하게 처리.

2. **`fetch_all` ancestor CQL 수정**
```rust
// Before (버그)
"ancestor = \"{ancestor_id}\" AND type = page ORDER BY title ASC"
// After
"ancestor = \"{ancestor_id}\" ORDER BY title ASC"
```
결과에서 `.filter(|p| p.content_type == "page")`로 folder 타입 제외.

3. **`fetch_changes` 결과 필터링 추가** — folder 타입 item이 빈 content로 인덱싱되는 문제 방지.

4. **`fetch_all_ancestor_ids` CQL 수정 + ID 수집 필터링** — 삭제 감지 오작동 방지.

5. **`ancestor_id` 숫자 전용 검증 추가 (보안 패치)**
```rust
// validate_config에서
if !ancestor_id.chars().all(|c| c.is_ascii_digit()) {
    return Err(PluginError::ConfigInvalid("ancestor_id must be a numeric content ID"));
}
```
CQL 인젝션 공격 방어. Confluence content ID는 항상 숫자이므로 안전하게 제약 가능.

#### 테스트 결과

- 신규 테스트 4개 (folder 필터링) + 2개 (ancestor_id 검증) = 6개 추가
- 전체 39개 테스트 통과
- 빌드 성공

#### 학습한 것

- Confluence Cloud의 "folder" 타입은 REST v1에서 `ancestor` CQL의 부모로는 동작하지만, `type = page` 조건과 함께 쓰면 결과를 0으로 만든다.
- v2 API의 `/folders/{id}/children`은 공식 문서에 있으나 실제 응답은 404 — 문서와 실제 동작이 다를 수 있으므로 직접 테스트가 필수.
- 외부 API 파라미터(ancestor_id)는 반드시 입력 검증을 통해 CQL 인젝션을 방어해야 한다.

## 2026-04-12 트러블슈팅: Confluence OAuth 토큰 만료 및 API 엔드포인트 수정

### 문제 상황

Confluence 플러그인으로 추가한 두 프로젝트 인덱싱 시 오류 발생:

- "개인/컨플 공유문서" (`ancestor_id: 4667998225`): 0개 문서 인덱싱 완료
- "컨플/마이 스페이스" (`space_key: ~222368988`): "문서 가져오기 실패: not found: resource not found"

### 원인 탐색 과정

#### 1단계 — DB config 분석

- "마이 스페이스": `base_url = "https://madup.atlassian.net"` (`/wiki` 없음) → API URL 잘못됨
- "공유문서": `base_url = "https://madup.atlassian.net/wiki"` (정상), `ancestor_id = "4667998225"`

#### 2단계 — base_url 자동 정규화 패치

`initialize()`에서 `.atlassian.net` URL에 `/wiki` 자동 append:

```rust
let normalized_base_url = if raw_base_url.contains(".atlassian.net") && !raw_base_url.ends_with("/wiki") {
    format!("{raw_base_url}/wiki")
} else {
    raw_base_url.to_string()
};
```

패치 후에도 여전히 0개 반환 → 추가 조사 필요.

#### 3단계 — fetch_all 응답 디버깅

`eprintln`으로 실제 URL, HTTP status, 응답 바디 로깅 추가. 결과:

- 두 케이스 모두 `{"results":[],"start":0,"limit":...}` — **200 OK이지만 0개**
- CQL `ancestor = "4667998225"` → 0개
- CQL `space = "~222368988" AND type = page` → 0개

#### 4단계 — Confluence API 직접 테스트 (브라우저)

- `/rest/api/content/4667998225/child/page` → **7개 페이지 정상 반환**
- `/rest/api/space?type=personal` → space key 목록 확인, `~222368988` 유효
- 브라우저(세션 쿠키)로는 동작, API Bearer 토큰으로는 0건 반환

#### 5단계 — CQL → 직접 REST 엔드포인트 전환

- ancestor: `/rest/api/content/{id}/descendant/page`
- space: `/rest/api/space/{key}/content/page`
- 결과: 두 케이스 모두 **404로 변경** → 응답 형태가 달라졌으나 근본 문제 미해결

#### 6단계 — 토큰 상태 확인 (최종 원인 발견)

keychain 확인:
- `doxus:com.doxus.confluence:api_token` = 비어있음
- `doxus:com.doxus.confluence:access_token` = JWT (2398바이트)

JWT 디코딩 결과: `exp=1775299850`, `now=1775961579` → **7.6일 전 만료**

### 최종 원인

OAuth `access_token`이 만료됐고, `refresh_token`이 keychain에 저장되지 않아 자동 갱신 불가.
`plugin_oauth_exchange` (`apps/desktop/src-tauri/src/commands/market.rs:403`)에서 `refresh_token`을 keychain에 저장하지 않는 구조적 버그.

### 구현된 코드 변경사항

**`crates/plugins/confluence/src/lib.rs`:**

- `initialize()`: `.atlassian.net` URL에 `/wiki` 자동 append
- `fetch_all()` ancestor 모드: ancestor 페이지 자체도 첫 페이지에 포함
- `fetch_all()` 전체: CQL → 직접 REST 엔드포인트로 전환
  - ancestor: `/rest/api/content/{id}/descendant/page`
  - space: `/rest/api/space/{key}/content/page`

**`~/.doxus/db/doxus.db` (직접 수정):**

- "컨플/마이 스페이스" 프로젝트 `base_url`: `https://madup.atlassian.net` → `https://madup.atlassian.net/wiki`

### 미해결 사항

| 항목 | 설명 |
|------|------|
| 사용자 재인증 필요 | 앱에서 Confluence OAuth 재로그인 |
| `refresh_token` 미저장 버그 | `plugin_oauth_exchange` (market.rs:403)에서 `refresh_token`을 keychain에 저장하지 않음 |
| `client_id`/`secret` 미전달 | `ensure_valid_token()`이 동작하려면 프로젝트 config에 OAuth 클라이언트 자격증명 필요 |

### 학습

- Confluence Cloud CQL은 **만료된 Bearer 토큰으로도 200 OK + 빈 결과**를 반환한다 (401 대신). 직접 REST 엔드포인트는 만료 토큰에서 404를 반환해 토큰 문제를 더 빨리 노출시킨다.
- personal space key `~222368988` 형식은 유효하나 구형 형식 — 신형은 `~{accountId}` UUID 형태도 존재한다.
- `/child/page`는 직접 자식만, `/descendant/page`는 모든 하위 페이지를 반환한다.
- OAuth 플로우에서 `refresh_token`을 keychain에 저장하지 않으면 `access_token` 만료 시 자동 갱신이 불가능하다. 인증 구현 시 반드시 `refresh_token` 저장을 체크해야 한다.

## 2026-04-12 오후 세션 — Confluence OAuth → Personal API Token 전환

### 배경

이전 세션에서 OAuth `access_token` 만료 문제를 발견했다. 재인증 후에도 Confluence 인덱싱이 0개를 반환하는 근본 원인을 추적한 결과, Atlassian OAuth 3LO Classic 토큰 자체가 사용자 콘텐츠에 접근할 수 없는 구조임을 확인했다. 이 세션에서는 인증 방식을 Personal API Token 기반 Basic auth로 전환했다.

### 발생한 문제들과 해결 과정

#### 1. `/descendant/page` 404 문제

`fetch_all`에서 `/rest/api/content/{id}/descendant/page` 사용 시 Confluence Cloud에서 404를 반환했다.

**해결**: CQL `ancestor = "ID" ORDER BY id ASC` 방식으로 전환.

#### 2. CQL 200 OK + empty results

CQL이 오류 없이 빈 결과를 반환하는 문제가 지속됐다.

**원인 분석**: Atlassian OAuth 3LO Classic 토큰(`aud=client_id`)은 `madup.atlassian.net` CQL에서 silent empty를 반환한다. HTTP 401이 아닌 200+empty로 응답하기 때문에 토큰 문제를 코드 레벨에서 감지하기 어렵다.

#### 3. cloudId 미저장 → 잘못된 base URL

OAuth 토큰 교환 후 `accessible-resources` API로 cloudId를 가져와 저장하는 코드를 추가했다. `search.rs`에서 cloudId를 이용해 `api.atlassian.com/ex/confluence/{cloudId}` URL을 사용하도록 변경했으나, 결과는 `401 "Unauthorized; scope does not match"` — `aud=client_id` 토큰은 `api.atlassian.com`에서도 작동하지 않는다.

#### 4. JWT 분석으로 근본 원인 확인

`security find-generic-password`로 keychain에서 토큰을 추출한 뒤 JWT payload를 디코딩했다.

```
aud: i1FQ2V4ePGrf29PdMmB0OsJQPDRdVLOe  (client_id) — Classic 토큰
authProfile: oauth.ecosystem.oauthIntegration
systemAccountEmail: ...@connect.atlassian.com
```

- `aud=client_id`: Classic 토큰 확인
- `systemAccountEmail`: 사용자가 아닌 시스템 계정(`connect.atlassian.com`)으로 발급됨
- CQL `type=page` (필터 없음)로도 0개 반환

**결론**: 이 토큰으로는 Confluence 사용자 콘텐츠에 접근 불가. OAuth 앱 설정 또는 Atlassian 테넌트 정책 문제로 추정되며, Personal API Token으로 전환하는 것이 가장 확실한 해결책이다.

### 최종 해결책: Personal API Token + Basic auth 전환

**설계 결정**: Atlassian Cloud OAuth 3LO가 시스템 계정 토큰을 발급해 사용자 콘텐츠에 접근 불가한 상황이므로, Personal API Token (`email:token` Basic auth)으로 인증 방식을 전환한다.

### 코드 변경 목록

#### `crates/plugins/confluence/src/lib.rs`

- `ConfluencePlugin`에 `email: Option<String>` 필드 추가
- `auth_header()` 메서드 추가: email이 있으면 `Basic base64(email:token)`, 없으면 `Bearer token`
  ```rust
  fn auth_header(&self) -> Result<String, PluginError> {
      match (&self.email, &self.api_token) {
          (Some(email), Some(token)) => {
              let encoded = base64::encode(format!("{email}:{token}"));
              Ok(format!("Basic {encoded}"))
          }
          (None, Some(token)) => Ok(format!("Bearer {token}")),
          _ => Err(PluginError::AuthRequired),
      }
  }
  ```
- `initialize()`에서 config의 `email` 필드 읽기
- `fetch_all_space_ids`, `fetch_all_ancestor_ids` 파라미터를 `api_token: &str`에서 `auth_header: &str`로 변경
- 모든 `format!("Bearer {api_token}")` → `self.auth_header()?`로 교체
- `Cargo.toml`에 `base64 = "0.22"` 추가

#### `apps/desktop/src-tauri/src/commands/market.rs`

- Confluence 플러그인 `auth_type`: `"oauth"` → `"api_token"`
- `auth_schema`: client_id/client_secret 필드 → `email` + `api_token` 필드로 변경
- `plugin_check_auth`에서 체크할 키: `["access_token", "api_token"]` → `["api_token", "email"]`
- OAuth 교환 시 `accessible-resources` 호출로 cloudId 저장 코드 추가 (향후 사용 대비)

#### `apps/desktop/src-tauri/src/commands/search.rs`

- keychain에서 `email`을 읽어 plugin config에 추가
- Personal API Token이면 `api_token`을 token으로 사용, Bearer OAuth면 `access_token` 사용

### 트러블슈팅 기록

| 문제 | 원인 | 해결 |
|------|------|------|
| CQL 200 OK + 0건 | OAuth Classic 토큰이 시스템 계정으로 발급 | Personal API Token으로 전환 |
| `/descendant/page` 404 | Confluence Cloud 미지원 엔드포인트 | CQL `ancestor = "ID"` 방식으로 변경 |
| `api.atlassian.com` 401 scope mismatch | `aud=client_id` 토큰은 `api.atlassian.com` 미지원 | `{org}.atlassian.net` 직접 사용 (Personal API Token과 함께) |

### 학습한 사항

- **Atlassian OAuth 3LO Classic 토큰** (`aud=client_id`): `{org}.atlassian.net` CQL에서 200+empty 반환. 401이 없어 코드 레벨 감지가 불가능하다.
- **Atlassian OAuth + cloudId**: `accessible-resources`로 cloudId 확인 후 `api.atlassian.com/ex/confluence/{cloudId}` 사용이 공식 방법이지만, `aud=client_id` Classic 토큰에서는 `scope does not match`로 실패한다.
- **Personal API Token**: `Authorization: Basic base64(email:token)` 형식, `{org}.atlassian.net` 직접 사용. 사용자 콘텐츠에 접근 가능하며 만료/갱신 문제가 없다.
- JWT `aud` 클레임으로 토큰 종류를 판별할 수 있다: `aud=client_id`는 Classic, `aud=api.atlassian.com`은 신형 토큰.

### 미완성 항목

| 항목 | 설명 |
|------|------|
| 앱 재빌드 후 E2E 테스트 | 설정 화면에서 email + Personal API Token 입력 및 저장 → 인덱싱 검증 필요 |
| debug eprintln 정리 | `lib.rs`, `search.rs`의 디버그 출력문 제거 필요 |
| `refresh_token` 저장 버그 | OAuth 방식 유지 시 `plugin_oauth_exchange` (market.rs:403)에서 `refresh_token` keychain 저장 필요 |

## 2026-04-12 저녁 세션 — UI 버그 수정 및 기능 추가

### 작업 목록

#### 1. Confluence 설정 UI 수정

- `apps/desktop/src-tauri/src/commands/market.rs`: mock fallback data의 `auth_type: "oauth"` → `"api_token"` 수정 (레지스트리 서버 접근 실패 시 사용되는 fallback)
- `apps/desktop/src/pages/MarketPage.tsx`: `PLUGIN_AUTH_SCHEMAS`에 `com.doxus.confluence` 추가 (email + api_token 필드). 기존에는 confluence 항목 자체가 없어서 schema가 빈 배열이었음
- `crates/plugins/confluence/GUIDE.md`: OAuth 앱 설정 절차 전체 → Personal API Token 발급/입력 방식으로 전면 교체. Base URL에 `/wiki` 미포함 주의사항, 트러블슈팅 섹션 추가

#### 2. 프로젝트 삭제 기능

- `apps/desktop/src-tauri/src/commands/search.rs`: `remove_project` Tauri 커맨드 추가 (`DELETE FROM projects WHERE name = ?1`, affected == 0이면 에러 반환)
- `apps/desktop/src-tauri/src/main.rs`: `remove_project` 커맨드 등록
- `apps/desktop/src/stores/useProjectStore.ts`: `removeProject` 액션 추가
- `apps/desktop/src/pages/ProjectsPage.tsx`: 각 프로젝트 카드에 빨간 삭제 버튼 추가, `window.confirm`으로 "인덱스 데이터만 삭제, 원본 유지" 안내

#### 3. 플러그인별 프로젝트 그룹핑

- `search.rs` `list_projects`: 쿼리에 `COALESCE(source_type, 'obsidian')` 추가
- `useProjectStore.ts` `Project` 타입에 `source_type: string` 추가
- `ProjectsPage.tsx`: `pluginMeta()` 헬퍼 추가, 프로젝트 목록을 `source_type`별로 그룹핑하여 아이콘+이름 섹션 헤더 표시 (Obsidian, Confluence, GitHub)

#### 4. 검색 프리뷰 다중 버그 수정

**근본 원인**: `documents` 테이블의 `file_path` 컬럼이 인덱싱 시 채워지지 않아 항상 NULL → 검색 히트의 `file_path`가 null → 프론트엔드가 `get_document_content` 호출을 스킵하고 snippet fallback으로 떨어짐

- `crates/core/src/search.rs`: FTS, vector 검색 쿼리 모두 `d.file_path` → `COALESCE(d.file_path, d.source_doc_id)` 수정 (2곳)
- `apps/desktop/src-tauri/src/commands/search.rs`:
  - `get_document_content_impl`: `LIMIT 1` → `ORDER BY id ASC` 전체 청크 조회 후 `\n\n` 합산
  - 조회 조건: `source_doc_id = ?1 OR file_path = ?1`
- `SearchPage.tsx`:
  - `previewLoading`, `previewError` state 추가 (에러 무음 처리 → 빨간 메시지 표시)
  - `rehype-raw` 적용 (`<b>` 등 HTML 태그 렌더링)
  - `remark-gfm` 적용 (GFM 테이블 렌더링)
  - prose 테이블 CSS 추가

### 발생한 문제와 해결

| 문제 | 원인 | 해결 |
|------|------|------|
| Confluence 설정에서 OAuth 폼 계속 표시 | mock fallback data `auth_type: "oauth"` + `PLUGIN_AUTH_SCHEMAS`에 confluence 없음 | 두 곳 모두 수정 |
| 프리뷰 silent 실패 | `file_path` NULL로 invoke 스킵 | `COALESCE(file_path, source_doc_id)` |
| `ORDER BY chunk_index` 에러 | V2 스키마에 해당 컬럼 없음 | `ORDER BY id ASC` |
| `<b>` 태그 그대로 노출 | `rehype-raw` 미적용 | import 후 적용 |
| 테이블 텍스트로 표시 | `remark-gfm` 미적용 | import 후 적용 |

### 학습한 사항

- Atlassian OAuth Classic 토큰은 `aud=client_id`로 발급되어 사용자 Confluence 콘텐츠 접근 불가. Personal API Token + Basic auth가 실용적 해법
- doxus `documents` 테이블의 `file_path`는 V2에 선언됐지만 인덱싱 파이프라인이 채우지 않음 → `source_doc_id`가 사실상 문서 식별자로 동작

---

## 2026-04-12 (오후)

<!-- docsmith: auto-generated 2026-04-12 -->

### 작업 1: SearchPage UI 전면 재설계 (TDD)

#### 배경

기존 SearchPage는 [상단 검색바 + 좌측 디렉토리 트리(w-48) + 중간 결과 카드 목록 + 우측 문서 프리뷰] 4패널 구조였다. 요구사항: 상단 검색바 + 하단 2패널(파일목록 넓게 + 프리뷰), VSCode 스타일 폴더 트리.

#### TDD 과정

**RED** — `tests/search_command_test.rs` 작성:
- `list_all_documents_returns_empty`
- `list_all_documents_returns_docs`
- `list_all_documents_excludes_disabled`
- `list_all_documents_counts_distinct_source_docs`
- `list_all_documents_groups_by_project`

실패 확인: `error[E0432]: unresolved import list_all_documents_impl` (함수 미구현)

**GREEN** — `crates/core/src/search.rs` 및 `commands/search.rs` 구현:

```rust
pub fn list_all_documents_impl(conn: &rusqlite::Connection) -> Result<serde_json::Value, String> {
    let mut stmt = conn.prepare(
        "SELECT MIN(d.id), MIN(d.title), MIN(d.source_doc_id), p.name, COALESCE(p.source_type, 'obsidian')
         FROM documents d
         JOIN projects p ON d.project_id = p.id
         WHERE p.status = 'active'
         GROUP BY d.source_doc_id, d.project_id
         ORDER BY p.name, MIN(d.title)"
    )...
}
```

결과: 5 tests GREEN

#### Frontend 변경 사항

**`useSearchStore.ts`**:
- `AllDocument` 인터페이스 추가 (`document_id`, `title`, `source_doc_id`, `project_name`, `source_type`)
- `allDocuments: AllDocument[]`, `allDocsLoading: boolean` 상태 추가
- `listAllDocuments()` 액션 추가 (`invoke('list_all_documents')`)

**`SearchPage.tsx` 전면 재설계**:
- 레이아웃: 상단 검색바 + 하단 `flex gap-3`
- 파일목록: `w-72 shrink-0` (기존 w-48 + 카드목록 두 패널 통합)
- 프리뷰: `flex-1` 항상 표시, 선택 없을 때 empty state
- 검색 전: `listAllDocuments()`로 전체 문서, 프로젝트별 그룹화
- 검색 후: `hits`를 프로젝트별 그룹화

**폴더 트리 (`buildTree` / `TreeNodeView`)**:
- `source_doc_id` 경로를 `/`로 파싱해 폴더/파일 계층 구성
- 폴더: `📁` + 노란색, depth당 12px 들여쓰기
- 폴더 기본 상태: 접힘 (`useState(false)`)
- 파일: `depth` prop으로 들여쓰기 반영

**Hover 툴팁**:
- 1초 대기 후 메타정보 표시 (title, source_doc_id, project_name, score)
- `useRef<setTimeout>` + `onMouseEnter/Leave/Move`

---

### 작업 2: `get_document_content` 플러그인 실시간 호출로 교체

#### 배경

기존: SQLite 캐시(`documents.content`)에서 읽음
변경: `project_name` 파라미터 추가 → 플러그인 `fetch_document()` 실시간 호출

#### 변경 내용

**`commands/search.rs`**:

```rust
pub async fn get_document_content(
    state: tauri::State<'_, crate::AppState>,
    file_path: String,
    project_name: Option<String>,  // 추가
) -> Result<serde_json::Value, String>
```

`source_type`별 분기:
- `confluence` → `ConfluencePlugin::fetch_document()` (API 호출)
- `github` → `GitHubPlugin::fetch_document()` (API 호출)
- `obsidian` → `ObsidianPlugin::fetch_document()` (로컬 파일)
- `project_name` None → SQLite fallback 유지

**`SearchPage.tsx`**:

```ts
invoke('get_document_content', {
  filePath: identifier,
  projectName: doc.project_name || undefined,
})
```

---

### 작업 3: Confluence 자동 재인덱싱 계획 수립 (미구현)

#### 배경

`fetch_document` 응답의 `raw.updated_at`과 DB `last_indexed`를 비교해 변경 시 자동 재인덱싱하는 기능.

#### 계획 (critic 리뷰: ACCEPT-WITH-RESERVATIONS)

주요 수정 포인트:
1. `should_reindex` 순수 함수 추출 → 테스트 가능
2. content_hash 비교가 timestamp보다 신뢰성 높음 (Critic 지적)
3. DB 쿼리 1번으로 통합: `LEFT JOIN documents d ON d.project_id = p.id AND d.source_doc_id = ?2`
4. 재인덱싱 실패 시 캐시 content + `reindex_error` 필드 반환 (graceful degradation)

미구현 상태 — `.omc/plans/`에 보존

---

### 발생한 문제와 해결

| 문제 | 원인 | 해결 |
|------|------|------|
| `list_all_documents` E0432 | 함수 미구현 | TDD GREEN 단계에서 구현 |
| 폴더 트리 모든 폴더 펼침 상태 | `useState(true)` 기본값 | `useState(false)`로 수정 |
| 프리뷰 패널 없는 경우 레이아웃 깨짐 | 조건부 렌더링 | 항상 렌더링 + empty state |

### 학습한 사항

- `source_doc_id`를 `/`로 파싱하면 별도 파일시스템 스캔 없이 폴더 트리 구성 가능
- content_hash 기반 변경 감지가 timestamp보다 신뢰성이 높음 (타임존, 서버 클락 드리프트 영향 없음)
- Tauri `invoke` 호출 시 파라미터는 camelCase로 전달 (Rust snake_case → JS camelCase 자동 변환)

---

## 2026-04-12 저녁 세션 — Confluence 자동 재인덱스 TDD 구현 및 인증 버그 수정

<!-- docsmith: auto-generated 2026-04-12 -->

### 작업 1: Confluence 자동 재인덱스 TDD 구현

#### 배경

Confluence 문서 프리뷰 시 `fetch_document`로 실시간 본문을 가져오는 기능이 구현된 상태였다. 문서 내용이 변경됐을 때 DB 인덱스를 자동 갱신하는 기능이 필요했으며, Critic 리뷰에서 `updated_at` 타임스탬프 비교보다 `content_hash` 비교가 더 신뢰성 있다고 권고했다.

#### TDD 과정 (RED → GREEN)

**테스트 파일: `apps/desktop/src-tauri/tests/confluence_reindex_test.rs`**

- `reindex_if_stale_skips_when_hash_same` — 동일 content_hash면 reindex 안 함
- `reindex_if_stale_updates_when_hash_differs` — hash 다를 때 reindex 후 DB 갱신 확인
- `reindex_if_stale_returns_false_when_doc_not_in_db` — 신규 문서는 false 반환

**구현: `apps/desktop/src-tauri/src/commands/search.rs`**

```rust
pub fn reindex_if_stale(
    conn: &rusqlite::Connection,
    project_name: &str,
    source_doc_id: &str,
    title: &str,
    content: &str,
) -> Result<bool, String>
```

- Single JOIN query로 `project_id` + `stored content_hash` 조회
- `sha256(new_content) != stored_hash`일 때만 `SearchEngine::index_document` 호출
- `rusqlite::OptionalExtension` 사용

**통합**: `get_document_content` Confluence 브랜치에서 `fetch_document` 후 자동으로 `reindex_if_stale` 호출, 응답에 `reindex_triggered: bool` 포함.

#### 기존 테스트 컴파일 오류 픽스

`RegistryEntry` 구조체에 `auth_type`, `guide_url` 필드가 추가됐으나 여러 파일에서 누락되어 컴파일 실패:

- `crates/core/tests/semver_range_test.rs`
- `crates/core/src/marketplace/installer.rs`
- `crates/core/src/plugin/registry.rs`
- `crates/core/src/plugin/manager.rs`
- `apps/desktop/src-tauri/tests/market_command_test.rs`

또한 `market_command_test.rs`가 `ed25519-dalek`, `mockito`, `hex`, `sha2`를 dev-dependency 선언 없이 사용하던 문제를 `apps/desktop/src-tauri/Cargo.toml`에 추가하여 해결.

---

### 작업 2 (troubleshooting): Confluence 프리뷰 "문서 가져오기 실패: not found" 버그

#### 증상

Confluence 문서 클릭 시 `"문서 가져오기 실패: not found: resource not found"` 에러 표시.

#### 원인 분석

1. `check_status` 함수에서 HTTP 404 → `PluginError::NotFound("resource not found")` 반환
2. `get_document_content` Confluence 브랜치에서 `access_token`(OAuth Bearer)을 읽지 않고 `api_token`만 읽음
3. `index_project`는 OAuth/PAT 토큰 선택 로직이 올바르게 구현되어 있었으나 `get_document_content`에는 반영되지 않음
4. curl 직접 테스트: Basic auth로 200 반환 확인 → credentials 자체는 정상
5. 디버그 로그(`token_len`) 추가 후 정확한 원인 특정

#### 수정

`get_document_content` Confluence 브랜치에 `index_project`와 동일한 토큰 선택 로직 적용:

```rust
let access_token = keyring::Entry::new("doxus", "doxus:com.doxus.confluence:access_token")...
let token = if !access_token.is_empty() && email.is_empty() {
    access_token  // OAuth Bearer
} else {
    api_token     // PAT Basic auth
};
```

#### 교훈

- 동일한 인증 로직이 `index_project`와 `get_document_content` 두 곳에 중복 — 추후 공통 함수로 추출 검토
- HTTP 404가 실제 "문서 없음"이 아닌 "인증 실패로 인한 404"일 수 있으므로 디버그 로그(`token_len` 등)가 원인 특정에 중요

## 관련 문서

- [[doxus 프로젝트 개요]]
- [[Confluence 플러그인 설계]]
