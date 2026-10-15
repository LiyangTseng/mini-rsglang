# 0001: Organize experiment data by experiment, with one current-results page

- **Status:** accepted
- **Date:** 2026-10-14

## Context

Measured results lived flat in `docs/benchmarks/`, named after project
phases (`phase8-benchmarks`, later `tokenizer-clone-fix-after`), with one
long `FINDINGS.md` that mixed current numbers, superseded numbers and a
wrong root-cause explanation. Two review comments made the cost concrete:
phase-numbered names need context a reader does not have
([PR #19](https://github.com/LiyangTseng/mini-rsglang/pull/19)), and keeping
the superseded analysis next to the current one only distracts (review on
commit `68d2f4b`). Renaming the phase-8 files after the fact also left their
manifests recording paths that no longer exist.

## Decision

- Each experiment is a directory named after its question,
  `docs/experiments/<YYYY-MM>-<question-slug>/`, with a README (question,
  hypotheses and verdicts, setup, runs, results, links), its runs under
  `runs/<YYYY-MM-DD>-<label>/`, and plots if it has any.
- A committed run directory is immutable. A wrong run is superseded by a
  new one, never edited or renamed.
- `docs/RESULTS.md` is the only page with current numbers, and cites the
  runs it uses. The root README shows its headline rows and links to it.
- Comparisons are made only within one session: an experiment reruns its
  baseline arms alongside its treatment arms.
- Raw traces stay out of git; plans stay in `docs/plans/`.
- `scripts/check_experiments.py`, run in CI through `scripts/check_all.sh`,
  enforces run immutability, the location of manifests and reports, and
  that every relative link and anchor in the docs resolves. The rules are
  written out in [docs/agents/experiments.md](../agents/experiments.md) and
  repeated in `.claude/CLAUDE.md`.

## Consequences

- Names describe the question, so a reader needs no phase history to find
  a result, and superseded runs stay available without competing with
  current numbers.
- Moving the existing runs into this layout left their recorded `--out`
  paths stale once; each experiment README notes it. Runs written from now
  on land in their final place, so this does not recur.
- The parity and baseline-profile scripts still default `--out` to the
  committed sidecar of their existing run. A new GPU run must pass `--out`
  to a new run directory, or the immutability check fails.
- The immutability check needs git history (CI checks out with
  `fetch-depth: 0`).
- The repo is developed with Claude Code only, so the rules live in
  `.claude/CLAUDE.md` and there is deliberately no `AGENTS.md`.
