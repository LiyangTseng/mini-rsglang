# Phase 4: Tokenizer & Detokenizer Parity - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-06
**Phase:** 4-tokenizer-detokenizer-parity
**Areas discussed:** Llama-3.x model & HF access, Tokenizer source of truth, Golden fixture corpus & generation, Chat-template test coverage

---

## Llama-3.x model & HF access

| Option | Description | Selected |
|--------|-------------|----------|
| Llama-3.2-1B-Instruct | Smallest, lightest download, fastest on Mac CPU; tokenizer files shared across sizes within a version | ✓ |
| Llama-3.1-8B-Instruct | Matches a more commonly benchmarked production size; heavier download | |
| Something else | A different specific Llama-3.x version/size | |

**User's choice:** Llama-3.2-1B-Instruct

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, already set up | License accepted, token available on the Mac | ✓ |
| No, need to request access | Becomes a known blocker/setup step | |
| Not sure | Verify early in research/planning | |

**User's choice:** Yes, already set up

| Option | Description | Selected |
|--------|-------------|----------|
| Fetch live via hf-hub (shared cache) | Avoids redistributing gated license files; requires HF_TOKEN wherever tests run | ✓ |
| Vendor just the tokenizer files | Zero-network test runs, only if license permits redistribution | |

**User's choice:** Fetch live via hf-hub (shared cache)

| Option | Description | Selected |
|--------|-------------|----------|
| Skip Llama tests, Qwen3 still hard-gates | Llama tests conditionally skip with a clear log; Qwen3-0.6B always runs | ✓ |
| Fail loudly, no skip | Missing token is a hard setup error | |

**User's choice:** Skip Llama tests, Qwen3 still hard-gates
**Notes:** Resolves the STATE.md blocker about gated Llama-3.x access.

---

## Tokenizer source of truth

| Option | Description | Selected |
|--------|-------------|----------|
| Load raw tokenizer.json + tokenizer_config.json directly | Matches CLAUDE.md's described `load_tokenizer()` fallback exactly | ✓ |
| Add a small Python exporter (like Phase 1's fixture generator) | Closes silent-drift gap but adds a sidecar artifact | |

**User's choice:** Load raw tokenizer.json + tokenizer_config.json directly

| Option | Description | Selected |
|--------|-------------|----------|
| Also special_tokens_map.json + chat_template.json (fallback) | Mirrors load_tokenizer()'s try/except fallback exactly | ✓ |
| Just tokenizer.json + tokenizer_config.json | Assumes both files hold everything needed | |

**User's choice:** Also special_tokens_map.json + chat_template.json (fallback)
**Notes:** User asked for Claude's recommendation; Claude recommended this option citing CLAUDE.md's Mistral chat-template-fallback example as precedent for why skipping it risks a silent parity gap. User accepted.

---

## Golden fixture corpus & generation

| Option | Description | Selected |
|--------|-------------|----------|
| A Python script run on the Mac (CPU-only) | No GPU/weights needed for tokenizer-only work; keeps phase GPU-free | ✓ |
| Generated on the GPU box instead | Unnecessary extra hop | |

**User's choice:** A Python script run on the Mac (CPU-only)

**User's choice (corpus content/design):** User asked Claude to pick the exact corpus. Claude proposed: a ~16-case curated `id_corpus.json` (empty/whitespace, long prompt, special-token-literal text, CJK, emoji/ZWJ, mixed-script, whitespace edge cases, NFC/NFD, code/JSON-like text, URL, repeated-char run); TOK-03 fixtures derived by replaying real token streams from the CJK/emoji/mixed-script corpus entries plus an explicit finished+EOS case per model; TOK-04 Llama fixtures reusing chat-template conversations for single-BOS and `clean_up_tokenization_spaces` checks; storage under `fixtures/tokenizer/` with a `scripts/gen_tokenizer_fixtures.py` generator (`--check` mode) wired into `scripts/check_all.sh`, mirroring Phase 1's `gen_wire_fixtures.py`. User approved as-is.
**Notes:** Rationale for curated-over-fuzzed: TOK-01/03/04 success criteria name specific failure classes (UTF-8 breakage, BOS double-add, special-token literals), which a small deliberately-targeted set proves better than volume/fuzzing, and matches the Phase 1 D-16 boundary-case precedent.

---

## Chat-template test coverage

| Option | Description | Selected |
|--------|-------------|----------|
| Claude picks a curated set (recommended) | Same curated-edge-case philosophy as TOK-01, user approves final list | ✓ |
| Only basic single-turn + system prompt | Minimal, defers exotic cases | |

**User's choice:** Claude picks a curated set (recommended)

**User's choice (final corpus):** Claude proposed an 8-case shared conversation set (no-system single-turn, system+single-turn, multi-turn, empty-vs-missing system message, consecutive same-role messages, non-ASCII-in-turn, long ~10-turn conversation, conditional tool-calling turn) rendered through both Qwen3-0.6B's and Llama-3.2-1B-Instruct's templates. User approved as-is.
**Notes:** Tool-calling case is conditional on research confirming Qwen3's template has a tool-call branch — not assumed.

---

## Claude's Discretion

- Exact crate layout for the new tokenizer/detokenizer code (dedicated crate vs. extending an existing one)
- Exact `scripts/gen_tokenizer_fixtures.py` CLI shape, manifest format, internal module layout
- Precise long-prompt length and repeated-character-run length for the TOK-01 corpus

## Deferred Ideas

None — discussion stayed within phase scope.
