---
title: doxus 남은 작업 목록
updated: 2026-04-09
---

# doxus 남은 작업 목록

> Phase 0~7 완료, Phase 8 (Desktop UI 고도화) 진행 중 기준.
> ChatDrawer, 6개 페이지, Tauri IPC commands는 구현 완료.

---

## Phase 8: Desktop UI 고도화 (미완성 항목)

### 시스템 트레이

- [ ] `tauri-plugin-tray` 의존성 추가 (`apps/desktop/src-tauri/Cargo.toml`)
- [ ] 트레이 아이콘 등록 및 메뉴 구성 (최소화, 빠른 동기화, 종료)
- [ ] 윈도우 닫기 → 트레이로 최소화 (종료 아님) 동작 구현

### 자동 업데이트

- [ ] `tauri-plugin-updater` 의존성 추가
- [ ] GitHub Releases 기반 업데이트 엔드포인트 설정
- [ ] 앱 시작 시 업데이트 확인 + 24시간 주기 백그라운드 확인
- [ ] 업데이트 알림 UI (다운로드 → 재시작 시 적용)

### 첫 실행 온보딩 위자드

- [ ] `~/.doxus/config.toml` 없으면 온보딩 플로우 진입
- [ ] ① 환영 화면
- [ ] ② Obsidian 볼트 연결 (폴더 선택 → 백그라운드 인덱싱)
- [ ] ③ Claude Code / Gemini CLI 자동 감지 → 사서 에이전트 사용 가능 여부 표시
- [ ] ④ 완료 → 메인 화면 이동
- [ ] 빈 상태(Empty State) UX (볼트 없음, 검색 결과 없음, 플러그인 없음)

### 키보드 단축키

- [ ] `Cmd+K` → 검색 페이지 포커스
- [ ] `Cmd+J` → Chat Drawer 토글
- [ ] `Cmd+N` → 새 문서 생성 모달
- [ ] 전역 `keydown` 핸들러 등록 (`AppShell.tsx` 또는 전용 훅)

### 테마 (light / dark / system)

- [ ] Tailwind `darkMode: 'class'` 설정
- [ ] `useTheme` 훅 — `localStorage` 저장 + `prefers-color-scheme` 감지
- [ ] 설정 페이지에 테마 선택 UI 추가
- [ ] 전체 페이지 dark 클래스 적용

### 오프라인 동작

- [ ] `navigator.onLine` 감지 → 오프라인 배너 표시
- [ ] 외부 소스 동기화 버튼 비활성화 (오프라인 시)
- [ ] 로컬 기능(Obsidian 검색, 워크스페이스)은 오프라인에서도 정상 동작 보장

### 성능 최적화

- [ ] 검색 결과 목록 가상 스크롤 (결과 많을 때 렌더링 최적화)
- [ ] 검색 입력 debounce (현재 미적용 확인 필요)
- [ ] 페이지별 코드 분할 (lazy import)

### 접근성

- [ ] 주요 인터랙티브 요소에 `aria-label` 추가 (버튼, 입력창)
- [ ] 키보드 네비게이션 (Tab 순서, focus 링)
- [ ] 색상 대비 검토 (WCAG AA 기준)

---

## 기타 미완성 항목

### MCP 서버 등록 ✅ 완료 (2026-04-09)

~~`.mcp.json`이 비어 있어 Claude Code가 인식 불가~~
→ `/Users/madup/gorillaProject/doxus/.mcp.json`에 `doxus-mcp` 등록 완료

### CLI `not_implemented` 도구 확인

- [ ] MCP `main.rs`의 `not_implemented` 힌트가 붙은 도구들 실제 구현 여부 점검
  - `doxus_find_path`, `doxus_get_cluster`, `doxus_explain_search` 등

### 릴리즈 빌드 자동화

- [ ] `cargo build --release` CI 스텝 추가 (현재 debug 빌드만 CI)
- [ ] `doxus-mcp` 바이너리 경로를 `~/.local/bin` 또는 PATH로 설치하는 스크립트

---

## 참고

- 설계 기준 문서: `brain/Ideas/doxus/doxus - 구현 계획.md`
- Phase 8 체크리스트 원본: `brain/Ideas/doxus/doxus - 데스크톱 앱 아키텍처 설계.md`
