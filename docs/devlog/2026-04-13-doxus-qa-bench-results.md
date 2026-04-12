---
title: "doxus QA & 벤치마크 첫 실행 결과"
aliases:
  - doxus-qa-bench-결과
  - doxus QA 벤치마크
tags:
  - devlog
  - troubleshooting
  - qa
  - benchmark
  - doxus
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

# doxus QA & 벤치마크 첫 실행 결과

## 배경

doxus CLI와 MCP 서버의 기능 검증(QA)과 성능·품질 벤치마킹을 처음으로 체계적으로 실행했다.
doxus-qa 스킬(21개 체크리스트)과 doxus-bench 스킬(레이턴시·MRR@5·인덱스 건강도)을 사용했다.

## 변경 내용

### QA 결과 (19/21 PASS)

#### FAIL #9 — `project disable <nonexistent>` exit 0 반환

존재하지 않는 프로젝트명을 disable해도 exit 0 + "Disabled" 성공 메시지가 출력된다.

- **원인**: DB UPDATE 쿼리가 0 rows affected여도 에러 처리가 없음
- **권장 수정**: affected rows == 0이면 `ProjectNotFound` 에러 반환 후 exit 1

#### FAIL #10 — `search ""` (빈 쿼리) raw DB 에러 노출

```
Error: database error: fts5: syntax error near ""
```

위 메시지가 사용자에게 그대로 노출된다.

- **원인**: 빈 문자열 쿼리를 검증 없이 FTS5에 직접 전달
- **권장 수정**: 검색 전 빈/공백 쿼리 early-return (빈 결과 또는 friendly 메시지)

#### 특이사항: MCP `mcp_call` 함수 이스케이프 버그

쉘 함수에서 변수를 이중 따옴표로 래핑할 때 JSON 이스케이프 문제 발생.
직접 `printf` + 리터럴 JSON으로 우회하면 정상 동작. MCP 서버 자체 버그는 아니고 테스트 헬퍼 구현 이슈.

### 벤치마크 결과

#### 검색 레이턴시 (cold-start 포함, bash time)

| 측정 대상 | 평균 레이턴시 |
|-----------|-------------|
| CLI | 21~29ms (쿼리별 차이 있음) |
| MCP | ~24ms |
| CLI vs MCP 오버헤드 | 사실상 없음 (cold-start 포함 시 동등) |

#### 검색 품질 MRR@5 = 0.70

FTS-only 모드에서 0.5~0.7이 일반적 범위이므로 정상 수준.

미스 케이스:
- `프론트엔드` — 한국어 복합어 토크나이징 한계
- `ai.txt` — 특수 토큰 처리 미흡

#### 인덱스 건강도 이슈

| 이슈 | 내용 |
|------|------|
| empty_content 1건 | `title='공유문서'` (source_doc_id=4667998225), 내용이 빈 문서가 인덱스에 포함됨 |
| indexing_status 전체 'pending' | 113개 청크가 실제 존재하고 검색도 정상이지만, 인덱싱 후 status가 'indexed'로 업데이트되지 않음 → 상태 기반 필터링·재인덱싱 로직 오동작 가능성 |
| avg_chunks_per_doc = 1.0 | 모든 문서가 단일 청크 — 대용량 문서에서 chunking 전략 검토 필요 |

### 영향 범위

- `crates/core/src/db/` — project disable 에러 처리
- `crates/core/src/search.rs` — 빈 쿼리 early-return
- `crates/core/src/index_engine.rs` — `indexing_status` 업데이트 누락 수정
- `crates/core/src/index_engine.rs` — chunking 전략 재검토

## 결과

- 단순 키워드 조회 에이전트로는 충분히 실용적 (24ms, crash 없음)
- ONNX 비활성 상태에서는 의미 검색·교차 언어 검색에 한계 존재
- 빈 쿼리 crash는 에이전트 입장에서 실제 문제가 될 수 있음 (방어 코드 필요)
- `indexing_status` 미갱신은 재인덱싱 판단 로직에 잠재적 오동작 위험

## 교훈

1. **에러 경계 명확화**: DB 레이어 에러는 사용자에게 노출하지 않고 도메인 에러로 변환해야 한다.
2. **입력 검증 우선**: FTS5 같은 엔진에 넣기 전에 애플리케이션 레이어에서 먼저 검증한다.
3. **상태 기록 신뢰성**: 실제 인덱싱 완료 후 status 필드를 반드시 업데이트해야 한다. 데이터가 있어도 status가 틀리면 운영 로직이 깨진다.
4. **테스트 헬퍼 이스케이프**: 쉘 기반 MCP 테스트 헬퍼에서 JSON을 변수로 조립할 때 printf + 리터럴 패턴이 안전하다.

## 관련 문서

- [[2026-04-12-doxus-ux-cache-toast-emoji]]
- [[2026-04-13-confluence-search-score-chunking]]
