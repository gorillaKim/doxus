# Confluence 플러그인 가이드

Confluence Cloud의 문서를 doxus로 가져와 검색할 수 있습니다.

## 사전 요구사항

- Atlassian 계정
- Atlassian Personal API Token

## Personal API Token 발급

1. [Atlassian API 토큰 관리 페이지](https://id.atlassian.com/manage-profile/security/api-tokens) 접속
2. **Create API token** 클릭
3. 토큰 이름 입력 (예: `doxus`) 후 **Create**
4. 생성된 토큰 복사 (한 번만 표시됨)

## 인증 설정

1. 마켓에서 Confluence 플러그인 설치
2. **설정** 버튼 클릭
3. **Atlassian 계정 이메일** 입력 (예: `you@company.com`)
4. **Personal API Token** 입력 (위에서 발급한 토큰)
5. **저장** 클릭

## 프로젝트 추가

인증 완료 후 **두 가지 연동 방식** 중 선택:

### 방식 1: 스페이스 전체 연동

```
소스: confluence
Base URL: https://yourcompany.atlassian.net
Space Key: ENG  (또는 개인 스페이스: ~222368988)
```

### 방식 2: 특정 페이지 하위 트리만 연동

```
소스: confluence
Base URL: https://yourcompany.atlassian.net
Page ID: 123456789  ← ancestor_id
```

> **페이지 ID 찾는 법**: Confluence에서 해당 페이지 열기 → URL에서 숫자 ID 확인
> 예) `https://madup.atlassian.net/wiki/spaces/~222368988/pages/123456789` → ID는 `123456789`

> **개인 스페이스 키 찾는 법**: 개인 스페이스 URL의 `~` 뒤 숫자
> 예) `https://madup.atlassian.net/wiki/spaces/~222368988` → Space Key는 `~222368988`

`space_key` 또는 `ancestor_id` 중 하나는 반드시 입력해야 합니다.

## 지원 콘텐츠

| 타입 | 지원 여부 |
|------|----------|
| 페이지 | ✅ |
| 블로그 포스트 | ✅ |
| 첨부파일 | ❌ (텍스트만) |
| 댓글 | ❌ |

## 문제 해결

**"AuthRequired" 오류**: 설정에서 이메일과 Personal API Token을 다시 확인하세요.

**"RateLimited" 오류**: Atlassian API 요청 한도 초과. 잠시 후 자동으로 재시도합니다.

**Space를 찾을 수 없음**: Space Key가 올바른지, 해당 Space 접근 권한이 있는지 확인하세요.

**0개 문서 인덱싱**: Base URL에 `/wiki`를 포함하지 마세요. `https://yourcompany.atlassian.net` 형식이 맞습니다.
