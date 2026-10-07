---
phase: 04-tokenizer-detokenizer-parity
plan: 01
subsystem: tokenizer
tags: [rust, tokenizers, hf-hub, parity, fixtures]

requires:
  - phase: 01-vendored-base-wire-codec
    provides: rsg-wire crate conventions (lib.rs doc-comment/error-enum pattern, fixtures_dir/manifest test helpers), gen_wire_fixtures.py's EnvError/origin-check/--check pattern, Mac-only CPython venv bootstrap
provides:
  - crates/rsg-tokenizer crate (workspace member) with ModelSpec/QWEN3_0_6B/TokenizerError and real loader.rs/encode.rs implementations
  - scripts/gen_tokenizer_fixtures.py CLI (--out/--check, plugin-discovered corpus_*.py modules, EnvError + origin-check guard)
  - scripts/tokenizer_fixtures package (models.py MODELS registry, corpus_ids.py 16-case D-08 corpus)
  - committed golden fixtures fixtures/tokenizer/id_corpus.json and fixtures/tokenizer/qwen3-0.6b/token_ids.json, generated from a live hf-hub fetch of the real Qwen/Qwen3-0.6B tokenizer
  - scripts/check_all.sh step 4 (tokenizer fixture freshness), now a 6-step gate
affects: [04-02-chat-template-rendering, 04-03-detokenization, 04-04-llama-model-support, 04-05, 04-06]

actuals:
  tokens: 38573
  tasks: 2
  commits: 3
  plan_head_before: 1431aa96519fea75404248288e50a24f7fe041fd
  plan_head_after: c45e370edba1203393c68252835ba7d1c3933ddc

tech-stack:
  added:
    - "tokenizers =0.22.2 (default-features = false, features = [onig, esaxx_fast])"
    - "hf-hub 1.0.0 (feature: blocking) — HFClientSync/HFRepositorySync blocking API, confirmed via docs.rs and the crate's own Cargo.toml (`blocking = [\"tokio/rt\"]`)"
    - "minijinja 2.24.0 (features: json, loader) and minijinja-contrib 2.24.0 (feature: pycompat) — added to workspace deps now, wired up starting Plan 04-02"
    - "proptest 1.11.0 — added to workspace deps now, used starting Plan 04-03"
  patterns:
    - "ModelSpec-parametrized loader/encode functions from the start (assumption-delta PROMOTE decision in the plan), rather than Qwen3-hardcoded functions re-derived under TOK-04"
    - "rsg-tokenizer mirrors rsg-wire's crate shape exactly: single TokenizerError enum with #[from] conversions, workspace-relative Cargo.toml deps, no pub use re-exports"
    - "Python tokenizer_fixtures package (models.py MODELS, corpus_*.py plugin modules discovered via pkgutil.iter_modules) is the single source of truth for fixture content, mirrored by hand in Rust's tests/common/mod.rs MODELS"

key-files:
  created:
    - crates/rsg-tokenizer/Cargo.toml
    - crates/rsg-tokenizer/src/lib.rs
    - crates/rsg-tokenizer/src/loader.rs
    - crates/rsg-tokenizer/src/encode.rs
    - crates/rsg-tokenizer/src/template.rs
    - crates/rsg-tokenizer/src/detokenize.rs
    - crates/rsg-tokenizer/tests/common/mod.rs
    - crates/rsg-tokenizer/tests/token_ids.rs
    - scripts/gen_tokenizer_fixtures.py
    - scripts/tokenizer_fixtures/__init__.py
    - scripts/tokenizer_fixtures/models.py
    - scripts/tokenizer_fixtures/corpus_ids.py
    - fixtures/tokenizer/id_corpus.json
    - fixtures/tokenizer/qwen3-0.6b/token_ids.json
  modified:
    - Cargo.toml (5 new workspace.dependencies entries)
    - scripts/check_all.sh (new step 4, renumbered to 6 steps total)

key-decisions:
  - "hf-hub 1.0.0's blocking API is HFClientSync::new()? -> client.model(owner, name) -> HFRepositorySync<RepoTypeModel> -> .download_file().filename(name).send()? -> HFResult<PathBuf>, confirmed directly from docs.rs and the crate's own Cargo.toml (blocking = [\"tokio/rt\"]), not RESEARCH.md's async-chain sketch."
  - "tokenizers 0.22.2 has no dedicated Error type; its Result alias is Result<T, Box<dyn std::error::Error + Send + Sync>>, confirmed via docs.rs. TokenizerError::Tokenizer(#[from] Box<dyn std::error::Error + Send + Sync>) wraps it directly."
  - "special_tokens_map.json is fetched best-effort (.ok(), defaulting to Value::Null), not as a hard requirement, because Qwen/Qwen3-0.6B's repo has no such file (confirmed via direct HTTP HEAD, 404) and AutoTokenizer.from_pretrained tolerates its absence -- matching upstream's real graceful-degradation behavior rather than the plan text's literal 'fetch X/Y/Z' phrasing."

requirements-completed: [TOK-01]

coverage:
  - id: D1
    description: "crates/rsg-tokenizer crate compiles and exposes ModelSpec/QWEN3_0_6B/TokenizerError plus working loader::load_model_assets and encode::encode_text"
    requirement: "TOK-01"
    verification:
      - kind: unit
        ref: "cargo test -p rsg-tokenizer --test token_ids -- token_ids_match_python_oracle_for_every_model"
        status: pass
      - kind: unit
        ref: "cargo test -p rsg-tokenizer -- tests::qwen3_model_spec_is_well_formed"
        status: pass
    human_judgment: false
  - id: D2
    description: "Rust token ids for Qwen3-0.6B match the Python oracle (load_tokenizer() + tokenizer.encode) byte-for-byte on all 16 D-08 corpus cases, via a live hf-hub fetch of the real repo"
    requirement: "TOK-01"
    verification:
      - kind: integration
        ref: "cargo test -p rsg-tokenizer --test token_ids (network-dependent: fetches Qwen/Qwen3-0.6B from huggingface.co)"
        status: pass
    human_judgment: false
  - id: D3
    description: "scripts/gen_tokenizer_fixtures.py generates/checks fixtures with an origin-check guard that rejects a non-vendored minisgl stand-in"
    verification:
      - kind: automated_ui
        ref: ".venv/bin/python scripts/gen_tokenizer_fixtures.py --check (exit 0)"
        status: pass
      - kind: other
        ref: "manual spot-check: _load_upstream() against a /tmp stand-in minisgl package raised EnvError as expected"
        status: pass
    human_judgment: false
  - id: D4
    description: "scripts/check_all.sh gates on tokenizer fixture freshness as step 4 of a 6-step sequence"
    verification:
      - kind: unit
        ref: "bash -n scripts/check_all.sh && grep -c '^step ' scripts/check_all.sh (== 6)"
        status: pass
    human_judgment: false

duration: ~50min
completed: 2026-10-06
status: complete
---

# Phase 04 Plan 01: Tracer — Qwen3-0.6B Token-ID Parity, End to End Summary

**Stood up `crates/rsg-tokenizer` from nothing and proved TOK-01 token-id parity for Qwen3-0.6B end to end: a live `hf-hub` blocking fetch of the real `Qwen/Qwen3-0.6B` tokenizer assets, through `tokenizers::Tokenizer::encode`, verified byte-for-byte against ids a vendored-`load_tokenizer()`-powered Python oracle produced, across all 16 D-08 corpus cases — with the generator's origin-check guard and the Mac gate's fixture-freshness step both wired and verified.**

## Performance
- **Duration:** ~50min
- **Started:** 2026-10-06 (approx. 18:26 UTC)
- **Completed:** 2026-10-06T19:16:55Z
- **Tasks:** 2 completed
- **Files modified:** 17 (14 created, 3 modified; excludes the mechanically-regenerated Cargo.lock)

## Accomplishments
- `crates/rsg-tokenizer` exists as a workspace member, compiling with zero warnings, exposing `ModelSpec`/`QWEN3_0_6B`/`TokenizerError` and working `loader::load_model_assets`/`encode::encode_text`.
- `scripts/gen_tokenizer_fixtures.py` + `scripts/tokenizer_fixtures/{models,corpus_ids}.py` generate the full 16-case D-08 corpus from a live hf-hub fetch, with a `--check` regen-and-diff mode and an origin-check guard mirroring `gen_wire_fixtures.py` exactly (verified against a hostile `/tmp` stand-in `minisgl` package).
- `cargo test -p rsg-tokenizer --test token_ids` passes for all 16 cases against the committed `fixtures/tokenizer/qwen3-0.6b/token_ids.json`.
- `scripts/check_all.sh` now gates on tokenizer-fixture freshness as its own numbered step (6-step gate total).
- Bootstrapped a fresh project-local `.venv` in this worktree via `scripts/bootstrap_mac_env.sh` (the worktree had none; `.venv` is gitignored and per-checkout) — confirmed `minisgl` resolves to the vendored tree, not a stray install.

## Task Commits
1. **Task 1: Tracer — Qwen3-0.6B token-id parity, end to end** — RED: `3bae4f1` (test), GREEN: `0a974f0` (feat)
2. **Task 2: Wire tokenizer-fixture freshness into the Mac gate** — `c45e370` (feat)

**Plan metadata:** commit pending (this SUMMARY + STATE/ROADMAP/REQUIREMENTS update)

## Files Created/Modified
- `crates/rsg-tokenizer/Cargo.toml` — new crate manifest, workspace-relative deps mirroring `rsg-wire`'s shape.
- `crates/rsg-tokenizer/src/lib.rs` — `ModelSpec`, `QWEN3_0_6B`, `TokenizerError` (6 variants incl. forward-declared `MissingChatTemplate`/`GatedAccessUnavailable`/`BosCountMismatch` for later plans), module declarations.
- `crates/rsg-tokenizer/src/loader.rs` — `ModelAssets`, `load_model_assets`: hf-hub blocking fetch of `tokenizer.json`/`tokenizer_config.json` (hard errors), best-effort `special_tokens_map.json`/`chat_template.json` fallback (`.ok()`, matching upstream's `try/except: pass`).
- `crates/rsg-tokenizer/src/encode.rs` — `encode_text`: `tokenizer.encode(text, true)` (HF default `add_special_tokens=true`).
- `crates/rsg-tokenizer/src/template.rs`, `src/detokenize.rs` — placeholder modules for Plan 04-02/04-03.
- `crates/rsg-tokenizer/tests/common/mod.rs` — `ModelCase`, `MODELS`, `fixtures_dir`, `model_fixture`.
- `crates/rsg-tokenizer/tests/token_ids.rs` — the TOK-01 fixture-driven parity test.
- `scripts/gen_tokenizer_fixtures.py` — CLI (`--out`/`--check`), `EnvError`, origin-check guard, plugin discovery over `corpus_*.py`.
- `scripts/tokenizer_fixtures/models.py`, `corpus_ids.py` — model registry and the 16 D-08 corpus cases.
- `fixtures/tokenizer/id_corpus.json`, `fixtures/tokenizer/qwen3-0.6b/token_ids.json` — committed golden fixtures from a live fetch.
- `Cargo.toml` — added `tokenizers`, `minijinja`, `minijinja-contrib`, `hf-hub`, `proptest` to `[workspace.dependencies]`.
- `scripts/check_all.sh` — new step 4 (tokenizer fixture freshness), renumbered to 6 steps.

## Decisions Made
- **hf-hub 1.0.0 blocking API shape** confirmed directly via docs.rs (not assumed from RESEARCH.md's async sketch): `HFClientSync::new()?.model(owner, name).download_file().filename(name).send()? -> PathBuf`. The `blocking` feature maps to `tokio/rt` only (confirmed from the crate's own published `Cargo.toml`).
- **`tokenizers` crate error type**: no dedicated `tokenizers::Error` exists; its `Result<T>` alias uses `Box<dyn std::error::Error + Send + Sync>`, confirmed via docs.rs. `TokenizerError::Tokenizer` wraps that boxed trait object directly via `#[from]`.
- **`special_tokens_map.json` is best-effort, not required** (see Deviations) — Qwen3-0.6B's repo has no such file.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `special_tokens_map.json` fetch was a hard error for a model repo that doesn't have the file**
- **Found during:** Task 1, GREEN phase (first `cargo test` run against the real hf-hub fetch)
- **Issue:** The plan's action text said to fetch `tokenizer.json`, `tokenizer_config.json`, and `special_tokens_map.json` (D-06) via `hf-hub`, without flagging the third as optional. `Qwen/Qwen3-0.6B`'s repo returns HTTP 404 for `special_tokens_map.json` (confirmed via direct `curl` HEAD request) — its special tokens live entirely inline in `tokenizer_config.json`. Treating the fetch as a hard `?` caused `load_model_assets` to fail with `TokenizerError::HfHub(EntryNotFound)` for every case.
- **Fix:** Changed the `special_tokens_map.json` fetch to best-effort (`.ok()`, same pattern as the `chat_template.json` fallback), defaulting `special_tokens_map` to `Value::Null` when absent — matching `AutoTokenizer.from_pretrained`'s own graceful tolerance of a missing file, which is what `load_tokenizer()` actually relies on (upstream never explicitly fetches this file itself; `AutoTokenizer` handles it internally).
- **Files modified:** `crates/rsg-tokenizer/src/loader.rs`
- **Verification:** `cargo test -p rsg-tokenizer --test token_ids` passes all 16 cases after the fix.
- **Committed in:** `0a974f0`

---
**Total deviations:** 1 auto-fixed (1 Rule 1 bug fix).
**Impact on plan:** None on scope or requirements — TOK-01 parity holds exactly as specified; the fix makes the loader match upstream's actual (already-correct) graceful-degradation behavior rather than a stricter reading of the plan's prose that upstream itself doesn't enforce. No prohibition was violated: `tokenizer.json`/`tokenizer_config.json` (the files that must exist for a valid tokenizer) remain hard errors on fetch failure.

## Issues Encountered
- This worktree had no `.venv` (gitignored, per-checkout) — bootstrapped one via the existing `scripts/bootstrap_mac_env.sh`, which synced `requirements-mac.txt` (confirms `tokenizers==0.22.2` matches the Rust pin) and editable-installed the vendored `minisgl` + `rsglang`. No code changes; a one-time environment setup step, not a deviation.
- `minisgl` is a PEP 420 namespace package (no `__init__.py`) in the vendored tree. Spot-checking the origin-check guard with a hostile stand-in required the stand-in to be a *regular* package (with `__init__.py`) to actually exercise the "wrong origin" branch, since a namespace-package stand-in would merge path portions rather than shadow outright. The guard behaved correctly once this was understood — no code change needed, confirms the guard matches `gen_wire_fixtures.py`'s identical, already-proven pattern.

## User Setup Required
None — no external service configuration required. (Network access to `huggingface.co` was available and used directly in this session; no `HF_TOKEN` was needed since `Qwen/Qwen3-0.6B` is a public, non-gated repo per D-01/D-02.)

## Next Phase Readiness
- `crates/rsg-tokenizer`'s module skeleton (`template.rs`, `detokenize.rs` placeholders; `ModelSpec`-parametrized `loader`/`encode`; the full `TokenizerError` enum with forward-declared variants) is ready for Plan 04-02 (chat-template rendering, TOK-02) and Plan 04-03 (incremental detokenization, TOK-03) to fill in without touching `lib.rs`'s structure.
- `scripts/tokenizer_fixtures/models.py`'s `MODELS` list and `crates/rsg-tokenizer/tests/common/mod.rs`'s `MODELS` list are both single-entry (`qwen3-0.6b` only) and ready for Plan 04-04/04-05 to append `llama-3.2-1b-instruct` to both, per the assumption-delta PROMOTE decision.
- No blockers. The hf-hub blocking API shape, the `tokenizers` error type, and the fixture-plugin layout are all now concretely proven (the exact purpose of this tracer plan) — no architectural dead-ends found.

## Known Stubs
- `crates/rsg-tokenizer/src/template.rs` and `src/detokenize.rs` are intentionally empty placeholder modules (one-line doc comments only), explicitly scoped to Plan 04-02 and Plan 04-03 respectively per this plan's own objective. Not a gap — by design.

---
*Phase: 04-tokenizer-detokenizer-parity*
*Completed: 2026-10-06*

## Self-Check: PASSED

All 14 created files confirmed present on disk; all 3 task commits (`3bae4f1`, `0a974f0`, `c45e370`) confirmed in `git log`.
