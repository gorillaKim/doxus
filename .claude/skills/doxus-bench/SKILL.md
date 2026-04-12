---
name: doxus-bench
description: doxus 검색 성능 벤치마킹. CLI/MCP 레이턴시(hyperfine), MRR@5 품질, 인덱스 건강도 측정 후 리포트 출력. 트리거: "doxus bench", "doxus benchmark", "doxus perf", "doxus 성능", "doxus 벤치", "doxus 벤치마크"
---

# doxus Benchmark 스킬

doxus CLI와 MCP 서버의 검색 성능·품질·인덱스 건강도를 측정하고 구조화된 리포트를 출력합니다.
**생산 DB (`~/.doxus/db/doxus.db`)를 읽기 전용으로 사용합니다 — 데이터 수정 없음.**

## 실행 절차

### Phase 0: 환경 점검

```bash
# 바이너리 탐색
DOXUS_BIN=$(which doxus 2>/dev/null \
  || ls "$HOME/.cargo/bin/doxus" 2>/dev/null \
  || find ~/gorillaProject/doxus/target/release -name doxus -maxdepth 1 2>/dev/null | head -1)

DOXUS_MCP_BIN=$(which doxus-mcp 2>/dev/null \
  || ls "$HOME/.cargo/bin/doxus-mcp" 2>/dev/null \
  || find ~/gorillaProject/doxus/target/release -name doxus-mcp -maxdepth 1 2>/dev/null | head -1)

PROD_DB="$HOME/.doxus/db/doxus.db"

# hyperfine 확인
HF=$(which hyperfine 2>/dev/null || echo "")
if [ -z "$HF" ]; then
  echo "[WARN] hyperfine not found — falling back to bash time (less accurate)"
  echo "Install: brew install hyperfine"
fi

# 생산 DB 상태 확인
DB_STATUS=$($DOXUS_BIN status 2>&1)
DOC_COUNT=$(echo "$DB_STATUS" | grep -oE '[0-9]+ doc' | head -1)
```

**사전 조건 실패 시 중단:**
- `$DOXUS_BIN` 없음 → 중단
- 생산 DB 없음 또는 0개 프로젝트 → 중단 (bench는 실제 데이터 필요)
- 문서 50개 미만 → WARN 출력 후 계속

### Phase 1: 환경 정보 수집

```bash
# 버전
DOXUS_VER=$($DOXUS_BIN --version 2>&1 | head -1 || echo "unknown")

# Embed 모드 (결과 해석에 중요)
EMBED_MODE=$($DOXUS_BIN status 2>&1 | grep -iE "onnx|fts.only|embed|hybrid" | head -1 || echo "unknown")

# 프로젝트/문서/청크 수
PROJECTS=$(sqlite3 "$PROD_DB" "SELECT COUNT(*) FROM projects WHERE status='active'" 2>/dev/null || echo "?")
DOCS=$(sqlite3 "$PROD_DB" "SELECT COUNT(*) FROM documents WHERE chunk_index=0" 2>/dev/null || echo "?")
CHUNKS=$(sqlite3 "$PROD_DB" "SELECT COUNT(*) FROM documents" 2>/dev/null || echo "?")
```

### Phase 2: 검색 레이턴시 벤치마크 (CLI)

생산 DB 타이틀에서 쿼리 5개를 자동 샘플링하고 고정 쿼리 5개를 추가합니다:

```bash
# 생산 DB 타이틀 샘플링 (쿼리 재료)
SAMPLED=$(sqlite3 "$PROD_DB" \
  "SELECT title FROM documents WHERE chunk_index=0 AND title IS NOT NULL ORDER BY RANDOM() LIMIT 5" 2>/dev/null)

# 고정 쿼리 5개 (항상 포함)
FIXED_QUERIES=(
  "검색 랭킹"
  "plugin configuration"
  "error handling"
  "존재하지않는단어XYZ"
  "API authentication"
)
```

**hyperfine 사용 시 (권장):**

```bash
BENCH_OUT=$(mktemp /tmp/doxus-bench-XXXXXX.json)

hyperfine \
  --warmup 3 \
  --runs 10 \
  --export-json "$BENCH_OUT" \
  --shell bash \
  "$DOXUS_BIN search '검색 랭킹'" \
  "$DOXUS_BIN search 'plugin configuration'" \
  "$DOXUS_BIN search 'error handling'" \
  "$DOXUS_BIN search '존재하지않는단어XYZ'" \
  "$DOXUS_BIN search 'API authentication'"

# JSON 파싱 (python3 또는 jq)
python3 -c "
import json, sys
data = json.load(open('$BENCH_OUT'))
for r in data['results']:
    cmd = r['command']
    mean_ms = r['mean'] * 1000
    p95 = sorted(r['times'])[int(len(r['times'])*0.95)] * 1000
    print(f'{cmd[:40]:40s} | mean={mean_ms:.1f}ms | p95={p95:.1f}ms')
"
```

**bash time 폴백 (hyperfine 없을 시):**

```bash
for q in "검색 랭킹" "plugin configuration" "error handling" "존재하지않는단어XYZ" "API authentication"; do
  START=$(python3 -c "import time; print(int(time.time()*1000))")
  $DOXUS_BIN search "$q" > /dev/null 2>&1
  END=$(python3 -c "import time; print(int(time.time()*1000))")
  echo "query='$q' time=$((END-START))ms"
done
```

### Phase 3: CLI vs MCP 레이턴시 비교

**참고: 두 측정 모두 cold-start(프로세스 시작) 포함 — 동등한 조건**

```bash
TEST_QUERY="error handling"

# CLI
if [ -n "$HF" ]; then
  CLI_RESULT=$(hyperfine --warmup 3 --runs 10 "$DOXUS_BIN search '$TEST_QUERY'" --export-json /tmp/cli-bench.json 2>&1)
fi

# MCP (initialize + tools/call pipe)
MCP_PIPE=$(printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}' \
  "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"doxus_search\",\"arguments\":{\"query\":\"$TEST_QUERY\"}}}")

if [ -n "$HF" ]; then
  MCP_RESULT=$(hyperfine --warmup 3 --runs 10 \
    "printf '$MCP_PIPE' | $DOXUS_MCP_BIN" \
    --export-json /tmp/mcp-bench.json 2>&1)
fi
```

### Phase 4: 검색 품질 — MRR@5

생산 DB 타이틀에서 ground truth 쿼리셋 구성:

```bash
# 타이틀 10개 샘플링
TITLES=$(sqlite3 "$PROD_DB" \
  "SELECT title FROM documents WHERE chunk_index=0 AND title IS NOT NULL LIMIT 10" 2>/dev/null)

# 각 타이틀의 핵심 단어(1~2개)로 검색 후 해당 문서가 top-5에 등장하는지 확인
# MRR = 각 쿼리의 (1/첫_등장_순위) 합계 / 쿼리 수
#
# 실행 방법:
# 1. 각 title에서 핵심 단어 추출 (첫 단어 또는 명사)
# 2. doxus search "<핵심단어>" 실행
# 3. 결과에서 원래 title이 몇 번째에 나오는지 기록
# 4. MRR = mean(1/rank), rank > 5이면 0
```

MRR@5가 0.5 미만이면 WARN, 0.3 미만이면 FAIL 수준으로 기록합니다.

### Phase 5: 인덱스 건강도 (생산 DB 읽기 전용)

```bash
sqlite3 "$PROD_DB" << 'SQL'
SELECT 'empty_content' as check_name,
       COUNT(*) as count
FROM documents
WHERE content = '' OR content IS NULL;

SELECT 'duplicate_hash' as check_name,
       COUNT(*) as count
FROM (
  SELECT content_hash
  FROM documents
  GROUP BY content_hash
  HAVING COUNT(*) > 1
);

SELECT 'orphan_chunks' as check_name,
       COUNT(*) as count
FROM documents d1
WHERE d1.chunk_index > 0
AND NOT EXISTS (
  SELECT 1 FROM documents d2
  WHERE d2.source_doc_id = d1.source_doc_id
  AND d2.project_id = d1.project_id
  AND d2.chunk_index = 0
);

SELECT 'avg_chunks_per_doc' as check_name,
       ROUND(CAST(COUNT(*) AS FLOAT) /
             NULLIF(COUNT(DISTINCT source_doc_id || '-' || project_id), 0), 1) as count
FROM documents;
SQL
```

건강도 판정:
- `empty_content > 0`: WARN
- `duplicate_hash > 0`: WARN
- `orphan_chunks > 0`: FAIL (인덱싱 버그 징후)
- `avg_chunks_per_doc < 1.0`: WARN (chunking 동작 안 함)

### Phase 6: 리포트 출력

```
## doxus Benchmark Report — {YYYY-MM-DD}

### Environment
- doxus: {경로} ({버전})
- doxus-mcp: {경로 또는 NOT FOUND}
- Embed mode: {FTS-only / hybrid(ONNX) / unknown}
  ⚠ FTS-only 모드: 벡터 검색 비활성 — 품질 지표 해석 시 참고
- Projects (active): {N} | Docs: {N} | Chunks: {N}
- hyperfine: {available vX.X / NOT FOUND (bash time 사용)}

### Search Latency — CLI (cold-start 포함)
| Query | Mean(ms) | P95(ms) | Min | Max |
|-------|----------|---------|-----|-----|
| "검색 랭킹" | 18.3 | 45.2 | 15.1 | 52.0 |
...

Overall: P50={N}ms | P95={N}ms

### Search Latency — MCP (cold-start 포함)
Overall: P50={N}ms | P95={N}ms
Overhead vs CLI: +{N}ms avg
(참고: 두 측정 모두 프로세스 시작 비용 포함)

### Search Quality (MRR@5)
쿼리 {N}개 기준 MRR@5: {0.XX}
(FTS-only 모드에서는 0.5~0.7이 일반적)
| Query | Target Doc | Rank | Reciprocal |
|-------|-----------|------|-----------|
...

### Index Health
| Check | Count | Status |
|-------|-------|--------|
| empty_content | 0 | OK |
| duplicate_hash | 2 | WARN |
| orphan_chunks | 0 | OK |
| avg_chunks_per_doc | 1.8 | OK |

### Issues
- [WARN] 2 documents share identical content_hash (possible duplicates)
- [INFO] FTS-only mode — ONNX embedding disabled
```

### Phase 7: 정리

```bash
rm -f /tmp/doxus-bench-*.json /tmp/cli-bench.json /tmp/mcp-bench.json
```

## 주의사항

- **생산 DB 읽기 전용** — `INSERT`/`UPDATE`/`DELETE` 절대 실행 금지
- hyperfine이 없으면 bash time 폴백 사용 (결과가 덜 정확함을 리포트에 명시)
- MCP가 없으면 CLI 벤치만 실행하고 MCP 섹션은 SKIP
- Embed mode는 결과 해석에 중요 — 항상 리포트 첫 줄에 명시
- 문서 50개 미만이면 MRR 샘플이 부족해 신뢰도 낮음 (WARN)
