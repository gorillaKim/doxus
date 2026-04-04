# Git 워크플로우

## 커밋 컨벤션

### 타입 prefix

| prefix | 대상 |
|--------|------|
| `feat(core):` | 핵심 엔진 기능 (검색, 인덱싱, DB, 임베딩) |
| `feat(plugin):` | 플러그인 시스템, DocSource, WASM |
| `feat(desktop):` | Tauri + React UI |
| `feat(agent):` | 에이전트 sidecar, CLI 감지 |
| `feat(mcp):` | MCP 서버, 도구 추가 |
| `feat(cli):` | CLI 커맨드 |
| `refactor:` | 기능 변경 없는 리팩토링 |
| `test:` | 테스트 추가 또는 수정 |
| `fix:` | 버그 수정 |
| `docs:` | 설계 문서 (코드 주석 아님) |
| `chore:` | 빌드, 의존성, CI 설정 |

### Phase 태그 (커밋 메시지 본문에 포함)

```
feat(core): add EmbeddingProvider trait with ONNX backend

[Phase 0 - Track A] ONNX 임베딩 엔진 PoC
- OnnxEmbedder: all-MiniLM-L6-v2 모델 번들
- 배치 인퍼런스 지원
- OllamaEmbedder: fallback 구현
```

| Phase | 태그 |
|-------|------|
| 0 | `[Phase 0 - Track A]` / `[Phase 0 - Track B]` |
| 1 | `[Phase 1]` |
| 2a | `[Phase 2a]` |
| 2b | `[Phase 2b]` |
| 2c | `[Phase 2c]` |
| 2d | `[Phase 2d]` |
| 3~8 | `[Phase 3]` ~ `[Phase 8]` |

## 브랜치 전략

```
main
├── phase/0-onnx          # Phase 0 Track A (ONNX)
├── phase/0-extism        # Phase 0 Track B (Extism PoC)
├── phase/1-scaffold      # Phase 1 (모노레포 + Core 포팅)
├── phase/2a-docsource    # Phase 2a (DocSource + Obsidian)
└── ...
```

- `main`: 항상 빌드 가능한 상태
- Phase 브랜치: Phase 완료 후 squash merge → main
- 핫픽스: `fix/이슈명` 브랜치에서 main으로 직접 PR

## PR 단위

- PR 단위: Phase 트랙 단위 (하나의 Phase 트랙 = 하나의 PR)
- 대규모 Phase (예: Phase 4 마켓)는 서브 PR로 분할 가능
- PR 제목: `[Phase X] 커밋 타입: 핵심 내용`

## 태그 규칙

```
v0.1.0-phase2b    # Phase 2b MVP 완료 시점
v0.2.0-phase3     # Phase 3 완료
v1.0.0            # 전체 릴리즈
```

## 규칙

- `main`에 직접 push 금지 — 항상 PR
- CI 통과 전 merge 금지
- 설계 문서 변경은 `docs:` prefix, 코드와 별도 커밋
- `Cargo.lock` 는 커밋에 포함 (재현 가능한 빌드)
- `node_modules/`, `target/`, `dist/` 는 `.gitignore`
