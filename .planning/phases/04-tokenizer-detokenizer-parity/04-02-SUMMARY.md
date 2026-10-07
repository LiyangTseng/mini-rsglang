---
phase: 04-tokenizer-detokenizer-parity
plan: 02
subsystem: tokenizer
tags: [rust, minijinja, chat-template, parity, fixtures]

requires:
  - phase: 04-tokenizer-detokenizer-parity
    provides: "Plan 04-01's crates/rsg-tokenizer scaffold (ModelSpec/TokenizerError/loader/encode_text), tests/common MODELS registry, scripts/tokenizer_fixtures package and gen_tokenizer_fixtures.py CLI"
provides:
  - "template.rs: build_environment() (minijinja Environment with pycompat wired in, raise_exception and strftime_now HF globals, a Python-json.dumps-separator-matching tojson filter override) and render_chat() (renders fresh every call, never caches)"
  - "encode.rs: PromptInput::{Raw,Chat} and encode_prompt(), the chat-template-render-then-encode path mirroring tokenize.py's isinstance(msg.text, list) branch"
  - "scripts/tokenizer_fixtures/corpus_chat.py: 9-conversation D-12 chat corpus (8 cases, case 4 split into system_empty_string/system_missing_key)"
  - "fixtures/tokenizer/qwen3-0.6b/chat_prompts.json: 9 committed golden fixtures from a live apply_chat_template call against the real Qwen/Qwen3-0.6B tokenizer"
  - "crates/rsg-tokenizer/tests/chat_templates.rs: byte-for-byte parity test for every fixture case, plus two D-12-specific assertions (both case-4 sub-cases present; tool_calling case fires the real <tool_call> branch)"
affects: [04-03-detokenization, 04-04-llama-model-support, 04-05]

actuals:
  tokens: 6800
  tasks: 2
  commits: 3
  plan_head_before: 8f342bcbbe56a042ef99da2e757a11bc0c13ea13
  plan_head_after: f6ac1beeff040adb87168eda9c625e79c1afaf66

tech-stack:
  added: []
  patterns:
    - "render_chat renders via minijinja::Environment::render_str (parse+render in one call, never Environment::add_template'd), so the environment is never mutated per-request and no rendered string is ever cached across calls -- the chat path is naturally safe for a later model whose template embeds the render-time clock (Llama/strftime_now, Plan 04-04)"
    - "Custom tojson filter overriding minijinja's own default registration: env.add_filter(\"tojson\", ...) called after Environment::new() replaces the built-in serde_json::to_string-based filter with one matching transformers' actual override (json.dumps with Python's default (', ', ': ') separators), confirmed by reading transformers/utils/chat_template_utils.py directly rather than assuming Jinja2's stock tojson behavior"

key-files:
  created:
    - scripts/tokenizer_fixtures/corpus_chat.py
    - fixtures/tokenizer/qwen3-0.6b/chat_prompts.json
    - crates/rsg-tokenizer/tests/chat_templates.rs
  modified:
    - crates/rsg-tokenizer/src/template.rs
    - crates/rsg-tokenizer/src/encode.rs

key-decisions:
  - "minijinja::Environment::render_str (parse-and-render-in-one-call) was used instead of add_template_owned, specifically because it takes &self (not &mut self) and never stores the template in the environment -- directly satisfying the plan's must_haves.prohibitions clause against caching a rendered chat-template string across calls."
  - "Qwen3's tool_calls message shape was read directly from the model's live-fetched chat_template.jinja source (not guessed): an assistant message's tool_calls entries are {\"function\": {\"name\": ..., \"arguments\": {...}}} -- the template unwraps tool_call.function when present, confirmed by tracing the template's {%- if tool_call.function %} branch before authoring the tool_calling fixture case."
  - "raise_exception was extracted to a named fn (not an inline closure) solely so rustfmt keeps env.add_function(\"raise_exception\", raise_exception) on one line -- an inline closure's full type signature exceeds rustfmt's line width and gets wrapped across lines, which would have broken this plan's single-line grep-based acceptance criterion."

requirements-completed: [TOK-02]

coverage:
  - id: D1
    description: "minijinja Environment (build_environment) with raise_exception returning Err(TokenizerError::Template) instead of panicking, and a strftime_now stub that never leaks into a Qwen3 fixture since Qwen3's template never calls it; encode_prompt added to encode.rs taking the chat-render-then-encode path for PromptInput::Chat"
    requirement: "TOK-02"
    verification:
      - kind: unit
        ref: "cargo test -p rsg-tokenizer --lib -- template::tests::raise_exception_in_template_returns_templated_error_with_message"
        status: pass
      - kind: unit
        ref: "cargo test -p rsg-tokenizer --lib -- template::tests::strftime_now_stub_returns_fixed_sentinel_regardless_of_format"
        status: pass
    human_judgment: false
  - id: D2
    description: "Rust chat-template rendering (render_chat) produces a prompt string identical to the Python apply_chat_template oracle for Qwen3-0.6B across all 9 fixture cases (8 D-12 conversation shapes, case 4 split in two), including the tool-calling branch exercised against a real tool_calls-shaped message"
    requirement: "TOK-02"
    verification:
      - kind: integration
        ref: "cargo test -p rsg-tokenizer --test chat_templates -- chat_template_renders_match_python_oracle_for_every_model (network-dependent: fetches Qwen/Qwen3-0.6B from huggingface.co)"
        status: pass
      - kind: integration
        ref: "cargo test -p rsg-tokenizer --test chat_templates -- case_4_sub_cases_are_both_present_and_independently_asserted"
        status: pass
      - kind: integration
        ref: "cargo test -p rsg-tokenizer --test chat_templates -- tool_calling_case_prompt_contains_tool_call_tag"
        status: pass
      - kind: automated_ui
        ref: ".venv/bin/python scripts/gen_tokenizer_fixtures.py --check (exit 0)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Full Mac gate (scripts/check_all.sh --offline) stays green with the new tokenizer-fixture and chat-template steps included"
    verification:
      - kind: e2e
        ref: "bash scripts/check_all.sh --offline (exit 0, all 6 steps OK)"
        status: pass
    human_judgment: false

duration: ~20min
completed: 2026-10-06
status: complete
---

# Phase 04 Plan 02: Chat-Template Rendering (Qwen3-0.6B) Summary

**Filled in `rsg-tokenizer`'s minijinja `Environment` (pycompat, `raise_exception`, `strftime_now` stub, and a Python-`json.dumps`-matching `tojson` override) and `encode_prompt`'s chat path, then proved TOK-02 chat-template parity against a live-fetched Qwen3-0.6B template across all 9 D-12 corpus cases including the tool-calling branch, byte-for-byte against the real `apply_chat_template` oracle.**

## Performance
- **Duration:** ~20min
- **Started:** 2026-10-06 (approx. 19:19 UTC)
- **Completed:** 2026-10-06T19:39:34Z
- **Tasks:** 2 completed
- **Files modified:** 5 (3 created, 2 modified)

## Accomplishments
- `template.rs`'s `build_environment()`/`render_chat()` are no longer placeholders: a real minijinja `Environment` with `minijinja-contrib`'s pycompat method shims wired in, both HF-required globals (`raise_exception`, `strftime_now` stub), and a custom `tojson` filter override matching the real oracle's JSON-separator behavior.
- `encode.rs` gained `PromptInput::{Raw,Chat}` and `encode_prompt`, mirroring `tokenize.py`'s `isinstance(msg.text, list)` branch exactly, with chat-template rendering never cached across calls.
- `scripts/tokenizer_fixtures/corpus_chat.py` encodes all 9 D-12 chat-corpus cases (8 shapes, case 4 split into two genuinely distinct sub-cases) and generates `fixtures/tokenizer/qwen3-0.6b/chat_prompts.json` from a live `apply_chat_template` call against the real `Qwen/Qwen3-0.6B` tokenizer.
- `crates/rsg-tokenizer/tests/chat_templates.rs` passes for all 9 fixture cases, plus two D-12-specific checks: both case-4 sub-cases exist and render to genuinely different prompts (confirming the empirical framing), and the `tool_calling` case's prompt contains the literal `<tool_call>` substring (confirming the real tool-call branch fired against a `{"function": {...}}`-shaped message, traced directly from Qwen3's live `chat_template.jinja` source).
- Found and fixed a real parity bug along the way: minijinja's built-in `tojson` filter is fully compact (no separator spaces), while `transformers`' own chat-template Jinja environment overrides `tojson` with `json.dumps(..., separators=None)`, whose default separators insert a space after every `,`/`:`. A custom `tojson` filter registration closes this gap.
- `scripts/check_all.sh --offline` (the full 6-step Mac gate) stays green with this plan's new tokenizer-fixture and chat-template work included.

## Task Commits
1. **Task 1: minijinja Environment with the two required HF globals** — RED: `5862953` (test), GREEN: `27d57ce` (feat)
2. **Task 2: Qwen3 chat-corpus fixtures and the chat_templates.rs test** (incl. the tojson deviation fix) — `f6ac1be` (feat)

**Plan metadata:** commit pending (this SUMMARY + STATE/ROADMAP/REQUIREMENTS update)

## Files Created/Modified
- `crates/rsg-tokenizer/src/template.rs` — `build_environment`, `raise_exception`, the `strftime_now` stub, `PySeparatorsFormatter`/`tojson` filter override, `render_chat`.
- `crates/rsg-tokenizer/src/encode.rs` — `PromptInput`, `encode_prompt`.
- `scripts/tokenizer_fixtures/corpus_chat.py` — `OUTPUT_NAME`, `CONVERSATIONS` (9 entries), `generate`.
- `fixtures/tokenizer/qwen3-0.6b/chat_prompts.json` — 9 committed fixture entries (messages + rendered prompt per case).
- `crates/rsg-tokenizer/tests/chat_templates.rs` — three tests: full-corpus byte-for-byte parity, case-4 sub-case presence, tool-calling branch firing.

## Decisions Made
- **`render_str` over `add_template_owned`:** the plan's action text flagged this as something to confirm via docs.rs before writing. `render_str` takes `&self` and never stores the parsed template on the environment, which directly satisfies the "never cache a rendered chat-template string" prohibition without any extra bookkeeping.
- **Tool-calling message shape sourced from the live template, not guessed:** read Qwen3's actual `chat_template.jinja` (`{%- if tool_call.function %} {%- set tool_call = tool_call.function %}`) before authoring `corpus_chat.py`'s `tool_calling` case, confirming the `{"function": {"name": ..., "arguments": {...}}}` wrapper shape.
- **`raise_exception` as a named fn, not an inline closure** — purely a rustfmt/line-width accommodation so the single-line `add_function("raise_exception", ...)` acceptance-criteria grep keeps matching; no behavioral difference.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] minijinja's built-in `tojson` filter doesn't match the real oracle's JSON-separator behavior**
- **Found during:** Task 2, first `cargo test -p rsg-tokenizer --test chat_templates` run — the `tool_calling` case failed with a byte-for-byte diff isolated entirely to JSON spacing (`{"location":"Paris"}` vs. the oracle's `{"location": "Paris"}`).
- **Issue:** minijinja's default `tojson` filter calls `serde_json::to_string`, which is fully compact. `transformers`' own chat-template Jinja environment overrides Jinja2's built-in `tojson` with `json.dumps(x, ensure_ascii=False, indent=None, separators=None, sort_keys=False)` (confirmed by reading `transformers/utils/chat_template_utils.py` directly) — Python's `json.dumps` default separators (`indent=None`) are `(', ', ': ')`, inserting a space after every comma and colon. Every `tool | tojson` / `tool_call.arguments | tojson` call in Qwen3's template was therefore rendering with the wrong spacing.
- **Fix:** Registered a custom `tojson` filter (`template.rs`: `PySeparatorsFormatter` implementing `serde_json::ser::Formatter`, plus a `tojson` function using it) via `env.add_filter("tojson", tojson)` **after** `Environment::new()`'s own default registration, so it overrides rather than duplicates. The override skips minijinja's own HTML-escaping post-process step too, since Python's override never applies it.
- **Files modified:** `crates/rsg-tokenizer/src/template.rs`
- **Verification:** `cargo test -p rsg-tokenizer --test chat_templates` passes all 9 cases (previously 1 of 9 failed) after the fix; `scripts/check_all.sh --offline` green end to end.
- **Committed in:** `f6ac1be`

---
**Total deviations:** 1 auto-fixed (1 Rule 1 bug fix).
**Impact on plan:** None on scope or requirements — TOK-02 parity holds exactly as specified for Qwen3-0.6B, including the tool-calling branch. The fix is scoped to `template.rs`'s own `tojson` filter registration (already a plan deliverable); no other file needed to change, and no Cargo.toml/dependency changes were required.

**Known limitation (not a deviation, documented for future plans):** the custom `tojson` filter preserves whatever key order `serde_json::Value::Object` reports internally, which — because this workspace's `serde_json` dependency does not enable the `preserve_order` feature — is alphabetical, not insertion order. Python's real `json.dumps(..., sort_keys=False)` preserves insertion order. This plan's `tool_calling` fixture case was authored with already-alphabetical argument keys (`location`, `unit`), so the mismatch never surfaces here, but a future conversation with non-alphabetical tool-argument keys could produce a key-order mismatch Rust would not catch today. Enabling `preserve_order` would be a workspace-wide `serde_json` feature change affecting every crate (Cargo feature unification), which is out of this plan's declared file scope — flagging for a later plan (likely 04-04/04-05, where Llama's richer templates get more exercise) rather than fixing speculatively here.

## Issues Encountered
None beyond the tojson deviation above, which was found, fixed, and verified within Task 2's normal execution.

## User Setup Required
None — no external service configuration required. Network access to `huggingface.co` was available and used directly (Qwen/Qwen3-0.6B is public, non-gated, matching Plan 04-01's precedent).

## Next Phase Readiness
- `crates/rsg-tokenizer`'s chat-template path (`template.rs`, `encode.rs::encode_prompt`) is fully wired for Qwen3-0.6B and ready for Plan 04-03 (incremental detokenization, TOK-03) to proceed independently, and for Plan 04-04 to extend `strftime_now` from its current stub to a real `chrono`-backed implementation plus clock override when Llama's template is added (its own package-legitimacy checkpoint, not touched by this plan).
- The known key-ordering limitation in the `tojson` override (see above) is documented and low-risk for this plan's scope; worth a one-line check when Plan 04-04/04-05 add Llama's tool-calling or richer-structured-argument fixtures.
- No blockers.

## Known Stubs
- `crates/rsg-tokenizer/src/template.rs`'s `strftime_now` global remains a fixed-sentinel stub (`"UNUSED_BEFORE_PLAN_04_04"`), by design — Qwen3's chat_template never calls it, and Plan 04-04's Task 1/2 own introducing `chrono` (its own package-legitimacy checkpoint) and wiring a real clock plus test-only override. Not a gap for this plan's scope.
- `crates/rsg-tokenizer/src/detokenize.rs` remains the Plan 04-01 placeholder, explicitly scoped to Plan 04-03. Untouched by this plan.

---
*Phase: 04-tokenizer-detokenizer-parity*
*Completed: 2026-10-06*

## Self-Check: PASSED

All 5 created/modified deliverable files confirmed present on disk; all 3 task commits (`5862953`, `27d57ce`, `f6ac1be`) confirmed in `git log`.
