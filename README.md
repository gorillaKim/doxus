# doxus (도커스)

> **WASM 플러그인 기반의 다중 소스 통합 문서 검색 허브**

doxus는 Obsidian, Confluence, GitHub 등 흩어져 있는 지식 소스를 하나로 통합하여 강력한 검색과 AI 에이전트 인터렉션을 제공하는 **로컬 퍼스트(Local-First)** 검색 엔진입니다.

---

## 🏛 프로젝트 철학 (Philosophy)

doxus는 단순한 검색 도구를 넘어, AI 시대에 걸맞은 개인 및 팀 지식 관리의 새로운 표준을 지향합니다.

*   **Local First**: 모든 지식 인덱스는 사용자의 로컬 장치(`~/.doxus/db`)에 저장됩니다. 데이터 주권과 프라이버시를 최우선으로 하며, 오프라인에서도 빠른 검색을 보장합니다.
*   **WASM-Based Extensibility**: 플러그인 시스템은 WebAssembly(WASM) 샌드박스에서 실행됩니다. 이를 통해 안전하고 언어에 구애받지 않는 확장성을 제공하며, 누구나 자신만의 소스 어댑터를 개발할 수 있습니다.
*   **Agent Friendly**: 단순한 UI 제공을 넘어, AI 에이전트(Claude Code, Gemini 등)가 직접 doxus의 도구를 사용할 수 있도록 **MCP(Model Context Protocol)**를 완벽히 지원합니다.
*   **Hybrid Search Excellence**: 전통적인 키워드 검색(FTS5)과 현대적인 벡터 검색(Semantic Search)을 **RRF(Reciprocal Rank Fusion)** 알고리즘으로 결합하여 최상의 검색 품질을 제공합니다.

---

## ✨ 핵심 기능 (Key Features)

*   **통합 검색 (Hybrid Search)**: Obsidian 메모, Confluence 페이지, GitHub 이슈/위키를 한 번에 검색합니다.
*   **보안 샌드박스**: 모든 플러그인은 Extism WASM 런타임 내에서 엄격하게 통제된 권한(Host Functions)으로만 실행됩니다.
*   **지식 그래프 탐색**: 문서 간의 링크 관계를 분석하여 백링크, 최단 경로, 관련 문서 추천 기능을 제공합니다.
*   **사서 에이전트 (Sidecar)**: 데스크톱 앱 내부에 내장된 에이전트와 대화하며 복잡한 지식 베이스에서 답을 찾을 수 있습니다.
*   **플러그인 마켓플레이스**: 검증된 플러그인을 즉시 설치하고 관리할 수 있는 레지스트리 시스템을 갖추고 있습니다.

---

## 🚀 성능 및 아키텍처 (Performance)

doxus의 검색 파이프라인은 속도와 정확도를 위해 최적화되어 있습니다.

*   **Indexing**: 문서 청킹(1500자) 및 ONNX 기반 로컬 임베딩(`all-MiniLM-L6-v2`)을 통해 고속 인덱싱을 수행합니다.
*   **Hybrid Ranking**: 
    - **FTS5 (BM25)**: 키워드 매칭의 정확성 보장.
    - **Vector (KNN)**: 의미적 유사성 포착.
    - **RRF (k=60)**: 두 검색 결과를 통계적으로 병합하여 신뢰도 높은 순위 산출.
*   **Latency**: 로컬 SQLite 기반으로 수만 개의 청크에서도 밀리초 단위의 검색 속도를 유지합니다.

---

## 🛠 설치 방법 (Installation)

### 요구 사양
*   **Rust**: 1.75 버전 이상
*   **Node.js**: 20.x 버전 이상 (Desktop 앱 빌드용)

### 빌드 및 실행
1.  **저장소 클론**
    ```bash
    git clone https://github.com/gorillaKim/doxus.git
    cd doxus
    ```
2.  **의존성 설치**
    ```bash
    npm install
    ```
3.  **임베딩 모델 다운로드**
    ```bash
    ./scripts/download-model.sh
    ```
4.  **개발 모드 실행**
    ```bash
    cargo tauri dev
    ```

---

## 📖 사용 방법 (Usage)

1.  **플러그인 설치**: 마켓 페이지에서 필요한 소스(Confluence, GitHub 등)의 플러그인을 설치합니다.
2.  **프로젝트 추가**: 'Add Project' 메뉴를 통해 로컬 Obsidian 폴더 경로나 외부 서비스의 API 토큰을 설정합니다.
3.  **인덱싱**: 프로젝트 추가 후 인덱싱 버튼을 누르면 doxus가 문서를 분석하여 하이브리드 인덱스를 생성합니다.
4.  **검색 및 탐색**: 검색창에서 질문을 입력하거나, 특정 문서의 관계 그래프를 탐색합니다.
5.  **에이전트 활용**: 우측의 Chat Drawer를 열어 내 지식 베이스를 잘 알고 있는 전용 에이전트에게 질문하세요.

---

## 📄 라이선스 (License)

본 프로젝트는 **MIT License**에 따라 배포됩니다.
