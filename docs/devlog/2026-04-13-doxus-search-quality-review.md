---
title: "doxus 검색 품질 코드 리뷰 반영 및 벤치마크 스킬 확장"
date: 2026-04-13
updated: 2026-04-13
tags: [devlog, doxus, search, benchmark, code-review]
aliases: [doxus-search-review-2026-04-13, 검색품질-코드리뷰]
status: completed
---

<!-- docsmith: auto-generated 2026-04-13 -->

## 배경

이전 세션에서 검색 품질 개선 Fix 1~5 구현을 완료했다. 이번 세션에서는 `code-reviewer` 에이전트를 통해 PR 리뷰를 수행하고, 발견된 이슈 8개를 수정했다. 또한 doxus-bench 스킬을 grep / obsidian-nexus / doxus CLI 3자 비교 체계로 확장했다.

참조 플랜: `.omc/plans/search-quality-fix.md`

---

## 1. 코드 리뷰

`oh-my-claudecode:code-reviewer` 에이전트로 다음 파일들을 대상으로 리뷰를 수행했다.

- `crates/core/src/search.rs`
- `crates/cli/src/main.rs`
- `crates/core/src/observability.rs`

### 발견된 이슈 8개

| 심각도 | 이슈 |
|--------|------|
| CRITICAL | 빈 쿼리 → `WHERE chunks_fts MATCH ?1` 런타임 에러 (FTS5 syntax error) |
| HIGH | `sanitize_fts_token`의 `"` → `""` 이스케이프가 phrase wrapping과 충돌 → invalid FTS5 |
| HIGH | prefix fallback SQL 30줄 중복 (DRY 위반) |
| MEDIUM | vector score=0.0 시 IEEE 754 inf 의존 (명시적 가드 필요) |
| MEDIUM | title boost 매직 넘버 `0.005` / `0.002` 미명명 |
| MEDIUM | fallback 비교 시 `build_fts_query(query)` 재호출 낭비 |
| LOW | `VECTOR_MAX_DISTANCE` 이름이 "코사인 유사도"로 오해 소지 (실제는 L2 거리) |
| LOW | `insert_test_project` 무의미 래퍼 함수 |

---

## 2. 코드 수정 — search.rs

### CRITICAL: 빈 쿼리 가드

`fts_search_sync` 및 `search_simple` 양쪽에 빈 쿼리 조기 반환을 추가했다.

```rust
let fts_query = build_fts_query(&query.text);
if fts_query.is_empty() {
    return Ok(vec![]);
}
```

FTS5는 빈 문자열을 MATCH 인자로 받으면 syntax error를 반환한다. 입력 검증을 쿼리 빌드 직후에 수행해 런타임 에러를 방지한다.

### HIGH: sanitize_fts_token 이스케이프 전략 변경

```rust
// AS-IS: .replace('"', "\"\"")
// TO-BE: .replace('"', "")
fn sanitize_fts_token(token: &str) -> String {
    token
        .replace('"', "")
        .replace(['(', ')', '^', '~'], "")
        .replace('-', " ")
}
```

phrase 내 `""` 이스케이프는 FTS5 스펙상 맞지만, phrase wrapping(`"token"`)과 결합하면 `"token""inner"` 형태가 되어 파서가 의도치 않게 해석한다. 사용자 검색 쿼리에서 리터럴 따옴표를 검색하는 경우는 없으므로 단순 제거가 더 안전하다.

### HIGH: fallback 비교 캐시

```rust
let fts_query_str = build_fts_query(query);
// ...
// fallback 분기에서 build_fts_query(query) 재호출 제거
if fallback_q != fts_query_str {
    // ...
}
```

순수 함수라도 비교 목적으로 재호출하는 것은 DRY 위반이자 불필요한 연산이다.

### MEDIUM: 상수 명명 + vector 가드

```rust
const VECTOR_MAX_L2_DISTANCE: f64 = 1.0;  // VECTOR_MAX_DISTANCE에서 rename
const TITLE_EXACT_BOOST: f64 = 0.005;     // RRF 스케일 기준 1~2 순위 상승
const TITLE_PARTIAL_BOOST: f64 = 0.002;   // 미세 보정

// inf 의존 제거 → 명시적 가드
hits.retain(|h| {
    if h.score <= 0.0 { return false; }
    let distance = (1.0 / h.score) - RRF_K as f64;
    distance <= VECTOR_MAX_L2_DISTANCE
});
```

`1/0.0 = inf`가 필터를 통과하는 동작에 암묵적으로 의존하는 것은 가독성과 안전성 모두에서 좋지 않다. 명시적 가드로 의도를 코드에 드러냈다.

### LOW: 테스트 정리

- `VECTOR_MAX_DISTANCE` → `VECTOR_MAX_L2_DISTANCE` 전면 rename
- `insert_test_project` 래퍼 제거, 테스트에서 `insert_project` 직접 호출

**결과:** `cargo test` 245개 전체 통과.

---

## 3. 커밋 내역

| 해시 | 내용 |
|------|------|
| d99b405 | fix(core,cli): 검색 품질 개선 — FTS 쿼리 빌더, 벡터 임계값, title boost (리뷰 반영) |
| 694a7ba | feat(core): ONNX 모델 경로 통합, 인증/시크릿 개선, 동기화 러너 확장 |
| 9aa7362 | feat(mcp): MCP 서버 도구 확장 및 응답 개선 |
| 6240bd1 | feat(desktop): 마켓, 워크스페이스, 검색 UI 개선 |
| 6c665b2 | docs: 검색 파이프라인 아키텍처, 데브로그, 구현 현황 업데이트 |

---

## 4. doxus-bench 스킬 확장

`.claude/skills/doxus-bench/SKILL.md`에 grep / obsidian-nexus / doxus CLI 3자 비교 체계를 추가했다. (커밋: ca49895)

### 추가된 Phase

| Phase | 내용 |
|-------|------|
| 2.5 (신규) | `grep -r` / `nexus_search` MCP / `doxus search` CLI 레이턴시 3자 비교 |
| 4 확장 | MRR@5 단독 → MRR@5 + Recall@5 + Precision@5 3자 비교 |
| 4.5 (신규) | 무관 쿼리 3개로 false positive (노이즈) 측정 |
| 6 확장 | Comparative 섹션 + Verdict 요약 추가 |

**공통 벤치마크 대상:** Brain vault (`~/gorillaProject/brain`) — doxus / nexus 양쪽 인덱싱됨

**폴백 규칙:**
- nexus MCP 미실행 시 → 해당 열 N/A, 나머지 정상 실행
- Brain vault 없음 → Comparative 섹션 SKIP

---

## 학습 사항

**FTS5 phrase escaping 함정**
`"token""inner"` 형태가 FTS5에서 의도치 않게 파싱된다. phrase 내 `""` 이스케이프는 이론적으로 맞지만, 실제 사용자 입력에서는 따옴표를 단순 제거하는 것이 더 안전하다.

**IEEE 754 inf 의존은 피하라**
`1/0.0 = inf`가 필터를 통과하는 동작에 의존하는 것보다 명시적 가드가 가독성과 안전성 모두에서 우월하다. 수학적으로 성립하더라도 의도가 코드에 드러나지 않으면 다음 리뷰어가 버그로 오해할 수 있다.

**캐시 vs 재계산**
`build_fts_query`처럼 순수 함수라도 비교를 위해 재호출하는 것은 DRY 위반이자 성능 낭비다. 결과를 변수에 캐시하고 재사용하는 습관이 중요하다.

---

## 현재 상태

| 지표 | 수치 |
|------|------|
| MRR@5 | 0.387 → 0.487 (목표 0.5 미달) |
| 테스트 | 245개 전체 통과 |
| doxus-bench | 3자 비교 준비 완료 (실제 실행은 다음 세션) |

"프론트엔드" 키워드 검색에서 rank >5 문제가 남아 있다. 다음 세션에서 3자 비교 벤치마크를 실행하고 추가 개선 방향을 결정할 예정이다.

---

## 관련 문서

- [[2026-04-13-confluence-search-score-chunking]]
- [[2026-04-13-doxus-onnx-model-path-unification]]
- [[2026-04-13-doxus-qa-bench-results]]
- [[search-pipeline]]
