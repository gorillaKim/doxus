# Enhancement: DateTime 메타데이터 및 날짜 기반 검색 기능

**작성일**: 2026-04-23  
**작성자**: Claude  
**상태**: Proposal  
**우선순위**: Medium  
**관련 이슈**: 날짜 기반 문서 검색 기능 부재

---

## 1. 문제 정의

### 1.1 현재 상황

doxus의 `doxus_list_documents`와 `doxus_get_metadata`에서 **날짜 정보가 메타데이터로 제공되지 않습니다**.

**현재 메타데이터 필드**:
```json
{
  "id": 23225,
  "source_id": "4813520900",
  "title": "AI 리포트 챗봇 소개 및 피드백 분석",
  "content_hash": "348d80f9...",
  "last_indexed": 1776900273  // Unix timestamp (인덱싱 시간, 생성시간 아님)
}
```

### 1.2 문제점

| 문제 | 영향 | 심각도 |
|------|------|--------|
| **생성 날짜 미제공** | 최신 문서 정확하게 찾기 불가 | 🔴 High |
| **수정 날짜 미제공** | 문서 업데이트 시점 파악 불가 | 🟡 Medium |
| **날짜 기반 쿼리 미지원** | "2026-04-20 이후 생성된 문서" 같은 검색 불가 | 🟡 Medium |
| **last_indexed만 존재** | 인덱싱 시간 ≠ 문서 작성 시간 | 🟡 Medium |

### 1.3 사용 사례

```
# 현재 불가능한 작업들
- "지난 주에 생성된 문서만 보여줘"
- "2026-04-20 이후 수정된 모든 문서"
- "최신 순으로 정렬된 문서 목록"
- "특정 기간 범위의 문서들"
```

---

## 2. 제안 (Solution)

### 2.1 메타데이터 확장

#### **doxus_get_metadata 응답에 추가**

```json
{
  "id": 23225,
  "source_id": "4813520900",
  "title": "AI 리포트 챗봇 소개 및 피드백 분석",
  "content_hash": "348d80f9...",
  "last_indexed": 1776900273,
  
  // ============ 신규 필드 (Unix timestamp만 제공) ============
  "created_at": 1776819000,    // Unix timestamp (초 단위)
  "updated_at": 1776900300     // Unix timestamp (초 단위)
}
```

#### **doxus_list_documents 응답에 추가**

```json
{
  "documents": [
    {
      "id": "4744904705",
      "title": "AI 리포트 챗봇 — 킥오프 인덱스 (2026-04-20)",
      
      // ============ 신규 필드 (Unix timestamp만 제공) ============
      "created_at": 1776646800,    // Unix timestamp
      "updated_at": 1776900300     // Unix timestamp
    }
  ],
  "next_cursor": null
}
```

### 2.2 검색 기능 확장

#### **doxus_search에 날짜 필터 파라미터 추가**

```python
doxus_search(
    query="AI 리포트",
    limit=20,
    
    # ============ 신규 파라미터 (필드명 통일: created_at, updated_at) ============
    created_at_after=1776819000,      # Unix timestamp
    created_at_before=1776900300,     # Unix timestamp (또는 ISO 8601 문자열 허용)
    updated_at_after=1776819000,
    updated_at_before=1776900300
)
```

#### **doxus_list_documents에 정렬 및 필터 옵션 추가**

```python
doxus_list_documents(
    project="AI 리포트 챗봇",
    limit=20,
    
    # ============ 신규 파라미터 (필드명 통일) ============
    sort_by="created_at",          # "created_at" | "updated_at" | "relevance"
    sort_order="desc",             # "asc" | "desc"
    created_at_after=1776819000,
    created_at_before=1776900300,
    updated_at_after=1776819000
)
```

### 2.3 사용 예시

**최신 문서 3개 조회**:
```python
doxus_list_documents(
    project="AI 리포트 챗봇",
    limit=3,
    sort_by="created_at",
    sort_order="desc"
)
```

**지난 7일 생성된 문서**:
```python
doxus_search(
    query="챗봇",
    created_at_after=1776233400  # 7일 전 Unix timestamp
)
```

**특정 기간에 수정된 문서**:
```python
doxus_list_documents(
    project="doxus",
    updated_at_after=1776028800,   # 2026-04-10
    updated_at_before=1776374400   # 2026-04-20
)
```

---

## 3. 기술 구현 범위

### 3.1 데이터 소스

| 출처 | created_at | updated_at | 비고 |
|------|-----------|-----------|------|
| **Confluence** | ✅ API 제공 | ✅ API 제공 | `created`, `version.when` |
| **GitHub** | ✅ API 제공 | ✅ API 제공 | `created_at`, `updated_at` |
| **로컬 파일** | ✅ 파일 시스템 | ✅ 파일 시스템 | `ctime`, `mtime` |
| **Notion** | ✅ API 제공 | ✅ API 제공 | `created_time`, `last_edited_time` |

### 3.2 구현 범위

#### **Phase 1: 메타데이터 저장 (1주)**
- [ ] 각 플러그인에서 created/updated 시간 추출
- [ ] SQLite 스키마에 `created_at`, `updated_at` 컬럼 추가
- [ ] 기존 문서들에 대해 백필 작업 (플러그인별 재인덱싱)
- [ ] 마이그레이션 스크립트 작성

#### **Phase 2: API 응답 확장 (1주)**
- [ ] `doxus_get_metadata` 응답에 날짜 필드 추가
- [ ] `doxus_list_documents` 응답에 날짜 필드 추가
- [ ] 기존 API 호출 호환성 유지 (하위 호환성)

#### **Phase 3: 검색 필터 구현 (1주)**
- [ ] `doxus_search`에 `created_after`, `created_before` 파라미터 추가
- [ ] `doxus_search`에 `updated_after`, `updated_before` 파라미터 추가
- [ ] `doxus_list_documents`에 `sort_by`, `sort_order` 파라미터 추가
- [ ] 필터 검증 로직 추가

#### **Phase 4: 테스트 및 문서화 (1주)**
- [ ] 단위 테스트 작성 (각 플러그인별)
- [ ] 통합 테스트 (date range filtering)
- [ ] API 문서 업데이트
- [ ] 마이그레이션 가이드 작성

### 3.3 아키텍처 변경

#### **SQLite 스키마 변경 (Unix timestamp 저장)**

```sql
-- 문서 메타데이터 테이블에 Unix timestamp 컬럼 추가
ALTER TABLE documents ADD COLUMN created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'));
ALTER TABLE documents ADD COLUMN updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'));

-- 인덱싱 (범위 검색 성능 최적화)
CREATE INDEX idx_documents_created_at ON documents(created_at);
CREATE INDEX idx_documents_updated_at ON documents(updated_at);
CREATE INDEX idx_documents_created_updated ON documents(created_at, updated_at);

-- 범위 검색을 위한 복합 인덱스
CREATE INDEX idx_documents_created_range ON documents(created_at DESC, id);
CREATE INDEX idx_documents_updated_range ON documents(updated_at DESC, id);
```

#### **플러그인 인터페이스 변경**

```rust
// 기존
pub struct DocumentMetadata {
    pub id: String,
    pub title: String,
    pub source_id: String,
    pub content_hash: String,
    pub last_indexed: i64,
}

// 개선 (필드명 통일, Unix timestamp만 제공)
pub struct DocumentMetadata {
    pub id: String,
    pub title: String,
    pub source_id: String,
    pub content_hash: String,
    pub last_indexed: i64,
    
    // ============ 신규 필드 (Unix timestamp) ============
    pub created_at: i64,   // Unix timestamp (초 단위)
    pub updated_at: i64,   // Unix timestamp (초 단위)
}
```

#### **검색 쿼리 빌더 개선 (필드명 통일)**

```rust
pub struct SearchFilter {
    pub query: String,
    
    // 날짜 필터 (필드명 통일: created_at, updated_at)
    pub created_at_after: Option<i64>,   // Unix timestamp
    pub created_at_before: Option<i64>,
    pub updated_at_after: Option<i64>,
    pub updated_at_before: Option<i64>,
    
    // 정렬 옵션
    pub sort_by: Option<SortField>,
    pub sort_order: Option<SortOrder>,
}

pub enum SortField {
    CreatedAt,
    UpdatedAt,
    Relevance,
}

pub enum SortOrder {
    Asc,
    Desc,
}
```

---

## 4. 예상 효과

### 4.1 개선 사항

| 기능 | 현재 | 개선 후 | 향상도 |
|------|------|--------|--------|
| **최신 문서 검색** | 타이틀 파싱 필요 | `sort_by="created_at"` 1줄 | ✅ 100% 개선 |
| **날짜 범위 검색** | ❌ 불가능 | `created_at_after=ts` 가능 | ✅ 새 기능 |
| **문서 정렬** | 고정 순서 | 날짜/관련성 유연한 정렬 | ✅ 새 기능 |
| **메타데이터 완전성** | 70% (last_indexed만) | 100% (created_at, updated_at) | ✅ 30% 증가 |
| **저장 공간** | - | Unix timestamp만 저장 (compact) | ✅ 효율성 |
| **쿼리 성능** | - | INTEGER 범위 검색 최적화 | ✅ 성능 |

### 4.2 사용자 경험 향상

```python
# ❌ Before: 타이틀에서 날짜 파싱 (불안정, 중복)
docs = doxus_list_documents(project="AI 리포트 챗봇")
latest = [d for d in docs if "2026-04-2" in d["title"]]

# ✅ After: 명확한 API (필드명 통일, Unix timestamp)
latest = doxus_list_documents(
    project="AI 리포트 챗봇",
    sort_by="created_at",
    sort_order="desc",
    limit=3
)

# 응답 예시
{
    "documents": [
        {
            "id": "4813520900",
            "title": "AI 리포트 챗봇 소개 및 피드백 분석",
            "created_at": 1776819000,    # Unix timestamp (클라이언트에서 필요시 변환)
            "updated_at": 1776900300
        }
    ]
}
```

---

## 5. 로드맵

| Phase | 기간 | 작업 | 담당 |
|-------|------|------|------|
| **1** | 1주 | 메타데이터 저장 (플러그인 개선) | Backend |
| **2** | 1주 | API 응답 확장 | API/Backend |
| **3** | 1주 | 검색 필터 구현 | Backend |
| **4** | 1주 | 테스트 & 문서화 | QA/Docs |
| **Total** | **4주** | 전체 구현 | - |

### 5.1 마일스톤

- **Week 1-2**: Phase 1-2 (메타데이터 기반 준비)
- **Week 3**: Phase 3 (검색 기능)
- **Week 4**: Phase 4 (테스트 & 배포)
- **배포**: 2026-05-21 (예상)

---

## 6. 하위 호환성 (Breaking Changes 없음)

### 6.1 기존 API 유지

```python
# 기존 코드 - 그대로 작동
doxus_list_documents(project="AI 리포트 챗봇", limit=20)

# 신규 파라미터는 선택사항
doxus_list_documents(
    project="AI 리포트 챗봇",
    limit=20,
    sort_by="created_at",  # 선택사항
    sort_order="desc"      # 선택사항
)
```

### 6.2 응답 포맷

```python
# 기존 응답 필드 유지 + 새 필드 추가
response = {
    "documents": [
        {
            "id": "4813520900",
            "title": "...",
            "created_at": "2026-04-22T10:30:00Z",  # 신규
            "updated_at": "2026-04-23T15:45:00Z"   # 신규
        }
    ]
}
```

---

## 7. 위험 분석 및 완화 전략

| 위험 | 영향 | 완화 전략 |
|------|------|----------|
| **플러그인별 시간대 차이** | 시간대 불일치 | UTC 표준화 + 문서화 |
| **백필 성능** | 인덱싱 속도 저하 | 배치 처리, 오프피크 시간 실행 |
| **기존 문서 시간 부정확** | 메타데이터 신뢰도 | "last_indexed"를 초기값으로 설정 |
| **DB 마이그레이션 실패** | 데이터 손실 | 백업 생성 + 롤백 계획 수립 |

---

## 8. 테스트 전략

### 8.1 단위 테스트

```python
def test_datetime_metadata_extraction():
    """각 플러그인에서 created/updated 추출 검증"""
    # Confluence, GitHub, 로컬 파일 등

def test_datetime_filtering():
    """날짜 범위 필터 검증"""
    # created_after, updated_before 등

def test_datetime_sorting():
    """날짜 기반 정렬 검증"""
    # sort_by="created_at", sort_order="desc"
```

### 8.2 통합 테스트

```python
def test_list_documents_by_date_range():
    """전체 프로젝트에서 날짜 범위 조회"""
    
def test_search_with_date_filter():
    """검색 + 날짜 필터 조합"""
    
def test_backward_compatibility():
    """기존 API 호출이 그대로 작동"""
```

---

## 9. 문서화 계획

- [ ] API 문서 업데이트 (`doxus_search`, `doxus_list_documents`)
- [ ] 마이그레이션 가이드 작성
- [ ] 사용 예시 추가 (최신 문서 찾기, 기간 검색 등)
- [ ] 플러그인 개발자 가이드 업데이트

---

## 10. 연결 문서

- [doxus 구현 로드맵](doxus-implementation-roadmap.md)
- [doxus 미구현 항목 트래킹](doxus-unimplemented-items.md)
- [doxus 검색 파이프라인 아키텍처](../architecture/search-pipeline.md)

---

## 11. 변경 이력

| 날짜 | 버전 | 변경 사항 | 작성자 |
|------|------|----------|--------|
| 2026-04-23 | v1.0 | 초안 작성 | Claude |

---

## Appendix: FAQ

**Q: Unix timestamp만 제공하면 클라이언트가 변환해야 하지 않나?**  
A: 네, 하지만 Unix timestamp는 저장 공간 절감, 계산 성능, 시간대 문제 해결에 최적. 클라이언트에서 필요시 간단히 ISO 8601로 변환 가능 (Python: `datetime.fromtimestamp(1776819000)`, JS: `new Date(1776819000 * 1000)`).

**Q: 파라미터도 Unix timestamp로만 입력해야 하나?**  
A: 주로 Unix timestamp를 권장하지만, 편의상 ISO 8601 또는 YYYY-MM-DD 문자열도 허용하고 내부에서 자동 변환.

**Q: created_at_after, created_at_before 같은 긴 이름이 필요한가?**  
A: REST API 표준이며, 명확성과 일관성 측면에서 최적. 필드명(created_at)과 범위 연산자(_after, _before) 분리로 직관적.

**Q: 백필 작업은 얼마나 걸리나?**  
A: 플러그인별 API 호출 속도에 따라 다름. Confluence는 Rate Limit 고려 필요. 대략 10,000개 문서당 1시간.

**Q: 기존 로컬 파일의 시간대는 정확한가?**  
A: 파일 시스템의 mtime을 사용하므로 신뢰도 높음. 파일 이동 시 변경될 수 있으므로 주의.

**Q: 클라이언트에서 ISO 8601로 변환할 때 시간대를 어떻게 처리하나?**  
A: 모든 timestamp는 UTC 기준. 클라이언트는 로컬 시간대로 자동 변환됨 (표준 라이브러리 사용).
