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
