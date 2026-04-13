---
title: "인덱싱 파이프라인 개선 필요점 분석"
aliases:
  - indexing-pipeline-improvement
  - 인덱싱-개선
  - embedding-chunking-fix
  - 2026-04-13 doxus 데브로그
tags:
  - devlog
  - improvement
  - search
  - embedding
  - chunking
  - vector-search
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

## 개요

현재 doxus 인덱싱 파이프라인을 분석한 결과, 벡터 검색 커버리지와 임베딩 품질에 영향을 주는 구조적 문제 3가지를 발견했다. FTS5 검색은 정상 동작하지만, 하이브리드 검색의 벡터 쪽 기여가 사실상 제한적으로만 작동하고 있다.

## 현재 파이프라인 구조

```
index_document_async
├─ embedder.embed(&[전체 content]) → 벡터 1개 생성
└─ spawn_blocking → index_document_sync
   ├─ documents 테이블 UPSERT
   ├─ split_chunks(content, 1500, 200) → N개 청크
   ├─ chunks INSERT → chunks_fts 자동 동기화 (트리거)
   ├─ chunk_embeddings INSERT (chunk_index = 0 만) ← 문제
   ├─ document_tags INSERT
   └─ document_aliases INSERT
```

**핵심 불일치**: FTS는 모든 청크(N개)를 인덱싱하지만, 벡터 검색은 첫 번째 청크(index=0)만 대상으로 한다.

## 문제점

### 1. 임베딩 입력 길이 초과 (심각도: 높음)

`all-MiniLM-L6-v2` 모델의 최대 입력 토큰은 **512 토큰 (~2000자)**이다.

현재 코드는 문서 전체 `content`를 임베딩 입력으로 넣는다:

```rust
// crates/core/src/search.rs
let embedding = embedder.embed(&[content]).await  // ← 문서 전체
```

긴 문서(예: Confluence 페이지, 긴 노트)는 512 토큰 초과분이 **자동 truncation**되어 버려진다. 문서 뒷부분 내용은 벡터 공간에서 완전히 사라진다.

### 2. 벡터 검색 커버리지 = 첫 청크만 (심각도: 높음)

```rust
// chunk_embeddings에 chunk_index = 0만 저장
if chunk.index == 0 {
    // embedding 저장
}
```

FTS 검색은 문서의 어느 부분이든 키워드로 찾을 수 있지만, 벡터 유사도 검색은 **각 문서의 첫 1500자 내용만** 대상이다. "문서 중반부 이후에만 있는 내용"은 의미 기반 검색에서 찾을 수 없다.

### 3. 마크다운 노이즈 미처리 (심각도: 중간)

임베딩 입력에 마크다운 문법이 그대로 포함된다:
- `---` frontmatter 구분선
- `## ## ###` 헤딩 기호
- `[[wikilink]]`, `[text](url)` 링크 문법
- 코드 블록 ` ``` ``` ` 내 코드

이 노이즈들이 임베딩 벡터 품질을 낮춘다. 의미적으로 무관한 토큰이 벡터 공간을 차지한다.

## 업계 표준과의 비교

| 방식 | 청크 단위 | 임베딩 대상 | 벡터 커버리지 |
|------|----------|------------|--------------|
| LlamaIndex / LangChain | 256~512 토큰, overlap 20% | 모든 청크 | 100% |
| Elasticsearch knn | 문단/섹션 | 모든 청크 | 100% |
| Weaviate / Qdrant | 모델 limit 기준 | 모든 청크 | 100% |
| **현재 doxus** | 1500자, overlap 200자 | **첫 청크만** | **~1/N** |

LlamaIndex, LangChain 등 주요 RAG 프레임워크는 모두 **청크별 임베딩**을 기본으로 한다.

## 개선 방안

### 단기 (즉시 적용 가능)

**① 청크별 배치 임베딩으로 전환**

```rust
// 변경 전
let embedding = embedder.embed(&[content]).await?;

// 변경 후
let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
let embeddings = embedder.embed(&texts).await?;  // N개 배치
// chunk_embeddings에 모든 청크 저장
```

`OnnxEmbedder`는 이미 배치 처리를 지원하므로(`embed(&[&str])`) API 변경 없이 적용 가능하다. `chunk_embeddings INSERT` 로직에서 `chunk_index == 0` 조건만 제거하면 된다.

**② 마크다운 전처리 후 임베딩**

```rust
fn clean_for_embedding(content: &str) -> String {
    // frontmatter 블록 제거 (--- ... --- 사이)
    // ## 헤딩 기호 제거 (텍스트는 유지)
    // [[wikilink]] → wikilink 텍스트만
    // [text](url) → text만
    // 코드 블록 → 언어 힌트 제거, 코드 내용 유지
}
```

임베딩 입력에만 적용하고, FTS와 DB 저장은 원본 마크다운 유지.

### 중기 (설계 개선)

**③ Heading-aware chunking**

현재 `\n\n` 기준 문단 분할 → `##` 헤딩 기준 계층적 분할로 개선.
청크에 `heading_path`를 context prefix로 포함:

```
"[문서제목] > [섹션명]: 실제 내용..."
```

문서 구조 정보가 청크에 포함되어 의미 검색 품질 향상. `chunks` 테이블의 `heading_path` 컬럼 활용.

**④ Parent document retrieval**

청크 검색 결과를 부모 문서로 그룹핑 후 반환. 현재 `chunks.document_id` 역참조 구조로 이미 가능하다. 검색 결과에서 같은 문서의 여러 청크가 중복 노출되는 문제도 해결.

## 우선순위 정리

| 이슈 | 심각도 | 구현 난이도 | 예상 효과 |
|------|--------|------------|---------|
| 전체 content 임베딩 → truncation | 높음 | 낮음 | 벡터 품질 직접 향상 |
| 벡터 검색 커버리지 첫 청크만 | 높음 | 낮음 | recall 대폭 향상 |
| 마크다운 노이즈 미처리 | 중간 | 낮음 | 벡터 품질 향상 |
| Heading-aware chunking | 중간 | 중간 | 검색 정밀도 향상 |
| Parent document retrieval | 낮음 | 중간 | UX 개선 |

**가장 먼저 고쳐야 할 것**: ①번 (청크별 배치 임베딩). `chunk_index == 0` 조건 한 줄 제거만으로 벡터 검색 커버리지가 N배 늘어난다.

## 관련 파일

- [crates/core/src/search.rs](../../crates/core/src/search.rs) — `index_document_async`, 임베딩 호출 위치
- [crates/core/src/chunker.rs](../../crates/core/src/chunker.rs) — `split_chunks` 구현
- [crates/core/src/embedding.rs](../../crates/core/src/embedding.rs) — `OnnxEmbedder`, 배치 embed 지원
- [crates/core/src/db/migrations/V4__embeddings.sql](../../crates/core/src/db/migrations/V4__embeddings.sql) — chunk_embeddings 스키마

## 관련 문서

- [[2026-04-13-confluence-search-score-chunking]] — BM25 점수 0.00 버그 및 청킹 구현
- [[2026-04-13-doxus-search-quality-review]] — 검색 품질 전체 리뷰
