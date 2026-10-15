# Experiments

One directory per question. Each holds its README, its immutable runs and
any plots. The current numbers are on one page,
[docs/RESULTS.md](../RESULTS.md); the rules are in
[docs/agents/experiments.md](../agents/experiments.md).

| Experiment | Question | Status | Conclusion |
|---|---|---|---|
| [2026-10-python-frontend-baseline-profile](2026-10-python-frontend-baseline-profile/README.md) | Where does the Python frontend spend its time and memory? | concluded | Set the per-process metrics the comparison captures, and split frontend from shared backend costs |
| [2026-10-rust-python-output-parity](2026-10-rust-python-output-parity/README.md) | Does the Rust frontend produce the same output as the Python frontend? | concluded | Yes: 128/128 byte-identical for both models; abort timing defaults to `deferred` |
| [2026-10-rust-vs-python-frontend](2026-10-rust-vs-python-frontend/README.md) | Does a drop-in Rust frontend beat the Python frontend on the same backend? | superseded (S1–S3) | Rust won on RAM and cold start, tied on throughput, lost on tail latency because of a bug |
| [2026-10-p99-ttft-tokenizer-clone](2026-10-p99-ttft-tokenizer-clone/README.md) | Why is the Rust frontend's P99 TTFT ~15× Python's under load? | concluded | A per-request tokenizer clone; sharing one copy brought S1 P99 from 4868 ms to 302 ms, tied with Python |

## Layout

```
docs/experiments/
  README.md                        this index
  _template/README.md              the sections every experiment README has
  <YYYY-MM>-<question-slug>/
    README.md                      question, hypotheses and verdicts, setup, runs, results, links
    runs/<YYYY-MM-DD>-<label>/     manifests plus the generated report.json / report.md; immutable
    plots/                         only if the experiment has plots
```

## How to add an experiment

1. Copy [`_template/`](_template/README.md) to
   `docs/experiments/<YYYY-MM>-<question-slug>/`. Name it after the
   question, never a phase number.
2. Fill in the Question and Hypotheses sections before running anything.
   Put a plan, if there is one, in `docs/plans/` and link it.
3. Write every run into a new `runs/<YYYY-MM-DD>-<label>/`, with the
   baseline arms rerun in the same session as the treatment arms. For
   `rsg-bench`, point every `--out` at that directory and run
   `rsg-bench report --manifests-dir <run> --out-json <run>/report.json --out-md <run>/report.md`.
4. Make sure the run records its git SHA, hardware, model and exact
   commands (the manifests do; otherwise add them to the README's Setup).
   Commit summaries only; keep large raw traces such as `--requests-jsonl`
   output out of git.
5. Fill in Runs, Results and the status line, and add a row to the table
   above.
6. If the results are now the current numbers, update
   [docs/RESULTS.md](../RESULTS.md) and the root README's headline. Never
   edit a committed run; supersede it with a new one.
7. Run `scripts/check_experiments.py` before pushing. CI runs it too.
