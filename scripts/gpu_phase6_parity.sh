#!/usr/bin/env bash
# Phase 6 GPU verification: ROADMAP Phase 6 criteria 1-4 (PAR-01, PAR-02) through
# scripts/parity_check.py. Runs on the Linux GPU box; --help works anywhere. A
# human runs it once at the end of the phase and signs off on the PASS/FAIL lines.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phase6_parity.sh [options]

Runs every GPU-only Phase 6 check through scripts/parity_check.py and prints
one PASS/FAIL line per step:
  1.  cargo build --release --workspace --all-targets
  2.  parity_check.py discover --frontend python
  3.  parity_check.py discover --frontend rust
  4.  parity_check.py run (writes --out)
  5.  parity_check.py validate --out --require-gpu
  6.  parity_check.py verdict --out --criterion 1
  7.  parity_check.py verdict --out --criterion 2
  8.  parity_check.py verdict --out --criterion 3
  9.  parity_check.py verdict --out --criterion 4
  10. scripts/check_upstream.py

Options:
  --model ID              gate model (default Qwen/Qwen3-0.6B)
  --llama-model ID        reported, not gated (default meta-llama/Llama-3.2-1B-Instruct, D-02)
  --concurrency N         PAR-02 concurrent-load level (default 128, D-10)
  --port N                HTTP port (default 1919)
  --timeout S             server-readiness timeout in seconds (default 900)
  --out PATH              parity-report.json path (default docs/benchmarks/parity-report.json)
  --corpus PATH           parity corpus path (default fixtures/parity/corpus.json)
  --python-server-cmd T   override for parity_check.py's --python-server-cmd
                          (default: parity_check.py's own default)
  --rust-server-cmd T     override for parity_check.py's --rust-server-cmd
                          (default: parity_check.py's own default -- the real
                          rsg-server binary via rsglang.launch, 05-08-SUMMARY.md)
  --stress-server-cmd T   override for parity_check.py's --stress-server-cmd
                          (default: parity_check.py's own default -- the rust
                          launch plus --abort-timing forwarded through the
                          launcher, 05-08-SUMMARY.md / 06-06)
  --stress-cmd T          override for parity_check.py's --stress-cmd
                          (default: parity_check.py's own default -- Phase 6's
                          own external-target stress driver, rsglang.parity
                          .stress_client; NOT Phase 5's stress_128.rs, which
                          has no external-target mode, see 06-06-SUMMARY.md)
  --run-extra-args STR    extra args, word-split, appended to the run step
  --help                  show this help

Environment:
  PYTHON               interpreter to use (default .venv/bin/python if present, else python3)
  CHECK_UPSTREAM_ARGS  extra args for scripts/check_upstream.py (for example --offline,
                       which can only prove a pristine tree)

Exits 0 only if every step passed. Logs go to a fresh mktemp -d directory.
EOF
}

DEFAULT_MODEL="Qwen/Qwen3-0.6B"
DEFAULT_LLAMA_MODEL="meta-llama/Llama-3.2-1B-Instruct"
MODEL="$DEFAULT_MODEL"
LLAMA_MODEL="$DEFAULT_LLAMA_MODEL"
CONCURRENCY=128
PORT=1919
TIMEOUT=900
OUT="docs/benchmarks/parity-report.json"
CORPUS="fixtures/parity/corpus.json"
PYTHON_SERVER_CMD=""
RUST_SERVER_CMD=""
STRESS_SERVER_CMD=""
STRESS_CMD=""
RUN_EXTRA_ARGS=""

while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="${2:?--model needs a value}"; shift 2 ;;
    --llama-model) LLAMA_MODEL="${2:?--llama-model needs a value}"; shift 2 ;;
    --concurrency) CONCURRENCY="${2:?--concurrency needs a value}"; shift 2 ;;
    --port) PORT="${2:?--port needs a value}"; shift 2 ;;
    --timeout) TIMEOUT="${2:?--timeout needs a value}"; shift 2 ;;
    --out) OUT="${2:?--out needs a value}"; shift 2 ;;
    --corpus) CORPUS="${2:?--corpus needs a value}"; shift 2 ;;
    --python-server-cmd) PYTHON_SERVER_CMD="${2:?--python-server-cmd needs a value}"; shift 2 ;;
    --rust-server-cmd) RUST_SERVER_CMD="${2:?--rust-server-cmd needs a value}"; shift 2 ;;
    --stress-server-cmd) STRESS_SERVER_CMD="${2:?--stress-server-cmd needs a value}"; shift 2 ;;
    --stress-cmd) STRESS_CMD="${2:?--stress-cmd needs a value}"; shift 2 ;;
    --run-extra-args) RUN_EXTRA_ARGS="${2:?--run-extra-args needs a value}"; shift 2 ;;
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

TMP_BASE="${TMPDIR:-/tmp}"
LOG_DIR="$(mktemp -d "${TMP_BASE%/}/gpu_phase6_parity.XXXXXX")"
echo "logs: $LOG_DIR"

declare -a RESULTS=()

record() {  # record <step> <PASS|FAIL> <detail>
  RESULTS+=("$2 step $1: $3")
  echo "$2 step $1: $3"
}

cleanup() {
  # Safety net only (Ctrl-C, unexpected exit): parity_check.py tears its own
  # sessions down. For every line of every $LOG_DIR/*/sessions.pgid file,
  # kill -9 that process group too (T-06-14).
  local pgid f
  local -a pgids=()
  for f in "$LOG_DIR"/*/sessions.pgid; do
    [ -f "$f" ] || continue
    while IFS= read -r pgid; do
      [ -n "$pgid" ] && pgids+=("$pgid")
    done <"$f"
  done
  for pgid in "${pgids[@]+"${pgids[@]}"}"; do
    kill -9 -- "-$pgid" 2>/dev/null || true
  done
}
trap cleanup EXIT

run_discover() {  # run_discover <frontend> <server_cmd_override>
  local frontend="$1" override="$2"
  local log="$LOG_DIR/discover-$frontend.log"
  local -a args=(discover --frontend "$frontend" --model "$MODEL" --port "$PORT" --timeout "$TIMEOUT" \
    --work-dir "$LOG_DIR/discover-$frontend")
  [ -n "$override" ] && args+=(--server-cmd "$override")
  "$PYTHON" scripts/parity_check.py "${args[@]}" >"$log" 2>&1
}

run_run() {
  local log="$LOG_DIR/run.log"
  local -a args=(run --models "$MODEL,$LLAMA_MODEL" --corpus "$CORPUS" --concurrency "$CONCURRENCY" \
    --port "$PORT" --timeout "$TIMEOUT" --out "$OUT" --work-dir "$LOG_DIR/run")
  [ -n "$PYTHON_SERVER_CMD" ] && args+=(--python-server-cmd "$PYTHON_SERVER_CMD")
  [ -n "$RUST_SERVER_CMD" ] && args+=(--rust-server-cmd "$RUST_SERVER_CMD")
  [ -n "$STRESS_SERVER_CMD" ] && args+=(--stress-server-cmd "$STRESS_SERVER_CMD")
  [ -n "$STRESS_CMD" ] && args+=(--stress-cmd "$STRESS_CMD")
  if [ -n "$RUN_EXTRA_ARGS" ]; then
    # Intentional word splitting: --run-extra-args is a single string of
    # space-separated flags/values to append, same convention as
    # $CHECK_UPSTREAM_ARGS below.
    # shellcheck disable=SC2206
    local -a extra=($RUN_EXTRA_ARGS)
    args+=("${extra[@]}")
  fi
  "$PYTHON" scripts/parity_check.py "${args[@]}" >"$log" 2>&1
}

run_verdict() {  # run_verdict <n> -> prints the "criterion N: ..." line; returns parity_check.py's exit code
  local n="$1" out rc=0
  local log="$LOG_DIR/verdict-$n.log"
  out="$("$PYTHON" scripts/parity_check.py verdict "$OUT" --criterion "$n" 2>&1)" || rc=$?
  printf '%s\n' "$out" >"$log"
  printf '%s\n' "$out" | grep -m1 "^criterion $n:" || true
  return "$rc"
}

# Mac tests in python/tests/test_gpu_phase6_parity_script.py source this file to
# reach the helpers above; nothing below this guard runs when sourced.
if [ "${BASH_SOURCE[0]}" != "${0}" ]; then
  return 0
fi

# A non-interactive SSH command never sources ~/.bashrc, so on WSL neither
# nvidia-smi (/usr/lib/wsl/lib) nor a user-space CUDA toolchain (the cuda128
# micromamba env, if present, for flashinfer's JIT) ever reach PATH. Prepend
# them here when present, mirroring the gpu_phase2_profile.sh fix exactly.
for extra_path in "$HOME/.local/micromamba/root/envs/cuda128/bin" "$ROOT/.venv/bin" /usr/lib/wsl/lib /usr/local/cuda/bin; do
  [ -d "$extra_path" ] && PATH="$extra_path:$PATH"
done
export PATH

# --- Preflight -----------------------------------------------------------------
missing=()
for tool in nvidia-smi cargo; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "FAIL preflight: not on PATH: ${missing[*]}"
  exit 1
fi
echo "GPU: $(nvidia-smi --query-gpu=name --format=csv,noheader | head -1)"
echo "python: $PYTHON   model: $MODEL   llama-model: $LLAMA_MODEL   concurrency: $CONCURRENCY"

# --- Step 1: release build (all targets, so Phase 5's stress tool is built too) ---
if cargo build --release --workspace --all-targets >"$LOG_DIR/build.log" 2>&1; then
  record 1 PASS "cargo build --release --workspace --all-targets"
else
  record 1 FAIL "cargo build --release --workspace --all-targets (see $LOG_DIR/build.log)"
fi

# --- Steps 2 and 3: discover each frontend against a fresh session ------------
if run_discover python "$PYTHON_SERVER_CMD"; then
  record 2 PASS "parity_check.py discover --frontend python"
else
  record 2 FAIL "parity_check.py discover --frontend python (see $LOG_DIR/discover-python.log)"
fi

if run_discover rust "$RUST_SERVER_CMD"; then
  record 3 PASS "parity_check.py discover --frontend rust"
else
  record 3 FAIL "parity_check.py discover --frontend rust (see $LOG_DIR/discover-rust.log)"
fi

# --- Step 4: the full run (writes $OUT) ----------------------------------------
if run_run; then
  record 4 PASS "parity_check.py run"
else
  record 4 FAIL "parity_check.py run (see $LOG_DIR/run.log)"
fi

# --- Step 5: validate the written sidecar, requiring real-GPU provenance -------
if [ -f "$OUT" ]; then
  if "$PYTHON" scripts/parity_check.py validate "$OUT" --require-gpu >"$LOG_DIR/validate.log" 2>&1; then
    record 5 PASS "parity_check.py validate --require-gpu"
  else
    record 5 FAIL "parity_check.py validate --require-gpu (see $LOG_DIR/validate.log)"
  fi
else
  record 5 FAIL "skipped: run wrote no sidecar"
fi

# --- Steps 6-9: one verdict per ROADMAP criterion ------------------------------
for n in 1 2 3 4; do
  step=$((n + 5))
  if [ ! -f "$OUT" ]; then
    record "$step" FAIL "skipped: run wrote no sidecar"
    continue
  fi
  rc=0
  detail="$(run_verdict "$n")" || rc=$?
  [ -n "$detail" ] || detail="criterion $n (see $LOG_DIR/verdict-$n.log)"
  if [ "$rc" = 0 ]; then
    record "$step" PASS "$detail"
  else
    record "$step" FAIL "$detail"
  fi
done

# --- Step 10: frozen Python frontend -------------------------------------------
if [ -f scripts/check_upstream.py ] && "$PYTHON" scripts/check_upstream.py ${CHECK_UPSTREAM_ARGS:-} \
     >"$LOG_DIR/check_upstream.log" 2>&1; then
  record 10 PASS "scripts/check_upstream.py"
else
  record 10 FAIL "scripts/check_upstream.py (see $LOG_DIR/check_upstream.log)"
fi

# --- Summary -------------------------------------------------------------------
echo
echo "=== Phase 6 GPU parity summary (logs: $LOG_DIR) ==="
failed=0
for r in "${RESULTS[@]}"; do
  echo "$r"
  case "$r" in FAIL*) failed=1 ;; esac
done
if [ "$failed" = 0 ]; then echo "ALL PASS"; exit 0; fi
echo "SOME STEPS FAILED"
exit 1
