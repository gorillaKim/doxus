---
title: 채팅 메시지 저장 한계 개선
category: improvements
priority: medium
phase: 6-8
created: 2026-04-04
---

# 채팅 메시지 저장 한계 개선

## 현황

`useChatStore` (`apps/desktop/src/stores/useChatStore.ts`)는 Zustand `persist` 미들웨어로 채팅 세션 전체를 `localStorage`에 저장한다.

- 세션 수: 최대 10개 (`sessions.slice(-9)`)
- 메시지 수: **무제한** (세션 내 메시지 상한 없음)

## 문제 가능성

| 상황 | 위험도 |
|------|--------|
| 일반적인 대화 (수십 회) | 낮음 |
| Claude가 대용량 문서 분석 | 중간 — 단일 응답이 수십KB |
| 도구 호출이 많은 세션 | 중간 — `toolInfo` JSON이 누적 |
| 장기 사용 (수백 회 대화) | 높음 — localStorage 5~10MB 초과 가능 |

## 개선 방안 (우선순위 순)

### 1. 세션당 메시지 수 상한 (단기)

```typescript
// sendMessage에서 메시지 추가 시
const MAX_MESSAGES_PER_SESSION = 200;
messages: [...sess.messages.slice(-MAX_MESSAGES_PER_SESSION + 1), msg]
```

가장 간단한 방어선. 오래된 메시지부터 자동 제거.

### 2. localStorage → Tauri 파일 저장 (중기)

Tauri의 `app-data` 디렉토리(`~/.doxus/chat/`)에 세션별 JSON 파일로 저장.

- 용량 제한 사실상 없음
- `@tauri-apps/plugin-fs` 또는 커스텀 Tauri 커맨드 필요
- `persist` 미들웨어 대신 커스텀 storage adapter 구현

```typescript
// 커스텀 storage adapter 예시
const tauriStorage = {
  getItem: async (key) => invoke('chat_storage_get', { key }),
  setItem: async (key, value) => invoke('chat_storage_set', { key, value }),
  removeItem: async (key) => invoke('chat_storage_remove', { key }),
};
```

### 3. 오래된 메시지 요약 (장기)

세션이 일정 크기를 넘으면 앞부분을 Claude로 요약하고 원본 삭제.
Phase 8 UI 고도화와 함께 검토.

## 관련 파일

- `apps/desktop/src/stores/useChatStore.ts` — persist partialize 설정
- `apps/desktop/src-tauri/src/commands/agent.rs` — 스트리밍 구현

## 추가 고려사항

- `localStorage` 쓰기 실패 시 에러 핸들링 없음 → 조용히 데이터 유실 가능
- Tauri 이전 시 기존 localStorage 데이터 마이그레이션 필요
