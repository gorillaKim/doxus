# Frontend 규칙 (Tauri v2 + React 19)

## 기술 스택

| 영역 | 기술 | 버전 |
|------|------|------|
| 데스크톱 프레임워크 | Tauri | v2 |
| UI 프레임워크 | React | 19 |
| 상태 관리 | Zustand | latest |
| 라우팅 | React Router | 7+ |
| 스타일 | Tailwind CSS | 4+ |
| 빌드 | Vite | 6+ |
| 타입 | TypeScript | strict mode |

## 디렉토리 구조

```
apps/desktop/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── state.rs        # AppState (DB, SearchEngine, PluginManager, ...)
│   │   └── commands/       # IPC 커맨드 (search, project, document, workspace, market, agent)
│   └── Cargo.toml
└── src/
    ├── components/
    │   └── layout/
    │       ├── AppShell.tsx
    │       ├── Sidebar.tsx
    │       ├── TopBar.tsx
    │       └── ChatDrawer.tsx  # 우측 고정 슬라이드
    ├── pages/              # dashboard, search, workspace, project, market, settings
    ├── stores/             # Zustand 스토어
    └── types/              # TypeScript 타입 (packages/types에서 import)
```

## Tauri IPC 커맨드 규칙

- 모든 커맨드는 `AppState`를 `tauri::State`로 받음
- 반환 타입은 `Result<T, String>` (에러는 String으로 직렬화)
- 장기 작업(인덱싱, 동기화)은 Tauri event로 진행률 전송

```rust
// 올바른 예
#[tauri::command]
pub async fn search_documents(
    state: tauri::State<'_, AppState>,
    query: SearchQuery,
) -> Result<SearchResults, String> {
    state.search_engine
        .search(&query)
        .await
        .map_err(|e| e.to_string())
}

// 이벤트로 진행률 전송
app_handle.emit("index:progress", IndexProgress { percent: 42, current_doc: "..." })?;
```

## Zustand 스토어 분리

스토어는 기능 단위로 분리하고 교차 의존 최소화:

| 스토어 | 역할 |
|--------|------|
| `useSearchStore` | 쿼리, 결과, 필터, 하이라이트 |
| `useWorkspaceStore` | 현재 워크스페이스, 템플릿 |
| `useProjectStore` | 프로젝트 목록, Active/Disabled 상태 |
| `usePluginStore` | 설치된 플러그인, 설정 |
| `useChatStore` | 에이전트 대화 히스토리, ChatDrawer 열림 상태 |
| `useSettingsStore` | 앱 전역 설정 (임베딩 모델, 언어 등) |

## ChatDrawer 규칙

- 위치: 우측 슬라이드 패널, **항상 오버레이로 표시** (레이아웃을 밀지 않음)
- 너비: 고정 (384px, `w-96`)
- 열림/닫힘: `useChatStore.isOpen`으로 제어
- Tauri 에이전트 커맨드와 연결

## React 패턴

- 서버 컴포넌트 사용 안 함 (Tauri는 SPA)
- 데이터 페칭은 Zustand action + `invoke()` 조합
- 무거운 목록은 가상화 (`@tanstack/react-virtual`)
- 컴포넌트는 기능 단위 폴더로 colocate (`Button/Button.tsx`, `Button/index.ts`)

```tsx
// Tauri 커맨드 호출 패턴
import { invoke } from '@tauri-apps/api/core';

const results = await invoke<SearchResults>('search_documents', { query });
```

## Tailwind 규칙

- Tailwind CSS 4+ 사용 (`@import "tailwindcss"`)
- 인라인 `style={}` 대신 Tailwind 클래스 우선
- 커스텀 색상/토큰은 `tailwind.config.ts`에 정의
- Dark mode: `class` 전략 (`dark:` prefix)
