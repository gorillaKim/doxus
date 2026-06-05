# LTM(장기 메모리) 요약 및 토큰 최적화 개선 계획서

> **v3** — 사용자 피드백(목차 요약 결합) 반영 (2026-06-05)

본 문서는 doxus의 에이전트 장기 기억(LTM) 검색 시 발생하는 토큰 낭비를 원천 차단하고,
에이전트 피드백과 동적 관계를 반영하는 고성능 검색 엔진으로 도약하기 위한 구체적인 개선 계획을 다룹니다.

---

## 0. 배경: 현재 아키텍처 제약

- **`documents.content` 컬럼은 V23에서 이미 삭제됨** — 현재 `documents` 테이블은
  메타데이터 + `content_hash`만 보관하는 경량 레코드이다.
- 실제 본문은 `DocumentService`를 통해 로컬 파일 / 캐시 / 원격 플러그인 순으로 on-demand 로드된다.
- `summary` 컬럼은 content와 달리 **파생 데이터(크기 고정, ≤ 500자)** 이므로 DB에 상주해도
  content 삭제와 동일한 비대화 문제가 발생하지 않는다.
- 기존 그래프 테이블(`document_links`, V5)은 Markdown wikilink 기반 정적 관계이며,
  에이전트 행동 기반 동적 관계와는 성격이 다르다.

---

## 1. 아키텍처 개요 (Multi-Level Retrieval)

에이전트가 문서를 탐색할 때 무조건 전체 문서를 로드하지 않고,
점진적으로 세부 내용에 접근하는 **계층형 다단계 검색 모델**을 구축한다.

```
+-------------------------------------------------------------+
| 1단계: Abstract Layer (요약 정보)                            |
| - doxus_search → 사전 저장된 summary 반환 (DB 조회만)        |
| - doxus_get_document(view="summary") → 온디맨드 Lead-3 추출  |
+------------------------------+------------------------------+
                               | (유관 문서 식별)
                               v
+-------------------------------------------------------------+
| 2단계: Outline Layer (구조 분석)                            |
| - doxus_get_toc를 통해 헤더 목록(TOC)을 로드하여 세부 위치 파악 |
+------------------------------+------------------------------+
                               | (필요 단락 식별)
                               v
+-------------------------------------------------------------+
| 3단계: Content Layer (상세 단락)                            |
| - doxus_get_section을 호출하여 특정 헤더 아래의 핵심 단락만 로드|
+-------------------------------------------------------------+
```

### 요약 전달 전략: 혼합 모델

| 도구 | 전략 | 이유 |
|------|------|------|
| `doxus_search` | **사전 저장** (DB `summary` 컬럼) | 검색 결과 N건마다 전체 본문 로드 시 I/O·네트워크 비용 과다 |
| `doxus_get_document` | **온디맨드** (본문 로드 후 즉석 추출) | `DocumentService`가 어차피 전체 본문을 로드하므로 추가 비용 제로 |

---

## 2. 세부 구현 계획

### Phase 1 (P0): Summary 인프라 — 최소 변경·최대 효과

#### 2-1) 데이터베이스 스키마 확장

> 마이그레이션 번호는 구현 시점에 확정한다. (현재 최신: V40)

```sql
-- Vnext__ltm_summary.sql
-- 1. documents 테이블에 summary 컬럼 추가 (파생 데이터, ≤ 500자)
ALTER TABLE documents ADD COLUMN summary TEXT;
```

#### 2-2) 하이브리드 요약 엔진 (목차 개요 + Lead-3 결합)

TextRank 대신 **Heading(목차) 정보와 Lead-3(본문 첫 3문장)을 결합한 하이브리드 요약**을 기본 전략으로 채택한다.

**이유**: doxus가 인덱싱하는 문서는 대부분 Markdown 기술 문서로, 앞부분의 첫 3문장(Lead-3)만으로는 문서의 전체 구조나 핵심 주제를 명확히 유추하기 어렵다. 따라서 문서 내부의 주요 헤더(#, ##) 목록을 간략히 정리한 **목차 개요**를 추출하여 요약문 머리에 덧붙임으로써, 에이전트가 본문을 통째로 읽지 않고도 문서 전체의 구조와 본문의 서론을 완벽히 유추할 수 있도록 돕는다.

```
[문서 본문]
    |
    v
[전처리] ── frontmatter 내 description 추출 시도
    |
    +--- [존재함] ──> [description 반환] ──> [documents.summary 저장]
    |
    +--- [존재안함] ──> frontmatter/코드 블록/HTML 태그 제거
                           |
                           +------------------------------+------------------------------+
                           | (목차 추출)                                                 | (본문 추출)
                           v                                                             v
                      [헤더(#, ##) 목록 추출 및 목차 개요 구성]                      [첫 번째 # 이후 첫 3문장 추출]
                      (제목 없는 경우 목차 생략)
                           |                                                             |
                           +------------------------------+------------------------------+
                                                          |
                                                          v
                                    [목차 개요 + Lead-3 결합 및 500자 자름 ("…")]
                                                          |
                                                          v
                                               [documents.summary 저장]
```

- **구현 위치**: `crates/core/src/summarizer.rs` (신규 모듈)
- **호출 시점 (인덱싱)**: `IndexingService::index_single_document()` 내에서
  `engine.index_document_async_with_meta()` 호출 직후, 요약 생성 결과를
  `UPDATE documents SET summary = ?` 로 저장.
- **호출 시점 (온디맨드)**: `DocumentService::fetch_full_content()` 반환값에서
  MCP 서빙 레이어가 `view="summary"` 요청 시 동일 함수로 즉석 추출.

#### 2-3) MCP 인터페이스 변경

**`doxus_search` 변경:**
- `include_summary: bool` 인자 추가 (기본값: `true`)
- `true`인 경우 DB에 사전 저장된 `summary` 값을 snippet 대신 반환
- summary가 `NULL`인 문서는 기존 snippet으로 **자동 fallback**

**`doxus_get_document` 변경:**
- `view` 파라미터 추가: `"full"` (기본) | `"summary"` | `"outline"`
- `"summary"`: `DocumentService`로 전체 본문 로드 후 Lead-3 온디맨드 추출하여 반환
- `"outline"`: `doxus_get_toc`와 동일한 결과 반환 (편의 alias)

---

### Phase 2 (P1–P2): 피드백 시스템

#### 2-4) 피드백 스키마

```sql
-- Vnext__ltm_feedback.sql
CREATE TABLE IF NOT EXISTS document_feedbacks (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    agent_id    TEXT NOT NULL,
    score       REAL NOT NULL CHECK(score >= -1.0 AND score <= 1.0),
    session_id  TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_feedbacks_doc ON document_feedbacks(document_id);
CREATE INDEX IF NOT EXISTS idx_feedbacks_session ON document_feedbacks(session_id);
```

#### 2-5) 피드백 랭킹 보정 알고리즘

하이브리드 검색 시 RRF 점수를 기반으로 랭킹을 매긴 후, 축적된 에이전트 피드백으로 최종 순위를 보정한다.

$$\text{Final Score}(d) = \text{RRF Score}(d) \times \left( 1.0 + \frac{\sum \text{score}_{d}}{N_{d} + K} \times \text{damping\_factor} \right)$$

- $N_{d}$: 해당 문서에 누적된 피드백의 개수
- $K$: 평활화 상수 (기본값: 5)
- $\text{damping\_factor}$: 평판 반영 강도 (기본값: 0.5)

**보정 적용 위치**: `SearchEngine::search_async()` 내 `spawn_blocking` 블록에서,
`paged_hits` 구성 직후 `document_id` 기준으로 피드백 평균 점수를 JOIN하여
score를 보정하고 재정렬한다.

> 현재 `rrf_merge`는 **chunk 단위**로 점수를 계산하지만, 피드백은 **document 단위**이다.
> 따라서 보정은 chunk→document 그룹핑 이후인 `paged_hits` 단계에서 수행해야 한다.

#### 2-6) 피드백 수집 가이드라인

에이전트가 `doxus_record_feedback`을 호출하는 **권장 시점**:

| 시점 | score 권고값 | 설명 |
|------|-------------|------|
| 문서를 읽고 작업에 직접 활용한 경우 | `+0.5` ~ `+1.0` | 코드 변경·의사결정에 기여 |
| 문서를 읽었으나 관련성이 낮은 경우 | `-0.3` ~ `-0.5` | 읽었지만 도움 안됨 |
| 문서 내용이 outdated/오류인 경우 | `-0.8` ~ `-1.0` | 잘못된 정보로 시간 낭비 |

MCP agent 시스템 프롬프트에 이 가이드라인을 포함하여, 에이전트가 문서를 읽은 후
자연스럽게 피드백을 남기도록 유도한다.

#### 2-7) MCP 피드백 도구

- **도구명**: `doxus_record_feedback`
- **매개변수**:
  - `project` (string, 필수): 프로젝트 이름
  - `id` (string, 필수): 대상 문서 ID (Source ID 또는 DB ID)
  - `score` (number, 필수): 피드백 점수 (-1.0 ~ 1.0)
  - `session_id` (string, 선택): 태스크 세션 식별자

---

### Phase 3 (P3): LLM 생성 요약 (선택적 보강)

외부 LLM API가 구성되어 있을 때, Lead-3 추출 요약을 고품질 생성 요약으로 교체한다.

**2-pass 인덱싱 전략**:
1. **1차 (동기)**: 기존 인덱싱 + Lead-3 추출 요약 → `documents.summary` 저장
2. **2차 (비동기 백그라운드)**: LLM API로 생성 요약 요청 → 응답 도착 시 summary 업데이트

```
[인덱싱 완료]
    |
    v
[Lead-3 summary 즉시 저장] ← 에이전트는 이 시점부터 summary 사용 가능
    |
    v (background job queue)
[LLM API 호출] → [Rate Limit 대기] → [응답]
    |
    v
[documents.summary 업데이트 (LLM 버전으로 교체)]
```

- **지원 API**: OpenAI, Anthropic, 로컬 Ollama
- **설정 위치**: `system_config` 테이블 (`llm_provider`, `llm_api_key`, `llm_model`)
- **실패 처리**: LLM 호출 실패 시 Lead-3 summary를 유지 (graceful degradation)
- **Rate Limit**: 초당 요청 수 제한 (기본: 5 req/s), 대량 인덱싱 시 배치 큐 사용

---

### Phase 4 (P4): 동적 연관 관계 (공동 참조 그래프)

#### 2-8) 스키마 설계

> 기존 `document_links` (V5)는 Markdown wikilink 기반 **정적** 관계.
> `document_co_refs`는 에이전트 세션 내 **행동 기반** 동적 관계로 성격이 다르다.

```sql
-- Vnext__ltm_co_refs.sql
CREATE TABLE IF NOT EXISTS document_co_refs (
    doc_a_id            INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    doc_b_id            INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    co_occurrence_count INTEGER NOT NULL DEFAULT 1,
    last_accessed       INTEGER NOT NULL,
    PRIMARY KEY (doc_a_id, doc_b_id),
    CHECK (doc_a_id < doc_b_id)  -- 정규화: 항상 작은 ID가 앞
);

CREATE INDEX IF NOT EXISTS idx_co_refs_b ON document_co_refs(doc_b_id);
```

**기존 계획 대비 변경점**:
- 테이블명을 `document_co_links` → `document_co_refs`로 변경 (기존 graph의 `document_links`와 혼동 방지)
- `CHECK(doc_a_id < doc_b_id)` 제약 추가로 `(A,B)`와 `(B,A)` 중복 방지
- `doc_b_id` 인덱스 추가 (양방향 조회 성능 보장)

**정리(Pruning) 정책**:
- `co_occurrence_count < 3`이고 `last_accessed`가 30일 이상 지난 레코드는 주기적으로 삭제
- 스케줄러(`V34__scheduler`)의 maintenance job으로 등록

**갱신 트리거**: 에이전트가 하나의 세션(`session_id`)에서 2개 이상의 문서를 참조(search 또는
get_document)하면, 세션 종료 시 모든 참조된 문서 쌍에 대해 `co_occurrence_count`를 +1 하고
`last_accessed`를 갱신한다. 실시간이 아닌 **세션 종료 시 배치 처리**.

---

## 3. 기대 효과 (토큰 시뮬레이션)

단일 문서 크기 평균 **5,000 토큰**, 검색 결과 5개 반환 상황을 가정한다.

| 검색 단계 및 조회 방식 | 에이전트 입력 토큰 소모량 (추정) | 비고 |
|---|---|---|
| **기존 방식** (전체 문서 2개 로드) | **10,000+ 토큰** | 조금만 훑어도 컨텍스트 윈도우 한계 도달 |
| **개선 방식 (1단계: Summary 검색)** | **~500 토큰** | 검색 결과 5개의 3문장 요약문만 확인 |
| **개선 방식 (2단계: TOC 구조 로드)** | **~100 토큰** | 필요한 1개 문서의 목차 정보만 확인 |
| **개선 방식 (3단계: 단락 정밀 로드)**| **~800 토큰** | 1개 문서 내 특정 1개 단락만 타겟 로드 |
| **총합 (개선 방식 전체 워크플로우)** | **~1,400 토큰** | **기존 대비 약 85% 이상의 토큰 절감 효과** |

---

## 4. 검증 계획

### 단위 테스트
- `summarizer::lead3_extract()` — Markdown 전처리 + 문장 추출 정확도
  - 코드 블록 포함 문서에서 코드가 요약에 포함되지 않는지 검증
  - frontmatter 제거 검증
  - 한국어/영어 혼용 문서 문장 분리 검증
- `rrf_merge` + 피드백 보정 — 피드백 반영 후 순위 변동 검증

### 통합 테스트
- 인덱싱 후 `documents.summary`가 올바르게 저장되는지 확인
- `doxus_search(include_summary=true)` 반환값에 summary 포함 확인
- `doxus_get_document(view="summary")` 온디맨드 추출 결과 확인

### 성능 목표
- 인덱싱 시간 증가: Lead-3 추출로 인한 추가 시간 **< 5%**
- 검색 응답 지연: 피드백 보정 추가로 인한 지연 **< 10ms**
- summary 저장 공간: 문서당 평균 **≤ 500 bytes** 추가

---

## 5. 구현 로드맵

| Phase | 우선순위 | 내용 | 예상 공수 |
|-------|---------|------|----------|
| **Phase 1** | P0 | `summary` 컬럼 + Lead-3 추출 + MCP `include_summary` / `view` 파라미터 | 2-3일 |
| **Phase 2** | P1–P2 | `document_feedbacks` + 랭킹 보정 + `doxus_record_feedback` MCP 도구 | 2-3일 |
| **Phase 3** | P3 | LLM 생성 요약 (2-pass 파이프라인, background job) | 3-5일 |
| **Phase 4** | P4 | `document_co_refs` 동적 연관 관계 + 세션 추적 + pruning | 3-4일 |
