---
phase: 01-vendored-base-wire-codec
plan: 01
subsystem: infra
tags: [vendoring, mini-sglang, uv, python-env, pytest, supply-chain]

requires: []
provides:
  - "vendor/mini-sglang/: pristine upstream mini-sglang @ 9a91cfa (tree 02d3e4ad...), MIT LICENSE intact"
  - "UPSTREAM.md: source record, frozen-path tiers A/B/C, empty machine-parseable modified-files table"
  - "pyproject.toml: rsglang package (package-dir python/), uv managed=false, pytest never collects vendor/"
  - "requirements-mac.in + requirements-mac.txt: human-approved, sha256-hashed Mac dev lock"
  - "scripts/bootstrap_mac_env.sh: idempotent project-local .venv bootstrap"
  - "python/rsglang/__init__.py: importable rsglang package root"
affects: [01-03-launcher, 01-04-rsg-wire, 01-05-fixtures, 01-06-check-upstream, phase-04-tokenizer]

actuals:
  tokens: 123294      # chars/4 over all plan-01-01 files (493177 chars). Vendored upstream ~103.8k + generated lock ~19.4k; hand-authored files only ~1.4k (5612 chars)
  tasks: 3
  commits: 7          # MEASURED rev-list 66bc454..603aa4d; includes 5 interleaved plan 01-02 commits (120541f..84fd703). Plan 01-01's own: 2 (bfc0bbd, 603aa4d)
plan_head_before: 66bc45442400fff6e75b94245d24b229001bfcc5
plan_head_after: 603aa4db691e7d89e40d1317a1a0e4645896b27c

tech-stack:
  added: [uv 0.9.2 (tool), Python 3.12.12, torch 2.9.1 (CPU), numpy 2.5.3, msgpack 1.2.3, pyzmq 27.2.0, transformers 4.57.3, tokenizers 0.22.2 (transitive), pytest 9.1.1, setuptools, wheel]
  patterns:
    - "Vendoring by git archive of a full SHA plus a tree-hash proof (git write-tree --prefix=vendor/mini-sglang/)"
    - "Mac env: uv venv + uv pip sync of a hash-pinned lock + --no-deps editable installs (never upstream's full CUDA deps)"
    - "Root pytest config is the inifile; vendor/ is never collected"

key-files:
  created:
    - vendor/mini-sglang/ (121 entries)
    - UPSTREAM.md
    - pyproject.toml
    - requirements-mac.in
    - requirements-mac.txt
    - scripts/bootstrap_mac_env.sh
    - python/rsglang/__init__.py
  modified:
    - .gitignore

key-decisions:
  - "Mac dev env is a project-local uv-managed .venv (gitignored); nothing is installed into the system or user Python (user preference at the Task 2 checkpoint)"
  - "Package gate approved by the human: torch==2.9.1, numpy==2.5.3, msgpack==1.2.3, pyzmq==27.2.0, transformers==4.57.3, pytest==9.1.1, setuptools>=61.0 + wheel, their uv-resolved transitive deps (hash-locked), and crates.io thiserror 2.0.21 for plan 01-04"
  - "requirements-mac.txt resolves tokenizers 0.22.2 via transformers 4.57.3, matching the Rust tokenizers =0.22.2 pin"

patterns-established:
  - "Rerun scripts/bootstrap_mac_env.sh per checkout/worktree; editable installs must point at this checkout's vendor/"
  - "Any new Python dependency goes into requirements-mac.in and is relocked with --relock (after a legitimacy check)"

requirements-completed: [BASE-01]

coverage:
  - id: D1
    description: "vendor/mini-sglang/ is a byte-exact copy of upstream 9a91cfa (whole repo, symlink and MIT LICENSE intact)"
    requirement: BASE-01
    verification:
      - kind: other
        ref: "test \"$(git rev-parse HEAD:vendor/mini-sglang)\" = 02d3e4ad34ec00c88f549fd9d287a4588958d824 && test -L vendor/mini-sglang/.dockerignore && grep -q 'Copyright (c) 2026 sgl-project' vendor/mini-sglang/LICENSE"
        status: pass
    human_judgment: false
  - id: D2
    description: "UPSTREAM.md records repo URL, full SHA, frozen-path tiers, and an empty '## Modified files' table with the exact header"
    requirement: BASE-01
    verification:
      - kind: other
        ref: "grep -q 9a91cfafe754aa85daee49998176275667eb58f2 UPSTREAM.md && grep -qxF '| Path | Reason | Shared backend fix (yes/no) |' UPSTREAM.md"
        status: pass
    human_judgment: false
  - id: D3
    description: "Human package-legitimacy gate passed before any install"
    verification: []
    human_judgment: true
    rationale: "Approval is a human act recorded in-session ('approve (use appropriate virtual environemnt such as uv ...)'); no automated check can prove it"
  - id: D4
    description: "Reproducible Mac .venv (Python 3.12) imports minisgl.{message,core,utils,scheduler,server.args} and rsglang from this checkout with no CUDA packages; bootstrap is idempotent"
    verification:
      - kind: other
        ref: ".venv/bin/python -c \"import minisgl.message, minisgl.core, minisgl.utils, minisgl.scheduler, minisgl.server.args, rsglang, torch, msgpack, zmq ... print('env-ok')\" (Task 3 verify)"
        status: pass
      - kind: other
        ref: "bash scripts/bootstrap_mac_env.sh (second run exits 0)"
        status: pass
    human_judgment: false

duration: 14min
completed: 2026-10-04
status: complete
---

# Phase 1 Plan 01: Vendored Base & Mac Env Summary

**Pristine upstream mini-sglang @ 9a91cfa vendored at vendor/mini-sglang/ (tree 02d3e4ad proven, MIT LICENSE intact), recorded in UPSTREAM.md, plus a human-approved, sha256-locked uv .venv on the Mac that imports the vendored backend without CUDA.**

## Performance

- **Duration:** 14 min wall-clock (includes the Task 2 human checkpoint wait and the interleaved plan 01-02 run)
- **Started:** 2026-10-04T02:39:46Z
- **Completed:** 2026-10-04T02:54:14Z
- **Tasks:** 3 (2 auto, 1 blocking-human checkpoint)
- **Files modified:** 128 (121 vendored entries, UPSTREAM.md, .gitignore, 5 Task 3 files)

## Accomplishments

- Whole upstream repo vendored by `git archive` of the full SHA; `git rev-parse HEAD:vendor/mini-sglang` equals upstream's root tree `02d3e4ad34ec00c88f549fd9d287a4588958d824`, so zero vendored files are modified. `.dockerignore` is still a symlink and the LICENSE reads "Copyright (c) 2026 sgl-project".
- UPSTREAM.md records the source, the license, the frozen-path tiers (A: frontend, B: shared wire/config, C: benchmarks) and a `## Modified files` table with zero data rows, ready for `scripts/check_upstream.py` (plan 01-06).
- The package-legitimacy gate passed before any install (see "Human Approval" below).
- Root `pyproject.toml` (rsglang, `managed = false`, pytest `testpaths = ["python/tests"]` with `vendor` in `norecursedirs`), `requirements-mac.in` (6 direct pins) and `requirements-mac.txt` (31 packages, 895 sha256 hashes).
- `scripts/bootstrap_mac_env.sh` builds `.venv` (Python 3.12.12), syncs the lock, editable-installs `vendor/mini-sglang` and `.` with `--no-deps`, and smoke-imports the backend. It supports `--relock` and `--help`, and a second run exits 0.

## Human Approval (Task 2, gate=blocking-human)

The user approved this in-session with: "approve (use appropriate virtual environemnt such as uv for less package dependency management)". Approved set:
- PyPI direct pins: torch==2.9.1, numpy==2.5.3, msgpack==1.2.3, pyzmq==27.2.0, transformers==4.57.3, pytest==9.1.1
- Build backends: setuptools>=61.0, wheel
- Their uv-resolved transitive deps, frozen with sha256 hashes into requirements-mac.txt
- crates.io: thiserror 2.0.21 (for plan 01-04)

The user also asked that everything go into a project-local uv-managed `.venv`. It does: `sys.prefix` is `/Users/li-yangtseng/Codes/mini-rsglang/.venv`, and nothing was installed globally or into the user site. The resolved transitive set contains only dependencies of the approved pins: certifi, charset-normalizer, filelock, fsspec, hf-xet, huggingface-hub, idna, iniconfig, jinja2, markupsafe, mpmath, networkx, packaging, pluggy, pygments, pyyaml, regex, requests, safetensors, setuptools, sympy, tokenizers, tqdm, typing-extensions, urllib3. No direct dependency beyond the approved list was needed.

## Task Commits

1. **Task 1: Vendor upstream mini-sglang @ 9a91cfa and record it in UPSTREAM.md** - `bfc0bbd` (feat)
2. **Task 2: Package legitimacy gate before any install** - no commit (human checkpoint, approved)
3. **Task 3: Root pyproject, hash-pinned Mac lock and the env bootstrap script** - `603aa4d` (chore)

## Files Created/Modified

- `vendor/mini-sglang/` - pristine upstream @ 9a91cfa (121 entries)
- `UPSTREAM.md` - source record, frozen tiers, empty modified-files table
- `.gitignore` - adds `/target/`, `/.venv/`, `__pycache__/`, `*.egg-info/`, `.pytest_cache/`
- `pyproject.toml` - rsglang package, uv unmanaged, pytest config excluding vendor/
- `requirements-mac.in` - the six approved direct pins
- `requirements-mac.txt` - uv-compiled, hash-pinned lock (Python 3.12, aarch64-apple-darwin)
- `scripts/bootstrap_mac_env.sh` - idempotent Mac env bootstrap
- `python/rsglang/__init__.py` - package root, `__version__ = "0.1.0"`

## Decisions Made

- Project-local uv `.venv` only, per the user's checkpoint instruction. This matches the plan's design.
- tokenizers resolves to 0.22.2 in the lock, consistent with the Rust `tokenizers =0.22.2` pin.

## Deviations from Plan

None. The plan was executed as written.

## Issues Encountered

- None blocking. Note: `uv pip sync` removes the two editable installs on every run because they are not in the lock, and the next two steps reinstall them. Reruns therefore cost a couple of seconds but stay idempotent.
- The `actuals.commits` value (7) is the measured `rev-list` count from the ledger base. It includes the five plan 01-02 commits that ran during the Task 2 checkpoint pause. Plan 01-01 itself made 2 code commits.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plans 01-03 (launcher), 01-05 (fixtures) and 01-06 (check_upstream) can import `minisgl` from `.venv`. Worktree executors must rerun `scripts/bootstrap_mac_env.sh` in their own checkout.
- BASE-01 is also declared by plan 01-06, so it stays open until 01-06 ships `scripts/check_upstream.py`.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED
