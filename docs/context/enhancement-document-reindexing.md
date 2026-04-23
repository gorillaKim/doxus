# Enhancement: Document 재인덱싱 기능 설계

**작성일**: 2026-04-23  
**작성자**: Claude  
**상태**: Proposal  
**우선순위**: Medium-High  
**관련 이슈**: 문서 메타데이터 변경 시 인덱스 동기화 필요

---

## 1. 문제 정의

### 1.1 현재 상황

doxus는 **프로젝트 전체를 재인덱싱하는 기능**(`doxus_index_project`, `doxus_sync_project`)은 있지만, **특정 문서 단일 또는 범위 기반 재인덱싱**이 불가능합니다.

**현재 동작 방식**:
```
프로젝트 전체 재인덱싱 (모든 문서 스캔)
  ↓
시간 소비 (대규모 프로젝트는 수십 분)
  ↓
메타데이터 변경 후 동기화 미흡
```

### 1.2 문제점

| 상황 | 현재 | 문제점 |
|------|------|--------|
| **한 문서 수정 후** | 전체 프로젝트 재인덱싱 필요 | ⏱️ 비효율적 (전체 스캔) |
| **메타데이터 필드 추가** (datetime) | 전체 문서 백필 필요 | 🔴 시간 초과, DB 잠금 위험 |
| **배치 메타데이터 수정** | 1개씩 수정 후 재인덱싱 반복 | 🔴 전체 재인덱싱 반복 |
| **플러그인 업그레이드** | 해당 플러그인 문서 모두 재인덱싱 필요 | ⏱️ 불편한 워크플로우 |
| **오류 복구** (특정 문서) | 해당 문서만 재인덱싱 불가 | 🔴 전체 재인덱싱 강제 |

### 1.3 사용 사례

```
# 사용 사례 1: DateTime 메타데이터 백필 (datetime-metadata PR 병합 후)
doxus_reindex_documents(
    project="AI 리포트 챗봇",
    scope="all",  # 모든 문서
    fields=["created_at", "updated_at"]  # 특정 필드만 재계산
)
→ 10,000개 문서, 3~5분 소요

# 사용 사례 2: 특정 Confluence 공간만 재인덱싱
doxus_reindex_documents(
    project="AI 리포트 챗봇",
    scope="plugin",
    plugin="confluence",
    space_key="AP"  # 특정 공간만
)
→ 500개 문서, 30초 소요

# 사용 사례 3: 특정 문서만 재인덱싱 (오류 복구)
doxus_reindex_documents(
    project="doxus",
    scope="document",
    document_id="4813520900"
)
→ 1개 문서, 1초 소요

# 사용 사례 4: 날짜 범위 기반 재인덱싱
doxus_reindex_documents(
    project="Brain",
    scope="date_range",
    created_after="2026-04-20",
    created_before="2026-04-23"
)
→ 최근 3일 생성 문서만, 30초 소요
```

---

## 2. 제안 (Solution)

### 2.1 재인덱싱 범위 (Scope) 정의

| Scope | 대상 | 예시 | 예상 시간 |
|-------|------|------|---------|
| **full** | 전체 프로젝트 모든 문서 | 모든 문서 재인덱싱 | 프로젝트 규모별 |
| **document** | 특정 문서 1개 | ID로 단일 지정 | <2초 |
| **documents** | 특정 문서 다수 | ID 배열 | 문서 수 × 1초 |
| **plugin** | 특정 플러그인 모든 문서 | Confluence/GitHub 선택 | 10K 문서 ~ 5분 |
| **date_range** | 생성/수정 날짜 범위 | 2026-04-20 ~ 04-23 | 범위 내 문서 수에 따름 |
| **status** | 인덱싱 상태별 | failed, outdated, pending | 오류 문서만 |

### 2.2 API 설계

#### **doxus_reindex_documents (신규)**

```python
# 기본 형식
doxus_reindex_documents(
    project: str,                        # 필수: 프로젝트명
    scope: str,                          # 필수: full|document|documents|plugin|date_range|status
    
    # Scope별 파라미터
    document_id: Optional[str] = None,          # scope=document
    document_ids: Optional[List[str]] = None,   # scope=documents
    plugin: Optional[str] = None,               # scope=plugin (confluence|github|local)
    plugin_param: Optional[Dict] = None,        # scope=plugin (space_key, org, path 등)
    created_after: Optional[int] = None,        # scope=date_range (Unix timestamp)
    created_before: Optional[int] = None,       # scope=date_range
    updated_after: Optional[int] = None,        # scope=date_range
    updated_before: Optional[int] = None,       # scope=date_range
    status: Optional[str] = None,               # scope=status (failed|outdated|pending)
    
    # 옵션
    fields: Optional[List[str]] = None,     # 특정 필드만 재계산 (null = 모든 필드)
    force: bool = False,                    # 강제 재인덱싱 (content_hash 무시)
    batch_size: int = 100,                  # 배치 처리 단위
    parallel: bool = True,                  # 병렬 처리 여부
    dry_run: bool = False,                  # 시뮬레이션 모드 (실제 저장 안함)
    
    # 콜백/진행률
    progress_callback: Optional[Callable] = None,  # 진행률 콜백
    timeout_per_doc: int = 30,              # 문서당 타임아웃 (초)
) -> ReindexResult
```

#### **응답 형식**

```python
@dataclass
class ReindexResult:
    project: str
    scope: str
    target_count: int              # 처리 대상 문서 수
    success_count: int             # 성공
    failed_count: int              # 실패
    skipped_count: int             # 스킵 (변경 없음)
    duration_seconds: float        # 소요 시간
    errors: List[ReindexError]     # 오류 상세
    
    # 메타데이터
    fields_updated: List[str]      # 업데이트된 필드들
    timestamp: int                 # 재인덱싱 완료 시각 (Unix)
    batch_details: Optional[List[BatchDetail]]  # 배치별 통계

@dataclass
class ReindexError:
    document_id: str
    error_type: str                # parsing_error, plugin_error, timeout, etc.
    error_message: str
    stacktrace: Optional[str]

@dataclass
class BatchDetail:
    batch_num: int
    documents_processed: int
    duration_seconds: float
    error_count: int
```

### 2.3 사용 예시

#### **예시 1: DateTime 필드 백필 (전체 프로젝트)**

```python
result = doxus_reindex_documents(
    project="AI 리포트 챗봇",
    scope="full",
    fields=["created_at", "updated_at"],  # 이 필드들만 재계산
    batch_size=50,
    parallel=True
)

# 출력
ReindexResult(
    project="AI 리포트 챗봇",
    target_count=1250,
    success_count=1248,
    failed_count=2,
    skipped_count=0,
    duration_seconds=185.3,
    fields_updated=["created_at", "updated_at"],
    errors=[
        ReindexError(
            document_id="4760404036",
            error_type="plugin_error",
            error_message="Confluence API 접근 불가"
        ),
        ...
    ]
)
```

#### **예시 2: 특정 문서 1개만 재인덱싱**

```python
result = doxus_reindex_documents(
    project="doxus",
    scope="document",
    document_id="4813520900",
    force=True  # content_hash 무시하고 강제 재인덱싱
)

# 출력 (빠름!)
ReindexResult(
    target_count=1,
    success_count=1,
    duration_seconds=0.8,
    ...
)
```

#### **예시 3: Confluence 특정 공간만**

```python
result = doxus_reindex_documents(
    project="Brain",
    scope="plugin",
    plugin="confluence",
    plugin_param={"space_key": "AP"},  # AP 공간만
    dry_run=True  # 미리 확인
)

# 드라이런 결과로 영향받을 문서 수 확인 후 실행
```

#### **예시 4: 최근 3일 생성 문서만**

```python
import time

three_days_ago = int(time.time()) - (3 * 24 * 60 * 60)
result = doxus_reindex_documents(
    project="Brain",
    scope="date_range",
    created_after=three_days_ago,
    created_before=int(time.time()),
    parallel=True
)
```

#### **예시 5: 실패한 문서만 재인덱싱 (오류 복구)**

```python
result = doxus_reindex_documents(
    project="doxus",
    scope="status",
    status="failed",  # 이전 재인덱싱에서 실패한 문서
    force=True,
    progress_callback=lambda current, total: print(f"{current}/{total}")
)
```

---

## 3. 기술 구현 범위

### 3.1 데이터베이스 스키마 확장

```sql
-- 재인덱싱 추적 테이블 (신규)
CREATE TABLE reindex_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project TEXT NOT NULL,
    scope TEXT NOT NULL,              -- full|document|plugin|date_range|status
    target_count INTEGER,
    success_count INTEGER,
    failed_count INTEGER,
    skipped_count INTEGER,
    duration_seconds REAL,
    start_time INTEGER,               -- Unix timestamp
    end_time INTEGER,
    errors_json TEXT,                 -- JSON 직렬화
    created_at INTEGER DEFAULT (strftime('%s', 'now'))
);

-- 문서 인덱싱 상태 추적 (확장)
ALTER TABLE documents ADD COLUMN last_reindex_at INTEGER;  -- 마지막 재인덱싱 시각
ALTER TABLE documents ADD COLUMN reindex_status TEXT DEFAULT 'success';  -- success|failed|pending|outdated
ALTER TABLE documents ADD COLUMN reindex_error_json TEXT;  -- 오류 상세

-- 인덱싱 인덱스 개선
CREATE INDEX idx_documents_reindex_status ON documents(reindex_status);
CREATE INDEX idx_documents_last_reindex ON documents(last_reindex_at DESC);
CREATE INDEX idx_reindex_history_project ON reindex_history(project, end_time DESC);
```

### 3.2 Rust 구현 구조

```rust
// reindex.rs (신규 모듈)

pub struct ReindexRequest {
    pub project: String,
    pub scope: ReindexScope,
    pub scope_params: ReindexScopeParams,
    pub options: ReindexOptions,
}

pub enum ReindexScope {
    Full,
    Document(String),
    Documents(Vec<String>),
    Plugin { name: String, params: Map<String, Value> },
    DateRange { created_after: Option<i64>, created_before: Option<i64>, ... },
    Status(String),  // failed, pending, outdated
}

pub struct ReindexOptions {
    pub fields: Option<Vec<String>>,  // null = 모든 필드
    pub force: bool,
    pub batch_size: usize,
    pub parallel: bool,
    pub dry_run: bool,
    pub timeout_per_doc: u64,
}

pub struct ReindexResult {
    pub project: String,
    pub scope: String,
    pub target_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub duration_seconds: f64,
    pub errors: Vec<ReindexError>,
    pub fields_updated: Vec<String>,
    pub timestamp: i64,
}

// 핵심 함수
pub async fn reindex_documents(req: ReindexRequest) -> Result<ReindexResult>
pub async fn reindex_document_single(doc_id: String, force: bool) -> Result<()>
pub async fn reindex_documents_batch(doc_ids: Vec<String>, options: ReindexOptions) -> Result<ReindexResult>
```

### 3.3 플러그인 인터페이스 개선

```rust
// 플러그인이 구현해야 하는 trait
pub trait ContentIndexer {
    // 기존
    async fn index_document(&self, doc_id: String) -> Result<DocumentMetadata>;
    
    // 신규 (선택사항)
    async fn reindex_document(&self, doc_id: String, force: bool) -> Result<DocumentMetadata> {
        // 기본 구현: index_document 호출
        self.index_document(doc_id).await
    }
    
    // 신규 (선택사항): 범위 기반 재인덱싱 (성능 최적화)
    async fn reindex_documents_batch(
        &self, 
        doc_ids: Vec<String>,
        options: Map<String, Value>
    ) -> Result<Vec<DocumentMetadata>> {
        // 기본 구현: 1개씩 순회
        let mut results = vec![];
        for id in doc_ids {
            results.push(self.reindex_document(id, false).await?);
        }
        Ok(results)
    }
    
    // 신규 (선택사항): 플러그인별 재인덱싱
    async fn reindex_all(
        &self,
        params: Map<String, Value>  // space_key 등
    ) -> Result<Vec<String>> {
        // 해당 플러그인의 모든 doc_id 반환
        todo!()
    }
}
```

### 3.4 진행률 추적 (선택사항)

```rust
pub struct ProgressTracker {
    pub current: usize,
    pub total: usize,
    pub duration_so_far: f64,
    pub estimated_remaining: f64,
    pub throughput: f64,  // docs/sec
}

// 콜백 함수 타입
pub type ProgressCallback = Box<dyn Fn(ProgressTracker) + Send + Sync>;
```

---

## 4. 구현 전략

### 4.1 Phase 1: 단순 재인덱싱 (1주)

- [ ] DB 스키마 확장
- [ ] `reindex_documents` API 구현 (scope=document, documents)
- [ ] 테스트 작성

**deliverable**: 특정 문서 1~N개 재인덱싱 기능

### 4.2 Phase 2: 플러그인별 재인덱싱 (1주)

- [ ] scope=plugin 구현
- [ ] Confluence, GitHub, 로컬 파일 지원
- [ ] 플러그인별 최적화 옵션 (space_key 등)

**deliverable**: "Confluence AP 공간만 재인덱싱" 같은 기능

### 4.3 Phase 3: 범위 기반 재인덱싱 (1주)

- [ ] scope=date_range 구현
- [ ] scope=status 구현
- [ ] created_at, updated_at 필드 기반 쿼리

**deliverable**: DateTime 백필 기능

### 4.4 Phase 4: 진행률 & 안정화 (1주)

- [ ] 진행률 콜백 추가
- [ ] 드라이런 모드
- [ ] 배치별 오류 처리
- [ ] 타임아웃 관리

**deliverable**: 대규모 재인덱싱 안정적 실행

---

## 5. 위험 및 완화 전략

| 위험 | 영향 | 완화 전략 |
|------|------|----------|
| **DB 잠금** | 전체 시스템 먹통 | 배치 처리 + 트랜잭션 분할 |
| **메모리 초과** | OOM으로 크래시 | batch_size 제한, 스트리밍 처리 |
| **플러그인 타임아웃** | 문서 손실 | timeout_per_doc + 재시도 로직 |
| **부분 실패** | 불완전한 인덱스 | 트랜잭션 롤백, reindex_status 추적 |
| **API 레이트 리미트** | Confluence/GitHub 차단 | rate limiting 준수, 재시도 전략 |

---

## 6. 테스트 전략

### 6.1 단위 테스트

```rust
#[test]
async fn test_reindex_single_document() {
    // scope=document 검증
}

#[test]
async fn test_reindex_multiple_documents() {
    // scope=documents 검증
}

#[test]
async fn test_reindex_by_date_range() {
    // scope=date_range 검증
}

#[test]
async fn test_reindex_with_dry_run() {
    // dry_run=true 검증 (실제 저장 안함)
}

#[test]
async fn test_reindex_partial_failure() {
    // 일부 실패 처리 검증
}
```

### 6.2 통합 테스트

```rust
#[test]
async fn test_reindex_full_project() {
    // scope=full로 전체 프로젝트 재인덱싱

#[test]
async fn test_reindex_confluence_space() {
    // scope=plugin(confluence) + space_key 검증
}

#[test]
async fn test_reindex_github_org() {
    // scope=plugin(github) + org 검증
}

#[test]
async fn test_reindex_datetime_backfill() {
    // DateTime 필드 백필 시뮬레이션
}
```

### 6.3 성능 테스트

```rust
#[test]
async fn bench_reindex_1000_documents() {
    // 1000개 문서 재인덱싱 소요 시간 측정
}

#[test]
async fn bench_reindex_parallel_vs_sequential() {
    // 병렬 vs 순차 성능 비교
}
```

---

## 7. 마이그레이션 계획 (DateTime PR 병합 후)

### 시나리오: created_at, updated_at 필드 추가

**Step 1**: DateTime PR 병합 후
```bash
doxus_reindex_documents(
    project="AI 리포트 챗봇",
    scope="full",
    fields=["created_at", "updated_at"]
)
```

**Step 2**: 진행률 확인
```
[============================] 1250/1250 documents (100%) - 3m 45s
Success: 1248 | Failed: 2 | Skipped: 0
```

**Step 3**: 실패 문서만 재인덱싱
```bash
doxus_reindex_documents(
    project="AI 리포트 챗봇",
    scope="status",
    status="failed",
    force=True
)
```

---

## 8. 로드맵

| Phase | 기간 | 내용 | 의존성 |
|-------|------|------|--------|
| **Phase 1** | 1주 | 단일/다중 문서 재인덱싱 | - |
| **Phase 2** | 1주 | 플러그인별 재인덱싱 | Phase 1 |
| **Phase 3** | 1주 | 범위 기반 재인덱싱 | Phase 1-2, datetime-metadata PR |
| **Phase 4** | 1주 | 진행률 & 안정화 | Phase 1-3 |
| **배포** | 2026-05-21 | 프로덕션 | Phase 1-4 |

---

## 9. 예상 효과

| 작업 | 현재 | 개선 후 | 향상도 |
|------|------|--------|--------|
| **한 문서 오류 복구** | 전체 재인덱싱 (수십 분) | 단일 문서 (1초) | ✅ 100배 빠름 |
| **DateTime 백필** | 불가능 | `scope="full", fields=["created_at"]` | ✅ 새 기능 |
| **Confluence 공간만 재인덱싱** | 전체 (모든 공간) | `plugin="confluence", space_key="AP"` | ✅ 새 기능 |
| **배치 메타데이터 수정** | 반복 재인덱싱 | 한 번에 처리 | ✅ 효율성 증가 |
| **진행률 모니터링** | 불가능 | progress_callback으로 실시간 추적 | ✅ 새 기능 |

---

## 10. 연결 문서

- [DateTime 메타데이터 및 날짜 기반 검색 기능](enhancement-datetime-metadata-search.md)
- [doxus 구현 로드맵](doxus-implementation-roadmap.md)
- [doxus 미구현 항목 트래킹](doxus-unimplemented-items.md)

---

## 11. 변경 이력

| 날짜 | 버전 | 변경 사항 | 작성자 |
|------|------|----------|--------|
| 2026-04-23 | v1.0 | 초안 작성 | Claude |

---

## Appendix: CLI 커맨드 예시

```bash
# 단일 문서
doxus reindex --project "doxus" --document-id "4813520900"

# 전체 프로젝트
doxus reindex --project "Brain" --scope full --batch-size 50

# 특정 Confluence 공간
doxus reindex --project "AP" --scope plugin --plugin confluence --space-key "AP"

# 실패 문서만 (오류 복구)
doxus reindex --project "Brain" --scope status --status failed --force

# 드라이런 (미리 확인)
doxus reindex --project "doxus" --scope full --dry-run

# 진행률 표시
doxus reindex --project "Brain" --scope full --progress
```

---

## Appendix: MCP API 사용 예시

```python
from doxus_mcp import DoxusClient

client = DoxusClient()

# 예시 1: 특정 문서
result = client.reindex_documents(
    project="doxus",
    scope="document",
    document_id="4813520900"
)
print(f"Success: {result.success_count}/{result.target_count}")

# 예시 2: DateTime 백필
result = client.reindex_documents(
    project="AI 리포트 챗봇",
    scope="full",
    fields=["created_at", "updated_at"]
)
print(f"Duration: {result.duration_seconds:.1f}s")

# 예시 3: 진행률과 함께
def progress_callback(tracker):
    print(f"[{tracker.current}/{tracker.total}] {tracker.throughput:.1f} docs/sec")

result = client.reindex_documents(
    project="Brain",
    scope="full",
    progress_callback=progress_callback
)
```
