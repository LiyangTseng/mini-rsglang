#!/usr/bin/env bash
# Bootstrap the Mac dev env: a project-local uv venv (Python 3.12) synced from the
# hash-pinned lock, plus --no-deps editable installs of the vendored backend and rsglang.
# Idempotent: safe to rerun in any checkout or worktree.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/bootstrap_mac_env.sh [--relock] [--help]

  --relock  Regenerate requirements-mac.txt from requirements-mac.in before syncing.
  --help    Show this help and exit.
USAGE
}

RELOCK=0
for arg in "$@"; do
  case "$arg" in
    --relock) RELOCK=1 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

cd "$(dirname "${BASH_SOURCE[0]}")/.."

PY=.venv/bin/python

if [ "$RELOCK" -eq 1 ]; then
  uv pip compile requirements-mac.in --python-version 3.12 \
    --python-platform aarch64-apple-darwin --generate-hashes -o requirements-mac.txt
fi

if [ ! -x "$PY" ]; then
  uv venv --python 3.12 .venv
fi

uv pip sync --python "$PY" requirements-mac.txt
# --no-deps: upstream's full dependency list fails on macOS (sgl-kernel is manylinux-only).
uv pip install --python "$PY" --no-deps -e vendor/mini-sglang
uv pip install --python "$PY" --no-deps -e .

"$PY" - <<'SMOKE'
import minisgl.message, minisgl.core, minisgl.utils, minisgl.scheduler, minisgl.server.args
import rsglang, torch, msgpack, zmq
print(f"torch {torch.__version__}, msgpack {msgpack.version}, pyzmq {zmq.__version__}")
print(f"minisgl from {minisgl.message.__file__}")
print(f"rsglang {rsglang.__version__} from {rsglang.__file__}")
SMOKE
