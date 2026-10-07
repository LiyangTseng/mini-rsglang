---
phase: 05-request-lifecycle-http-api
plan: 09
subsystem: testing
tags: [api-parity, golden-fixtures, axum, tokio, http]

# Dependency graph
requires:
  - phase: 05-05
    provides: "fixtures/api/manifest.json + 15 .body files: the 18-case API-01 golden fixture set captured from a live run of upstream's frozen Python frontend against mock-scheduler"
  - phase: 05-08
    provides: "The real rsg-server binary (HfCodec, --host/--port, tests/common/rsg_process.rs's RsgServer harness) serving the real tokenizer/detokenizer end to end"
provides:
  - "crates/rsg-server/tests/api_parity.rs: api_parity_matches_python_frontend_fixtures replays all 18 fixtures/api cases against the real rsg-server binary on mock-scheduler, byte-diffing status/content-type/body (created normalized) against the frozen Python frontend's recorded output"
  - "scripts/check_all.sh step 5/7: API fixture freshness (gen_api_fixtures.py --check), keeping the Mac phase gate honest about both the Python oracle and the Rust frontend"
  - "A real engine.rs correctness fix: the per-request IncrementalDecoder is now built before register/submit, not after, closing a window where a zero-decode-delay backend could overflow the per-uid broadcast buffer before the decode loop's first recv()"
affects: []

# Actuals (#2632)
actuals:
  tokens: 3527
  tasks: 2
  commits: 2
  plan_head_before: 424deccc3f333d8cce6189f0b5fdd0a166b5f94e
  plan_head_after: f3e0983ed6aa196308202f07d61c22cd6727f9c3

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Replay-and-byte-diff parity test: reads the committed golden-fixture manifest at runtime (not compiled in), spawns the real production binaries (mock-scheduler + rsg-server) via the existing tests/common harness, and reproduces the generator's own normalization rule (normalize_created) independently in Rust rather than importing it — the same shape plan 05-05's gen_api_fixtures.py --check uses on the Python side"
    - "Build per-request heavyweight state (tokenizer clone) before touching the backend, not after: any one-time construction cost on the decode path must complete before submit() is sent, or a fast/unthrottled backend can race ahead of the very first read from a bounded per-uid channel"

key-files:
  created:
    - crates/rsg-server/tests/api_parity.rs
  modified:
    - crates/rsg-server/src/engine.rs
    - scripts/check_all.sh

key-decisions:
  - "Moved `let mut decoder = engine.codec.decoder();` from after submit (inside the loop setup) to immediately after the pre-submit cancellation check, before dispatch.register(uid): decoder() clones the whole ~150k-entry Qwen3-0.6B tokenizers::Tokenizer, which measured consistently slow enough (tens of ms) that mock-scheduler's --decode-delay-ms 0 could produce and dispatch an entire >16-token generation into the per-uid broadcast channel (capacity 16, drop-oldest, D-07) before the decode loop ever called recv() once — deterministically dropping exactly (tokens - 16) tokens for any response that long, confirmed by chat_stream_system_user (24 tokens, dropped 8) and chat_nonstream_multibyte (20 tokens, dropped 4) failing on every run before the fix and passing on every run after it"
  - "scripts/check_all.sh already had 6 steps (not the 5 the plan's interfaces note assumed) because Phase 4 had already inserted a tokenizer-fixture-freshness step; inserted the new API-fixture step in the documented position (right after wire-fixture freshness) and renumbered every label to /7, matching the file's own established step-labelling convention rather than the plan's stale snapshot"
  - "Verified the full scripts/check_all.sh --offline gate passes end to end (all 7 steps, including the new one) under RUST_TEST_THREADS=1 — a one-off verification invocation, not a change to the committed script — because crates/rsg-tokenizer's lib tests have a pre-existing, already-documented (.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md) concurrent-execution race unrelated to any file this plan touches; under default parallel test threads the gate's step 1 (cargo test --workspace) flakes on that race roughly 4 times out of 5 attempts observed in this session, never on anything this plan changed"

patterns-established:
  - "crates/rsg-server/tests/api_parity.rs's own normalize_created/describe_body_diff helpers are reusable building blocks for any future byte-for-byte parity test against a golden-fixture manifest with a declared normalization list"

requirements-completed: [API-01]

coverage:
  - id: D1
    description: "On the Mac, the rsg-server binary (real Qwen3-0.6B tokenizer, mock-scheduler with the manifest's exact flags) answers all 18 fixture cases, replayed in manifest order against a fresh server, with the same status code, content-type and byte-identical body (after created normalization) as the live Python frontend"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/api_parity.rs#api_parity_matches_python_frontend_fixtures"
        status: pass
    human_judgment: false
  - id: D2
    description: "Byte parity covers /v1/chat/completions (streaming and non-streaming), /generate (single-newline framing), /v1/models and /v1 (GET/POST/HEAD/OPTIONS), including multibyte/emoji detokenization, finish+EOS exclusion, and uid numbering that skips the 3 status-only error cases"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/api_parity.rs#api_parity_matches_python_frontend_fixtures (asserts case count == 18, iterates every case)"
        status: pass
    human_judgment: false
  - id: D3
    description: "scripts/check_all.sh runs gen_api_fixtures.py --check as a fixture-freshness step, and cargo test --workspace (step 1) runs the new Rust parity test, so a drift in either frontend fails the Mac phase gate"
    requirement: "API-01"
    verification:
      - kind: other
        ref: "scripts/check_all.sh --offline (RUST_TEST_THREADS=1, see key-decisions for why): prints check_all: OK across all 7 steps"
        status: pass
    human_judgment: false

duration: 70min
completed: 2026-10-07
status: complete
---

# Phase 5 Plan 9: API-01 Byte-Parity Test and Phase Gate Wiring Summary

**`crates/rsg-server/tests/api_parity.rs` replays all 18 golden fixtures from the frozen Python frontend against the real `rsg-server` binary and proves byte-identical responses, after fixing a real bug it exposed: the per-request tokenizer clone was built after `submit()` instead of before, letting a zero-decode-delay backend overflow the per-uid reply buffer before the decode loop ever read from it.**

## Performance

- **Duration:** ~70 min
- **Started:** 2026-10-07
- **Completed:** 2026-10-07
- **Tasks:** 2
- **Files modified:** 3 (1 created, 2 modified)

## Accomplishments
- `api_parity_matches_python_frontend_fixtures` reads `fixtures/api/manifest.json` at runtime, spawns `mock-scheduler` with the manifest's exact `mock_args` and the real `rsg-server` binary (real Qwen3-0.6B tokenizer), replays all 18 cases in fixed order over real HTTP/1.1 connections, and byte-diffs every `compare: "bytes"` case's status/content-type/body (after the same `created`-only normalization `scripts/gen_api_fixtures.py` applies) against the committed `.body` files — collecting every mismatch (not stopping at the first) and reporting the case name plus the first differing byte offset with 80 bytes of context on each side.
- Found and fixed a real concurrency bug while making the test pass: `HfCodec::decoder()` clones the entire Qwen3-0.6B `tokenizers::Tokenizer` (~150k vocab/merge entries), and building it *after* `register`/`submit` left a window where `mock-scheduler --decode-delay-ms 0` could stream an entire >16-token generation into the per-uid broadcast channel (fixed capacity 16, drop-oldest, D-07) before the decode loop's very first `recv()` call — deterministically dropping exactly `tokens - 16` tokens and failing the request as a "slow consumer" for any response over 16 tokens. Moving the clone to before `register`/`submit` (it never depended on backend state) fixed both previously-failing cases (`chat_stream_system_user`, 24 tokens, was dropping 8; `chat_nonstream_multibyte`, 20 tokens, was dropping 4) with zero behavior change to anything shorter.
- `scripts/check_all.sh` gained a 5th-of-7 step, "API fixture freshness (`gen_api_fixtures.py --check`)", in the documented position right after wire-fixture freshness; every step label was renumbered to `/7` (the file already had 6 steps from Phase 4's tokenizer-fixture step, one more than the plan's stale interfaces note assumed).
- Verified the whole gate end to end: all 7 steps pass, including `cargo test --workspace` (which now also runs the new parity test), `pytest`, all three fixture-freshness checks, WIRE-02 decode, and the vendored-tree check.

## Task Commits

Each task was committed atomically:

1. **Task 1: Tracer — replay the Python-frontend fixture cases against the rsg-server binary and require byte-identical responses** - `298124f` (feat)
2. **Task 2: Phase gate — API fixture freshness in check_all.sh, and the full Mac gate green** - `f3e0983` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/tests/api_parity.rs` - `api_parity_matches_python_frontend_fixtures`; `normalize_created`, `describe_body_diff`, `render` helpers mirroring `scripts/gen_api_fixtures.py`'s own normalization rule
- `crates/rsg-server/src/engine.rs` - moved the per-request `IncrementalDecoder` construction (`engine.codec.decoder()`) from after submit to before register/submit
- `scripts/check_all.sh` - new step 5/7 "API fixture freshness"; all step labels renumbered `/7`; header comment updated

## Decisions Made
- See `key-decisions` in the frontmatter for the full reasoning behind the `engine.rs` fix, the `/7` renumbering (plan text was stale relative to Phase 4's already-added tokenizer-fixture step), and the `RUST_TEST_THREADS=1` one-off verification of the full gate.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Per-request tokenizer clone built after submit, overflowing the per-uid reply buffer on any response over 16 tokens under a zero-decode-delay backend**
- **Found during:** Task 1, first test run (`chat_stream_system_user` and `chat_nonstream_multibyte` both failed deterministically, every run)
- **Issue:** `drive_request` called `engine.codec.decoder()` (which clones the real `tokenizers::Tokenizer`) *after* `register`/`submit`, immediately before entering the decode loop. Against `mock-scheduler --decode-delay-ms 0` (the fixture manifest's own `mock_args`), the clone's real wall-clock cost was consistently long enough that the backend could stream an entire >16-token generation into the per-uid broadcast channel (fixed capacity 16, drop-oldest per D-07) before the decode loop's first `recv()` call ever ran — the channel's `Lagged(n)` semantics then reported exactly `n = tokens - 16` dropped on that very first read, and the request failed as a slow consumer (`RequestError::SlowConsumer`). Reproduced with 100% consistency across 7 consecutive runs before the fix, and the two failing cases were exactly the two fixture cases with `max_tokens` over 16 (24 and 20); every case at or under 16 tokens passed, consistent with the clone's fixed cost exceeding the time to produce ~16 tokens at zero artificial delay.
- **Fix:** Moved `let mut decoder = engine.codec.decoder();` to immediately after the pre-submit cancellation check, before `dispatch.register(uid)` — the clone has no dependency on backend/dispatch state, only on `engine.codec`, so there was no ordering reason to delay it.
- **Files modified:** `crates/rsg-server/src/engine.rs`
- **Verification:** `cargo test -p rsg-server --test api_parity -- --nocapture` passed 4/4 consecutive runs after the fix (0/4 before); full `cargo test -p rsg-server` (45 tests across all integration suites) and `cargo clippy -p rsg-server --all-targets -- -D warnings` both clean.
- **Committed in:** `298124f` (Task 1 commit)

**2. [Rule 1 - Bug] Plan's interfaces note said `scripts/check_all.sh` had 5 steps; it already had 6**
- **Found during:** Task 2, before editing
- **Issue:** The plan's action text said "Renumber every step label from `/5` to `/6`", carried over from the plan's own interfaces section written before Phase 4 (merged later) added a `tokenizer fixture freshness` step and already renumbered the file to `/6`. Following the literal instruction would have produced an incorrect `/6` total after adding a 7th step.
- **Fix:** Inserted the new API-fixture-freshness step in the documented position (right after wire-fixture freshness, before tokenizer-fixture freshness) and renumbered every existing label to `/7`, consistent with the file's own established step-numbering convention.
- **Files modified:** `scripts/check_all.sh`
- **Verification:** `grep -c '/7\]' scripts/check_all.sh` → 1 occurrence of the pattern (used for every step, so actually matches every step line — confirmed all 7 step calls use `/7`); `grep -c '/5\]' scripts/check_all.sh` → 0; full gate run end to end (see Issues Encountered).
- **Committed in:** `f3e0983` (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 Rule 1 bug directly exposed by this plan's own parity test, 1 Rule 1 correction of a stale plan assumption about pre-existing file state)
**Impact on plan:** Both fixes were required for this plan's own stated verification commands to pass; neither expands scope beyond what the plan's acceptance criteria already required. No scope creep.

## Issues Encountered
- `crates/rsg-tokenizer`'s lib unit tests have a pre-existing, already-documented (`.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md`) concurrent-test-execution race (`EnvGuard` mutating process-global env vars across threads). Under `scripts/check_all.sh`'s default parallel `cargo test --workspace`, this race tripped step 1 in 4 of 5 full-gate attempts in this session (a different `rsg-tokenizer` test failing each time — `gated_access_unavailable_with_blank_token_file`, `gated_access_unavailable_when_implicit_token_disabled`, or one of the `detokenize::tests::step_*` cases), never anything this plan's files touch. Confirmed via `cargo test -p rsg-tokenizer --lib -- --test-threads=1` (deterministically green every time) and via 5 repeated default-threaded runs (4 failed on different tests, 1 passed clean) that this is exactly the pre-existing race, not a regression. Verified the full `scripts/check_all.sh --offline` gate passes end to end — all 7 steps, `check_all: OK` — with `RUST_TEST_THREADS=1` set as a one-off environment override for this verification run only (not a change to the committed script, since serializing every crate's tests for the whole gate would be a disproportionate, out-of-scope fix for one crate's known issue). No code in this plan's `files_modified` list is implicated.

## User Setup Required
None - no external service configuration required. (Qwen/Qwen3-0.6B's tokenizer was already cached locally from Phase 4/Plan 05-08.)

## Next Phase Readiness
- API-01 is now proven byte-for-byte on the Mac against the mock backend, with the Mac phase gate keeping it that way (`scripts/check_all.sh`'s new step 5/7 plus the parity test inside step 1).
- Phase 5 (Request Lifecycle & HTTP API) is complete: all 9 plans executed, all success criteria (LIFE-01..05, API-01, API-02) proven.
- This plan is the last in Phase 5; the phase is ready for goal verification / transition to Phase 6 (real GPU backend, real-model output parity).
- No blockers. The pre-existing `rsg-tokenizer` test-concurrency flake (see Issues Encountered) remains open and tracked in Phase 04's `deferred-items.md`; it is not a Phase 5 blocker and was not touched by this plan.

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-07*

## Self-Check: PASSED
