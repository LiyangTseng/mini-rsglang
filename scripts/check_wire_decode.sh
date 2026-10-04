#!/usr/bin/env bash
# WIRE-02 (D-15) in one command: the Rust codec dumps every fixture case, then pytest decodes each
# file with upstream's real decoder and checks the re-encode is byte-identical.
set -euo pipefail
cd "$(dirname "$0")/.."

PYTHON="${PYTHON:-.venv/bin/python}"
DUMP_DIR="$(mktemp -d)"
trap 'rm -rf "$DUMP_DIR"' EXIT
export DUMP_DIR

cargo test -p rsg-wire --test dump -- --nocapture
RSGLANG_REQUIRE_DUMP=1 "$PYTHON" -m pytest python/tests/test_wire_decode.py -q
echo "check_wire_decode: OK"
