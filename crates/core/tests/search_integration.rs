/// Integration test: 프로젝트 생성 → 문서 등록 → 검색 전체 플로우 검증
///
/// TestDb (in-memory SQLite + 전체 마이그레이션)를 사용하므로
/// 실제 ~/.doxus/db/nexus.db 에는 아무런 영향을 주지 않습니다.
use doxus_core::{
    db::TestDb,
    search::{SearchEngine, SearchQuery},
};
use rusqlite;

// ── 헬퍼 ─────────────────────────────────────────────────────────────────────

/// projects 테이블에 행을 추가하고 생성된 id를 반환합니다.
fn add_project(db: &TestDb, name: &str, display_name: &str, path: &str) -> i64 {
    db.conn
        .execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', unixepoch(), unixepoch())",
            rusqlite::params![name, display_name, path],
        )
        .unwrap_or_else(|e| panic!("insert project '{name}' failed: {e}"));

    db.conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1",
            [name],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap()
}

// ── 테스트 1: 단일 프로젝트, 단일 문서 ───────────────────────────────────────

#[test]
fn register_project_index_document_and_search() {
    let db = TestDb::new();
    let engine = SearchEngine::sync(&db.conn);

    // 1. 예제 프로젝트 생성
    let pid = add_project(&db, "my-notes", "My Notes", "/home/user/notes");

    // 2. 문서 등록 (인덱싱)
    engine
        .index_document(pid, "doc-rust", "Rust 언어 소개", "Rust 는 메모리 안전성을 보장하는 시스템 프로그래밍 언어입니다.", "full")
        .expect("index_document should succeed");

    // 3. 검색 (FTS5 unicode61은 공백 기준 토큰화 — 단어 경계에 공백이 있어야 매칭됨)
    let hits = engine
        .search(&SearchQuery::new("Rust"))
        .expect("search should succeed");

    assert!(!hits.is_empty(), "검색 결과가 있어야 합니다");
    assert_eq!(hits[0].source_doc_id, "doc-rust");
    assert_eq!(hits[0].title.as_deref(), Some("Rust 언어 소개"));
    assert_eq!(hits[0].project_id, pid);
}

// ── 테스트 2: 여러 프로젝트 + 프로젝트 범위 필터 ────────────────────────────

#[test]
fn search_scoped_to_single_project() {
    let db = TestDb::new();
    let engine = SearchEngine::sync(&db.conn);

    let p_work = add_project(&db, "work-wiki", "Work Wiki", "/work/wiki");
    let p_personal = add_project(&db, "personal", "Personal", "/personal");

    engine
        .index_document(p_work, "arch", "Architecture Decision", "microservice 아키텍처 결정 기록", "full")
        .unwrap();
    engine
        .index_document(p_personal, "diary", "오늘의 일기", "microservice 관련 공부를 했다", "full")
        .unwrap();

    // work-wiki 프로젝트만 검색
    let query = SearchQuery::new("microservice")
        .with_projects(vec![p_work])
        .with_limit(10);
    let hits = engine.search(&query).unwrap();

    assert_eq!(hits.len(), 1, "work-wiki 문서 1건만 나와야 합니다");
    assert_eq!(hits[0].source_doc_id, "arch");
    assert_eq!(hits[0].project_id, p_work);
}

// ── 테스트 3: 다중 문서 RRF 랭킹 ────────────────────────────────────────────

#[test]
fn multiple_documents_ranked_by_relevance() {
    let db = TestDb::new();
    let engine = SearchEngine::sync(&db.conn);

    let pid = add_project(&db, "kb", "Knowledge Base", "/kb");

    engine
        .index_document(pid, "d1", "Rust 완전 정복", "Rust 소유권, Rust 빌림, Rust 라이프타임 — Rust의 핵심 개념", "full")
        .unwrap();
    engine
        .index_document(pid, "d2", "Python 입문", "Python은 배우기 쉬운 프로그래밍 언어입니다", "full")
        .unwrap();
    engine
        .index_document(pid, "d3", "시스템 프로그래밍", "C와 Rust로 OS 커널 구현하기", "full")
        .unwrap();

    let hits = engine
        .search(&SearchQuery::new("Rust").with_limit(10))
        .unwrap();

    assert!(!hits.is_empty());
    // "Rust" 가 가장 많이 등장하는 d1이 상위에 있어야 함
    assert_eq!(hits[0].source_doc_id, "d1", "가장 관련성 높은 문서가 첫 번째여야 합니다");
    // Python 문서는 결과에 없거나 하위에 위치
    let python_pos = hits.iter().position(|h| h.source_doc_id == "d2");
    assert!(python_pos.is_none(), "관련 없는 Python 문서는 결과에 없어야 합니다");
}

// ── 테스트 4: 문서 재등록 (upsert) ───────────────────────────────────────────

#[test]
fn reindex_document_updates_content() {
    let db = TestDb::new();
    let engine = SearchEngine::sync(&db.conn);

    let pid = add_project(&db, "blog", "Blog", "/blog");

    // 초기 등록
    engine
        .index_document(pid, "post-1", "첫 번째 포스트", "초기 내용입니다", "full")
        .unwrap();

    // 내용 변경 후 재등록
    engine
        .index_document(pid, "post-1", "첫 번째 포스트 (개정판)", "Rust로 WebAssembly 컴파일하기", "full")
        .unwrap();

    // 업데이트된 내용으로 검색
    let hits = engine
        .search(&SearchQuery::new("WebAssembly"))
        .unwrap();
    assert!(!hits.is_empty(), "업데이트된 내용으로 검색되어야 합니다");
    assert_eq!(hits[0].source_doc_id, "post-1");
    assert_eq!(hits[0].title.as_deref(), Some("첫 번째 포스트 (개정판)"));

    // documents 테이블에 중복 없음
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE source_doc_id = 'post-1'",
            [],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "upsert 후 문서는 1건이어야 합니다");
}

// ── 테스트 5: disabled 프로젝트는 검색에서 제외 ─────────────────────────────

#[test]
fn disabled_project_excluded_from_search() {
    let db = TestDb::new();
    let engine = SearchEngine::sync(&db.conn);

    let pid = add_project(&db, "archived", "Archived", "/archived");

    engine
        .index_document(pid, "old-doc", "오래된 문서", "검색에서 제외되어야 할 고유한 내용 xyzzy42", "full")
        .unwrap();

    // 프로젝트를 disabled 로 변경
    db.conn
        .execute(
            "UPDATE projects SET status = 'disabled' WHERE id = ?1",
            rusqlite::params![pid],
        )
        .unwrap();

    let hits = engine
        .search(&SearchQuery::new("xyzzy42"))
        .unwrap();
    assert!(hits.is_empty(), "disabled 프로젝트 문서는 검색되지 않아야 합니다");
}

// ── 테스트 6: 존재하지 않는 쿼리 → 빈 결과 ────────────────────────────────

#[test]
fn search_nonexistent_query_returns_empty() {
    let db = TestDb::new();
    let engine = SearchEngine::sync(&db.conn);

    let pid = add_project(&db, "sample", "Sample", "/sample");
    engine
        .index_document(pid, "s1", "샘플 문서", "일반적인 내용", "full")
        .unwrap();

    let hits = engine
        .search(&SearchQuery::new("존재하지않는단어zzz9999"))
        .unwrap();
    assert!(hits.is_empty());
}

// ── 테스트 7: SearchQuery API (fts_search 경로) ───────────────────────────────

#[test]
fn search_query_api_finds_indexed_document() {
    let db = TestDb::new();

    let pid = add_project(&db, "docs", "Docs", "/docs");
    db.conn
        .execute(
            "INSERT INTO documents (project_id, source_doc_id, title, content_hash)
             VALUES (?1, 'api-guide', 'API 가이드', 'h1')",
            [pid],
        )
        .unwrap();
    let did: i64 = db
        .conn
        .query_row(
            "SELECT id FROM documents WHERE source_doc_id = 'api-guide'",
            [],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO chunks (document_id, content, chunk_index)
             VALUES (?1, 'REST API 설계 원칙과 모범 사례', 0)",
            [did],
        )
        .unwrap();

    let engine = SearchEngine::sync(&db.conn);
    let query = SearchQuery::new("REST API").with_projects(vec![pid]).with_limit(5);
    let hits = engine.search(&query).unwrap();

    assert!(!hits.is_empty());
    assert_eq!(hits[0].title.as_deref(), Some("API 가이드"));
    // Verify metadata presence
    assert!(hits[0].metadata_json.is_some(), "Metadata should be retrieved");
}
