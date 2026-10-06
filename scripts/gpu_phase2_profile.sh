#!/usr/bin/env bash
# Phase 2 GPU profiling run (BENCH-01): runs on the Linux GPU box; --help works
# anywhere. A human runs it on the GPU machine and signs off on the PASS/FAIL lines.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phase2_profile.sh [--model Qwen/Qwen3-0.6B] [--port 1919] [--timeout 900] [--out docs/benchmarks/baseline-profile.json] [--py-spy-sudo] [--help]

Runs every GPU-only Phase 2 profiling step and prints one PASS/FAIL line per step:
  preflight: nvidia-smi, curl, py-spy, hyperfine on PATH; psutil/aiohttp/openai/
             transformers import; hyperfine >= 1.19.0; py-spy can attach to a
             sibling process
  1. scripts/baseline_profile.py discover (privilege, topology and hook smoke)
  2. scripts/baseline_profile.py run (writes docs/benchmarks/baseline-profile.json)
  3. scripts/baseline_profile.py validate --require-gpu
  4. scripts/check_upstream.py (proves the vendored tree was not touched)

Options:
  --model ID         model to serve (default Qwen/Qwen3-0.6B)
  --port N           HTTP port for the profiled server (default 1919)
  --timeout S        seconds to wait for server readiness (default 900)
  --out PATH         baseline profile JSON output path (default docs/benchmarks/baseline-profile.json)
  --py-spy-sudo      attach py-spy via `sudo -n` instead of a CAP_SYS_PTRACE grant
  --help             show this help

Environment:
  PYTHON             interpreter to use (default .venv/bin/python if present, else python3)

Privileges:
  py-spy must attach to already-running sibling processes (the scheduler and
  tokenizer children), which needs elevated privileges on Linux. Pick one, and
  remove the grant again once the run is done:
    - (recommended) grant CAP_SYS_PTRACE on the py-spy binary itself:
        sudo setcap cap_sys_ptrace+ep "$(readlink -f .venv/bin/py-spy)"
      remove it afterwards with:
        sudo setcap -r "$(readlink -f .venv/bin/py-spy)"
    - pass --py-spy-sudo, with a NOPASSWD sudo rule scoped to py-spy
    - set kernel.yama.ptrace_scope=0 for the run and restore its previous value after

Exits 0 only if every step passed. Logs go to a fresh mktemp -d directory.
EOF
}

DEFAULT_MODEL="Qwen/Qwen3-0.6B"
MODEL="$DEFAULT_MODEL"
PORT=1919
TIMEOUT=900
OUT="docs/benchmarks/baseline-profile.json"
PYSPY_SUDO=()

while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="${2:?--model needs a value}"; shift 2 ;;
    --port) PORT="${2:?--port needs a value}"; shift 2 ;;
    --timeout) TIMEOUT="${2:?--timeout needs a value}"; shift 2 ;;
    --out) OUT="${2:?--out needs a value}"; shift 2 ;;
    --py-spy-sudo) PYSPY_SUDO=(--py-spy-sudo); shift ;;
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
LOG_DIR="$(mktemp -d "${TMP_BASE%/}/gpu_phase2_profile.XXXXXX")"
echo "logs: $LOG_DIR"

declare -a RESULTS=()

record() {  # record <step> <PASS|FAIL> <detail>
  RESULTS+=("$2 step $1: $3")
  echo "$2 step $1: $3"
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

# hyperfine_ok -> 0 if hyperfine is on PATH and at least 1.19.0 (needs --conclude).
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
  echo "hyperfine $v is older than 1.19.0 (needs --conclude); install 1.20.0: cargo install hyperfine --version 1.20.0 --locked"
  return 1
}

# pyspy_can_attach -> 0 if py-spy can dump a sibling process's stack. Always
# tears down its own probe sleeper, on both the success and failure path.
pyspy_can_attach() {
  "$PYTHON" -c 'import time; time.sleep(30)' &
  local sleeper=$!
  sleep 2
  local out rc=0
  if [ "${#PYSPY_SUDO[@]}" -gt 0 ]; then
    out="$(sudo -n "$(command -v py-spy)" dump --nonblocking --pid "$sleeper" 2>&1)" || rc=$?
  else
    out="$(py-spy dump --nonblocking --pid "$sleeper" 2>&1)" || rc=$?
  fi
  kill -9 "$sleeper" 2>/dev/null || true
  wait "$sleeper" 2>/dev/null || true
  if [ "$rc" = 0 ] && printf '%s' "$out" | grep -q "Thread"; then
    return 0
  fi
  echo "$out"
  echo "py-spy could not attach to a sibling process. Remediation options:"
  echo "  sudo setcap cap_sys_ptrace+ep \"\$(readlink -f .venv/bin/py-spy)\""
  echo "  or pass --py-spy-sudo with a NOPASSWD sudo rule for py-spy"
  echo "  or set kernel.yama.ptrace_scope=0 for the run"
  return 1
}

# Mac helper tests in python/tests/test_gpu_profile_script.py source this file to
# reach the helpers above; nothing below this guard runs when sourced.
if [ "${BASH_SOURCE[0]}" != "${0}" ]; then
  return 0
fi

# --- Preflight -----------------------------------------------------------------
missing=()
for tool in nvidia-smi curl py-spy hyperfine; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "FAIL preflight: not on PATH: ${missing[*]}"
  echo "install py-spy==0.4.2, psutil==7.2.2 and aiohttp==3.14.4 into the venv: uv pip install py-spy==0.4.2 psutil==7.2.2 aiohttp==3.14.4"
  echo "install hyperfine: cargo install hyperfine --version 1.20.0 --locked"
  exit 1
fi

if ! "$PYTHON" -c "import psutil, aiohttp, openai, transformers" >/dev/null 2>"$LOG_DIR/preflight-import.log"; then
  echo "FAIL preflight: python import failed: psutil, aiohttp, openai, transformers (see $LOG_DIR/preflight-import.log)"
  exit 1
fi

if ! hyperfine_ok; then
  echo "FAIL preflight: hyperfine too old (needs >= 1.19.0 for --conclude)"
  exit 1
fi

if ! pyspy_can_attach; then
  echo "FAIL preflight: py-spy cannot attach to a sibling process"
  exit 1
fi

echo "GPU: $(nvidia-smi --query-gpu=name --format=csv,noheader | head -1)"
echo "py-spy: $(py-spy --version)"
echo "hyperfine: $(hyperfine --version)"
echo "python: $PYTHON   model: $MODEL   port: $PORT   timeout: ${TIMEOUT}s   out: $OUT"

# --- Step 1: discover smoke (privilege, topology and hook) ---------------------
if "$PYTHON" scripts/baseline_profile.py discover --model "$MODEL" --port "$PORT" --timeout "$TIMEOUT" \
     --out "$LOG_DIR/discover.json" ${PYSPY_SUDO[@]+"${PYSPY_SUDO[@]}"} >"$LOG_DIR/discover.log" 2>&1; then
  record 1 PASS "scripts/baseline_profile.py discover"
  STEP1_OK=1
else
  record 1 FAIL "scripts/baseline_profile.py discover (see $LOG_DIR/discover.log)"
  STEP1_OK=0
fi

# --- Step 2: full run (D-14: the driver writes the baseline profile JSON) ------
STEP2_OK=0
if [ "$STEP1_OK" = 1 ]; then
  if "$PYTHON" scripts/baseline_profile.py run --model "$MODEL" --port "$PORT" --timeout "$TIMEOUT" \
       --out "$OUT" --work-dir "$LOG_DIR/work" ${PYSPY_SUDO[@]+"${PYSPY_SUDO[@]}"} >"$LOG_DIR/run.log" 2>&1; then
    record 2 PASS "scripts/baseline_profile.py run"
    STEP2_OK=1
  else
    record 2 FAIL "scripts/baseline_profile.py run (see $LOG_DIR/run.log)"
  fi
else
  record 2 FAIL "skipped: discover failed"
fi

# --- Step 3: validate the written sidecar ---------------------------------------
if [ "$STEP2_OK" = 1 ]; then
  if "$PYTHON" scripts/baseline_profile.py validate "$OUT" --require-gpu >"$LOG_DIR/validate.log" 2>&1; then
    record 3 PASS "scripts/baseline_profile.py validate --require-gpu"
  else
    record 3 FAIL "scripts/baseline_profile.py validate --require-gpu (see $LOG_DIR/validate.log)"
  fi
elif [ "$STEP1_OK" = 1 ]; then
  record 3 FAIL "skipped: run failed"
else
  record 3 FAIL "skipped: discover failed"
fi

# --- Step 4: frozen Python frontend (the vendored tree was not touched) --------
if [ -f scripts/check_upstream.py ] && "$PYTHON" scripts/check_upstream.py >"$LOG_DIR/check_upstream.log" 2>&1; then
  record 4 PASS "scripts/check_upstream.py"
else
  record 4 FAIL "scripts/check_upstream.py (see $LOG_DIR/check_upstream.log)"
fi

# --- Summary ---------------------------------------------------------------------
echo
echo "=== Phase 2 GPU profiling summary (logs: $LOG_DIR) ==="
failed=0
for r in "${RESULTS[@]}"; do
  echo "$r"
  case "$r" in FAIL*) failed=1 ;; esac
done

echo
echo "Privilege cleanup reminder: remove any grant you made before this run:"
echo "  sudo setcap -r \"\$(readlink -f .venv/bin/py-spy)\""
echo "  restore kernel.yama.ptrace_scope to its previous value if you lowered it"
echo -n "current py-spy capabilities (must print nothing): "
getcap "$(readlink -f .venv/bin/py-spy)" 2>/dev/null || true
echo

if [ "$failed" = 0 ]; then echo "ALL PASS"; exit 0; fi
echo "SOME STEPS FAILED"
exit 1
