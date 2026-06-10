/// 검색 품질 벤치마크 테스트
///
/// obsidian-nexus `.claude/skills/search-benchmark/SKILL.md` quality 모드 기준을 참고.
/// doxus SearchEngine 의 FTS5 검색 품질을 PASS/FAIL rank 로 검증합니다.
///
/// ⚠ FTS5 `unicode61` 토크나이저 제약:
///   - 한국어 조사가 붙은 단어("검색을", "추가한다")는 독립 토큰
///   - 쿼리 "검색"은 "검색을"에 매칭되지 않음
///   - 코퍼스는 핵심 키워드를 공백 단위로 분리하여 작성
///
/// 측정 지표:
///   - rank       : 기대 문서가 결과 몇 위인지 (미등장 시 usize::MAX)
///   - top1_match : 1위 결과가 기대 문서인가
///   - top3_match : top-3 내에 기대 문서가 있는가
///   - score_gap  : 1위-2위 score 차이
use doxus_core::{
    db::TestDb,
    search::{SearchEngine, SyncSearchEngine},
};
use rusqlite;

// ── 코퍼스 픽스처 ────────────────────────────────────────────────────────────
// 핵심 검색어가 조사 없이 공백으로 분리된 "keyword-rich" 스타일로 작성.

struct Doc {
    id: &'static str,
    title: &'static str,
    content: &'static str,
}

const CORPUS: &[Doc] = &[
    Doc {
        id: "rust-ownership",
        title: "Rust 소유권 시스템",
        content: "Rust ownership borrow lifetime 메모리 안전성 보장 소유권 빌림 스코프 해제",
    },
    Doc {
        id: "rust-async",
        title: "Rust 비동기 프로그래밍",
        content: "async await tokio runtime Future 비동기 이벤트 루프 poll executor",
    },
    Doc {
        id: "rust-wasm",
        title: "Rust WebAssembly 컴파일",
        content: "wasm-pack WebAssembly WASM 컴파일 브라우저 Node.js extism 플러그인",
    },
    Doc {
        id: "sqlite-fts5",
        title: "SQLite FTS5 전문 검색",
        content: "FTS5 가상 테이블 BM25 알고리즘 전문 검색 bm25 함수 snippet 하이라이트 unicode61 토크나이저",
    },
    Doc {
        id: "sqlite-vec",
        title: "sqlite-vec 벡터 검색",
        content: "sqlite-vec 익스텐션 SQLite 벡터 유사도 검색 vec0 가상 테이블 float32 KNN",
    },
    Doc {
        id: "rrf-ranking",
        title: "Reciprocal Rank Fusion 랭킹",
        content: "RRF Reciprocal Rank Fusion 랭킹 알고리즘 score rank 합산 k=60 FTS 벡터 병합",
    },
    Doc {
        id: "tauri-ipc",
        title: "Tauri IPC 커맨드",
        content: "Tauri v2 IPC command invoke AppState tauri::command 프론트엔드 백엔드 연결",
    },
    Doc {
        id: "react-zustand",
        title: "React Zustand 상태 관리",
        content: "React 19 Zustand 상태 관리 create useStore 훅 useChatStore useSearchStore useProjectStore",
    },
    Doc {
        id: "obsidian-vault",
        title: "Obsidian 볼트 구조",
        content: "Obsidian 볼트 마크다운 파일 wikilink 태그 .obsidian 플러그인 설정",
    },
    Doc {
        id: "wasm-plugin",
        title: "WASM 플러그인 샌드박스",
        content: "Extism WASM 플러그인 샌드박스 격리 Host Function http_request 도메인 허용 매니페스트 차단",
    },
    Doc {
        id: "embedding-onnx",
        title: "ONNX 임베딩 엔진",
        content: "ONNX Runtime all-MiniLM-L6-v2 임베딩 384 차원 벡터 배치 인퍼런스 오프라인 로컬",
    },
    Doc {
        id: "mcp-protocol",
        title: "MCP 도구 프로토콜",
        content: "MCP Model Context Protocol JSON-RPC tools/list tools/call doxus_search docnx 에이전트 도구",
    },
    Doc {
        id: "hybrid-search",
        title: "하이브리드 검색 아키텍처",
        content: "하이브리드 검색 FTS5 전문 검색 sqlite-vec 벡터 유사도 RRF 병합 랭킹 최종 결과",
    },
    Doc {
        id: "ci-matrix",
        title: "CI 매트릭스 설정",
        content: "GitHub Actions CI 매트릭스 macOS stable nightly Node 18 20 22 clippy rustfmt 테스트",
    },
    Doc {
        id: "plugin-sdk",
        title: "DocSource 플러그인 SDK",
        content: "DocSource trait fetch_all fetch_changes fetch_document plugin-sdk 크레이트 독립 구현",
    },
    Doc {
        id: "db-migration",
        title: "데이터베이스 마이그레이션",
        content: "마이그레이션 V1 V2 V3 V4 V5 V6 V7 V8 SQL CREATE TABLE IF NOT EXISTS 멱등성 _migrations 버전 추적",
    },
    Doc {
        id: "agent-sidecar",
        title: "에이전트 사이드카 프로토콜",
        content: "사이드카 Node.js stdio JSONL 프로토콜 start text result 세션 claude-agent-sdk 에이전트 스트리밍",
    },
    Doc {
        id: "keychain-secrets",
        title: "Keychain 자격증명 관리",
        content: "keyring Keychain API 토큰 자격증명 secrets_get Host Function 보안 저장소 환경변수",
    },
    Doc {
        id: "workspace-template",
        title: "워크스페이스 템플릿",
        content: "Handlebars 템플릿 워크스페이스 회고 일기 회의록 결정 기록 내장 템플릿 사용자 정의 DB 등록",
    },
    Doc {
        id: "confluence-plugin",
        title: "Confluence 플러그인",
        content: "Confluence Cloud REST API 페이지 SSRF 방지 localhost 127.0.0.1 10.x 차단 타임아웃 30초",
    },
];

// ── 헬퍼 ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct QualityResult {
    scenario: String,
    query: &'static str,
    expected_id: &'static str,
    rank: usize,
    top1_match: bool,
    top3_match: bool,
    score_gap: f64,
}

impl QualityResult {
    fn pass_top1(&self) -> &'static str {
        if self.top1_match {
            "PASS"
        } else {
            "FAIL"
        }
    }
    fn pass_top3(&self) -> &'static str {
        if self.top3_match {
            "PASS"
        } else {
            "FAIL"
        }
    }
}

fn rank_of(hits: &[doxus_core::db::schema::SearchHit], id: &str) -> usize {
    hits.iter()
        .position(|h| h.source_doc_id == id)
        .map(|i| i + 1)
        .unwrap_or(usize::MAX)
}

fn score_gap(hits: &[doxus_core::db::schema::SearchHit]) -> f64 {
    if hits.len() < 2 {
        0.0
    } else {
        hits[0].score - hits[1].score
    }
}

fn setup() -> (TestDb, i64) {
    let db = TestDb::new();
    db.conn
        .execute(
            "INSERT INTO projects (name, display_name, path, status, created_at, updated_at)
             VALUES ('bench', 'Bench', '/bench', 'active', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();
    let pid: i64 = db
        .conn
        .query_row(
            "SELECT id FROM projects WHERE name = 'bench'",
            [],
            |r: &rusqlite::Row| r.get(0),
        )
        .unwrap();

    let engine = SearchEngine::sync(&db.conn);
    for doc in CORPUS {
        engine
            .index_document(pid, doc.id, doc.title, doc.content, "full")
            .unwrap_or_else(|e| panic!("index '{}' failed: {e}", doc.id));
    }
    (db, pid)
}

fn measure(
    engine: &SyncSearchEngine,
    pid: i64,
    label: &str,
    query: &'static str,
    expected_id: &'static str,
) -> QualityResult {
    let query_obj = doxus_core::search::SearchQuery::new(query)
        .with_projects(vec![pid])
        .with_limit(10);
    let hits = engine.search(&query_obj).unwrap();
    let rank = rank_of(&hits, expected_id);
    QualityResult {
        scenario: label.to_string(),
        query,
        expected_id,
        rank,
        top1_match: rank == 1,
        top3_match: rank <= 3,
        score_gap: score_gap(&hits),
    }
}

fn print_report(results: &[QualityResult]) {
    println!("\n╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║           doxus SearchEngine — Quality Benchmark Report                    ║");
    println!("╠════════════════════════╦═══════════════════════════════╦═════╦════╦════╦════╣");
    println!("║ 시나리오               ║ 쿼리                          ║rank ║top1║top3║gap ║");
    println!("╠════════════════════════╬═══════════════════════════════╬═════╬════╬════╬════╣");
    for r in results {
        let rank_str = if r.rank == usize::MAX {
            "∞".to_string()
        } else {
            r.rank.to_string()
        };
        println!(
            "║ {:<22} ║ {:<29} ║{:>4} ║{:>4}║{:>4}║{:.2}║",
            r.scenario,
            r.query,
            rank_str,
            r.pass_top1(),
            r.pass_top3(),
            r.score_gap,
        );
    }
    println!("╚════════════════════════╩═══════════════════════════════╩═════╩════╩════╩════╝");
    let p1 = results.iter().filter(|r| r.top1_match).count();
    let p3 = results.iter().filter(|r| r.top3_match).count();
    let n = results.len();
    println!(
        "  top-1: {}/{} ({:.0}%)   top-3: {}/{} ({:.0}%)\n",
        p1,
        n,
        p1 as f64 / n as f64 * 100.0,
        p3,
        n,
        p3 as f64 / n as f64 * 100.0,
    );
}

// ── 시나리오 1: 키워드 검색 (단문) ───────────────────────────────────────────

/// 제목/본문에 키워드가 명확히 등장 — top-1 전원 PASS 기대
#[test]
fn quality_s1_keyword_search() {
    let (db, pid) = setup();
    let engine = SearchEngine::sync(&db.conn);

    // ⚠ FTS5 쿼리에서 '-' 는 NOT 연산자이므로 단독 사용 금지
    //   "sqlite-vec" → sqlite AND NOT vec (의도와 반대!)
    //   대신 공백 분리 또는 큰따옴표 phrase 사용
    let cases: &[(&str, &str)] = &[
        ("ownership borrow lifetime", "rust-ownership"),
        ("FTS5 BM25 snippet", "sqlite-fts5"),
        ("RRF Reciprocal Rank Fusion", "rrf-ranking"),
        ("Tauri IPC invoke AppState", "tauri-ipc"),
        ("ONNX 임베딩 384", "embedding-onnx"),
        ("MCP Protocol docnx", "mcp-protocol"),
        ("Confluence SSRF localhost", "confluence-plugin"),
        ("keyring Keychain 자격증명", "keychain-secrets"),
    ];

    let results: Vec<QualityResult> = cases
        .iter()
        .enumerate()
        .map(|(i, (q, id))| measure(&engine, pid, &format!("키워드-{}", i + 1), q, id))
        .collect();
    print_report(&results);

    for r in &results {
        assert!(
            r.top1_match,
            "키워드 검색 FAIL: query='{}' expected='{}' rank={}",
            r.query, r.expected_id, r.rank
        );
    }
}

// ── 시나리오 2: 개념/의미 검색 (장문) ────────────────────────────────────────

/// 정확한 파일명 모르는 상황 — top-3 기준, 최소 5/7 PASS 기대
#[test]
fn quality_s2_concept_search() {
    let (db, pid) = setup();
    let engine = SearchEngine::sync(&db.conn);

    let cases: &[(&str, &str)] = &[
        ("벡터 유사도 KNN vec0", "sqlite-vec"),
        ("마이그레이션 SQL CREATE TABLE", "db-migration"),
        ("사이드카 JSONL 스트리밍", "agent-sidecar"),
        ("Handlebars 템플릿 회고 회의록", "workspace-template"),
        ("WASM 샌드박스 격리 Host Function", "wasm-plugin"),
        ("하이브리드 검색 FTS 벡터 RRF", "hybrid-search"),
        ("wasm-pack WASM 컴파일 extism", "rust-wasm"),
    ];

    let results: Vec<QualityResult> = cases
        .iter()
        .enumerate()
        .map(|(i, (q, id))| measure(&engine, pid, &format!("개념-{}", i + 1), q, id))
        .collect();
    print_report(&results);

    let pass3 = results.iter().filter(|r| r.top3_match).count();
    assert!(
        pass3 >= 5,
        "개념 검색 top-3 정확도 {}/{} — 최소 5/7 PASS 필요\n실패 케이스: {}",
        pass3,
        results.len(),
        results
            .iter()
            .filter(|r| !r.top3_match)
            .map(|r| format!("query='{}' rank={}", r.query, r.rank))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

// ── 시나리오 3: 랭킹 명확도 (score_gap) ─────────────────────────────────────

/// 1위가 명확히 앞서야 함 — gap >= 0 & top-3
#[test]
fn quality_s3_ranking_clarity() {
    let (db, pid) = setup();
    let engine = SearchEngine::sync(&db.conn);

    let cases: &[(&str, &str)] = &[
        ("keyring Keychain secrets_get", "keychain-secrets"),
        ("DocSource trait fetch_all fetch_changes", "plugin-sdk"),
        ("async await tokio Future executor", "rust-async"),
        ("GitHub Actions CI stable nightly clippy", "ci-matrix"),
        ("Obsidian 볼트 마크다운 wikilink 태그", "obsidian-vault"),
    ];

    for (query, expected_id) in cases {
        let query_obj = doxus_core::search::SearchQuery::new(*query)
            .with_projects(vec![pid])
            .with_limit(10);
        let hits = engine.search(&query_obj).unwrap();
        let rank = rank_of(&hits, expected_id);
        let gap = score_gap(&hits);

        assert!(
            rank <= 3,
            "query='{}' expected='{}' rank={} — top-3 밖",
            query,
            expected_id,
            rank
        );
        println!(
            "  [OK] query='{}' rank={} score_gap={:.4}",
            query, rank, gap
        );
    }
}

// ── 시나리오 4: 크로스 문서 구분 ────────────────────────────────────────────

/// 유사 주제 문서 중 정확한 것이 더 높은 순위를 가져야 함
#[test]
fn quality_s4_document_discrimination() {
    let (db, pid) = setup();
    let engine = SearchEngine::sync(&db.conn);

    {
        let query_obj = doxus_core::search::SearchQuery::new("sqlite vec KNN float32")
            .with_projects(vec![pid])
            .with_limit(10);
        let hits = engine.search(&query_obj).unwrap();
        let r_vec = rank_of(&hits, "sqlite-vec");
        let r_hyb = rank_of(&hits, "hybrid-search");
        println!(
            "  [구분] sqlite-vec rank={}, hybrid-search rank={}",
            r_vec, r_hyb
        );
        assert!(
            r_vec <= 3,
            "sqlite-vec 이 top-3 안에 있어야 함 (실제 rank={})",
            r_vec
        );
        assert!(r_vec < r_hyb, "sqlite-vec 이 hybrid-search 보다 높아야 함");
    }

    // "RRF Reciprocal Rank Fusion" → rrf-ranking 이 hybrid-search 보다 상위
    {
        let query_obj = doxus_core::search::SearchQuery::new("RRF Reciprocal Rank Fusion")
            .with_projects(vec![pid])
            .with_limit(10);
        let hits = engine.search(&query_obj).unwrap();
        let r_rrf = rank_of(&hits, "rrf-ranking");
        let r_hyb = rank_of(&hits, "hybrid-search");
        println!(
            "  [구분] rrf-ranking rank={}, hybrid-search rank={}",
            r_rrf, r_hyb
        );
        assert!(
            r_rrf == 1,
            "rrf-ranking 이 1위여야 함 (실제 rank={})",
            r_rrf
        );
        assert!(r_rrf < r_hyb, "rrf-ranking 이 hybrid-search 보다 높아야 함");
    }

    // "Tauri IPC command invoke" → tauri-ipc 가 react-zustand 보다 상위
    {
        let query_obj = doxus_core::search::SearchQuery::new("Tauri IPC command invoke")
            .with_projects(vec![pid])
            .with_limit(10);
        let hits = engine.search(&query_obj).unwrap();
        let r_tauri = rank_of(&hits, "tauri-ipc");
        let r_react = rank_of(&hits, "react-zustand");
        println!(
            "  [구분] tauri-ipc rank={}, react-zustand rank={}",
            r_tauri, r_react
        );
        assert!(
            r_tauri == 1,
            "tauri-ipc 가 1위여야 함 (실제 rank={})",
            r_tauri
        );
    }
}

// ── 시나리오 5: 무결과 내성 ──────────────────────────────────────────────────

/// 완전히 관련 없는 쿼리는 빈 결과 반환
#[test]
fn quality_s5_no_result_for_unrelated() {
    let (db, pid) = setup();
    let engine = SearchEngine::sync(&db.conn);

    let unrelated = [
        "photosynthesis chlorophyll",
        "quiche lorraine recipe",
        "symphonie fantastique berlioz",
    ];

    for query in &unrelated {
        let query_obj = doxus_core::search::SearchQuery::new(*query)
            .with_projects(vec![pid])
            .with_limit(5);
        let hits = engine.search(&query_obj).unwrap();
        assert!(
            hits.is_empty(),
            "query='{}' — 관련 없는 쿼리는 빈 결과여야 함 (got {}건)",
            query,
            hits.len()
        );
        println!("  [OK] query='{}' → 0건", query);
    }
}
