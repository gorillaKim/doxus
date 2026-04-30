---
title: doxus 검색 파이프라인 아키텍처
updated: 2026-04-30
tags:
  - search
  - architecture
  - fts5
  - vector-search
  - rrf
  - chunking
  - embedding
---

# doxus 검색 파이프라인 아키텍처

> **대상 독자:** doxus 기여자, 검색 동작을 이해하려는 개발자  
> **관련 코드:** `crates/core/src/search/mod.rs`, `crates/core/src/search/highlighter.rs`, `crates/core/src/chunker.rs`, `crates/core/src/embedding.rs`

---

## 전체 흐름 요약

```
문서 (Obsidian / Confluence / GitHub)
        │
        ▼
  [ 인덱싱 파이프라인 ]
  1. content → split_chunks() → Vec<Chunk>
  2. 첫 번째 청크 → ONNX embed() → Vec<f32> (384차원)
  3. chunks 테이블 INSERT (+ FTS5 트리거 자동 동기화)
  4. chunk_embeddings 테이블 INSERT (chunk_index=0만)
        │
        ▼
  [ 검색 파이프라인 ]
  쿼리 → FTS5 검색 (BM25)  ─┐
       → 벡터 검색 (KNN)   ─┤→ RRF 병합 → 최종 랭킹
```

---

## 1. 문서 청킹 (`crates/core/src/chunker.rs`)

### 왜 청킹이 필요한가?

BM25 알고리즘은 문서 내 용어 빈도를 문서 길이로 정규화한다 (`dl/avgdl`).
Confluence 문서(1,000~5,000 단어)는 Obsidian 노트(300~500 단어)에 비해 길이 패널티를 받아
동일한 키워드가 포함되어도 점수가 낮게 나오는 문제가 있었다.

청킹은 이 문제를 해결한다: 각 청크가 비슷한 길이를 가지므로 BM25 정규화가 공평하게 동작한다.

### 알고리즘

```
split_chunks(text, max_chars=1500, overlap_chars=200)

1. 빈 줄 경계("\n\n")로 단락 분리
2. 단락을 순서대로 합산하다가 max_chars 초과 직전에 청크 확정 (flush)
3. flush된 청크의 마지막 overlap_chars를 tail로 저장
4. 다음 청크 시작: "{tail}\n\n{새 단락}"
5. 끝까지 반복 후 남은 내용 flush
```

| 파라미터 | 기본값 | 역할 |
|---------|--------|------|
| `max_chars` | 1,500 | 청크 최대 길이 (characters) |
| `overlap_chars` | 200 | 인접 청크 간 겹치는 문자 수 |

**특이 사항:**
- 단락 하나가 max_chars를 초과해도 그 자체로 하나의 청크가 됨 (강제 분리 없음)
- UTF-8 char boundary를 고려하여 안전하게 슬라이싱
- 빈 청크는 반환하지 않음

### DB 저장 구조

```
documents 1 ──── N chunks ──── 1 chunk_embeddings
                              (chunk_index = 0 만)
```

임베딩은 청크 0번(문서 첫 부분)에만 저장된다. 나머지 청크는 FTS5 검색에만 참여한다.

---

## 2. 임베딩 생성 (`crates/core/src/embedding.rs`)

### EmbeddingProvider Trait

```rust
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    fn dimension(&self) -> usize;
    fn model_info(&self) -> &ModelInfo;
}
```

### 구현체

| 구현체 | 모델 | 차원 | 동작 방식 |
|--------|------|------|----------|
| `OnnxEmbedder` | all-MiniLM-L6-v2 (번들) | 384 | 배치 추론, mean pooling, L2 정규화 |
| `OllamaEmbedder` | 설정 가능 (기본 768dim) | 가변 | HTTP POST `/api/embeddings`, 텍스트 1개씩 |
| `NoOpEmbedder` | — | 384 | 항상 오류 반환 (FTS-only 모드용) |
| `MockEmbedder` | — | 설정 가능 | 랜덤 벡터 (테스트용) |

### ONNX 추론 과정

```
texts[] → 토크나이저 (WordPiece) → input_ids / attention_mask
                                  → ONNX Session (단일 스레드, Mutex로 보호)
                                  → token_embeddings [batch, seq_len, 384]
                                  → mean_pool (attention_mask 적용)
                                  → L2 normalize
                                  → Vec<Vec<f32>>
```

**배치 처리:** 여러 텍스트를 한 번의 ONNX 추론으로 처리.
단, ONNX Session은 `Mutex`로 감싸져 있어 동시에 하나의 추론만 실행된다.

### 임베딩 저장 형식

```
Vec<f32> → iter().flat_map(|f| f.to_le_bytes()) → Vec<u8> (BLOB)
```

sqlite-vec 확장이 읽을 수 있는 little-endian f32 바이트 배열로 저장된다.

---

## 3. 인덱싱 (`search.rs` → `index_document_sync()`)

```rust
fn index_document_sync(conn, project_id, source_doc_id, title, content, emb_bytes, meta)
```

실행 순서:

1. `content_hash = SHA-256(content)` — 변경 감지용
2. `documents` 테이블 UPSERT (ON CONFLICT 처리, 기존 문서면 업데이트)
3. `document_tags`, `document_aliases`, `document_metadata` 삭제 후 재삽입
4. `chunks` 전체 삭제 → `split_chunks()` → 새 청크 INSERT
   - FTS5 트리거가 `chunks_fts` 자동 동기화
5. 임베딩 바이트가 있으면 `chunk_embeddings`에 chunk_index=0에 저장

**비동기 버전 (`index_document_async`):**
임베딩 생성 (async I/O bound) → `spawn_blocking`으로 DB 작업 (sync, blocking) 분리.
`AuditEvent::IndexStart`, `IndexComplete`를 audit_log에 기록.

---

## 4. 검색 엔진 (`SearchEngine`)

### 두 가지 엔진

| 엔진 | 타입 | 용도 |
|------|------|------|
| `SearchEngine` | `Arc<Mutex<Connection>>` 소유 | async, 임베딩 지원, 하이브리드 검색 |
| `SyncSearchEngine<'a>` | `&Connection` 빌림 | sync, FTS-only, CLI/MCP에서 사용 |

### SearchMode

```rust
pub enum SearchMode {
    Hybrid,  // FTS5 + Vector → RRF (기본값)
    Fts,     // FTS5만
    Vector,  // sqlite-vec KNN만
}
```

---

## 5. FTS5 검색

### SQL

```sql
SELECT d.id, c.id, d.title, COALESCE(d.file_path, d.source_doc_id), c.heading_path,
       snippet(chunks_fts, 0, '<b>', '</b>', '…', 20) AS snippet,
       bm25(chunks_fts) AS score
FROM chunks_fts
JOIN chunks c ON c.id = chunks_fts.rowid
JOIN documents d ON d.id = c.document_id
JOIN projects p ON p.id = d.project_id
WHERE chunks_fts MATCH ?1
  AND p.status = 'active'   -- 또는 project_id IN (...)
ORDER BY score              -- BM25는 음수, 절댓값이 클수록 좋음
LIMIT ?2
```

### BM25 점수 처리

SQLite FTS5의 `bm25()` 함수는 **음수 값**을 반환한다 (값이 작을수록 더 관련성 높음).
`score: row.get::<_, f64>(6).unwrap_or(0.0).abs()` — 절댓값으로 양수화.

### 스니펫

`snippet(chunks_fts, 0, '<b>', '</b>', '…', 20)` — 매칭된 텀 주변 20 토큰을 HTML bold로 강조.

---

## 6. 벡터 검색 (sqlite-vec KNN)

### SQL

```sql
SELECT c.id, c.document_id, d.title, ..., knn.distance
FROM (
    SELECT chunk_id, distance FROM chunk_embeddings
    WHERE embedding MATCH ?1   -- 쿼리 임베딩 BLOB
    AND k = ?2                 -- 결과 개수
) knn
JOIN chunks c ON knn.chunk_id = c.id
JOIN documents d ON d.id = c.document_id
JOIN projects p ON p.id = d.project_id
WHERE p.status = 'active'
ORDER BY knn.distance
```

sqlite-vec 가상 테이블(`vec0`)은 내부적으로 코사인 거리 또는 L2 거리로 KNN을 수행한다.
distance 값은 0(완전 동일)에 가까울수록 관련성이 높다.

### 벡터 점수 변환

```rust
score: 1.0 / (RRF_K as f64 + distance)  // RRF_K = 60
```

distance가 0이면 `1/60 ≈ 0.0167`, distance가 크면 점수가 0에 수렴.
이렇게 변환하면 FTS 점수와 동일한 RRF 스케일이 된다.

---

## 7. RRF 병합

### Reciprocal Rank Fusion

```rust
fn rrf_score(rank: usize) -> f64 {
    1.0 / (60 + rank) as f64
}
```

| rank | rrf_score |
|------|-----------|
| 1 | 1/61 ≈ 0.01639 |
| 2 | 1/62 ≈ 0.01613 |
| 10 | 1/70 ≈ 0.01429 |
| 60 | 1/120 ≈ 0.00833 |

### 병합 알고리즘

```
fts_hits (rank 1..N) + vec_hits (rank 1..M)
  → HashMap<chunk_id, (score, hit)>
  → FTS rank i → score += 1/(60+i)
  → Vec rank j → score += 1/(60+j)
  → 두 리스트 모두에 있으면 두 점수 합산 (보너스)
  → score 내림차순 정렬
```

**RRF의 장점:** 두 스코어의 절대값 스케일을 맞출 필요가 없다. rank 순위만 사용하므로 이질적인 점수 체계(BM25 vs 코사인 거리)를 자연스럽게 통합한다.

---

## 8. 검색 결과 타입

### `SearchHit` (내부 — `db/schema.rs`)

```rust
pub struct SearchHit {
    pub document_id: i64,
    pub chunk_id: i64,
    pub title: Option<String>,
    pub file_path: Option<String>,
    pub heading_path: Option<String>,  // e.g. "## Section > ### Subsection"
    pub snippet: String,               // HTML bold 강조 포함
    pub score: f64,                    // RRF score (0.0 ~ ~0.033)
}
```

### `Hit` (공개 API — `search.rs`)

```rust
pub struct Hit {
    pub document_id: i64,
    pub project_id: i64,
    pub source_doc_id: String,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub score: f64,
}
```

---

## 9. 프론트엔드 표시 (Desktop)

```
SearchPage.tsx
  → invoke('search_documents', { query, projects, limit })
  → Tauri command: commands/search.rs
  → SyncSearchEngine::search_simple()
  → Vec<Hit> → JSON
  → useSearchStore (Zustand)
  → FileItem 컴포넌트
    └─ DocTooltip: 스코어 {:.6} 포맷 (2026-04-13 수정)
                   프로젝트 이모지, 스니펫, heading path 표시
```

**점수 표시 수정 이력 (2026-04-13):**
RRF 점수는 `1/61 ≈ 0.016` 수준으로, `{:.2}` 포맷이면 `0.00`으로 표시된다.
`{:.6}` 으로 변경하여 `0.016393`처럼 표시.

---

## 10. MCP 검색 도구

| 도구 | 내부 호출 | 특징 |
|------|----------|------|
| `doxus_search` | `search_async()` (Hybrid) | 기본 하이브리드 검색, cursor 페이지네이션 |
| `doxus_search_quality` | `search_async()` + 분석 | MRR, nDCG 등 품질 지표 반환 |
| `doxus_explain_search` | `search_async()` + 설명 | 각 Hit의 점수 근거 설명 |

---

## 11. 성능 특성

| 항목 | 값 |
|------|-----|
| 청크 크기 | 최대 1,500 chars (≈ 300 tokens) |
| 오버랩 | 200 chars |
| 임베딩 차원 | 384 (all-MiniLM-L6-v2) |
| RRF k 상수 | 60 |
| 기본 검색 limit | 20 |
| FTS 방식 | SQLite FTS5 (BM25 랭킹) |
| 벡터 방식 | sqlite-vec vec0 (KNN) |

---

## 12. 알려진 제약

- **임베딩은 chunk_index=0 전용:** chunk 1, 2, ... 는 FTS5만 참여. 벡터 검색은 문서의 첫 청크 기준으로만 동작한다.
- **SyncSearchEngine은 FTS-only:** Vector 모드를 요청해도 FTS로 폴백된다.
- **ONNX Session 단일 스레드:** 동시 임베딩 요청은 Mutex로 직렬화된다.
- **스니펫은 청크 단위:** 매칭된 청크의 일부만 표시, 문서 전체 컨텍스트가 아님.

---

## 13. 고성능 스니펫 생성 (`crates/core/src/search/highlighter.rs`)

### 두 가지 모드

| 모드 | 대상 | 동작 방식 |
|------|------|----------|
| `highlight_file` | Reference 전략 프로젝트 (Obsidian 등) | 파일을 memmap2로 메모리 맵, 바이트 오프셋으로 청크 범위 직접 읽기 |
| `highlight_text` | Full 전략 프로젝트 (Confluence, GitHub 등) | DB에 저장된 청크 텍스트에서 직접 스니펫 추출 |

### 구현 구조

```rust
pub struct Highlighter {
    ac: AhoCorasick,   // 검색어 멀티 패턴 매처 (케이스 인센시티브)
}

pub struct HighlightingResult {
    pub snippet: String,         // HTML <b> 태그 강조 포함
    pub matches_found: usize,    // 매칭된 검색어 개수
}
```

### Aho-Corasick 설정

```rust
AhoCorasick::builder()
    .match_kind(MatchKind::LeftmostFirst)  // 좌측 우선 최장 일치
    .ascii_case_insensitive(true)          // 대소문자 무시
    .build(keywords)
```

- 여러 검색어(토큰)를 단일 패스로 동시 탐색 — O(n) 시간복잡도
- `adjust_to_unicode_boundary`: UTF-8 멀티바이트 문자 경계 안전 처리

### Reference 모드 흐름 (파일 기반)

```
chunks.start_byte, chunks.end_byte   (DB에서 조회)
       │
       ▼
File::open(path) → Mmap::map()       (커널 페이지 캐시 활용, 복사 없음)
       │
       ▼
&mmap[start_byte..end_byte]          (해당 범위만 참조)
       │
       ▼
AhoCorasick::find_iter()             (검색어 위치 탐색)
       │
       ▼
<b>키워드</b> 강조된 스니펫 반환
```

### 성능 특성

| 항목 | 값 |
|------|-----|
| 파일 접근 방식 | memmap2 (커널 페이지 캐시, 복사 없음) |
| 검색어 탐색 | Aho-Corasick O(n + m) |
| Unicode 안전성 | UTF-8 바이트 경계 자동 조정 |
| 최대 스니펫 길이 | 청크 범위 내 전체 (설정 없음)
