# 데이터베이스 규칙

## 경로 규칙

```
~/.doxus/
└── db/
    └── nexus.db        # SQLite 메인 DB (단일 파일)
```

- DB 경로는 환경변수 `DOXUS_DB_PATH`로 오버라이드 가능 (테스트용)
- 프로덕션에서는 항상 `~/.doxus/db/nexus.db`
- DB 파일 직접 편집 금지 — 반드시 마이그레이션 SQL로만 스키마 변경

## 마이그레이션 원칙

- 마이그레이션 파일 위치: `crates/core/src/db/migrations/`
- 파일명: `V{번호}__{설명}.sql` (예: `V7__add_source_instances.sql`)
- V1~V6: obsidian-nexus에서 계승 (수정 금지)
- V7: `source_instances` 테이블 추가 (플러그인 인스턴스별 설정)
- V8: 예약 (필요 시 추가)
- 마이그레이션은 **멱등성** 보장 (`CREATE TABLE IF NOT EXISTS` 등)
- 이미 적용된 마이그레이션 파일 내용 변경 금지

## 핵심 테이블

### projects
```sql
CREATE TABLE projects (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,       -- slug, 검색 키
    display_name TEXT NOT NULL,             -- 사용자에게 표시
    description TEXT,
    path        TEXT NOT NULL,             -- 폴더 경로 또는 소스 식별자
    status      TEXT NOT NULL DEFAULT 'active'  -- 'active' | 'disabled'
                CHECK(status IN ('active', 'disabled')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
```

### documents
```sql
CREATE TABLE documents (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_doc_id TEXT NOT NULL,           -- 플러그인이 발급한 원본 ID
    title        TEXT,
    content      TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    chunk_index  INTEGER NOT NULL DEFAULT 0,
    last_indexed INTEGER NOT NULL,
    UNIQUE(project_id, source_doc_id, chunk_index)
);
```

### source_instances (V7 신규)
```sql
CREATE TABLE source_instances (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id   TEXT NOT NULL,             -- 'com.doxus.confluence'
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    config_json TEXT NOT NULL DEFAULT '{}',
    last_sync   INTEGER,
    sync_cursor TEXT,                      -- opaque, 플러그인 전용
    created_at  INTEGER NOT NULL
);
```

### audit_log
```sql
CREATE TABLE audit_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,  -- 'index_start', 'index_complete', 'plugin_error', ...
    payload    TEXT,           -- JSON
    occurred_at INTEGER NOT NULL
);
```

## SQLite 설정

앱 시작 시 반드시 적용:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -32000;   -- 32MB
```

- `sqlite-vec` 익스텐션은 DB 연결 직후 로드
- FTS5 가상 테이블은 `documents` 테이블과 트리거로 동기화

## 데이터 접근 규칙

- 모든 DB 접근은 `crates/core/src/db/` 모듈을 통해서만
- 직접 SQL은 core 내부에서만 — plugin/cli/mcp는 core API 사용
- 트랜잭션: 다중 문서 인덱싱은 단일 트랜잭션으로 묶음
- 쿼리 결과는 도메인 타입으로 즉시 변환 (rusqlite Row를 외부로 노출 금지)
