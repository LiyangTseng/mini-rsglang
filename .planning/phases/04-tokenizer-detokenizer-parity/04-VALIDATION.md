---
phase: "4"
slug: "tokenizer-detokenizer-parity"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-06"
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `04-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (workspace) — no dedicated `nextest.toml` found despite `cargo-nextest` being listed as a dev tool in CLAUDE.md; Python-side fixture generator uses the Phase 1 `python/tests/` + `pytest` convention |
| **Config file** | none dedicated — relies on workspace `Cargo.toml` and the `python/tests/` convention already established in Phase 1 |
| **Quick run command** | `cargo test -p rsg-tokenizer` |
| **Full suite command** | `scripts/check_all.sh` (existing Phase 1 gate; this phase adds a `gen_tokenizer_fixtures.py --check` step) |
| **Estimated runtime** | ~30-60 seconds (estimate — no measured baseline yet; CPU-only, no GPU/model-weight loading) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p rsg-tokenizer`
- **After every plan wave:** Run `scripts/check_all.sh`
- **Before `/gsd-verify-work`:** Full suite must be green, including the Llama-3.x cases (or cleanly skipped per D-04 if `HF_TOKEN` is unavailable)
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

Requirement -> test map, filled in against the actual plan set (`04-01-PLAN.md` through
`04-06-PLAN.md`):

| Req ID | Behavior | Test Type | Automated Command | Plan(s) | File Exists | Status |
|--------|----------|-----------|-------------------|---------|-------------|--------|
| TOK-01 | Rust `encode()` matches Python ids on the 16-case corpus, Qwen3-0.6B | unit | `cargo test -p rsg-tokenizer --test token_ids` | 04-01 | ❌ Wave 0 | ⬜ pending |
| TOK-02 | Rust chat-template render matches Python's rendered prompt string, Qwen3-0.6B, 8 conversations (9 fixture entries, case 4 split) incl. tool-calling | unit | `cargo test -p rsg-tokenizer --test chat_templates` | 04-02 | ❌ Wave 0 | ⬜ pending |
| TOK-03 | Rust incremental detokenize stream matches Python's, CJK/emoji/finished+EOS cases, plus a proptest no-panic property | unit | `cargo test -p rsg-tokenizer --test detokenize_streams` | 04-03 | ❌ Wave 0 | ⬜ pending |
| TOK-04 | TOK-01..03 also pass for Llama-3.2-1B-Instruct, plus BOS-count (checked against the real oracle, not assumed) and `clean_up_tokenization_spaces` assertions; gated behind a chrono package-legitimacy checkpoint | unit | same three test files, parametrized over `model_slug`, skip-if-no-`HF_TOKEN` per D-04 | 04-04, 04-05, 04-06 | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Addressed by `04-01-PLAN.md`'s tracer task (crate scaffold, generator, Mac gate wiring) and
expanded by the subsequent plans — not yet executed, so still unchecked:

- [ ] `crates/rsg-tokenizer/` — new crate scaffold (04-01)
- [ ] `scripts/gen_tokenizer_fixtures.py` — new fixture generator (D-07) (04-01, extended by 04-05)
- [ ] `fixtures/tokenizer/` — new fixture directory (D-11) (04-01, extended by 04-02/04-03/04-05)
- [ ] `crates/rsg-tokenizer/tests/{token_ids,chat_templates,detokenize_streams}.rs` (04-01/04-02/04-03, parametrized further by 04-06)
- [ ] `scripts/check_all.sh` — new step for `gen_tokenizer_fixtures.py --check`, parallel to the existing fixture-freshness step (04-01)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Llama-3.2-1B-Instruct `tokenizer_config.json` from the `unsloth` mirror matches the canonical gated `meta-llama` repo | TOK-04 | Research session had no `HF_TOKEN`; only the author's Mac has gated access (D-02) | Diff the mirror-sourced file against a fresh `hf-hub` download using the author's local `HF_TOKEN`, before trusting Llama fixtures generated from it |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
