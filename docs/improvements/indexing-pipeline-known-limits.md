---
title: 인덱싱 파이프라인 알려진 한계
updated: 2026-04-30
tags:
  - improvement
  - indexing
  - vector-search
  - embedding
---

# 인덱싱 파이프라인 알려진 한계 (Known Limits)

> 현재 구현 상태와 목표 상태를 추적하는 문서.
> 해결 완료 시 해당 행을 삭제하거나 완료 표시로 업데이트할 것.

## 벡터 검색 커버리지 한계

| 항목 | 현재 동작 | 목표 상태 | 심각도 |
|------|----------|---------|--------|
| 벡터 임베딩 대상 | `chunk_index=0` (첫 청크)만 | 전체 청크 배치 임베딩 | 높음 |
| 임베딩 입력 | 전체 `content` (512 토큰 초과 시 자동 truncation) | 청크별 clean text | 높음 |
| 마크다운 전처리 | 없음 (frontmatter, `##`, `[[wikilink]]` 노이즈 포함) | `clean_for_embedding()` 전처리 | 중간 |
| Heading-aware chunking | `\n\n` 기준 단순 분할 | `##` 헤딩 계층 기준 분할 + `heading_path` context prefix | 중간 |

### 핵심 문제

FTS5 검색은 **모든 청크(N개)**를 인덱싱하지만, 벡터 유사도 검색은 **각 문서의 첫 1,500자**만 대상이다.
문서 중반부 이후 내용은 의미 기반 검색에서 찾을 수 없다.

업계 표준(LlamaIndex, LangChain, Elasticsearch)은 모두 **청크별 임베딩**을 기본으로 한다.

### 단기 개선 방안 (즉시 적용 가능)

**① 청크별 배치 임베딩으로 전환**

`OnnxEmbedder`는 이미 배치 API를 지원하므로 `chunk_index == 0` 조건 한 줄 제거가 핵심.

```rust
// 변경 전 — crates/core/src/search/mod.rs
if chunk.index == 0 {
    save_embedding(&chunk, &embedding)?;
}

// 변경 후 — 모든 청크에 임베딩 저장
let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
let embeddings = embedder.embed(&texts).await?;
for (chunk, emb) in chunks.iter().zip(embeddings.iter()) {
    save_embedding(chunk, emb)?;
}
```

**② 마크다운 전처리 후 임베딩**

```rust
fn clean_for_embedding(content: &str) -> String {
    // frontmatter 블록 제거 (--- ... --- 사이)
    // ## 헤딩 기호 제거 (텍스트는 유지)
    // [[wikilink]] → wikilink 텍스트만
    // [text](url) → text만
}
// 임베딩 입력에만 적용 — FTS와 DB 저장은 원본 마크다운 유지
```

### 관련 파일

- [crates/core/src/search/mod.rs](../../crates/core/src/search/mod.rs) — `index_document_async`, 임베딩 저장 위치
- [crates/core/src/chunker.rs](../../crates/core/src/chunker.rs) — `split_chunks` 구현
- [crates/core/src/embedding.rs](../../crates/core/src/embedding.rs) — `OnnxEmbedder`, 배치 embed 지원

> **근거**: 2026-04-13 devlog `indexing-pipeline-improvement-needed` — 벡터 검색 커버리지 구조적 한계 분석.
