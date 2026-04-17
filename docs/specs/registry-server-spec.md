# doxus Registry Server Specification

## 1. 개요 (Overview)

doxus 레지스트리 서버는 플러그인 생태계의 중앙 허브로, 플러그인 바이너리(.wasm)의 안전한 호스팅과 메타데이터 공급을 담당합니다. 본 명세서는 플러그인 설치, 업데이트 및 보안 검증을 위한 레지스트리 서버의 아키텍처와 인터페이스를 정의합니다.

## 2. 아키텍처 (Architecture)

### 2.1 정적 호스팅 엔진 (Static Hosting)
레지스트리는 고성능과 보안을 위해 GitHub Pages, Cloudflare Pages 또는 AWS S3와 같은 정적 웹 호스팅 서비스 기반으로 구축하는 것을 권장합니다.

*   **Registry URL**: `https://registry.doxus.io` (예시)
*   **파일 구조**:
    ```text
    /
    ├── plugins.json           # 플러그인 전체 목록 (메타데이터)
    ├── download/              # WASM 바이너리 저장소
    │   ├── com.doxus.confluence/
    │   │   ├── 1.0.0.wasm
    │   │   └── 1.2.0.wasm
    │   └── com.doxus.github/
    │       └── 1.0.0.wasm
    └── guides/                # 플러그인 가이드 (Markdown)
        ├── com.doxus.confluence.md
        └── com.doxus.github.md
    ```

## 3. 데이터 스키마 (Data Schema)

### 3.1 `plugins.json`

레지스트리의 모든 플러그인 정보는 루트의 `plugins.json` 파일에 배열 형태로 저장됩니다.

```json
[
  {
    "plugin_id": "com.doxus.confluence",
    "version": "1.0.0",
    "display_name": "Confluence",
    "download_url": "https://registry.doxus.io/download/com.doxus.confluence/1.0.0.wasm",
    "checksum_sha256": "abcdef1234567890abcdef...",
    "public_key_hex": "deadbeefdeadbeef...",
    "auth_type": "api_token",
    "guide_url": "https://registry.doxus.io/guides/com.doxus.confluence.md"
  }
]
```

| 필드명 | 타입 | 설명 |
| :--- | :--- | :--- |
| `plugin_id` | String | 고유 식별자 (Reverse Domain Name 추천) |
| `version` | String | SemVer 2.0 규격의 버전 번호 |
| `display_name` | String | UI에 표시될 이름 |
| `download_url` | String | `.wasm` 바이너리 직접 다운로드 주소 |
| `checksum_sha256` | String | 다운로드 파일의 SHA-256 해시값 (무결성 검증) |
| `public_key_hex` | String | 플러그인 서명 검증을 위한 Ed25519 공개키 (hex) |
| `auth_type` | String | `none` \| `api_token` \| `oauth` |
| `guide_url` | String | 설치 후 설정을 돕는 Markdown 문서 URL |

## 4. 보안 정책 (Security Policy)

### 4.1 전자 서명 (Code Signing)
모든 공식 또는 검증된 플러그인은 doxus의 `crates/core/marketplace/signing.rs` 로직에 따라 개인키로 서명되어야 합니다. 앱은 설치 시 레지스트리에 등록된 `public_key_hex`와 대조하여 서명을 검증합니다.

### 4.2 무결성 검증 (Integrity)
`checksum_sha256`은 다운로드된 파일이 전송 과정에서 변조되지 않았음을 보장합니다. 앱의 `MarketplaceInstaller`는 다운로드 직후 해시를 재생성하여 비교합니다.

### 4.3 신뢰 모델 (Trust Model)
*   **Official**: doxus 공식 팀이 관리하는 키로 서명된 플러그인.
*   **Verified**: 커뮤니티 개발자가 등록했으나, 레지스트리 운영자가 검토 후 승인한 플러그인.
*   **Unverified**: 별도 검증 없이 URL 기반으로 직접 설치된 플러그인 (사용자 주의 필요).

## 5. 배포 프로세스 (Release Workflow)

1.  **빌드**: 플러그인 소스를 WASM으로 컴파일.
2.  **서명**: `doxus-cli sign` (예정) 명령어를 사용하여 `.wasm` 파일에 서명 수행.
3.  **업로드**: `.wasm` 파일을 레지스트리의 `download/` 경로에 배치.
4.  **갱신**: `plugins.json`에 새로운 버전 항목을 추가하거나 기존 항목 업데이트.
5.  **캐시 무효화**: CDN 캐시를 갱신하여 클라이언트가 즉시 새 버전을 인지하도록 처리.

## 6. 클라이언트 연동 (Client Integration)

클라이언트는 기본적으로 `market_fetch_registry` 명령어를 호출할 때 지정된 레지스트리 루트 URL을 참조합니다.

*   **Default Registry**: `https://registry.doxus.io`
*   **Custom Registry**: 설정 페이지에서 사용자가 직접 다른 레지스트리 주소를 입력하여 사내 전용 레지스트리 운영 가능.
