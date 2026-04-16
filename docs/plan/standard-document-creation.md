# Doxus 표준 문서 생성 및 계층 구조 지원 계획

## 1. 개요 (Overview)
본 문서는 Doxus 플랫폼의 모든 플러그인(Obsidian, Confluence, GitHub)에 적용될 문서 생성 표준을 정의합니다. 기존의 템플릿(doc_type) 기반 제약을 제거하고, 직관적인 제목, 본문, 그리고 계층형 폴더 구조를 지원하는 것을 핵심 목표로 합니다.

## 2. 주요 아키텍처 변경 사항

### 2.1 SDK 레벨 표준화
- **Target**: `crates/plugin-sdk/src/wasm_types.rs`
- **변경**: `CreateDocumentOptsWasm` 구조체에 `folder: Option<String>` 필드 추가.
- **의도**: 모든 플러그인이 동일한 계층 구조 데이터 모델을 공유하도록 강제함.

### 2.2 플러그인별 구현 전략
- **Obsidian**: 실제 파일 시스템의 경로로 매핑하며, 상위 디렉토리가 없을 경우 자동으로 생성(`mkdir -p`).
- **Confluence**: 상위 페이지 트리로 매핑. 재귀적 조회를 통해 부모 페이지를 자동 생성하여 계층 구조 유지.
- **GitHub**: 레포지토리 내 파일 경로로 매핑. 현재 Fetch 전용에서 쓰기(Create/Update) 지원으로 확장.

### 2.3 MCP 도구 간소화
- `doxus_create_document` 도구에서 `doc_type`에 대한 강제성을 제거.
- `folder` 인자를 추가하여 에이전트가 "Engineering/Design/Specs"와 같은 경로를 직접 처리할 수 있도록 함.

## 3. 구현 단계 (Phases)
1. **Phase 1 (SDK)**: 공통 데이터 타입 및 SDK 유틸리티 수정.
2. **Phase 2 (Core)**: MCP 서버의 도구 스키마 및 호출 브릿지 로직 반영.
3. **Phase 3 (Plugins)**: Obsidian -> Confluence -> GitHub 순으로 계층 구조 로직 구현 및 전사 배포.

## 4. 검증 계획
- 계층 구조가 정상적으로 시각화되는지 확인 (웹UI 및 각 서비스 플랫폼).
- 비어 있는 하위 경로에 대한 자동 생성 로직의 안정성 검증.
