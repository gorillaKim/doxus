<p align="center">
  <img src="apps/desktop/src/assets/doxus-logo-minimal.png" width="120" alt="doxus logo">
</p>

# doxus (도커스)

> **흩어져 있는 나의 모든 지식을 한 곳에서, 똑똑하게 검색하세요.**

doxus는 Obsidian, Confluence, GitHub 등 여러 곳에 저장된 나의 문서들을 하나로 통합하여, 필요할 때 즉시 찾아주는 **개인용 지식 검색 허브**입니다. 모든 데이터는 당신의 컴퓨터에만 저장되어 안전합니다.

---

## 🚀 빠른 시작 가이드 (Quick Start)

일반 사용자분들은 아래 단계에 따라 간편하게 시작하실 수 있습니다.

### 1단계: 앱 설치하기
*   **macOS 사용자**: [최신 버전 .dmg 다운로드](https://github.com/gorillaKim/doxus/releases) 후, 앱을 `Applications` 폴더로 드래그하세요.
*   **기타 플랫폼**: 현재 macOS를 우선 지원하며, 윈도우 및 리눅스 버전은 준비 중입니다.

### 2단계: '인공지능 두뇌' 설정하기 (최초 1회)
doxus의 핵심인 **'의미 기반 검색'**을 사용하기 위해서는 검색 엔진의 두뇌 역할을 하는 모델 파일이 필요합니다. 아래 명령어를 터미널에 복사해서 붙여넣기만 하면 자동으로 설정됩니다.

```bash
# 터미널을 열고 아래 한 줄을 복사해서 붙여넣으세요.
curl -fsSL https://raw.githubusercontent.com/gorillaKim/doxus/main/scripts/download-model.sh | bash
```
> [!TIP]
> **왜 수동으로 하나요?** 
> 개인정보 보호를 최우선으로 하는 doxus는 외부 서버를 거치지 않고 사용자 컴퓨터에서 직접 인공지능을 돌립니다. 이 과정에 필요한 수십 MB의 모델 파일을 리포지토리에 포함하지 않고 안전하게 직접 내려받도록 설계했습니다.

### 3단계: 지식 연결하고 검색하기
1.  앱을 실행하고 **'Add Project'** 버튼을 누르세요.
2.  나의 Obsidian 폴더 경로나 Confluence API 정보를 입력합니다.
3.  **'Index'** 버튼을 눌러 doxus가 지식을 학습하게 한 뒤, 검색창에서 원하는 내용을 찾아보세요!

---

## ✨ doxus가 특별한 이유

*   **진짜 로컬 퍼스트**: 당신의 메모가 클라우드에 올라갈까 봐 걱정하지 마세요. 모든 검색과 데이터 저장은 오직 당신의 기기 내에서만 이루어집니다.
*   **하이브리드 검색**: 단순히 단어만 찾는 것이 아니라, "그때 그 프로젝트에 대해서 쓴 거 어디 있지?"와 같은 질문의 **의미**를 이해하고 결과를 찾아줍니다.
*   **지식의 연결 고리 탐색**: 문서 간의 링크나 백링크를 분석하여 지식 사이의 관계를 시각적으로 보여줍니다.
*   **AI 에이전트 친구**: Claude나 Gemini 같은 AI 에이전트가 doxus의 도구를 사용하여 당신의 지식 베이스를 바탕으로 답변할 수 있습니다.

---

## 🛠 개발자 및 고급 사용자 가이드

직접 빌드하거나 명령줄(CLI)에서 사용하고 싶은 분들을 위한 매뉴얼입니다.

### 요구 사양
*   **Rust**: 1.75+
*   **Node.js**: 20.x+

### 빌드 및 실행
```bash
git clone https://github.com/gorillaKim/doxus.git
cd doxus
npm install

# 모델 다운로드
./scripts/download-model.sh

# 데스크톱 앱 실행
npm run tauri dev

# CLI 빌드 및 실행
cargo build --release --bin doxus
./target/release/doxus --help
```

---

## ⚙️ 작동 원리 (Deep Dive)

doxus는 최상의 검색 품질을 위해 다음 기술을 조합하여 사용합니다.

1.  **SQLite FTS5**: 키워드 기반의 빠른 정확도 보장.
2.  **ONNX Local Embedding**: 로컬 CPU를 활용한 고효율 벡터 생성.
3.  **RRF (Reciprocal Rank Fusion)**: 키워드 검색과 벡터 검색 순위를 통계적으로 병합하여 최적의 결과를 도출합니다.
4.  **WASM Plugin System**: Extism 기반의 샌드박스에서 안전하게 외부 소스(Confluence, GitHub 등)의 데이터를 가져옵니다.

---

## 🧩 에이전트 연동 (MCP)
doxus는 **MCP (Model Context Protocol)** 서버를 내장하고 있습니다. 
Claude Desktop 설정에 `doxus-mcp` 바이너리를 추가하면 에이전트가 `doxus_search`, `doxus_get_document` 등의 도구를 사용하여 당신의 지식을 활용할 수 있게 됩니다.

---

## 📄 라이선스 (License)
본 프로젝트는 **MIT License**를 따릅니다.
