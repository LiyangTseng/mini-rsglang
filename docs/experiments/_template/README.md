# <Question, phrased as a question?>

<!--
Copy this directory to docs/experiments/<YYYY-MM>-<question-slug>/ and fill in
every section. Delete these comments. Rules: docs/agents/experiments.md.
Placeholders are in code spans; replace them with real relative links.
-->

**Status:** open | concluded | superseded by `<link to the experiment or run that replaces it>`

**Conclusion:** <One or two sentences. Write "open" until a run answers the
question.>

## Question

<What this experiment answers, and why it matters. One paragraph.>

## Hypotheses and verdicts

| Hypothesis | Prediction | Verdict | Deciding data |
|---|---|---|---|
| H1: <cause> | If H1, then <changing X> makes <Y> disappear | open / confirmed / rejected | <run id and metric> |

## Setup

- **Git SHA:** <commit the run was built from; one per run if they differ>
- **Hardware:** <GPU, host, OS>
- **Model:** <model id>
- **Arms:** <which frontends and settings; baseline arms rerun in the same
  session as the treatment arms>
- **Commands:**

```bash
<exact commands, with every --out pointing into runs/<YYYY-MM-DD>-<label>/>
```

## Runs

Committed run directories are immutable. Supersede a wrong run with a new
one and say so here.

| Run | What it measures | Notes |
|---|---|---|
| `runs/<YYYY-MM-DD>-<label>/` (link it to its report) | <arms and scenarios> | <supersedes / superseded by> |

## Results

<The numbers that answer the question, each citing its run. If they become
current numbers, update docs/RESULTS.md and the README headline too.>

## Links

- Plan: `docs/plans/<plan>.md` (as a relative link)
- Issues: #<n>
