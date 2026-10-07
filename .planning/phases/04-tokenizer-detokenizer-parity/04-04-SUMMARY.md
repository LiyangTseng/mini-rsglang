---
phase: 04-tokenizer-detokenizer-parity
plan: 04
subsystem: tokenizer
tags: [chrono, hf-hub, gated-access, minijinja, llama, rsg-tokenizer]

# Dependency graph
requires:
  - phase: 04-tokenizer-detokenizer-parity
    provides: "Plan 04-02's template.rs stub (build_environment), Plan 04-03's detokenizer and lib.rs's ModelSpec/TokenizerError scaffolding"
provides:
  - "chrono workspace dependency, approved via blocking-human package-legitimacy checkpoint"
  - "LLAMA_3_2_1B_INSTRUCT ModelSpec (lib.rs) and matching models.py entry, gated=True"
  - "load_model_assets gated-access detection: GatedAccessUnavailable on missing credentials (no network call) or on a rejected 401/403 token (bounded 15s timeout), never a hang or silent wrong-model fallback"
  - "build_environment(now_override: Option<String>): real chrono::Local::now() in production, test/fixture-only fixed-string override for strftime_now"
  - "Live canonical meta-llama/Llama-3.2-1B-Instruct spot-check, run against the real gated repo after the human's access grant: 3 of 4 RESEARCH.md mirror-sourced facts confirmed exactly; one divergence found and documented (add_bos_token absent, not Some(true)) with the underlying double-BOS mechanism re-confirmed via tokenizer.json's post-processor"
affects: ["04-05-PLAN (Llama fixture generation)", "04-06-PLAN (TOK-04 BOS-count/clean_up_tokenization_spaces parity tests)"]

# Actuals (#2632)
actuals:
  tokens: 7851
  tasks: 2
  commits: 1
  plan_head_before: c019a8713a3a23e554de9ed291772a8f6461643e
  plan_head_after: 63b6a4b

# Tech tracking
tech-stack:
  added: ["chrono 0.4.45 (strftime_now backing)"]
  patterns:
    - "Gated-model loading: check ambient credentials before any network call; convert auth-shaped hf-hub errors (401/403) into one distinguishable TokenizerError variant so callers can skip cleanly (D-04)"
    - "Clock injection via an explicit Option<String> parameter threaded only from test/fixture-generation call sites, never a hidden default — avoids silently freezing production time"

key-files:
  created: []
  modified:
    - "Cargo.toml"
    - "crates/rsg-tokenizer/Cargo.toml"
    - "crates/rsg-tokenizer/src/lib.rs"
    - "crates/rsg-tokenizer/src/loader.rs"
    - "crates/rsg-tokenizer/src/template.rs"
    - "crates/rsg-tokenizer/tests/chat_templates.rs"
    - "scripts/tokenizer_fixtures/models.py"

key-decisions:
  - "chrono approved via blocking-human package-legitimacy checkpoint (Task 1) before being added to the workspace — it was absent from RESEARCH.md's audited six, so it was never silently approved alongside them."
  - "LLAMA_3_2_1B_INSTRUCT placed in lib.rs next to QWEN3_0_6B (where that constant actually lives), not loader.rs as the plan's action text said — a stale file-location reference in the plan, corrected as a Rule 1 deviation."
  - "Canonical re-fetch of meta-llama/Llama-3.2-1B-Instruct's tokenizer_config.json (RESEARCH.md Open Question 2) found one real divergence from the unsloth mirror: add_bos_token is entirely absent from the canonical file (not Some(true)). The double-BOS risk Pitfall 2 describes is still real — tokenizer.json's own post-processor (a TemplateProcessing step) unconditionally prepends <|begin_of_text|> for both single and pair encodings, independent of the add_bos_token key — confirmed directly against the live tokenizer via a new canonical_llama_post_processor_prepends_bos_once test. Plan 04-05/04-06 should rely on the post-processor mechanism, not on tokenizer_config.json's add_bos_token key, when reasoning about Llama's BOS handling."

patterns-established:
  - "Pattern: ignore-gated, human-run-only spot-check tests for one-time verification against a gated external resource, documented in-source with findings from the actual run rather than left as an untested assumption."

requirements-completed: [TOK-04]

coverage:
  - id: D1
    description: "chrono approved and wired into the workspace, backing a real production strftime_now"
    requirement: "TOK-04"
    verification:
      - kind: unit
        ref: "crates/rsg-tokenizer/src/template.rs#chrono_strftime_matches_python_format"
        status: pass
      - kind: unit
        ref: "crates/rsg-tokenizer/src/template.rs#strftime_now_without_override_calls_the_real_clock"
        status: pass
    human_judgment: false
  - id: D2
    description: "load_model_assets returns GatedAccessUnavailable fast (no hang) when HF_TOKEN/cached credentials are absent, and when an implicit token is disabled or blank"
    requirement: "TOK-04"
    verification:
      - kind: unit
        ref: "crates/rsg-tokenizer/src/loader.rs#gated_access_unavailable_without_token"
        status: pass
      - kind: unit
        ref: "crates/rsg-tokenizer/src/loader.rs#gated_access_unavailable_with_blank_token_file"
        status: pass
      - kind: unit
        ref: "crates/rsg-tokenizer/src/loader.rs#gated_access_unavailable_when_implicit_token_disabled"
        status: pass
    human_judgment: false
  - id: D3
    description: "Canonical meta-llama/Llama-3.2-1B-Instruct spot-check run live against the real gated repo after human-granted access; chat_template/add_bos_token/clean_up_tokenization_spaces findings recorded, with one divergence from RESEARCH.md's mirror-sourced assumptions documented and resolved"
    requirement: "TOK-04"
    verification:
      - kind: unit
        ref: "crates/rsg-tokenizer/src/loader.rs#canonical_llama_config_spot_check (run manually with --ignored; requires live HF_TOKEN)"
        status: pass
      - kind: unit
        ref: "crates/rsg-tokenizer/src/loader.rs#canonical_llama_post_processor_prepends_bos_once (run manually with --ignored; requires live HF_TOKEN)"
        status: pass
    human_judgment: false
  - id: D4
    description: "build_environment(now_override) threading: Some(fixed) freezes strftime_now for fixture/test use, None calls the real clock in production, never the reverse by default"
    requirement: "TOK-04"
    verification:
      - kind: unit
        ref: "crates/rsg-tokenizer/src/template.rs#strftime_now_override_returns_fixed_string_regardless_of_format"
        status: pass
    human_judgment: false

duration: ~20min (this continuation session; Task 1's checkpoint and the prior agent's uncommitted implementation happened in earlier sessions)
completed: 2026-10-07
status: complete
---

# Phase 4 Plan 4: Llama Registry + Gated-Access Foundation Summary

**chrono-backed strftime_now, hf-hub gated-access detection with a bounded-timeout clean-skip path, and a live-verified canonical Llama-3.2-1B-Instruct tokenizer_config.json spot-check that found and resolved one real divergence from RESEARCH.md's mirror-sourced assumptions**

## Performance

- **Duration:** ~20 min (this continuation session)
- **Completed:** 2026-10-07T00:31:54Z
- **Tasks:** 2 (Task 1: package-legitimacy checkpoint, approved in a prior session; Task 2: registry/gated-access/clock, completed this session)
- **Files modified:** 8 (7 code/config + Cargo.lock)

## Accomplishments

- `chrono = "0.4.45"` approved via Task 1's blocking-human package-legitimacy checkpoint (verdict `OK`: github.com/chronotope/chrono, ~14.9M weekly downloads, not deprecated) and wired into the workspace.
- `LLAMA_3_2_1B_INSTRUCT: ModelSpec` added in `lib.rs` (alongside `QWEN3_0_6B`) and the matching `llama-3.2-1b-instruct` entry added to `scripts/tokenizer_fixtures/models.py` (D-01).
- `load_model_assets` now detects gated-access unavailability before and during the fetch: no ambient credential (checked via the same `HF_TOKEN` → `HF_TOKEN_PATH` → `$HF_HOME/token` precedence hf-hub itself uses, honoring `HF_HUB_DISABLE_IMPLICIT_TOKEN`) short-circuits immediately with zero network calls; a present-but-rejected token (401/403, via a bounded 15-second-timeout `reqwest` client) converts to the same `TokenizerError::GatedAccessUnavailable` variant — both paths distinguishable from every other failure mode, satisfying D-04's clean-skip requirement.
- `template::build_environment(now_override: Option<String>)` replaces the Plan 04-02 stub: `None` calls `chrono::Local::now().format(&fmt)` for real in production; `Some(fixed)` returns `fixed` unconditionally, reserved for fixture-generation/test code paths only.
- **Live canonical spot-check, run after the human's access grant resolved the prior 403:** `canonical_llama_config_spot_check` and a new `canonical_llama_post_processor_prepends_bos_once` test both pass against the real `meta-llama/Llama-3.2-1B-Instruct` repo. Findings recorded below.
- `chrono`'s `"%d %b %Y"` formatting spot-checked against Python's `strftime` output for three dates spanning different months (Jan/Jul/Dec 2026) — byte-identical in all three cases.

## Canonical Spot-Check Findings (live run, 2026-10-07)

Fetched `meta-llama/Llama-3.2-1B-Instruct`'s real `tokenizer_config.json` and `tokenizer.json` via the author's now-working gated credentials (human-granted access, independently confirmed by the orchestrator moments before this session). Compared against RESEARCH.md's Pitfalls 2/3/5, which were sourced from the third-party `unsloth/Llama-3.2-1B-Instruct` mirror:

| Fact | RESEARCH.md assumption (mirror) | Canonical repo (live, this session) | Match? |
|---|---|---|---|
| `chat_template` contains `{{- bos_token }}` | yes | yes | ✅ matches |
| `chat_template` contains a `strftime_now(` call | yes | yes | ✅ matches |
| `clean_up_tokenization_spaces` | `true` | `true` | ✅ matches |
| `add_bos_token` | `true` (explicit key) | **key absent entirely** (not `false`, just missing) | ❌ diverges |

**Divergence resolution:** the canonical `tokenizer_config.json` simply does not set `add_bos_token` — unlike the `unsloth` mirror, which set it explicitly to `true`. This does **not** weaken Pitfall 2's double-BOS risk. `tokenizer.json`'s own `post_processor` field is a `Sequence` whose `TemplateProcessing` step unconditionally prepends the `<|begin_of_text|>` special token for both `single` and `pair` encodings — this is independent of any `tokenizer_config.json`-level flag, and the Rust `tokenizers` crate applies it automatically via `Tokenizer::encode(_, add_special_tokens: true)` (the same default upstream's `tokenize.py` uses). A new test, `canonical_llama_post_processor_prepends_bos_once`, confirms this directly: encoding `"hello"` with `add_special_tokens=true` against the live canonical tokenizer produces exactly one `<|begin_of_text|>` token, positioned first.

**Guidance for Plan 04-05/04-06:** rely on the post-processor mechanism (confirmed above) for BOS-handling assumptions, not on `add_bos_token`'s presence in `tokenizer_config.json` — this live fetch proved that field is absent on the canonical repo. D-10's "assert BOS appears exactly once in final `input_ids`" test remains the correct assertion; the mechanism producing the risk (template-level `{{- bos_token }}` plus tokenizer-level auto-prepend) is confirmed real via a different file than RESEARCH.md assumed.

## Task Commits

Task 1 (package-legitimacy checkpoint) was a pure approval gate with no code changes — no commit associated with it, consistent with the plan's own text ("(none — pure approval gate)").

1. **Task 2: Llama registry entry, gated-access detection, and a test-overridable clock** — `63b6a4b` (feat)

**Plan metadata:** (this commit, created after this SUMMARY)

## Files Created/Modified

- `Cargo.toml` — added `chrono = "0.4.45"` and `reqwest = "0.13.5"` to `[workspace.dependencies]` (the latter needed for the gated-fetch timeout client; not in the plan's literal file list but required by its own action text's "bounded timeout" instruction — Rule 2).
- `crates/rsg-tokenizer/Cargo.toml` — `chrono.workspace = true`, `reqwest.workspace = true`, `tempfile.workspace = true` (dev-dependency, for the env-var-isolation test guard).
- `crates/rsg-tokenizer/src/lib.rs` — added `LLAMA_3_2_1B_INSTRUCT: ModelSpec` next to `QWEN3_0_6B`, plus a unit test confirming its shape.
- `crates/rsg-tokenizer/src/loader.rs` — `has_gated_credentials`, `is_auth_error`, `gated_or`, `build_client`, gated-access branch in `load_model_assets`; six new tests including the two live canonical spot-checks.
- `crates/rsg-tokenizer/src/template.rs` — `build_environment(now_override: Option<String>)`; four tests (one updated, three new) covering the override, the real-clock path, and the chrono/Python format spot-check.
- `crates/rsg-tokenizer/tests/chat_templates.rs` — updated the one call site to `build_environment(None)`.
- `scripts/tokenizer_fixtures/models.py` — added the `llama-3.2-1b-instruct` `ModelSpec` entry (`gated=True`).

## Decisions Made

- `chrono` approved via blocking-human checkpoint, never silently added — documented above.
- `LLAMA_3_2_1B_INSTRUCT` placed in `lib.rs`, not `loader.rs`, correcting the plan's stale file-location text (Rule 1).
- Canonical `add_bos_token` divergence resolved by treating the canonical repo's data as authoritative over RESEARCH.md's mirror-sourced assumption, per the plan's own instruction — recorded here and in the test's doc comments, with the underlying BOS-insertion mechanism re-verified via a different, more fundamental signal (the tokenizer's own post-processor) rather than left unresolved.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Corrected plan's stale file-location reference for `LLAMA_3_2_1B_INSTRUCT`**
- **Found during:** Task 2
- **Issue:** The plan's action text said to add the `ModelSpec` constant "next to `QWEN3_0_6B`" in `loader.rs`, but `QWEN3_0_6B` actually lives in `lib.rs` (set by Plan 04-01).
- **Fix:** Added `LLAMA_3_2_1B_INSTRUCT` in `lib.rs`, next to `QWEN3_0_6B`, as the plan's stated intent (co-locate the two `ModelSpec` constants) actually required.
- **Files modified:** `crates/rsg-tokenizer/src/lib.rs`
- **Verification:** `cargo build -p rsg-tokenizer` succeeds; `llama_model_spec_is_well_formed` unit test passes.
- **Committed in:** `63b6a4b`

**2. [Rule 2 - Missing Critical] Added `reqwest` as an explicit dependency for the gated-fetch timeout client**
- **Found during:** Task 2
- **Issue:** The plan's action text required "a bounded timeout on the gated fetch," but hf-hub's blocking client doesn't expose a standalone per-request timeout knob — only a way to supply a pre-configured `reqwest::Client`.
- **Fix:** Added `reqwest = "0.13.5"` to the workspace and `rsg-tokenizer`, building a `reqwest::Client` with a 15-second timeout and passing it to `HFClientBuilder::new().client(...)` for gated models only; ungated (Qwen3) fetches keep hf-hub's own default untimed client, unchanged from Plan 04-01.
- **Files modified:** `Cargo.toml`, `crates/rsg-tokenizer/Cargo.toml`, `crates/rsg-tokenizer/src/loader.rs`
- **Verification:** `gated_access_unavailable_without_token` passes and completes in well under 5 seconds (asserted directly in the test).
- **Committed in:** `63b6a4b`

**3. [Rule 1 - Bug] Corrected `canonical_llama_config_spot_check`'s `add_bos_token` assertion to match the live canonical value**
- **Found during:** this continuation session, re-running the ignored canonical spot-check test after the human's access grant
- **Issue:** The test (written by a prior agent before live gated access was available) hard-asserted `add_bos_token == Some(true)`, mirroring RESEARCH.md's third-party-mirror-sourced assumption. Running it against the real canonical repo failed: the field is absent entirely (`None`), not `Some(true)`.
- **Fix:** Investigated the discrepancy directly (read the cached canonical `tokenizer_config.json` and `tokenizer.json`), confirmed the double-BOS mechanism is still real via `tokenizer.json`'s own post-processor, updated the test's assertion to `None` with an explanatory comment, and added a new `canonical_llama_post_processor_prepends_bos_once` test that confirms the actual BOS-insertion mechanism directly against the live tokenizer (rather than leaving the open question unresolved).
- **Files modified:** `crates/rsg-tokenizer/src/loader.rs`
- **Verification:** Both `canonical_llama_config_spot_check` and `canonical_llama_post_processor_prepends_bos_once` pass against the live gated repo (run manually with `cargo test -p rsg-tokenizer --lib -- --ignored canonical_llama`).
- **Committed in:** `63b6a4b`

---

**Total deviations:** 3 auto-fixed (1 bug/stale-plan-reference, 1 missing-critical/timeout-dependency, 1 bug/test-assertion-corrected-against-live-data)
**Impact on plan:** All three were necessary for correctness — none represent scope creep. The third deviation is also this plan's core deliverable: resolving RESEARCH.md's Open Question 2 against real canonical data.

## Issues Encountered

None beyond the deviations documented above. The original blocker (403 `GatedRepo` error on the canonical fetch) was resolved by the human being granted gated access between sessions; this session re-verified that resolution by actually running the live network test rather than trusting the orchestrator's curl check alone, per the checkpoint-resolution instructions.

## User Setup Required

None — no external service configuration required beyond the already-completed HF gated-access grant (D-02), which the author had already set up.

## Next Phase Readiness

- Plan 04-05 (Llama fixture generation) can now rely on: a working gated loader, a `build_environment(Some(fixed))` clock freeze for deterministic fixtures, and the corrected understanding that Llama's double-BOS risk is driven by `tokenizer.json`'s post-processor, not `tokenizer_config.json`'s `add_bos_token` key (which the canonical repo doesn't set).
- Plan 04-06 (TOK-04 BOS-count/clean_up_tokenization_spaces parity tests) should assert BOS-count via the post-processor-driven mechanism (D-10), and can treat `clean_up_tokenization_spaces: true` as confirmed on the canonical repo.
- No blockers for either downstream plan.

---
*Phase: 04-tokenizer-detokenizer-parity*
*Completed: 2026-10-07*

## Self-Check: PASSED

All 7 claimed modified files and this SUMMARY.md confirmed present on disk via direct `[ -f ... ]` checks. Commit `63b6a4b` confirmed present via `git log --oneline --all | grep 63b6a4b`.
