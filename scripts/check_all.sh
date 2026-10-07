#!/usr/bin/env bash
# Phase gate (D-14): Rust tests, Python tests, fixture freshness, tokenizer fixture freshness,
# the WIRE-02 decode check and the vendored-tree check, stopping at the first failure. Fixture
# freshness (step 3) and tokenizer fixture freshness (step 4) run alongside the vendored-tree
# check (step 6), so a vendored-code change cannot silently stale either fixture set.
#
# Usage: scripts/check_all.sh [--offline]   (--offline is passed to check_upstream.py)
set -euo pipefail
cd "$(dirname "$0")/.."

PYTHON="${PYTHON:-.venv/bin/python}"
UPSTREAM_ARGS=()
for arg in "$@"; do
  case "$arg" in
    --offline) UPSTREAM_ARGS+=(--offline) ;;
    *) echo "usage: $0 [--offline]" >&2; exit 2 ;;
  esac
done

step() { echo; echo "=== check_all [$1/6] $2 ==="; }

# crates/rsg-server/tests/stress_128.rs needs more open files than macOS's
# default soft limit of 256 (128 client sockets, 128 server sockets, plus
# the mock's own fds). Raise the soft limit here when it's below 4096,
# leaving it alone otherwise. A shell whose hard limit is itself below
# 4096 makes `ulimit -n 4096` fail; `|| true` keeps that from failing this
# script under `set -e` — the stress test's own guard reports the problem
# instead, with a clearer message than a mid-run "too many open files".
current_nofile_limit=$(ulimit -n)
if [ "$current_nofile_limit" != "unlimited" ] && [ "$current_nofile_limit" -lt 4096 ]; then
  ulimit -n 4096 || true
fi

step 1 "cargo test --workspace"
cargo test --workspace
step 2 "pytest python/tests"
"$PYTHON" -m pytest python/tests -q
step 3 "fixture freshness (gen_wire_fixtures.py --check)"
"$PYTHON" scripts/gen_wire_fixtures.py --check
step 4 "tokenizer fixture freshness (gen_tokenizer_fixtures.py --check)"
"$PYTHON" scripts/gen_tokenizer_fixtures.py --check
step 5 "WIRE-02 decode (check_wire_decode.sh)"
bash scripts/check_wire_decode.sh
step 6 "vendored tree (check_upstream.py${UPSTREAM_ARGS[*]:+ ${UPSTREAM_ARGS[*]}})"
"$PYTHON" scripts/check_upstream.py ${UPSTREAM_ARGS[@]+"${UPSTREAM_ARGS[@]}"}

echo
echo "check_all: OK"
