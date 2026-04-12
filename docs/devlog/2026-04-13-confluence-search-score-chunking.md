---
title: "Confluence 검색 점수 0.00 버그 수정 — 표시 버그 + 문서 청킹 구현"
aliases:
  - confluence-score-fix
  - 검색-점수-수정
  - chunking-구현
  - 2026-04-13 doxus 데브로그
tags:
  - devlog
  - troubleshooting
  - search
  - bm25
  - chunking
created: "2026-04-13"
updated: "2026-04-13"
---

<!-- docsmith: auto-generated 2026-04-13 -->

## 개요

doxus CLI/MCP QA 테스트 중 Confluence 문서 검색 결과에서 score가 모두 `0.00`으로 표시되는 이슈를 발견했다.
조사 결과 두 가지 독립적인 문제가 얽혀 있었다: **포맷 잘림 버그**와 **문서 길이 불균형으로 인한 BM25 점수 저하**.

## 증상

```
$ doxus search '문서' --project '개인/컨플 공유문서'
1. 페이지 A [score: 0.00]
2. 페이지 B [score: 0.00]
3. 페이지 C [score: 0.00]
```

MCP `doxus_search` 동일 쿼리에서는 정상 점수 반환 → CLI와 MCP 결과 불일치.

## 디버깅 과정

### 1단계: 초기 가설 (오답)

- 가설: CLI는 `NoOpEmbedder`(FTS-only), MCP는 `OnnxEmbedder`(Hybrid) → 서로 다른 검색 경로
- critic 에이전트 리뷰에서 **REJECT**

실제 코드 확인 결과 CLI와 MCP 모두 동일한 `SyncSearchEngine` 경로를 사용:

```
MCP: SearchEngine::new(&self.conn)  // mcp-server/src/lib.rs:441
CLI: SearchEngine::new(conn)        // cli/src/main.rs:257
```

### 2단계: DB 직접 쿼리

`sqlite3`로 FTS5 bm25 점수를 직접 확인:

```sql
SELECT bm25(chunks_fts)
FROM chunks_fts
JOIN chunks c ON c.id = chunks_fts.rowid
JOIN documents d ON d.id = c.document_id
WHERE d.project_id = 3 AND chunks_fts MATCH '문서';
```

결과: `-1.15e-6`, `-1.03e-6`, `-8.33e-7`

점수 자체는 존재하지만 극히 작은 음수 값 (FTS5 bm25는 음수 반환, 절댓값이 클수록 관련성 높음).

### 3단계: 근본 원인 확정

두 가지 문제가 중첩되어 있었다:

**문제 1 — 표시 버그**: `{:.2}` 포맷이 `1e-6` 규모의 값을 `0.00`으로 잘라냄

```rust
// crates/cli/src/main.rs:273 (수정 전)
println!("{}. {} [score: {:.2}]", i + 1, title, hit.score);
```

**문제 2 — BM25 점수 저하**: `index_document_sync`가 전체 문서를 단일 청크(`chunk_index=0`)로 저장.
Confluence 페이지는 1000~5000단어, Obsidian 파일은 300~500단어 수준이다.
FTS5 bm25는 `dl/avgdl` 비율로 점수를 정규화하기 때문에, 문서 길이가 평균보다 훨씬 길면 점수가 기하급수적으로 낮아진다.

추가 확인:
- CLI와 MCP 모두 `~/.doxus/db/doxus.db` 동일 DB 사용
- Confluence chunks_fts 등록: 10개 모두 정상 등록됨
- FTS 미등록 문제가 아니라 문서 길이 불균형이 원인

## 수정 내용

### Fix 1: 표시 버그 (`crates/cli/src/main.rs`)

```rust
// 수정 전
println!("{}. {} [score: {:.2}]", i + 1, title, hit.score);
// 수정 후
println!("{}. {} [score: {:.6}]", i + 1, title, hit.score);
```

### Fix 2: 청킹 모듈 신규 생성 (`crates/core/src/chunker.rs`)

```rust
pub fn split_chunks(text: &str, max_chars: usize, overlap_chars: usize) -> Vec<Chunk>
```

- `DEFAULT_MAX_CHARS = 1500`, `DEFAULT_OVERLAP_CHARS = 200`
- 단락(`\n\n`) 경계 기준으로 분할
- 마지막 `overlap_chars`를 다음 청크에 prepend하여 문맥 연속성 유지
- 한국어 UTF-8 포함 12개 단위 테스트 작성 (empty, single chunk, multi chunk, sequential indices, overlap, Korean UTF-8, single long para 등)

### Fix 3: 인덱서 청킹 적용 (`crates/core/src/search.rs`)

```rust
// index_document_sync 내 기존 단일 INSERT 교체
let chunks = crate::chunker::split_chunks(content, DEFAULT_MAX_CHARS, DEFAULT_OVERLAP_CHARS);
for chunk in &chunks {
    conn.execute(
        "INSERT INTO chunks (document_id, content, chunk_index) ...",
        ...
    )?;
}
```

## 부가 작업: ABI 버전 검증 테스트

작업 중 `manager.rs`에 ABI 버전 검증 테스트가 누락된 것을 확인하여 4개 테스트를 추가했다:

- `get_source_returns_none_for_unsupported_abi`
- `get_source_abi_check_uses_supported_abi_constant`
- `get_source_returns_none_when_only_manifest_present`
- `get_source_rejects_path_traversal_plugin_id`

## 결과

- 커밋: `cd47944`
- 전체 테스트: 550+ passed, 0 failed
- clippy 경고: 0

## 관련 파일

- `crates/core/src/chunker.rs` — 신규 생성
- `crates/core/src/search.rs` — 청킹 적용
- `crates/cli/src/main.rs` — 점수 포맷 수정
- `crates/core/src/plugin/manager.rs` — ABI 테스트 추가

## 관련 문서

- [[2026-04-11-doxus-abi-secret-oauth-tdd]]
- [[2026-04-11-doxus-remaining-10-tasks-tdd]]
- [[2026-04-12-doxus-market-guide-feature]]
