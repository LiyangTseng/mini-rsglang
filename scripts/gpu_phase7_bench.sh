#!/usr/bin/env bash
# Phase 7 GPU-box wrapper (BENCH-02..BENCH-08): the one reproducible command
# that produces the headline Python-vs-Rust frontend comparison. Runs on the
# Linux GPU box, after Phase 6 (GPU End-to-End Parity) has landed; --help and
# --dry-run work anywhere, including this Mac.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phase7_bench.sh [--model Qwen/Qwen3-0.6B] [--port 1919] [--runs 5] [--candidates 0,1,2,4] [--out-dir docs/benchmarks/frontend-benchmarks] [--dry-run] [--help]

Run this on the Linux GPU box, after Phase 6 (GPU End-to-End Parity) has
landed. It runs preflight, the --num-tokenizer sweep, Scenario 1, Scenario 2
(open loop), the optional vllm/sglang cross-checks, Scenario 3, the standard
throughput check, the combined report, and check_upstream.py -- printing a
PASS/FAIL line per step. Exits 0 only if every step passed.

  preflight: nvidia-smi lists a GPU, hyperfine >= 1.19, the minisgl/openai/
             transformers/rsglang python imports, a release build
  1. sweep-num-tokenizer: picks the best Python --num-tokenizer setting
  2. s1 (BENCH-03), s2 --mode open (BENCH-04)
  3. crosscheck vllm / sglang -- only when VLLM_BIN / SGLANG_PYTHON are set
  4. s3 (BENCH-05), throughput (BENCH-06)
  5. report: writes <out-dir>.json and <out-dir>.md (BENCH-07/BENCH-08)
  6. scripts/check_upstream.py (the vendored tree was not touched)

Options:
  --model ID         model to serve (default Qwen/Qwen3-0.6B)
  --port N           HTTP port for every launched server (default 1919)
  --runs N           A/B rounds for every scenario except the sweep (default 5)
  --candidates LIST  comma-separated --num-tokenizer sweep candidates (default 0,1,2,4)
  --out-dir DIR      manifest directory and report base path; writes
                     DIR/*.manifest.json, DIR.json and DIR.md
                     (default docs/benchmarks/frontend-benchmarks)
  --dry-run          print every `RUN: ` command this script would run, then exit 0
  --help             show this help

Environment:
  PYTHON          interpreter to use (default .venv/bin/python if present, else python3)
  VLLM_BIN        path to a vllm CLI in its own venv; enables the vllm cross-check
  SGLANG_PYTHON   path to a python with sglang installed in its own venv; enables the sglang cross-check
  BACKEND_EXTRA   extra args applied to both the python and rust launch templates
  RUST_CMD_EXTRA  extra args applied to the rust launch template only (e.g. an abort-timing flag)

Exits 0 only if every step passed. Logs go to a fresh mktemp -d directory.
This script never runs env/printenv and never echoes variable values beyond
the paths and flags shown above (T-07-26).
EOF
}

MODEL="Qwen/Qwen3-0.6B"
PORT=1919
RUNS=5
CANDIDATES="0,1,2,4"
OUT_DIR="docs/benchmarks/frontend-benchmarks"
DRY_RUN=0

while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="${2:?--model needs a value}"; shift 2 ;;
    --port) PORT="${2:?--port needs a value}"; shift 2 ;;
    --runs) RUNS="${2:?--runs needs a value}"; shift 2 ;;
    --candidates) CANDIDATES="${2:?--candidates needs a value}"; shift 2 ;;
    --out-dir) OUT_DIR="${2:?--out-dir needs a value}"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [ -z "${PYTHON:-}" ]; then
  if [ -x .venv/bin/python ]; then PYTHON=.venv/bin/python; else PYTHON=python3; fi
fi
export PYTHONPATH="$ROOT/python${PYTHONPATH:+:$PYTHONPATH}"

BENCH=target/release/rsg-bench
BACKEND_EXTRA="${BACKEND_EXTRA:-}"
RUST_CMD_EXTRA="${RUST_CMD_EXTRA:-}"
VLLM_BIN="${VLLM_BIN:-}"
SGLANG_PYTHON="${SGLANG_PYTHON:-}"

# D-11/BACKEND_EXTRA applies to both arms; RUST_CMD_EXTRA is the Rust
# frontend's own configuration (e.g. an abort-timing setting Phase 6 settles)
# and never touches the Python template.
PYTHON_TEMPLATE="{python} -m rsglang.launch --frontend python --model {model} --port {port} --num-tokenizer {num_tokenizer} $BACKEND_EXTRA"
RUST_TEMPLATE="{python} -m rsglang.launch --frontend rust --model {model} --port {port} $BACKEND_EXTRA $RUST_CMD_EXTRA"

# --- --dry-run: print the exact command sequence, touching nothing --------------
if [ "$DRY_RUN" = 1 ]; then
  BEST="<best-from-sweep>"
  echo "RUN: cargo build --release -p rsg-bench -p rsg-server"
  echo "RUN: $BENCH sweep-num-tokenizer --runs 1 --candidates $CANDIDATES --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/num_tokenizer_sweep.manifest.json\""
  echo "RUN: $BENCH s1 --backend-kind real --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --runs $RUNS --python-best-num-tokenizer \"$BEST\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/s1_cancel.manifest.json\""
  echo "RUN: $BENCH s2 --mode open --backend-kind real --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --runs $RUNS --python-best-num-tokenizer \"$BEST\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/s2_saturation.manifest.json\""
  if [ -n "$VLLM_BIN" ]; then
    echo "RUN: $BENCH crosscheck --tool vllm --vllm-bin \"$VLLM_BIN\" --backend-kind real --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --runs 1 --python-best-num-tokenizer \"$BEST\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/crosscheck_vllm.manifest.json\""
  fi
  if [ -n "$SGLANG_PYTHON" ]; then
    echo "RUN: $BENCH crosscheck --tool sglang --sglang-python \"$SGLANG_PYTHON\" --backend-kind real --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --runs 1 --python-best-num-tokenizer \"$BEST\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/crosscheck_sglang.manifest.json\""
  fi
  echo "RUN: $BENCH s3 --backend-kind real --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --runs $RUNS --python-best-num-tokenizer \"$BEST\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/s3_coldstart.manifest.json\""
  echo "RUN: $BENCH throughput --backend-kind real --model-arg \"$MODEL\" --port \"$PORT\" --python \"$PYTHON\" --runs $RUNS --python-best-num-tokenizer \"$BEST\" --python-cmd \"$PYTHON_TEMPLATE\" --rust-cmd \"$RUST_TEMPLATE\" --out \"$OUT_DIR/standard_throughput.manifest.json\""
  echo "RUN: $BENCH report --manifests-dir \"$OUT_DIR\" --out-json \"$OUT_DIR.json\" --out-md \"$OUT_DIR.md\""
  echo "RUN: $PYTHON scripts/check_upstream.py"
  exit 0
fi

TMP_BASE="${TMPDIR:-/tmp}"
LOG_DIR="$(mktemp -d "${TMP_BASE%/}/gpu_phase7_bench.XXXXXX")"
echo "logs: $LOG_DIR"

declare -a RESULTS=()
FAILED=0

record() {  # record <step> <PASS|FAIL> <detail>
  RESULTS+=("$2 step $1: $3")
  echo "$2 step $1: $3"
}

print_summary_and_exit() {
  echo
  echo "=== Phase 7 GPU-box benchmark summary (logs: $LOG_DIR) ==="
  for r in "${RESULTS[@]}"; do
    echo "$r"
  done
  if [ "$FAILED" = 0 ]; then
    echo "ALL PASS"
    exit 0
  fi
  echo "SOME STEPS FAILED"
  exit 1
}

# version_ge A B -> 0 if A >= B, comparing dotted version strings field by
# field numerically (a missing field counts as 0).
version_ge() {
  local a="$1" b="$2"
  local -a af bf
  IFS='.' read -r -a af <<<"$a"
  IFS='.' read -r -a bf <<<"$b"
  local n=${#af[@]}
  [ ${#bf[@]} -gt "$n" ] && n=${#bf[@]}
  local i av bv
  for ((i = 0; i < n; i++)); do
    av="${af[i]:-0}"
    bv="${bf[i]:-0}"
    if [ "$av" -gt "$bv" ] 2>/dev/null; then return 0; fi
    if [ "$av" -lt "$bv" ] 2>/dev/null; then return 1; fi
  done
  return 0
}

# hyperfine_ok -> 0 if hyperfine is on PATH and at least 1.19.0.
hyperfine_ok() {
  if ! command -v hyperfine >/dev/null 2>&1; then
    echo "hyperfine not found on PATH"
    return 1
  fi
  local v
  v="$(hyperfine --version | awk '{print $2}')"
  if version_ge "$v" "1.19.0"; then
    return 0
  fi
  echo "hyperfine $v is older than 1.19.0; install 1.20.0: cargo install hyperfine --version 1.20.0 --locked"
  return 1
}

# --- Preflight -------------------------------------------------------------------
# A single failure records FAIL step preflight and exits 1 immediately,
# before any build or benchmark step runs.
if ! command -v nvidia-smi >/dev/null 2>&1 || ! nvidia-smi -L >"$LOG_DIR/preflight-gpu.log" 2>&1; then
  record preflight FAIL "nvidia-smi -L did not list a GPU (see $LOG_DIR/preflight-gpu.log)"
  FAILED=1
  print_summary_and_exit
fi

if ! hyperfine_ok >"$LOG_DIR/preflight-hyperfine.log" 2>&1; then
  record preflight FAIL "hyperfine missing or older than 1.19.0 (see $LOG_DIR/preflight-hyperfine.log)"
  FAILED=1
  print_summary_and_exit
fi

if ! "$PYTHON" -c "import minisgl, openai, transformers, rsglang" >"$LOG_DIR/preflight-import.log" 2>&1; then
  record preflight FAIL "python import failed: minisgl, openai, transformers, rsglang (see $LOG_DIR/preflight-import.log)"
  FAILED=1
  print_summary_and_exit
fi

if ! cargo build --release -p rsg-bench -p rsg-server >"$LOG_DIR/preflight-build.log" 2>&1; then
  record preflight FAIL "cargo build --release -p rsg-bench -p rsg-server (see $LOG_DIR/preflight-build.log)"
  FAILED=1
  print_summary_and_exit
fi

record preflight PASS "nvidia-smi, hyperfine, python imports and the release build are all ready"

mkdir -p "$OUT_DIR"

echo "model: $MODEL   port: $PORT   runs: $RUNS   candidates: $CANDIDATES   out-dir: $OUT_DIR"

# --- Step 1: --num-tokenizer sweep (D-09) ----------------------------------------
SWEEP_LOG="$LOG_DIR/sweep-num-tokenizer.log"
BEST=""
if "$BENCH" sweep-num-tokenizer \
    --runs 1 --candidates "$CANDIDATES" \
    --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
    --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
    --out "$OUT_DIR/num_tokenizer_sweep.manifest.json" \
    >"$SWEEP_LOG" 2>&1; then
  BEST="$(grep -o 'best_num_tokenizer=[0-9]\+' "$SWEEP_LOG" | tail -1 | cut -d= -f2)"
fi
if [ -z "$BEST" ]; then
  record sweep-num-tokenizer FAIL "no best_num_tokenizer= line in stdout (see $SWEEP_LOG)"
  FAILED=1
  print_summary_and_exit
fi
record sweep-num-tokenizer PASS "best_num_tokenizer=$BEST (see $SWEEP_LOG)"

run_bench_step() {  # run_bench_step <record-name> <rsg-bench subcommand+args...>
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  if "$BENCH" "$@" >"$log" 2>&1; then
    record "$name" PASS "see $log"
  else
    record "$name" FAIL "see $log"
    FAILED=1
  fi
}

# --- Step 2: Scenario 1 (BENCH-03), Scenario 2 open loop (BENCH-04) --------------
run_bench_step s1 s1 \
  --backend-kind real --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
  --runs "$RUNS" --python-best-num-tokenizer "$BEST" \
  --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
  --out "$OUT_DIR/s1_cancel.manifest.json"

run_bench_step s2 s2 --mode open \
  --backend-kind real --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
  --runs "$RUNS" --python-best-num-tokenizer "$BEST" \
  --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
  --out "$OUT_DIR/s2_saturation.manifest.json"

# --- Step 3: optional third-party cross-checks (D-04) ----------------------------
if [ -n "$VLLM_BIN" ]; then
  run_bench_step crosscheck_vllm crosscheck --tool vllm --vllm-bin "$VLLM_BIN" \
    --backend-kind real --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
    --runs 1 --python-best-num-tokenizer "$BEST" \
    --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
    --out "$OUT_DIR/crosscheck_vllm.manifest.json"
fi
if [ -n "$SGLANG_PYTHON" ]; then
  run_bench_step crosscheck_sglang crosscheck --tool sglang --sglang-python "$SGLANG_PYTHON" \
    --backend-kind real --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
    --runs 1 --python-best-num-tokenizer "$BEST" \
    --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
    --out "$OUT_DIR/crosscheck_sglang.manifest.json"
fi

# --- Step 4: Scenario 3 (BENCH-05), standard throughput (BENCH-06) ---------------
run_bench_step s3 s3 \
  --backend-kind real --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
  --runs "$RUNS" --python-best-num-tokenizer "$BEST" \
  --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
  --out "$OUT_DIR/s3_coldstart.manifest.json"

run_bench_step throughput throughput \
  --backend-kind real --model-arg "$MODEL" --port "$PORT" --python "$PYTHON" \
  --runs "$RUNS" --python-best-num-tokenizer "$BEST" \
  --python-cmd "$PYTHON_TEMPLATE" --rust-cmd "$RUST_TEMPLATE" \
  --out "$OUT_DIR/standard_throughput.manifest.json"

# --- Step 5: the combined report (BENCH-07/BENCH-08) -----------------------------
run_bench_step report report \
  --manifests-dir "$OUT_DIR" --out-json "$OUT_DIR.json" --out-md "$OUT_DIR.md"

# --- Step 6: the vendored tree was not touched -----------------------------------
CHECK_LOG="$LOG_DIR/check_upstream.log"
if "$PYTHON" scripts/check_upstream.py >"$CHECK_LOG" 2>&1; then
  record check_upstream PASS "scripts/check_upstream.py"
else
  record check_upstream FAIL "scripts/check_upstream.py (see $CHECK_LOG)"
  FAILED=1
fi

print_summary_and_exit
