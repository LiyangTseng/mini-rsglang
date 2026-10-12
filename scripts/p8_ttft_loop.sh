#!/usr/bin/env bash
# Phase 8 (#14) feedback loop for the Rust frontend's P99 TTFT tail spike.
# Drives the real Rust frontend (rsg-server, release build) backed by
# mock-scheduler (rsg-mock-stack) with an S1-lite load -- 128 closed-loop
# agents, 25% client cancellations, real Qwen3-0.6B tokenizer -- and exits
# non-zero when TTFT P99/P50 exceeds --max-ratio in any round. The mock gives
# every request a fixed prefill delay with no queueing, so any tail above it
# is added outside the backend model. No GPU needed. Writes only under
# target/rsg-bench/p8-loop/<run>.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/p8_ttft_loop.sh [--agents 128] [--duration-s 10] [--runs 1]
                               [--max-ratio 10] [--port 31011] [--no-build]

Prints, per round: TTFT P50/P99, the TTFT of the first wave (requests sent
in the first --think-max-s, one per agent) vs the rest, then RED or GREEN.
Exits 0 only if every round is GREEN (P99/P50 <= --max-ratio), 1 if any
round is RED, 2 on bad usage or a harness failure.

Environment:
  PYTHON   interpreter for the report (default .venv/bin/python if present, else python3)
EOF
}

AGENTS=128
DURATION_S=10
RUNS=1
MAX_RATIO=10
PORT=31011
BUILD=1
THINK_MAX_S=0.5

while [ $# -gt 0 ]; do
  case "$1" in
    --agents) AGENTS="${2:?--agents needs a value}"; shift 2 ;;
    --duration-s) DURATION_S="${2:?--duration-s needs a value}"; shift 2 ;;
    --runs) RUNS="${2:?--runs needs a value}"; shift 2 ;;
    --max-ratio) MAX_RATIO="${2:?--max-ratio needs a value}"; shift 2 ;;
    --port) PORT="${2:?--port needs a value}"; shift 2 ;;
    --no-build) BUILD=0; shift ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [ -z "${PYTHON:-}" ]; then
  if [ -x .venv/bin/python ]; then PYTHON=.venv/bin/python; else PYTHON=python3; fi
fi

if [ "$BUILD" = 1 ]; then
  cargo build --release -p rsg-bench -p rsg-server >/dev/null
fi

OUT="target/rsg-bench/p8-loop/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$OUT/work"

STACK="target/release/rsg-mock-stack --rsg-server-bin target/release/rsg-server --mock-scheduler-bin target/release/mock-scheduler --port {port} --prefill-delay-ms 20 --decode-delay-ms 5 --rsg-server-arg=--port={port}"

if ! target/release/rsg-bench s1 --backend-kind mock --arms rust --runs "$RUNS" \
    --model-arg Qwen/Qwen3-0.6B --port "$PORT" \
    --python-cmd "$STACK" --rust-cmd "$STACK" --rust-frontend-process-name rsg-server \
    --gc-hook off --warmup-requests 2 --work-root "$OUT/work" \
    --agents "$AGENTS" --duration-s "$DURATION_S" --cancel-fraction 0.25 --max-tokens 256 \
    --think-max-s "$THINK_MAX_S" --requests-jsonl \
    --out "$OUT/s1.manifest.json" >"$OUT/bench.log" 2>&1; then
  echo "rsg-bench s1 failed; see $OUT/bench.log" >&2
  exit 2
fi

"$PYTHON" - "$OUT" "$MAX_RATIO" "$THINK_MAX_S" <<'PY'
import glob, json, os, sys

out, max_ratio, think_max_ms = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]) * 1000
doc = json.load(open(os.path.join(out, "s1.manifest.json")))
red = 0
for t in doc["trials"]:
    if t["status"] != "ok":
        print(f"round {t['round']}: trial status {t['status']!r}: {t.get('error')!r}")
        sys.exit(2)
    ttft = t["result"]["latency"]["ttft_ms"]
    p50, p99 = ttft["p50"], ttft["p99"]
    ratio = p99 / p50
    [path] = glob.glob(os.path.join(out, "work", "*", f"trial-{t['index']:02d}-*", "requests.jsonl"))
    rows = [json.loads(l) for l in open(path)]
    rows = [r for r in rows if r["ttft_ms"] is not None]
    first = sorted(r["ttft_ms"] for r in rows if r["send_ms"] < think_max_ms)
    rest = sorted(r["ttft_ms"] for r in rows if r["send_ms"] >= think_max_ms)
    med = lambda v: v[len(v) // 2] if v else float("nan")
    verdict = "RED" if ratio > max_ratio else "GREEN"
    red += verdict == "RED"
    print(
        f"round {t['round']}: ttft p50={p50:.0f}ms p99={p99:.0f}ms p99/p50={ratio:.1f} "
        f"| first wave n={len(first)} p50={med(first):.0f}ms max={max(first, default=float('nan')):.0f}ms "
        f"| rest n={len(rest)} p50={med(rest):.0f}ms max={max(rest, default=float('nan')):.0f}ms "
        f"| {verdict} (max ratio {max_ratio:g})"
    )
print(f"out: {out}")
sys.exit(1 if red else 0)
PY
