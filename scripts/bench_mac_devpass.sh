#!/usr/bin/env bash
# D-03 Mac dev pass: exercises the Phase 7 harness end to end against the
# project's real Rust frontend (rsg-server), backed by mock-scheduler, on a
# GPU-free Mac -- before any GPU-box run. Both A/B arms launch the identical
# rsg-mock-stack command (R-vs-R mechanics, RESEARCH Pitfall 3): this proves
# the harness's loop modes, manifest/report writing, histogram decoding and
# client-cancellation-reaches-the-server path, never a Python-vs-Rust
# performance claim. All output stays under target/rsg-bench/devpass; never
# docs/benchmarks.
#
# Usage: scripts/bench_mac_devpass.sh   (no arguments)
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/bench_mac_devpass.sh

D-03 Mac dev pass: no arguments. Builds rsg-server and rsg-bench, then
drives rsg-server (backed by mock-scheduler, via rsg-mock-stack) through
s1 (cancellations), s2 (open and closed loop), s3 (hyperfine cold start),
asserts the written manifests, builds the combined report and asserts its
mock banner, then proves a client-aborted streaming request increments
rsg-server's /metrics cancellation counter. Prints "devpass: OK" and exits
0 only when every assertion passes. Writes only under
target/rsg-bench/devpass/<run>; never touches docs/benchmarks.

Environment:
  PYTHON   interpreter used for JSON assertions (default .venv/bin/python
           if present, else python3)
EOF
}

for arg in "$@"; do
  case "$arg" in
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [ -z "${PYTHON:-}" ]; then
  if [ -x .venv/bin/python ]; then PYTHON=.venv/bin/python; else PYTHON=python3; fi
fi

BENCH="target/debug/rsg-bench"
STACK="target/debug/rsg-mock-stack"
PORT=19191
MODEL="Qwen/Qwen3-0.6B"

echo "=== bench_mac_devpass: build ==="
cargo build -p rsg-server --bins -p rsg-bench

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="target/rsg-bench/devpass/$RUN_ID"
mkdir -p "$OUT" "$OUT/closed" "$OUT/work"
echo "out: $OUT"

# Both arms are the identical rsg-mock-stack command: R-vs-R mechanics,
# never a Python-vs-Rust comparison. `--rsg-server-arg=--port={port}` (the
# `=` form, not two tokens) is required: clap rejects a hyphen-prefixed
# value for a repeatable --long option unless passed as --long=value.
STACK_TPL="$STACK --port {port} --prefill-delay-ms 20 --decode-delay-ms 5 --rsg-server-arg=--port={port}"

COMMON_ARGS=(
  --backend-kind mock
  --model-arg "$MODEL"
  --port "$PORT"
  --python-cmd "$STACK_TPL"
  --rust-cmd "$STACK_TPL"
  --rust-frontend-process-name rsg-server
  --gc-hook off
  --warmup-requests 2
  --work-root "$OUT/work"
)

echo "=== bench_mac_devpass: s1 (cancellations) ==="
"$BENCH" s1 "${COMMON_ARGS[@]}" \
  --agents 16 --duration-s 10 --max-tokens 32 --cancel-fraction 0.5 --runs 2 \
  --out "$OUT/s1_cancel.manifest.json"

echo "=== bench_mac_devpass: s2 (open loop) ==="
"$BENCH" s2 "${COMMON_ARGS[@]}" \
  --mode open --rates 20,40 --requests-per-level 50 --runs 1 \
  --out "$OUT/s2_saturation.manifest.json"

echo "=== bench_mac_devpass: s3 (hyperfine cold start) ==="
"$BENCH" s3 "${COMMON_ARGS[@]}" \
  --hyperfine-runs 2 --hyperfine-warmup 1 --runs 1 \
  --python-backend-ready-marker "backend ready; handshake sent to rsg-server" \
  --out "$OUT/s3_coldstart.manifest.json"

echo "=== bench_mac_devpass: s2 (closed loop, separate dir) ==="
"$BENCH" s2 "${COMMON_ARGS[@]}" \
  --mode closed --concurrency 1,8 --requests-per-level 40 --runs 1 \
  --out "$OUT/closed/s2_saturation.manifest.json"

# --- Assertions: s1 ---------------------------------------------------------
"$PYTHON" - "$OUT/s1_cancel.manifest.json" <<'PY'
import json, sys
path = sys.argv[1]
doc = json.load(open(path))
arms = [t["arm"] for t in doc["trials"]]
expected = ["python-default", "rust", "python-default", "rust"]
assert arms == expected, f"s1 trial arm order {arms!r} != {expected!r}"
for t in doc["trials"]:
    assert t["status"] == "ok", f"s1 trial {t['arm']!r} round {t['round']}: status {t['status']!r}, error={t.get('error')!r}"
    counts = t["result"]["counts"]
    assert counts["completed"] > 0, f"s1 trial {t['arm']!r} round {t['round']}: completed={counts['completed']}"
    assert counts["cancelled"] > 0, f"s1 trial {t['arm']!r} round {t['round']}: cancelled={counts['cancelled']}"
print(f"OK: {path}")
PY

# --- Assertions: both s2 manifests (open + closed) --------------------------
for s2_manifest in "$OUT/s2_saturation.manifest.json" "$OUT/closed/s2_saturation.manifest.json"; do
  "$PYTHON" - "$s2_manifest" <<'PY'
import json, sys
path = sys.argv[1]
doc = json.load(open(path))
for t in doc["trials"]:
    assert t["status"] == "ok", f"s2 trial {t['arm']!r}: status {t['status']!r}, error={t.get('error')!r}"
    curve = t["result"]["curve"]
    assert len(curve) == 2, f"s2 trial {t['arm']!r}: curve has {len(curve)} points, want 2"
    offered = [p["offered"] for p in curve]
    assert offered == sorted(offered) and offered[0] < offered[1], f"s2 trial {t['arm']!r}: curve not ascending: {offered!r}"
print(f"OK: {path}")
PY
done

# --- Assertions: s3 ----------------------------------------------------------
"$PYTHON" - "$OUT/s3_coldstart.manifest.json" <<'PY'
import json, sys
path = sys.argv[1]
doc = json.load(open(path))
for t in doc["trials"]:
    assert t["status"] == "ok", f"s3 trial {t['arm']!r}: status {t['status']!r}, error={t.get('error')!r}"
    means = t["result"]["means"]
    assert means["e2e_ready_s"] is not None and means["e2e_ready_s"] > 0, f"s3 trial {t['arm']!r}: means.e2e_ready_s={means.get('e2e_ready_s')!r}"
    assert means["frontend_tail_s"] is not None, f"s3 trial {t['arm']!r}: means.frontend_tail_s is null"
print(f"OK: {path}")
PY

# --- Every histograms entry decodes: `report` reads every manifest in a ----
# --- dir, so a clean exit is the decode check for every histogram therein --
echo "=== bench_mac_devpass: report (histogram decode + mock banner) ==="
"$BENCH" report --manifests-dir "$OUT" --out-json "$OUT/report.json" --out-md "$OUT/report.md"
"$BENCH" report --manifests-dir "$OUT/closed" --out-json "$OUT/closed/report.json" --out-md "$OUT/closed/report.md"

first_summary_line="$(grep -m1 '^- ' "$OUT/report.md" || true)"
case "$first_summary_line" in
  *"NOT A FRONTEND COMPARISON"*"mock"*) ;;
  *)
    echo "report.md's first summary line missing NOT A FRONTEND COMPARISON / mock: ${first_summary_line:-<none>}" >&2
    exit 1
    ;;
esac
echo "OK: $OUT/report.md banner: $first_summary_line"

# --- docs/benchmarks was never touched ---------------------------------------
if [ -n "$(git -C "$ROOT" status --porcelain docs/benchmarks 2>/dev/null || true)" ]; then
  echo "docs/benchmarks was modified by this dev pass (prohibited):" >&2
  git -C "$ROOT" status --porcelain docs/benchmarks >&2
  exit 1
fi

# --- Cancellation reaching the server (LIFE-02 from the harness side) ------
echo "=== bench_mac_devpass: cancellation reaching the server ==="
CANCEL_PORT="$("$PYTHON" -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
CANCEL_BASE="http://127.0.0.1:$CANCEL_PORT"

target/debug/rsg-mock-stack \
  --port "$CANCEL_PORT" \
  --prefill-delay-ms 20 --decode-delay-ms 5 \
  "--rsg-server-arg=--port=$CANCEL_PORT" \
  >"$OUT/cancel-instance.log" 2>&1 &
CANCEL_PID=$!

cleanup_cancel_instance() {
  kill -TERM "$CANCEL_PID" 2>/dev/null || true
  wait "$CANCEL_PID" 2>/dev/null || true
}
trap cleanup_cancel_instance EXIT

deadline=$(( $(date +%s) + 20 ))
until curl -sf "$CANCEL_BASE/health/ready" >/dev/null 2>&1; do
  if ! kill -0 "$CANCEL_PID" 2>/dev/null; then
    echo "cancellation-test rsg-mock-stack instance exited before becoming ready; see $OUT/cancel-instance.log" >&2
    exit 1
  fi
  if [ "$(date +%s)" -gt "$deadline" ]; then
    echo "cancellation-test rsg-mock-stack instance did not become ready within 20s; see $OUT/cancel-instance.log" >&2
    exit 1
  fi
  sleep 0.2
done

read_cancelled_total() {
  curl -sf "$CANCEL_BASE/metrics" | awk '/^rsg_requests_cancelled_total /{print $2}'
}

before="$(read_cancelled_total)"
echo "rsg_requests_cancelled_total before: $before"

for i in 1 2 3; do
  curl -sN --max-time 0.2 -o /dev/null \
    -H 'Content-Type: application/json' \
    -d '{"model":"m","messages":[{"role":"user","content":"count slowly please"}],"max_tokens":512,"stream":true}' \
    "$CANCEL_BASE/v1/chat/completions" || true
done

sleep 1
after="$(read_cancelled_total)"
echo "rsg_requests_cancelled_total after: $after"

grew_by=$(( after - before ))
if [ "$grew_by" -lt 3 ]; then
  echo "rsg_requests_cancelled_total grew by $grew_by, want >= 3 (before=$before after=$after)" >&2
  exit 1
fi
echo "OK: cancellation counter grew by $grew_by"

cleanup_cancel_instance
trap - EXIT

echo
echo "devpass: OK"
