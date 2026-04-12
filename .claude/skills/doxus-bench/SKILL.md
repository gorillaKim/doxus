---
name: doxus-bench
description: doxus 검색 성능 벤치마킹. CLI/MCP 레이턴시(hyperfine), MRR@5 품질, 인덱스 건강도 측정 + grep/obsidian-nexus 3자 비교 리포트 출력. 트리거: "doxus bench", "doxus benchmark", "doxus perf", "doxus 성능", "doxus 벤치", "doxus 벤치마크"
---

# doxus Benchmark 스킬

doxus CLI와 MCP 서버의 검색 성능·품질·인덱스 건강도를 측정하고, **grep(베이스라인)** 및 **obsidian-nexus** 와 3자 비교 리포트를 출력합니다.

**생산 DB (`~/.doxus/db/doxus.db`)를 읽기 전용으로 사용합니다 — 데이터 수정 없음.**

## 실행 절차

### Phase 0: 환경 점검

```bash
# ── doxus 바이너리 탐색 ──────────────────────────────────────────
DOXUS_BIN=$(which doxus 2>/dev/null \
  || ls "$HOME/.local/bin/doxus" 2>/dev/null \
  || ls "$HOME/.cargo/bin/doxus" 2>/dev/null \
  || find ~/gorillaProject/doxus/target/release -name doxus -maxdepth 1 2>/dev/null | head -1)

DOXUS_MCP_BIN=$(which doxus-mcp 2>/dev/null \
  || ls "$HOME/.local/bin/doxus-mcp" 2>/dev/null \
  || ls "$HOME/.cargo/bin/doxus-mcp" 2>/dev/null \
  || find ~/gorillaProject/doxus/target/release -name doxus-mcp -maxdepth 1 2>/dev/null | head -1)

PROD_DB="$HOME/.doxus/db/doxus.db"

# ── hyperfine 확인 ───────────────────────────────────────────────
HF=$(which hyperfine 2>/dev/null || echo "")
if [ -z "$HF" ]; then
  echo "[WARN] hyperfine not found — falling back to bash time (less accurate)"
  echo "Install: brew install hyperfine"
fi

# ── 생산 DB 상태 확인 ────────────────────────────────────────────
DB_STATUS=$($DOXUS_BIN status 2>&1)
DOC_COUNT=$(echo "$DB_STATUS" | grep -oE '[0-9]+ doc' | head -1)

# ── 비교 대상 환경 점검 ──────────────────────────────────────────
GREP_BIN="/usr/bin/grep"

# 공통 벤치마크 볼트 (doxus + obsidian-nexus 양쪽에 인덱싱된 Brain vault)
COMMON_VAULT="$HOME/gorillaProject/brain"
SKIP_COMPARATIVE=false
if [ ! -d "$COMMON_VAULT" ]; then
  echo "[WARN] Common vault not found: $COMMON_VAULT — Comparative 섹션 SKIP"
  SKIP_COMPARATIVE=true
fi

# obsidian-nexus MCP 가용성: nexus_search MCP 도구를 테스트 호출로 확인
# (스킬 내에서 nexus_search(query="test", limit=1) 호출 후 결과 유무로 판정)
# NEXUS_AVAILABLE=true/false 를 아래 Phase 2.5/4/4.5에서 참조
```

**사전 조건 실패 시 중단:**
- `$DOXUS_BIN` 없음 → 중단
- 생산 DB 없음 또는 0개 프로젝트 → 중단 (bench는 실제 데이터 필요)
- 문서 50개 미만 → WARN 출력 후 계속
- `$COMMON_VAULT` 없음 → WARN 출력, Comparative 섹션 SKIP

---

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

# 공통 볼트 파일 수 (비교용)
VAULT_FILES=$(find "$COMMON_VAULT" -name "*.md" 2>/dev/null | wc -l | tr -d ' ')
```

---

### Phase 2: 검색 레이턴시 벤치마크 (doxus CLI 단독)

생산 DB 타이틀에서 쿼리 5개를 자동 샘플링하고 고정 쿼리 5개를 추가합니다:

```bash
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

python3 -c "
import json
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

---

### Phase 2.5: 3자 레이턴시 비교 (grep / nexus / doxus)

> SKIP 조건: `$SKIP_COMPARATIVE=true`

동일 쿼리 5개를 grep, obsidian-nexus, doxus로 실행하여 레이턴시를 비교합니다.
**공통 볼트(`$COMMON_VAULT`)를 대상으로 측정합니다.**

```bash
# 비교용 고정 쿼리셋 (한국어 2 + 영어 2 + 무관 1)
COMP_QUERIES=("검색 랭킹" "plugin configuration" "error handling" "API authentication" "존재하지않는단어XYZ")

# ── grep 레이턴시 ────────────────────────────────────────────────
# grep은 cold-start 없음 (시스템 바이너리), 파일 목록만 반환
declare -A GREP_TIMES
for q in "${COMP_QUERIES[@]}"; do
  if [ -n "$HF" ]; then
    RESULT=$(hyperfine --warmup 1 --runs 5 --export-json /tmp/grep-bench.json \
      "$GREP_BIN -r -l '$q' '$COMMON_VAULT'" 2>/dev/null)
    GREP_TIMES["$q"]=$(python3 -c "import json; d=json.load(open('/tmp/grep-bench.json')); print(f\"{d['results'][0]['mean']*1000:.1f}\")")
  else
    START=$(python3 -c "import time; print(int(time.time()*1000))")
    $GREP_BIN -r -l "$q" "$COMMON_VAULT" > /dev/null 2>&1
    END=$(python3 -c "import time; print(int(time.time()*1000))")
    GREP_TIMES["$q"]="$((END-START))"
  fi
done

# ── doxus CLI 레이턴시 ───────────────────────────────────────────
# (Phase 2에서 측정한 값 재사용 또는 재측정)
declare -A DOXUS_TIMES
for q in "${COMP_QUERIES[@]}"; do
  START=$(python3 -c "import time; print(int(time.time()*1000))")
  $DOXUS_BIN search "$q" > /dev/null 2>&1
  END=$(python3 -c "import time; print(int(time.time()*1000))")
  DOXUS_TIMES["$q"]="$((END-START))"
done

# ── obsidian-nexus 레이턴시 ─────────────────────────────────────
# MCP 도구 호출이므로 스킬 내에서 직접 측정:
# 각 쿼리에 대해:
#   1. python3 -c "import time; print(int(time.time()*1000))" 로 시작 시간 기록
#   2. nexus_search(query=q, mode="hybrid", limit=5) MCP 도구 호출
#   3. 종료 시간 기록 → elapsed 계산
# NEXUS_AVAILABLE=false 시 해당 셀은 "N/A"
```

리포트용 테이블 데이터를 수집합니다. 각 엔진의 평균 레이턴시를 계산하고 Winner를 결정합니다.

---

### Phase 3: doxus CLI vs MCP 레이턴시 비교

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

---

### Phase 4: 검색 품질 비교 — MRR@5 / Recall@5 / Precision@5

> 기본: doxus 단독 MRR@5 측정
> 비교: `$SKIP_COMPARATIVE=false` 이면 grep/nexus/doxus 3자 비교 추가

**Ground truth 구성 (Brain 볼트 기준):**

```bash
# Brain 프로젝트 ID 확인
BRAIN_PID=$(sqlite3 "$PROD_DB" \
  "SELECT id FROM projects WHERE name LIKE '%brain%' OR name LIKE '%Brain%' LIMIT 1" 2>/dev/null)

# Brain 볼트 타이틀 10개 샘플링 (ground truth)
GT_TITLES=$(sqlite3 "$PROD_DB" \
  "SELECT title FROM documents
   WHERE chunk_index=0 AND title IS NOT NULL
   AND project_id = $BRAIN_PID
   ORDER BY RANDOM() LIMIT 10" 2>/dev/null)

# BRAIN_PID 없으면 전체 DB에서 10개 샘플링
if [ -z "$BRAIN_PID" ]; then
  GT_TITLES=$(sqlite3 "$PROD_DB" \
    "SELECT title FROM documents WHERE chunk_index=0 AND title IS NOT NULL LIMIT 10" 2>/dev/null)
fi
```

**각 엔진별 검색 실행 및 rank 수집:**

```bash
# 각 title에서 핵심 단어 추출 (첫 단어 또는 공백 분리 2단어)
# 검색 후 결과에서 ground truth title이 몇 번째에 등장하는지 기록

# ── doxus ────────────────────────────────────────────────────────
# $DOXUS_BIN search "<핵심단어>" --limit 5
# 결과 타이틀 파싱 → rank 결정

# ── grep ─────────────────────────────────────────────────────────
# $GREP_BIN -r -l "<핵심단어>" "$COMMON_VAULT" | head -5
# 결과 파일명 → md 파일명 = 타이틀 (Obsidian 관행)
# rank 결정: 파일명에 ground truth title이 포함되면 해당 순위

# ── nexus ────────────────────────────────────────────────────────
# nexus_search(query="<핵심단어>", mode="hybrid", limit=5)
# 결과 title 목록에서 ground truth 매칭
```

**메트릭 계산 (python3):**

```python
def mrr_at_k(ranks, k=5):
    """ranks: list of int or None (None = not found in top-k)"""
    return sum(1.0/r if r and r <= k else 0.0 for r in ranks) / max(len(ranks), 1)

def recall_at_k(ranks, k=5):
    """relevant found in top-k / total queries"""
    return sum(1 for r in ranks if r and r <= k) / max(len(ranks), 1)

def precision_at_k(relevant_counts, k=5):
    """avg(relevant_in_top_k / k)"""
    return sum(c / k for c in relevant_counts) / max(len(relevant_counts), 1)

# 각 엔진의 ranks 리스트와 relevant_counts 리스트로 계산
# 결과를 리포트 테이블로 출력
```

MRR@5 판정:
- 0.5 이상: OK
- 0.3~0.5: WARN
- 0.3 미만: FAIL

---

### Phase 4.5: 노이즈 비율 측정

> SKIP 조건: `$SKIP_COMPARATIVE=true`

완전 무관한 쿼리에서 각 엔진이 얼마나 false positive를 반환하는지 측정합니다.

```bash
# 무관 쿼리 3개 (Brain 볼트에 절대 없을 단어)
NOISE_QUERIES=(
  "xyzzy플랑크톤quantum"
  "asdfghjkl1234567890"
  "zzz_nonexistent_topic_zzz"
)

for q in "${NOISE_QUERIES[@]}"; do
  # grep: 매칭 파일 수
  GREP_COUNT=$($GREP_BIN -r -l "$q" "$COMMON_VAULT" 2>/dev/null | wc -l | tr -d ' ')

  # doxus: 결과 건수
  DOXUS_COUNT=$($DOXUS_BIN search "$q" 2>/dev/null | grep -c "^" || echo "0")

  # nexus: nexus_search(query=q, limit=5) 결과 수
  # NEXUS_AVAILABLE=false 시 → "N/A"

  # 판정: 0건 = CLEAN, 1건+ = NOISY
  echo "| '$q' | ${GREP_COUNT} ($([ "$GREP_COUNT" = "0" ] && echo CLEAN || echo NOISY)) | ..."
done
```

노이즈 판정 기준:
- grep: 정확 매치이므로 일반적으로 0건 (CLEAN 기대)
- doxus/nexus: 시맨틱 검색이므로 일부 결과 가능 — 0건이면 임계값 필터 효과 있음

---

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

---

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
- Common vault: {경로} ({N} .md files)
- obsidian-nexus: {available / NOT AVAILABLE (Comparative 섹션 N/A)}
- grep: /usr/bin/grep

---

### Search Latency — doxus CLI (cold-start 포함)
| Query | Mean(ms) | P95(ms) |
|-------|----------|---------|
| "검색 랭킹" | 18.3 | 45.2 |
...
Overall: P50={N}ms | P95={N}ms

### Search Latency — doxus MCP (cold-start 포함)
Overall: P50={N}ms | P95={N}ms
Overhead vs CLI: +{N}ms avg

---

### Comparative: Latency (common vault 기준, 5 queries)
| Query | grep(ms) | nexus(ms) | doxus(ms) | Winner |
|-------|----------|-----------|-----------|--------|
| "검색 랭킹" | 120 | 85 | 22 | doxus |
| "plugin configuration" | 95 | 78 | 19 | doxus |
| "error handling" | 88 | 91 | 21 | doxus |
| "API authentication" | 102 | 79 | 18 | doxus |
| "존재하지않는단어XYZ" | 45 | 72 | 15 | doxus |
| **Average** | **90** | **81** | **19** | **doxus** |

### Comparative: Search Quality (Brain vault, 10 queries)
| Metric | grep | nexus | doxus | Winner |
|--------|------|-------|-------|--------|
| MRR@5 | 0.XX | 0.XX | 0.XX | X |
| Recall@5 | 0.XX | 0.XX | 0.XX | X |
| Precision@5 | 0.XX | 0.XX | 0.XX | X |

### Comparative: Noise (3 irrelevant queries)
| Query | grep | nexus | doxus |
|-------|------|-------|-------|
| "xyzzy플랑크톤quantum" | 0 (CLEAN) | N (NOISY/CLEAN) | N (NOISY/CLEAN) |
| "asdfghjkl..." | 0 (CLEAN) | N | N |
| "zzz_nonexistent..." | 0 (CLEAN) | N | N |

### Verdict
- **Latency**: doxus가 평균 Xms로 grep(Xms)/nexus(Xms) 대비 X배 빠름
- **Quality**: doxus MRR@5={X} vs nexus={X} vs grep={X}
  - nexus 대비 doxus: {+X% / -X% / 동등}
  - grep 대비 doxus: {+X% 향상} (시맨틱 검색 효과)
- **Noise**: doxus {0건 (CLEAN) / N건 (vector 임계값으로 필터됨)}
- **종합 효용성**: doxus는 grep 대비 검색 품질 X% 향상, nexus 대비 레이턴시 X배 우위

---

### Search Quality — doxus 단독 MRR@5
쿼리 {N}개 기준 MRR@5: {0.XX}
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

---

### Phase 7: 정리

```bash
rm -f /tmp/doxus-bench-*.json /tmp/cli-bench.json /tmp/mcp-bench.json /tmp/grep-bench.json
```

---

## 주의사항

- **생산 DB 읽기 전용** — `INSERT`/`UPDATE`/`DELETE` 절대 실행 금지
- hyperfine이 없으면 bash time 폴백 사용 (결과가 덜 정확함을 리포트에 명시)
- MCP가 없으면 CLI 벤치만 실행하고 MCP 섹션은 SKIP
- obsidian-nexus MCP가 없으면 Comparative 섹션의 nexus 열은 N/A 처리, 나머지는 정상 진행
- Embed mode는 결과 해석에 중요 — 항상 리포트 첫 줄에 명시
- 문서 50개 미만이면 MRR 샘플이 부족해 신뢰도 낮음 (WARN)
- grep은 정확 매치 / doxus·nexus는 시맨틱 검색 — 노이즈 비율 차이는 특성 차이임을 Verdict에 명시
