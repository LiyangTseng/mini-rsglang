# Experiments and results

How experiment data is stored in this repo. The decision and its reasons are
in [ADR 0001](../adr/0001-experiment-data-layout.md).

## Layout

```
docs/experiments/
  README.md                        index: one row per experiment, and "How to add an experiment"
  _template/README.md              the sections every experiment README has
  <YYYY-MM>-<question-slug>/
    README.md                      question, hypotheses and verdicts, setup, runs, results, links
    runs/<YYYY-MM-DD>-<label>/     manifests plus the generated report.json / report.md; immutable
    plots/                         only if the experiment has plots
docs/RESULTS.md                    the single page of current numbers, citing the runs it uses
README.md                          headline rows and plots only, taken from docs/RESULTS.md
```

## Rules

1. **Name an experiment after its question, with a date prefix.**
   `docs/experiments/<YYYY-MM>-<question-slug>/`, never a phase or issue
   number. Runs are `runs/<YYYY-MM-DD>-<label>/`.
2. **A committed run directory is never renamed or edited.** A wrong run is
   superseded by a new run, and the experiment README says which run
   supersedes which.
3. **Each run records its git SHA, hardware, model and exact commands**, in
   the manifests (`rsg-bench` does this) or in the experiment README's Setup.
4. **`docs/RESULTS.md` is the only place for current numbers.** When a run
   supersedes another, update `docs/RESULTS.md` and the README headline,
   never the old run.
5. **Compare only within a session.** An experiment reruns its baseline arms
   in the same session as its treatment arms; never compare against numbers
   from another day's run.
6. **Large raw traces stay out of git** (for example `--requests-jsonl`
   output, py-spy profiles). Commit summaries only.
7. **Plans stay in `docs/plans/`.** The experiment README links to its plan.

To start one, copy [`docs/experiments/_template/`](../experiments/_template/README.md)
and follow [How to add an experiment](../experiments/README.md#how-to-add-an-experiment).

## Enforcement

`scripts/check_experiments.py` runs in `scripts/check_all.sh`, and so in CI.
It fails when:

- a file that existed under `docs/experiments/*/runs/` at the merge base
  with `origin/main` is modified, renamed or deleted (rule 2);
- a `*.manifest.json` or an `rsg-bench` report under `docs/` sits outside
  `docs/experiments/<YYYY-MM>-<slug>/runs/<YYYY-MM-DD>-<label>/` (rules 1
  and 3);
- a relative link, image path or heading anchor in a `*.md` file outside
  `vendor/` and `.planning/` does not resolve.

Rules 4–7 are reviewed by people, not checked by the script. The four hard
rules are repeated in `.claude/CLAUDE.md`.

## Default output paths

- `scripts/gpu_phase7_bench.sh` writes into
  `docs/experiments/2026-10-rust-vs-python-frontend/runs/<today>-gpu/` by
  default. Pass `--out-dir` to write into another experiment.
- `scripts/parity_check.py` and `scripts/baseline_profile.py` default
  `--out` to the committed sidecar of their existing run, which a non-GPU
  run refuses to overwrite. A new GPU run must pass `--out` to a new run
  directory; keeping the default would modify a committed run and fail
  the check.
