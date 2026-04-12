# GitHub 플러그인 가이드

GitHub Issues, Wiki, Discussions를 doxus로 가져와 검색할 수 있습니다.

## 사전 요구사항

- GitHub 계정
- Personal Access Token (PAT) 발급

## Personal Access Token 발급

1. GitHub → **Settings** → **Developer settings** → **Personal access tokens** → **Tokens (classic)**
2. **Generate new token** 클릭
3. 필요한 권한 선택:
   - `repo` — 비공개 저장소 접근 시 필요
   - `public_repo` — 공개 저장소만 사용 시
   - `read:discussion` — Discussions 동기화 시
4. 토큰 복사 (한 번만 표시됨)

## 인증 설정

1. 마켓에서 GitHub 플러그인 설치
2. **설정** 버튼 클릭
3. Personal Access Token 입력 후 저장

## 프로젝트 추가

인증 완료 후 프로젝트 탭에서:

```
소스: github
저장소: owner/repo 형식 (예: octocat/Hello-World)
```

## 지원 콘텐츠

| 타입 | 지원 여부 |
|------|----------|
| Issues | ✅ |
| Issue 댓글 | ✅ |
| Wiki 페이지 | ✅ |
| Discussions | ✅ |
| Pull Requests | ❌ |
| 코드 파일 | ❌ |

## 동기화 주기

기본값: 30분마다 변경사항 자동 동기화 (생성/수정/닫힘 이벤트)

## 문제 해결

**"AuthRequired" 오류**: 설정에서 PAT를 다시 입력하세요.

**비공개 저장소 접근 불가**: PAT에 `repo` 권한이 있는지 확인하세요.

**"RateLimited" 오류**: GitHub API 시간당 5,000 요청 한도 초과. 잠시 후 자동 재시도됩니다.

**Discussions가 보이지 않음**: 저장소에서 Discussions 기능이 활성화되어 있는지 확인하세요.
