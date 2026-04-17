# Doxus 데이터베이스 최적화 전략 (Database Storage Optimization Strategy)

## 1. 개요 (Overview)
Doxus 검색 시스템은 현재 SQLite FTS5와 벡터 검색을 병행하며 데이터를 저장하고 있습니다. 하지만 원본 문서 전체가 `documents` 테이블에 저장됨과 동시에, 검색 단위인 `chunks` 테이블에도 중복 저장되어 저장 용량이 기하급수적으로 늘어나는 문제가 발생하고 있습니다. 본 문서는 이를 해결하기 위한 데이터 다이어트 및 인덱스 최적화 전략을 기술합니다.

## 2. 핵심 최적화 전략 (Core Strategies)

### 2.1 데이터 다이어트 (Data Diet)
*   **원칙:** "원본 텍스트의 DB 내 영구 저장을 최소화한다."
*   **실행:** `documents.content` 필드를 비우고, 인덱싱 시점에 해시(`content_hash`)만 유지합니다. 원본 텍스트는 DB 외부(파일 시스템 또는 원격지)에서 관리하는 방향으로 전환합니다.

### 2.2 하이브리드 원문 조회 (On-Demand Retrieval)
원본 본문이 필요한 경우(예: `get_document`), 아래의 우선순위에 따라 데이터를 동적으로 가져오고 관리합니다.
1.  **로컬 파일:** `file_path`가 존재하는 Obsidian 등의 문서는 디스크에서 원문을 직접 읽습니다. (가장 정확하고 깨끗함)
2.  **원격지 실시간 Fetch:** 웹 소스(Confluence 등)의 경우 필요한 시점에 플러그인을 통해 원문을 가져옵니다.
3.  **지능형 캐싱 (TTL):** 가져온 원문은 `ContentCache` 테이블에 임시 보관하며, 플러그인별로 설정된 TTL(Time To Live)을 따릅니다.
4.  **폴백 (Fallback):** 소스 접근이 불가능할 경우, 복원(Reconstruction) 대신 가용한 청크(Context)들을 나열하여 사용자에게 정보를 제공합니다.

### 2.3 벡터 양자화 (Vector Quantization)
*   **대상 모델:** `multilingual-e5-small` (384차원)
*   **방안:** 기존 `FLOAT32` 벡터 데이터를 `int8`로 양자화하여 저장합니다.
*   **효과:** 정확도 손실을 1% 내외로 유지하면서 벡터 스토리지 용량을 약 75% 절감합니다.

### 2.4 전문 검색 인덱스 최적화 (FTS5 Advanced)
*   **구조:** `content='chunks'` 옵션을 활용한 **External Content Table** 방식을 유지하여 인덱스 테이블 자체의 데이터 중복을 0으로 관리합니다.
*   **튜닝:** 인덱싱 효율을 높이기 위해 검색 품질과 용량 사이의 균형을 맞춘 `detail` 옵션을 조정합니다.

## 3. 무결성 및 성능 관리 (Management)

### 3.1 삭제 연동 (Cascade Enforcement)
가상 테이블(벡터 인덱스, FTS5) 및 임시 캐시 데이터의 무결성을 위해 `ON DELETE CASCADE` 트리거를 강화합니다.
*   문서 삭제 시 연관된 벡터 행(`chunk_embeddings`) 및 캐시 행(`content_cache`) 즉시 삭제.

### 3.2 자동 정소 (Garbage Collection)
*   **Startup Cleanup:** 앱 시작 시 만료된 캐시 데이터를 소거하여 항상 쾌적한 상태를 유지합니다.
*   **Runtime Cleanup:** 동기화 스케줄러를 통해 주기적으로 쓰레기 데이터를 관리합니다.

## 4. 로드맵 (Roadmap)

1.  **Phase 1 (마이그레이션):** `V20`(데이터 비우기), `V21`(트리거 추가) 적용.
2.  **Phase 2 (코어 수정):** 인덱싱 코드(`index_document_sync`) 및 원문 조회 통합 서비스 개발.
3.  **Phase 3 (양자화):** 인덱싱 로직을 `int8` 기반으로 수정하고 전면 재인덱싱 실시.

---
> [!NOTE]
> 본 전략은 고수준 추론 에이전트의 기술 검토를 거쳐 설계되었습니다. 상세 구현 시 `multilingual-e5-small` 모델의 L2 정규화 특성을 최대한 활용합니다.
