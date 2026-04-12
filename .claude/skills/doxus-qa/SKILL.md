---
name: doxus-qa
description: doxus CLI와 MCP 서버 기능 검증. 시드 데이터 기반 21개 체크리스트 실행 후 pass/fail 리포트 출력. 트리거: "doxus qa", "doxus test", "doxus 테스트", "doxus 기능 확인", "doxus 검증"
---

# doxus QA 스킬

doxus CLI와 doxus-mcp 서버를 시드 데이터 기반으로 검증하고 구조화된 리포트를 출력합니다.

## 실행 절차

### Phase 0: 바이너리 탐색

다음 순서로 바이너리 경로를 결정합니다:

```bash
# doxus CLI
DOXUS_BIN=$(which doxus 2>/dev/null \
  || ls "$HOME/.cargo/bin/doxus" 2>/dev/null \
  || find ~/gorillaProject/doxus/target/release -name doxus -maxdepth 1 2>/dev/null | head -1)

# doxus-mcp
DOXUS_MCP_BIN=$(which doxus-mcp 2>/dev/null \
  || ls "$HOME/.cargo/bin/doxus-mcp" 2>/dev/null \
  || find ~/gorillaProject/doxus/target/release -name doxus-mcp -maxdepth 1 2>/dev/null | head -1)
```

`$DOXUS_BIN` 또는 `$DOXUS_MCP_BIN`이 없으면 해당 카테고리 전체를 SKIP 처리합니다.

### Phase 1: 시드 환경 구축 (필수 — 빈 DB 문제 방지)

```bash
QA_DIR=$(mktemp -d /tmp/doxus-qa-XXXXXX)
QA_DB="$QA_DIR/doxus-qa.db"
QA_VAULT="$QA_DIR/vault"
mkdir -p "$QA_VAULT"

# 시드 마크다운 5개 생성
cat > "$QA_VAULT/rust-error-handling.md" << 'EOF'
# Rust Error Handling
thiserror와 anyhow를 사용한 에러 처리 패턴.
라이브러리 크레이트는 thiserror, 바이너리는 anyhow를 사용한다.
EOF

cat > "$QA_VAULT/sqlite-fts5.md" << 'EOF'
# SQLite FTS5 전문 검색
FTS5는 BM25 랭킹을 기본으로 제공한다.
bm25() 함수는 음수 값을 반환하며 절대값이 클수록 관련도가 높다.
EOF

cat > "$QA_VAULT/wasm-plugin.md" << 'EOF'
# WASM 플러그인 시스템
Extism 기반 WASM 샌드박스로 외부 플러그인을 격리한다.
DocSource trait을 구현한 플러그인만 등록 가능하다.
EOF

cat > "$QA_VAULT/embedding-onnx.md" << 'EOF'
# ONNX 임베딩
all-MiniLM-L6-v2 모델로 384차원 벡터를 생성한다.
배치 인퍼런스를 지원하여 대량 문서 인덱싱 시 성능이 좋다.
EOF

cat > "$QA_VAULT/hybrid-search.md" << 'EOF'
# 하이브리드 검색
FTS5와 sqlite-vec 벡터 검색을 병렬 실행하고 RRF로 합산한다.
Reciprocal Rank Fusion은 이질적인 점수 스케일을 통일한다.
EOF

# 프로젝트 등록 + 인덱싱
DOXUS_DB_PATH="$QA_DB" $DOXUS_BIN project add qa-seed "$QA_VAULT"
DOXUS_DB_PATH="$QA_DB" $DOXUS_BIN index qa-seed
```

인덱싱 완료 후 `doxus status`로 chunk 수 확인. 0이면 index 실패로 CLI 검색 테스트 SKIP.

### Phase 2: Embed 모드 확인

```bash
EMBED_MODE=$(DOXUS_DB_PATH="$QA_DB" $DOXUS_BIN status 2>&1 | grep -iE "onnx|fts.only|embed|hybrid" | head -1 || echo "unknown")
```

리포트 환경 섹션에 기록합니다.

### Phase 3: 체크리스트 실행

각 항목을 순서대로 실행하고 PASS/FAIL/SKIP을 기록합니다.

#### CLI 읽기 테스트 (시드 DB 대상)

| # | 커맨드 | 통과 조건 |
|---|--------|----------|
| 1 | `DOXUS_DB_PATH=$QA_DB doxus status` | exit 0, doc/chunk 카운트 출력 |
| 2 | `DOXUS_DB_PATH=$QA_DB doxus project list` | exit 0, qa-seed 포함 |
| 3 | `DOXUS_DB_PATH=$QA_DB doxus search "rust error"` | exit 0, rust-error-handling 관련 결과 포함 |
| 4 | `DOXUS_DB_PATH=$QA_DB doxus search "bm25 검색"` | exit 0, sqlite-fts5 관련 결과 포함 |
| 5 | `DOXUS_DB_PATH=$QA_DB doxus search "존재하지않는쿼리XYZ"` | exit 0, crash 없음 |
| 6 | `DOXUS_DB_PATH=$QA_DB doxus plugin list` | exit 0 |
| 7 | `DOXUS_DB_PATH=$QA_DB doxus workspace list` | exit 0 (empty OK) |

#### CLI 에러 케이스 테스트

| # | 커맨드 | 통과 조건 |
|---|--------|----------|
| 8 | `doxus project add` (인자 없음) | exit non-0, usage 또는 에러 출력 |
| 9 | `DOXUS_DB_PATH=$QA_DB doxus project disable nonexistent-project` | exit non-0, 에러 메시지 출력 |
| 10 | `DOXUS_DB_PATH=$QA_DB doxus search ""` | exit 0 또는 non-0, crash(panic) 없음 |

#### CLI 쓰기 테스트 (시드 DB)

```bash
# 빈 vault 생성
mkdir -p "$QA_DIR/empty-vault"
```

| # | 커맨드 | 통과 조건 |
|---|--------|----------|
| 11 | `DOXUS_DB_PATH=$QA_DB doxus project add tmp-write $QA_DIR/empty-vault` | exit 0 |
| 12 | `DOXUS_DB_PATH=$QA_DB doxus project disable tmp-write` | exit 0 |
| 13 | `DOXUS_DB_PATH=$QA_DB doxus project enable tmp-write` | exit 0 |
| 14 | `DOXUS_DB_PATH=$QA_DB doxus project remove tmp-write` | exit 0, 목록에서 사라짐 |

#### MCP 테스트 (initialize 핸드셰이크 포함)

MCP 헬퍼 함수 — **반드시 initialize 먼저 전송**:

```bash
mcp_call() {
  local tool="$1"
  local args="${2:-{\}}"
  printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"doxus-qa","version":"1.0"}}}' \
    "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$tool\",\"arguments\":$args}}" \
    | DOXUS_DB_PATH="$QA_DB" "$DOXUS_MCP_BIN" 2>/dev/null
}

mcp_list() {
  printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"doxus-qa","version":"1.0"}}}' \
    '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
    | DOXUS_DB_PATH="$QA_DB" "$DOXUS_MCP_BIN" 2>/dev/null
}
```

| # | 테스트 | 통과 조건 |
|---|--------|----------|
| 15 | initialize 단독 응답 | `protocolVersion` 포함 응답 |
| 16 | `tools/list` | tools 배열, 33개 이상 항목 |
| 17 | `doxus_status` | project/doc 카운트 포함 응답 |
| 18 | `doxus_search {"query":"rust error"}` | result 포함 응답, 에러 없음 |
| 19 | `doxus_list_projects {}` | qa-seed 포함 응답 |
| 20 | `doxus_help {}` | 비어있지 않은 텍스트 응답 |
| 21 | 존재하지 않는 도구 `{"name":"doxus_nonexistent"}` | error response 반환, crash 없음 |

### Phase 4: 리포트 출력

```
## doxus QA Report — {YYYY-MM-DD}

### Environment
- doxus: {경로} ({버전 또는 unknown})
- doxus-mcp: {경로 또는 NOT FOUND}
- Embed mode: {FTS-only / hybrid(ONNX) / unknown}
- Seed vault: 5 docs indexed, {N} chunks

### Results
| # | Category | Test | Result | Detail |
|---|----------|------|--------|--------|
| 1 | CLI/read | status | PASS | 1 project, 5 docs, N chunks |
| 3 | CLI/search | "rust error" | PASS | rust-error-handling.md rank#1 |
| 9 | CLI/error | disable nonexistent | FAIL | exit 0 반환 (expected non-0) |
...

Summary: N/21 PASS, N SKIP, N FAIL

### Issues
- [FAIL] #{번호}: {설명}
- [WARN] {경고}
```

### Phase 5: 정리

```bash
rm -rf "$QA_DIR"
```

## 판정 기준

- **PASS**: 기대 조건 완전 충족
- **FAIL**: 기대와 불일치, crash(panic/segfault), 잘못된 exit code
- **SKIP**: 바이너리 없음, 또는 선행 단계(index) 실패로 실행 불가
- **WARN**: 동작하나 예상과 다른 부분 (예: exit 0 반환했지만 에러 없음 출력 시)

## 주의사항

- 생산 DB (`~/.doxus/db/doxus.db`) 절대 수정 금지 — 항상 `DOXUS_DB_PATH=$QA_DB` 사용
- 모든 테스트는 시드 데이터가 있는 임시 DB 대상
- 스킬 종료 시 반드시 `rm -rf /tmp/doxus-qa-*` 실행
