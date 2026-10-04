#!/usr/bin/env bash
# Phase 1 GPU verification: ROADMAP criteria 2 and 3, the D-12 no-orphan backstop,
# and the frozen-frontend check. Runs on the Linux GPU box; --help works anywhere.
# A human runs it once at the end of the phase and signs off on the PASS/FAIL lines.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phase1_check.sh [--model Qwen/Qwen3-0.6B] [--port 1919] [--timeout 900] [--help]

Runs every GPU-only Phase 1 check and prints one PASS/FAIL line per step:
  1. cargo build --release -p rsg-server
  2. --frontend python serves a chat completion (unmodified Python frontend)
  3. --frontend rust: rsg-server logs the real handshake (max_seq_len, eos_token_id,
     page_size, max_running_req, num_pages, upstream SHA) and the scheduler armed
     PDEATHSIG (no "PDEATHSIG unavailable" in the log)
  4. kill -9 of the launcher leaves no rsg-server or scheduler process (ps, nvidia-smi)
     4b. the same check repeated with kill -9 right after the scheduler spawns, before it is ready
  5. scripts/check_upstream.py passes (frozen Python frontend)

Options:
  --model ID      model to serve (default Qwen/Qwen3-0.6B)
  --port N        HTTP port for python mode (default 1919)
  --timeout S     seconds to wait for each mode to become ready (default 900)
  --help          show this help

Environment:
  PYTHON          interpreter to use (default .venv/bin/python if present, else python3)

Exits 0 only if every step passed. Logs go to a fresh mktemp -d directory.
EOF
}

DEFAULT_MODEL="Qwen/Qwen3-0.6B"
MODEL="$DEFAULT_MODEL"
PORT=1919
TIMEOUT=900

while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="${2:?--model needs a value}"; shift 2 ;;
    --port) PORT="${2:?--port needs a value}"; shift 2 ;;
    --timeout) TIMEOUT="${2:?--timeout needs a value}"; shift 2 ;;
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
LOG_DIR="$(mktemp -d "${TMP_BASE%/}/gpu_phase1_check.XXXXXX")"
echo "logs: $LOG_DIR"

declare -a RESULTS=()
declare -a STARTED_PGIDS=()

record() {  # record <step> <PASS|FAIL> <detail>
  RESULTS+=("$2 step $1: $3")
  echo "$2 step $1: $3"
}

cleanup() {
  # Safety net only (Ctrl-C, unexpected exit): step 4 judges the real backstop first.
  local pgid
  for pgid in "${STARTED_PGIDS[@]+"${STARTED_PGIDS[@]}"}"; do
    kill -9 -- "-$pgid" 2>/dev/null || true
  done
}
trap cleanup EXIT

# Start "$@" in its own session in the background; sets BG_PID (= its process group id).
start_session() {
  local log="$1"; shift
  # A non-interactive shell starts background jobs with SIGINT ignored, and an ignored
  # disposition survives exec (python mode would then ignore our kill -INT). Restore the
  # default before exec'ing setsid; setsid then execs in place, so $! is the run's pid.
  "$PYTHON" -c 'import os, signal, sys; signal.signal(signal.SIGINT, signal.SIG_DFL); os.execvp(sys.argv[1], sys.argv[1:])' \
    setsid "$@" >"$log" 2>&1 &
  BG_PID=$!
  STARTED_PGIDS+=("$BG_PID")
  # Poll for up to 5 s: a slower interpreter start plus `exec setsid` must not fail a
  # healthy run (the old fixed half-second wait did).
  local pgid round
  for round in $(seq 1 50); do
    pgid="$(ps -o pgid= -p "$BG_PID" 2>/dev/null | tr -d ' ' || true)"
    if [ -z "$pgid" ]; then
      return 0  # already exited; the caller's liveness check reports this
    fi
    [ "$pgid" = "$BG_PID" ] && return 0
    sleep 0.1
  done
  echo "setsid did not exec in place (pid $BG_PID, pgid $pgid after 5 s)" >&2
  # Never signal a process group here: at this moment BG_PID still shares the script's
  # own process group, so a group kill would hit the script itself.
  kill -9 "$BG_PID" 2>/dev/null || true
  wait "$BG_PID" 2>/dev/null || true
  return 1
}

alive() { kill -0 "$1" 2>/dev/null; }

gpu_pids() { nvidia-smi --query-compute-apps=pid --format=csv,noheader | tr -d ' '; }  # let failure propagate

on_gpu() {  # on_gpu <pid> -> 0 listed, 1 not listed, 2 nvidia-smi failed
  local out
  out="$(gpu_pids)" || { echo "nvidia-smi failed" >&2; return 2; }
  grep -qx "$1" <<<"$out"
}

# The scheduler prints this when prctl(PR_SET_PDEATHSIG) failed and it fell back to
# the polling watchdog (01-10, WR-06); verification treats that as a failure (plan A).
# Normalize to 0/1: grep returns 2 (not 0 or 1) on a missing/unreadable log.
pdeathsig_degraded() { grep -qF 'PDEATHSIG unavailable' "$1" 2>/dev/null && return 0; return 1; }

wait_no_orphans() {  # wait_no_orphans <timeout_s> <pid>... -> 0 all gone and unlisted
  local timeout="$1"; shift
  local deadline=$((SECONDS + timeout))
  local pid rc all_gone failed
  while [ $SECONDS -lt $deadline ]; do
    all_gone=1
    for pid in "$@"; do
      if ps -p "$pid" >/dev/null 2>&1; then all_gone=0; continue; fi
      rc=0; on_gpu "$pid" || rc=$?
      if [ "$rc" = 2 ]; then
        echo "nvidia-smi failed while checking pid $pid" >&2
        return 1
      elif [ "$rc" = 0 ]; then
        all_gone=0
      fi
    done
    [ "$all_gone" = 1 ] && return 0
    sleep 1
  done
  failed=0
  for pid in "$@"; do
    if ps -p "$pid" >/dev/null 2>&1; then
      echo "pid $pid still running ${timeout} s after kill -9 of the launcher"
      failed=1
      continue
    fi
    rc=0; on_gpu "$pid" || rc=$?
    if [ "$rc" = 2 ]; then
      echo "nvidia-smi failed while checking pid $pid"
      failed=1
    elif [ "$rc" = 0 ]; then
      echo "pid $pid still listed by nvidia-smi"
      failed=1
    fi
  done
  [ "$failed" = 0 ] && return 0
  return 1
}

# Mac helper tests in python/tests/test_gpu_check_script.py source this file to reach the
# helpers above; nothing below this guard runs when sourced.
if [ "${BASH_SOURCE[0]}" != "${0}" ]; then
  return 0
fi

# --- Preflight -----------------------------------------------------------------
missing=()
for tool in nvidia-smi cargo curl setsid; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "FAIL preflight: not on PATH: ${missing[*]}"
  exit 1
fi
echo "GPU: $(nvidia-smi --query-gpu=name --format=csv,noheader | head -1)"
echo "     (SM100 boards report page_size 64, others 1)"
echo "python: $PYTHON   model: $MODEL   port: $PORT   timeout: ${TIMEOUT}s"

# --- Step 1: release build -----------------------------------------------------
RUST_BIN="$ROOT/target/release/rsg-server"
if cargo build --release -p rsg-server >"$LOG_DIR/build.log" 2>&1 && [ -x "$RUST_BIN" ]; then
  record 1 PASS "cargo build --release -p rsg-server"
  BUILD_OK=1
else
  record 1 FAIL "cargo build --release -p rsg-server (see $LOG_DIR/build.log)"
  BUILD_OK=0
fi

# --- Step 2: python mode serves a chat completion (criterion 2) ----------------
step2() {
  local log="$LOG_DIR/python-mode.log" pid deadline body resp content
  start_session "$log" "$PYTHON" -m rsglang.launch --frontend python --model "$MODEL" --port "$PORT" || return 1
  pid=$BG_PID
  deadline=$((SECONDS + TIMEOUT))
  until curl -sf "http://127.0.0.1:$PORT/v1/models" >/dev/null 2>&1; do
    if ! alive "$pid"; then echo "python-mode launcher exited early (see $log)"; return 1; fi
    if [ $SECONDS -ge $deadline ]; then echo "python mode not ready after ${TIMEOUT}s"; STEP2_PID=$pid; return 1; fi
    sleep 1
  done
  STEP2_PID=$pid
  body="$("$PYTHON" -c 'import json,sys; print(json.dumps({"model": sys.argv[1], "messages": [{"role": "user", "content": "Say hello."}], "max_tokens": 16, "temperature": 0}))' "$MODEL")"
  resp="$(curl -sf -H 'content-type: application/json' -d "$body" "http://127.0.0.1:$PORT/v1/chat/completions")" || {
    echo "chat completion request failed"; return 1; }
  echo "$resp" >"$LOG_DIR/python-mode-response.json"
  content="$(printf '%s' "$resp" | "$PYTHON" -c '
import json, sys
c = json.load(sys.stdin)["choices"][0]["message"]["content"]
if not (isinstance(c, str) and c):
    sys.exit("empty or non-string content: %r" % (c,))
print(c)')" || { echo "bad chat completion: $resp"; return 1; }
  echo "completion content: $content"
}

stop_session() {  # stop_session <pgid>: SIGINT the group, wait up to 60 s, then SIGKILL it
  local pgid="$1" deadline=$((SECONDS + 60))
  kill -INT -- "-$pgid" 2>/dev/null || return 0
  # Wait for the whole group (upstream's api server, tokenizer, scheduler), not just its leader.
  while kill -0 -- "-$pgid" 2>/dev/null && [ $SECONDS -lt $deadline ]; do sleep 1; done
  if kill -0 -- "-$pgid" 2>/dev/null; then
    echo "WARN: process group $pgid still running 60 s after SIGINT; sending SIGKILL"
    kill -9 -- "-$pgid" 2>/dev/null || true
  fi
  wait "$pgid" 2>/dev/null || true
}

STEP2_PID=""
if step2; then
  record 2 PASS "--frontend python served a non-empty chat completion"
else
  record 2 FAIL "--frontend python chat completion (see $LOG_DIR/python-mode.log)"
fi
[ -n "$STEP2_PID" ] && stop_session "$STEP2_PID"

# --- Step 3: rust mode logs the real handshake (criteria 2 and 3) --------------
LAUNCHER_PID=""
RSG_PID=""
SCHED_PID=""
check_field() {  # check_field <line> <key> -> prints the value or fails
  local value
  value="$(printf '%s\n' "$1" | grep -o "$2=[^ ]*" | head -1 | cut -d= -f2-)"
  [ -n "$value" ] || { echo "handshake line has no $2=" >&2; return 1; }
  printf '%s' "$value"
}

step3() {
  local log="$LOG_DIR/rust-mode.log" deadline line sha v
  start_session "$log" "$PYTHON" -m rsglang.launch --frontend rust --model "$MODEL" --port "$PORT" \
    --rust-bin "$RUST_BIN" --ready-timeout "$TIMEOUT" || return 1
  LAUNCHER_PID=$BG_PID
  deadline=$((SECONDS + TIMEOUT + 60))
  until line="$(grep -m1 'handshake received' "$log")"; do
    if ! alive "$LAUNCHER_PID"; then echo "rust-mode launcher exited early (see $log)"; return 1; fi
    if [ $SECONDS -ge $deadline ]; then echo "no handshake after ${TIMEOUT}s"; return 1; fi
    sleep 1
  done
  RSG_PID="$(grep -o 'spawned rsg-server pid=[0-9]*' "$log" | head -1 | cut -d= -f2)"
  SCHED_PID="$(grep -o 'spawned scheduler rank=0 pid=[0-9]*' "$log" | head -1 | cut -d= -f2)"
  if pdeathsig_degraded "$log"; then
    echo "scheduler fell back to the polling watchdog: PDEATHSIG unavailable (see $log)"
    return 1
  fi
  echo "$line"
  sha="$(cat vendor/UPSTREAM_SHA)"
  v="$(check_field "$line" upstream_sha)" && [ "$v" = "$sha" ] || { echo "upstream_sha mismatch (want $sha)"; return 1; }
  v="$(check_field "$line" max_running_req)" && [ "$v" = 256 ] || { echo "max_running_req != 256"; return 1; }
  v="$(check_field "$line" num_pages)" && [[ "$v" =~ ^[0-9]+$ ]] && [ "$v" -gt 1 ] || { echo "num_pages not > 1"; return 1; }
  v="$(check_field "$line" max_seq_len)" && [[ "$v" =~ ^[0-9]+$ ]] && [ "$v" -ge 1 ] && [ "$v" -le 40960 ] \
    || { echo "max_seq_len not in 1..40960"; return 1; }
  v="$(check_field "$line" page_size)" && { [ "$v" = 1 ] || [ "$v" = 64 ]; } || { echo "page_size not 1 or 64"; return 1; }
  if [ "$MODEL" = "$DEFAULT_MODEL" ]; then
    v="$(check_field "$line" eos_token_id)" && [ "$v" = 151645 ] || { echo "eos_token_id != 151645"; return 1; }
  fi
  [ -n "$RSG_PID" ] && [ -n "$SCHED_PID" ] || { echo "child pids not found in $log"; return 1; }
}

if [ "$BUILD_OK" = 1 ] && step3; then
  record 3 PASS "--frontend rust handshake carries real backend values"
else
  record 3 FAIL "--frontend rust handshake (see $LOG_DIR/rust-mode.log)"
fi

# --- Step 4: kill -9 of the launcher leaves no orphan (D-12) -------------------
step4() {
  local rc
  [ -n "$LAUNCHER_PID" ] && [ -n "$RSG_PID" ] && [ -n "$SCHED_PID" ] || { echo "step 3 did not start a full run"; return 1; }
  alive "$LAUNCHER_PID" || { echo "launcher already gone before kill -9"; return 1; }
  kill -9 "$LAUNCHER_PID"
  rc=0; wait_no_orphans 30 "$RSG_PID" "$SCHED_PID" || rc=$?
  rm -f /tmp/minisgl_{0..4}.rsg="$LAUNCHER_PID"
  [ "$rc" = 0 ] || return 1
  echo "rsg-server $RSG_PID and scheduler $SCHED_PID exited after kill -9 of launcher $LAUNCHER_PID"
}

if step4; then
  record 4 PASS "no rsg-server or scheduler left after kill -9 of the launcher"
else
  record 4 FAIL "orphan check after kill -9 of the launcher"
fi

# --- Step 4b: kill -9 of the launcher while the scheduler is still booting (G-01-3) ---
# Step 4 kills after the handshake, outside the window where the old watchdog missed the
# launcher's death; this one kills right after "spawned scheduler rank=0".
step4_early() {
  local log="$LOG_DIR/rust-mode-early-kill.log" launcher rsg sched deadline rc
  start_session "$log" "$PYTHON" -m rsglang.launch --frontend rust --model "$MODEL" --port "$PORT" \
    --rust-bin "$RUST_BIN" --ready-timeout "$TIMEOUT" || return 1
  launcher=$BG_PID
  deadline=$((SECONDS + 120))
  # Poll every 0.1 s: the boot window is only seconds long.
  until grep -q 'spawned scheduler rank=0 pid=[0-9]*' "$log"; do
    if ! alive "$launcher"; then echo "early-kill launcher exited before spawning the scheduler (see $log)"; return 1; fi
    if [ $SECONDS -ge $deadline ]; then echo "no scheduler spawn line within 120 s (see $log)"; return 1; fi
    sleep 0.1
  done
  if grep -q 'backend ready' "$log"; then echo "not an early kill: backend already ready"; return 1; fi
  rsg="$(grep -o 'spawned rsg-server pid=[0-9]*' "$log" | head -1 | cut -d= -f2)"
  sched="$(grep -o 'spawned scheduler rank=0 pid=[0-9]*' "$log" | head -1 | cut -d= -f2)"
  [ -n "$rsg" ] && [ -n "$sched" ] || { echo "child pids not found in $log"; return 1; }
  kill -9 "$launcher"
  # The boot window with CUDA torch is unmeasured (a projection): the scheduler exits as soon
  # as run_scheduler starts, so allow up to 120 s.
  rc=0; wait_no_orphans 120 "$rsg" "$sched" || rc=$?
  rm -f /tmp/minisgl_{0..4}.rsg="$launcher"
  [ "$rc" = 0 ] || return 1
  echo "rsg-server $rsg and scheduler $sched exited after early kill -9 of launcher $launcher"
}

if [ "$BUILD_OK" = 1 ] && step4_early; then
  record 4b PASS "no rsg-server or scheduler left after kill -9 of the launcher during scheduler boot"
else
  record 4b FAIL "early-kill orphan check (see $LOG_DIR/rust-mode-early-kill.log)"
fi

# --- Step 5: frozen Python frontend --------------------------------------------
if [ -f scripts/check_upstream.py ] && "$PYTHON" scripts/check_upstream.py >"$LOG_DIR/check_upstream.log" 2>&1; then
  record 5 PASS "scripts/check_upstream.py"
else
  record 5 FAIL "scripts/check_upstream.py (see $LOG_DIR/check_upstream.log)"
fi

# --- Summary -------------------------------------------------------------------
echo
echo "=== Phase 1 GPU check summary (logs: $LOG_DIR) ==="
failed=0
for r in "${RESULTS[@]}"; do
  echo "$r"
  case "$r" in FAIL*) failed=1 ;; esac
done
if [ "$failed" = 0 ]; then echo "ALL PASS"; exit 0; fi
echo "SOME STEPS FAILED"
exit 1
