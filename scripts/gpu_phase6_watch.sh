#!/usr/bin/env bash
# Phase 6 D-12 process-health watcher: watches one scheduler pid around the
# 128-request cancellation stress run against the real backend and reports
# crash/zombie/restart. Detection lives here, never in Phase 5's stress tool
# (D-11/D-12). Started and stopped by scripts/parity_check.py's stress part.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phase6_watch.sh --pid PID [--interval SECONDS] [--out FILE] [--launcher-log FILE] [--help]

Watches one scheduler pid and reports crash, zombie or restart. It is a thin
read-only observer: it never generates load and never changes Phase 5's
stress tool (D-11, D-12).

Options:
  --pid PID            pid of the scheduler process to watch (required, numeric)
  --interval SECONDS   seconds between samples (default 1)
  --out FILE           append each sample line and the summary line to FILE
  --launcher-log FILE  launcher log to scan for scheduler respawns
  --help                show this help

Sample line, appended to --out when it is given:
  t=<epoch seconds> pid=<pid> state=<ps state letter|gone> gpu=<listed|unlisted|nvsmi_error>

Summary line, printed to stdout and appended to --out:
  WATCH summary pid=<pid> samples=<n> crashed=<0|1> zombie=<0|1> restarts=<n> gpu_unlisted=<n> nvsmi_errors=<n> verdict=<healthy|unhealthy>

Exit codes: 0 healthy, 1 unhealthy, 2 usage error.
EOF
}

PID=""
INTERVAL=1
OUT=""
LAUNCHER_LOG=""

while [ $# -gt 0 ]; do
  case "$1" in
    --pid)
      if [ $# -lt 2 ]; then echo "--pid needs a value" >&2; usage >&2; exit 2; fi
      PID="$2"
      if ! [[ "$PID" =~ ^[0-9]+$ ]]; then
        echo "--pid must be numeric: $PID" >&2
        usage >&2
        exit 2
      fi
      shift 2
      ;;
    --interval) INTERVAL="${2:?--interval needs a value}"; shift 2 ;;
    --out) OUT="${2:?--out needs a value}"; shift 2 ;;
    --launcher-log) LAUNCHER_LOG="${2:?--launcher-log needs a value}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

alive() { kill -0 "$1" 2>/dev/null; }

gpu_pids() { nvidia-smi --query-compute-apps=pid --format=csv,noheader | tr -d ' '; }  # let failure propagate

on_gpu() {  # on_gpu <pid> -> 0 listed, 1 not listed, 2 nvidia-smi failed
  local out
  out="$(gpu_pids)" || { echo "nvidia-smi failed" >&2; return 2; }
  grep -qx "$1" <<<"$out"
}

proc_state() {  # proc_state <pid> -> first char of `ps -o stat=`, or "gone"
  local pid="$1" stat
  stat="$(ps -o stat= -p "$pid" 2>/dev/null | tr -d ' ' || true)"
  if [ -z "$stat" ]; then
    echo "gone"
  else
    echo "${stat:0:1}"
  fi
}

scheduler_spawns() {  # scheduler_spawns <log> -> number of scheduler rank=0 spawn lines, 0 if missing
  local log="$1" n
  if [ ! -f "$log" ]; then
    echo 0
    return 0
  fi
  n="$(grep -c 'spawned scheduler rank=0 pid=' "$log" || true)"
  echo "${n:-0}"
}

# Mac tests in python/tests/test_gpu_phase6_watch.py source this file to reach the
# helpers above; nothing below this guard runs when sourced.
if [ "${BASH_SOURCE[0]}" != "${0}" ]; then
  return 0
fi

if [ -z "$PID" ]; then
  echo "missing required --pid" >&2
  usage >&2
  exit 2
fi

samples=0
crashed=0
zombie=0
restarts=0
gpu_unlisted=0
nvsmi_errors=0

STOP=0
trap 'STOP=1' TERM INT

# Resolution of the sleep slicing below: a SIGTERM/SIGINT is noticed within
# about one slice (0.1 s), regardless of the configured --interval.
NUM_SLICES="$(awk -v t="$INTERVAL" 'BEGIN{n=int((t/0.1)+0.5); if (n<1) n=1; print n}')"

while [ "$STOP" -eq 0 ]; do
  state="$(proc_state "$PID")"

  rc=0
  on_gpu "$PID" || rc=$?
  if [ "$rc" = 0 ]; then
    gpu_state="listed"
  elif [ "$rc" = 1 ]; then
    gpu_state="unlisted"
    gpu_unlisted=$((gpu_unlisted + 1))
  else
    gpu_state="nvsmi_error"
    nvsmi_errors=$((nvsmi_errors + 1))
  fi

  line="t=$(date +%s) pid=$PID state=$state gpu=$gpu_state"
  samples=$((samples + 1))
  if [ -n "$OUT" ]; then
    echo "$line" >>"$OUT"
  fi

  if [ "$state" = "gone" ]; then
    crashed=1
    break
  fi
  if [ "$state" = "Z" ]; then
    zombie=1
    break
  fi

  if [ -n "$LAUNCHER_LOG" ]; then
    spawns="$(scheduler_spawns "$LAUNCHER_LOG")"
    restarts=$(( spawns > 1 ? spawns - 1 : 0 ))
  fi

  for ((_slice = 0; _slice < NUM_SLICES && STOP == 0; _slice++)); do
    sleep 0.1
  done
done

verdict="healthy"
if [ "$crashed" = 1 ] || [ "$zombie" = 1 ] || [ "$restarts" -gt 0 ]; then
  verdict="unhealthy"
fi

summary="WATCH summary pid=$PID samples=$samples crashed=$crashed zombie=$zombie restarts=$restarts gpu_unlisted=$gpu_unlisted nvsmi_errors=$nvsmi_errors verdict=$verdict"
echo "$summary"
if [ -n "$OUT" ]; then
  echo "$summary" >>"$OUT"
fi

if [ "$verdict" = "unhealthy" ]; then
  exit 1
fi
exit 0
