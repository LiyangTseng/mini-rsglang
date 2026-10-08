---
phase: 06-gpu-end-to-end-parity
plan: 03
subsystem: testing
tags: [parity, corpus, pytest, tdd, localization]

requires:
  - phase: 06-gpu-end-to-end-parity
    provides: "06-01's rsglang.parity package contract: CorpusItem/load_corpus/corpus_sha256, LAYERS/compare_prompt/summarize, the sidecar schema, and scripts/parity_check.py's run/validate CLI -- consumed exactly, signatures and record shapes unchanged"
provides:
  - "fixtures/parity/corpus.json: the curated 128-prompt parity corpus (D-01), shared by both models (D-03)"
  - "corpus.py: CATEGORIES, CANONICAL_COUNTS, validate_canonical -- full-shape enforcement wired into load_corpus for the canonical path only"
  - "compare.py: full D-05 layer precedence (tokenization, sampling_params, incomplete added ahead of/between the 06-01 layers), annotate_sequence, and the `explain` CLI"
affects: [06-04-concurrent-load, 06-05-abort-stress, 06-06-gpu-script, 06-07-report]

actuals:
  tokens: 32000
  tasks: 2
  commits: 3
  plan_head_before: 5a52106e8b42dc9c13fe31f1a89ef97c8ef1f84a
  plan_head_after: c67e6dd38d6b3dbf95ed44f8d598de23b4a93ed1

tech-stack:
  added: []
  patterns:
    - "Canonical-path-scoped validation: load_corpus runs validate_canonical only when the resolved path equals the repo's canonical fixtures/parity/corpus.json, so any other corpus (test fixtures, future variants) keeps 06-01's generic-only checks"
    - "First-match-wins layer precedence with independent summary fields: compare_prompt computes ids_match/text_match the same way regardless of which layer's divergence is ultimately reported, keeping the record's aggregate fields meaningful even when an earlier layer wins the user-facing divergence"

key-files:
  created: []
  modified:
    - fixtures/parity/corpus.json
    - python/rsglang/parity/corpus.py
    - python/rsglang/parity/compare.py
    - python/tests/test_parity_corpus.py
    - python/tests/test_parity_compare.py

key-decisions:
  - "16 of 128 corpus items reused from Phase 4's tokenizer corpus (scripts/tokenizer_fixtures/corpus_ids.py and corpus_chat.py), marked source phase4:<path>#<id>; the remaining 112 are original phase6 content, per D-01's Phase-4-reuse instruction"
  - "Phase4's long_multi_turn conversation ends on an assistant turn (it feeds apply_chat_template directly with no such constraint); reused here with its trailing assistant reply dropped so the item still ends on 'user', matching 06-01's existing chat-item validation"
  - "Backend and tokenization divergences default to note: None (unspecified by the plan's action block); only request_error, sampling_params, detokenization_or_api, and annotate_sequence's radix-cache override set a non-null note -- avoids inventing undocumented note text for layers the plan left unspecified"
  - "gsd_run check tdd-red-evidence not run for Task 2's RED phase, matching 06-01/06-02 precedent: workflow.tdd_mode is disabled project-wide and the tool parses only Node --test TAP output, not pytest's. RED evidence (an ImportError on the not-yet-existing annotate_sequence) was verified manually and is recorded in TDD Gate Compliance below"

requirements-completed: [PAR-01]

coverage:
  - id: D1
    description: "fixtures/parity/corpus.json holds exactly 128 curated prompts across the eight required categories with correct per-category counts; loading the canonical path enforces this shape (and every per-category content property) and lists every violation"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_corpus.py#test_canonical_counts"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_corpus.py#test_validate_canonical_rejects"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_corpus.py#test_load_corpus_noncanonical_path_skips_counts"
        status: pass
    human_judgment: false
  - id: D2
    description: "The same curated corpus is shared by both models with no model-specific field; the full 128-item corpus flows end to end through the sweep (parity_check.py run) with per-category results and its sha256/n recorded in the sidecar meta"
    requirement: "PAR-01"
    verification:
      - kind: integration
        ref: "python/tests/test_parity_corpus.py#test_tracer_canonical_corpus_through_sweep"
        status: pass
    human_judgment: false
  - id: D3
    description: "Every mismatch is localized to the first diverging token and a layer, in the fixed precedence request_error, tokenization, sampling_params, backend, incomplete, detokenization_or_api; a backend divergence following an earlier input-level divergence in the same sequence is noted as possibly radix-cache-driven"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_layer_precedence_request_error"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_layer_precedence_tokenization"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_layer_precedence_sampling"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_backend_first_index"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_backend_window_clipping"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_incomplete"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_detok_layer"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_match"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_annotate_sequence"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_compare.py#test_summarize_by_layer"
        status: pass
    human_judgment: false
  - id: D4
    description: "`python -m rsglang.parity.compare explain REPORT PROMPT_ID --model M` prints the layer, first index and both id windows for one prompt, and exits 2 on a missing prompt; importing compare never imports transformers"
    requirement: "PAR-01"
    verification:
      - kind: integration
        ref: "python/tests/test_parity_compare.py#test_explain_cli"
        status: pass
    human_judgment: false

duration: 50min
completed: 2026-10-07
status: complete
---

# Phase 6 Plan 3: Curated 128-Prompt Corpus and Full D-05 Localization Summary

**Authored the 128-item parity corpus across 8 categories (D-01/D-03, 16 items reused from Phase 4), wired canonical-shape enforcement into `load_corpus`, and extended `compare.py` with the full D-05 layer-precedence bisection (tokenization/sampling_params/incomplete), `annotate_sequence`'s radix-cache note, and the `explain` CLI.**

## Performance

- **Duration:** 50 min
- **Tasks:** 2 completed
- **Files modified:** 5 (1 data file, 2 source modules, 2 test files)

## Accomplishments

- `fixtures/parity/corpus.json`: schema_version 1, exactly 128 items in the 8 required categories (short 24, long 12, multi_turn 24, code 20, cjk 16, emoji 12, raw 12, edge 8), every item's `source` recorded as `phase4:<path>#<id>` (16 items) or `phase6` (112 items)
- `corpus.py`: `CATEGORIES`, `CANONICAL_COUNTS`, and `validate_canonical` enforcing exact category counts, `category=="raw"` iff `kind=="raw"`, no duplicate `(kind, messages, prompt)` content, multi_turn shape (>=3 messages, >=1 assistant), long/short content-length bounds, cjk/emoji code-point presence, the four required edge properties (max_tokens 1, max_tokens 2, a `<|im_start|>` literal, whitespace+3-newline-run), and non-edge `max_tokens==128`; wired into `load_corpus` for the canonical path only
- `compare.py`: full D-05 precedence (`request_error > tokenization > sampling_params > backend > incomplete > detokenization_or_api`, first match wins), `annotate_sequence` (notes a later backend divergence as possibly radix-cache-driven when an earlier input-level divergence occurred in the same sequence), and a module CLI (`python -m rsglang.parity.compare explain FILE PROMPT_ID --model M [--block ...] [--tokenizer ...]`) with lazy `transformers` import
- Proved end to end: the full 128-item corpus run through `scripts/parity_check.py run` against the fake server reports `128/128` matched, `by_category` matching `CANONICAL_COUNTS` exactly, and `meta.corpus.sha256`/`meta.corpus.n` matching the committed file

## Task Commits

1. **Task 1: Tracer -- the curated 128-prompt corpus loads, passes canonical validation, and flows through the sweep per category** - `b345150` (feat)
2. **Task 2 RED: failing tests for full D-05 layer precedence in compare.py** - `d915161` (test)
3. **Task 2 GREEN: full D-05 layer precedence, annotate_sequence and explain CLI** - `c67e6dd` (feat)

No REFACTOR commit: the GREEN implementation needed no behavior-neutral cleanup.

**Plan metadata:** recorded separately in the `docs(06-03)` commit that adds this SUMMARY, STATE.md, ROADMAP.md and REQUIREMENTS.md.

## Files Created/Modified

- `fixtures/parity/corpus.json` - the curated 128-item parity corpus (new)
- `python/rsglang/parity/corpus.py` - `CATEGORIES`, `CANONICAL_COUNTS`, `validate_canonical`, canonical-path wiring in `load_corpus` (Task 1)
- `python/rsglang/parity/compare.py` - full layer precedence, `annotate_sequence`, `main`/`explain` CLI (Task 2)
- `python/tests/test_parity_corpus.py` - canonical-count, rejection-mutation, non-canonical-path, and tracer-through-sweep tests (new, Task 1)
- `python/tests/test_parity_compare.py` - the 11 behavior-block tests for layer precedence, `annotate_sequence`, `summarize`, and the `explain` CLI (new, Task 2)

## Decisions Made

- Reused 16 of Phase 4's tokenizer-corpus items (`scripts/tokenizer_fixtures/corpus_ids.py`'s `cjk_text`, `mixed_cjk_emoji_ascii`, `emoji_with_zwj`, `degenerate_repeat`, `unicode_nfd`, `multiple_internal_whitespace`, `ascii_sentence`, `ascii_paragraph`, `json_like_text`, `url_text`, `code_snippet`, `very_long_prompt`; `corpus_chat.py`'s `single_turn_no_system`, `system_plus_single_turn`, `multi_turn`, `long_multi_turn`), each marked `source: "phase4:<path>#<id>"`. The remaining 112 items are original (`phase6`): 22 short Q&A/instruction prompts, 12 long synthetic-but-readable essays (>=2000 chars each, topic-specific sentences cycled with point numbering to reach length deterministically), 22 multi-turn conversations across varied topics (3-5 exchanges each), 20 code-review prompts across Python/Rust/JavaScript with fenced blocks/tabs/indentation, 14 CJK sentences, 11 emoji sentences (ZWJ families, flags, skin-tone modifiers), 6 raw completion-style prompts, and 4 original edge items (max_tokens 1, max_tokens 2, a `<|im_start|>` literal, whitespace+3-newline-run).
- Phase 4's own `long_multi_turn` conversation (from `corpus_chat.py`) ends on an assistant turn because it feeds `apply_chat_template` directly, with no "ends on user" constraint. This corpus's chat items must end on `user` (06-01's existing `load_corpus` check, unchanged), so the reused version drops the trailing assistant reply before being included (documented inline in the one-off corpus generator, not checked into the repo).
- `compare_prompt`'s `backend` and `tokenization` divergences default `note` to `None` rather than inventing text: the plan's action block specifies note content only for `request_error`, `sampling_params`, and `detokenization_or_api`, plus `annotate_sequence`'s own override for a later backend divergence. Leaving the other two `None` by default keeps the record honest about what the plan actually asked for, and `test_annotate_sequence`'s "keeps note None" assertion for an unpreceded backend divergence depends on this.
- `ids_match`/`text_match` are computed as independent summary fields (output_ids equality; text compared only when ids match) regardless of which layer's divergence is ultimately selected by the first-match-wins precedence chain, preserving their exact 06-01 meaning even though three new layers were inserted ahead of `backend`.
- The 128-item corpus content was generated by a one-off Python script (stdlib-only, run once via `python3 -I`, not checked into the repo) that asserted every canonical-shape property (counts, uniqueness, length bounds, category properties) before writing `fixtures/parity/corpus.json`, so the committed file is proven correct by construction rather than by post-hoc inspection alone.

## Deviations from Plan

None - plan executed exactly as written. Both tasks' `<action>` blocks fully specified the corpus shape and the layer-precedence/annotate_sequence/CLI behavior; no bugs, missing functionality, or blockers were found during implementation.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

`fixtures/parity/corpus.json`'s 128 items (with stable ids `<category>-<NN>` and the `source` provenance field) and `compare.py`'s now-complete `LAYERS`/`compare_prompt`/`annotate_sequence`/`summarize` contract are ready for plan 06-04 (concurrent load, which reads the same corpus and compare functions through the signatures 06-01 fixed) and 06-05 (abort stress). `annotate_sequence` itself is implemented but not yet wired into `scripts/parity_check.py run` -- that wiring is explicitly plan 06-05's job per this plan's `key_links` contract. No changes were made to `vendor/`, `scripts/parity_check.py`'s CLI flags, or `sidecar.py`'s schema; `test_parity_check.py` (06-01's own suite) still passes unchanged, confirming the record shape it validates is intact.

## TDD Gate Compliance

Task 2 (`tdd="true"`) followed RED -> GREEN; no REFACTOR commit was needed.

| Gate | Commit | Status |
|------|--------|--------|
| RED | `d915161` `test(06-03): add failing tests for full D-05 layer precedence in compare.py` | Collection failed intentionally: `ImportError: cannot import name 'annotate_sequence' from 'rsglang.parity.compare'` -- exactly the Task 2 behavior not yet implemented |
| GREEN | `c67e6dd` `feat(06-03): full D-05 layer precedence, annotate_sequence and explain CLI` | All 11 tests in `test_parity_compare.py` pass (21 total across `test_parity_corpus.py` + `test_parity_compare.py` + `test_parity_check.py`) |
| REFACTOR | none | No behavior-neutral cleanup needed |

**`gsd_run check tdd-red-evidence` not run**, matching 06-01/06-02 precedent: the tool's TAP-summary parser is Node-`--test`-specific and does not understand pytest's default output format, and `workflow.tdd_mode` is absent (disabled) from this project's `.planning/config.json`, so the automated gate is not applicable. Manual RED evidence:

```
command: .venv/bin/python -m pytest python/tests/test_parity_compare.py -q -m "not slow"
exit_code: 1 (collection error, before GREEN)
result: ImportError: cannot import name 'annotate_sequence' from 'rsglang.parity.compare'
  (/Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/memoized-beaming-kurzweil/python/rsglang/parity/compare.py)
expected: all 11 behaviors described in the plan's <behavior> block
actual (after GREEN): .venv/bin/python -m pytest python/tests/test_parity_compare.py -q -> 11 passed
```

This is a collection-level failure rather than a per-test assertion failure, because the test module imports `annotate_sequence` at module scope. It is still genuine, intentional RED evidence of exactly the missing Task 2 behavior (not a syntax error, fixture crash, or unrelated failure), confirmed by the GREEN run passing all 11 tests immediately after `annotate_sequence`/the precedence changes/`main` were added.

## Self-Check: PASSED

- All modified/created files found on disk: `fixtures/parity/corpus.json`, `python/rsglang/parity/corpus.py`, `python/rsglang/parity/compare.py`, `python/tests/test_parity_corpus.py`, `python/tests/test_parity_compare.py`, plus this SUMMARY.
- All 3 task commits (`b345150`, `d915161`, `c67e6dd`) found in `git log`.
- Re-ran plan-level `<verification>`: `.venv/bin/python -m pytest python/tests/test_parity_corpus.py python/tests/test_parity_compare.py -q` -> 18 passed; `.venv/bin/python -m pytest python/tests/test_parity_check.py -q` -> 3 passed (06-01 contract unbroken).
- Re-ran every task's `<acceptance_criteria>` command: Task 1 (`pytest -q` -> 7 passed, item count `128`, `grep -c '"source": "phase'` -> `128`, `git ls-files --error-unmatch fixtures/parity/corpus.json` -> exit 0) and Task 2 (`pytest -q` -> 11 passed, `grep -n "def annotate_sequence\|def main"` -> 2 lines, `'transformers' in sys.modules` -> `False`) all passed.
