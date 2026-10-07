# Rust vs Python Frontend Output Parity

PAR-01/PAR-02's measured GPU parity run, read directly from
`docs/benchmarks/parity-report.json` (the sidecar `scripts/parity_check.py run`
writes, via `scripts/gpu_phase6_parity.sh`). Every number below is copied from
that file; none are estimated. Where the JSON carries a `null` for a field,
this report says so explicitly rather than filling in a number.

**This is the corrected, final version of this report.** An earlier version
of this document (git history `224fcb6`, superseded here) reported Criterion
2 as a genuine FAIL for both models and attributed it to GPU backend
run-to-run nondeterminism. That attribution was wrong. The real cause was a
bug in the measurement harness itself (`python/rsglang/parity/sweep.py`),
fixed in commit `ae8feec`; see `## PAR-01 off-by-one investigation (Phase 6
scope expansion)` below for the full corrected account. This run was produced
after that fix, on top of the `--abort-timing deferred` default change
(commit `de520b9`); Criterion 2 passes outright.

## Run

- **Date:** 2026-10-07T23:11:20Z (`meta.created_utc`)
- **GPU:** NVIDIA GeForce RTX 3050 (`meta.gpu`)
- **Git commit:** `ae8feecdb2a50fb4788916ea9da2baac84e17087` (`meta.git_commit`; `meta.git_dirty` is `false` — the GPU-box checkout was clean at run time)
- **Upstream mini-sglang SHA:** `9a91cfafe754aa85daee49998176275667eb58f2` (`meta.upstream_sha`)
- **Models:** Qwen/Qwen3-0.6B, meta-llama/Llama-3.2-1B-Instruct (`meta.models`)
- **Gate model:** Qwen/Qwen3-0.6B (`meta.gate_model`)
- **Concurrency:** 128 (`meta.concurrency`)
- **Mode:** `run` (`meta.mode`); **Platform:** `linux` (`meta.platform`); **Python:** `3.12.3` (`meta.python`)

## Corpus

- **Path:** `fixtures/parity/corpus.json` (`meta.corpus.path`)
- **n:** 128 (`meta.corpus.n`)
- **sha256:** `1448e464db068c189d5e4aac735d9991e47b751201d10d5952967a36d876d3c2` (`meta.corpus.sha256`) — matches the committed corpus file's own sha256, verified by `test_parity_report.py`.
- **Per-category counts:** short 24, long 12, multi_turn 24, code 20, cjk 16, emoji 12, raw 12, edge 8 (sums to 128).
- **Items reusing Phase 4's corpus:** 16 of 128 (every item whose `source` field starts with `phase4:`, drawn from `scripts/tokenizer_fixtures/corpus_chat.py` and `scripts/tokenizer_fixtures/corpus_ids.py`). The remaining 112 items are Phase 6's own (`source: "phase6"`).

## Criterion 1: Endpoints through the real backend

From `endpoints` — every entry's `ok` field, verbatim:

**Rust (`endpoints.rust`, 8 endpoints):**

| Endpoint | ok |
|---|---|
| `GET /v1/models` | true |
| `GET /v1` | true |
| `POST /v1/chat/completions` | true |
| `POST /v1/chat/completions stream` | true |
| `POST /generate` | true |
| `GET /health` | true |
| `GET /health/ready` | true |
| `GET /metrics` | true |

**Python (`endpoints.python`, 5 endpoints):**

| Endpoint | ok |
|---|---|
| `GET /v1/models` | true |
| `GET /v1` | true |
| `POST /v1/chat/completions` | true |
| `POST /v1/chat/completions stream` | true |
| `POST /generate` | true |

Every Rust endpoint and every Python endpoint answers `ok: true`. **Criterion 1 passes.**

## Criterion 2: Greedy parity, one request at a time (PAR-01)

### Qwen/Qwen3-0.6B (hard gate)

- **Status:** `ok` (reason: none — `sequential["Qwen/Qwen3-0.6B"].status`/`.reason`)
- **matched/n:** 128/128
- **ids_matched/n:** 128/128
- **text_matched/n:** 128/128
- **by_layer:** `request_error` 0, `tokenization` 0, `sampling_params` 0, `backend` 0, `incomplete` 0, `detokenization_or_api` 0

**Result: the zero-tolerance hard gate (D-04) PASSES.** `n=128` clears the `n >= 100` floor and `matched=128=n` — every prompt in the corpus produced byte-identical token ids and text on both frontends. **Criterion 2's hard gate passes for Qwen/Qwen3-0.6B.**

### meta-llama/Llama-3.2-1B-Instruct (reported, not gated)

- **Status:** `ok` (reason: none)
- **matched/n:** 128/128
- **ids_matched/n:** 128/128
- **text_matched/n:** 128/128
- **by_layer:** `request_error` 0, `tokenization` 0, `sampling_params` 0, `backend` 0, `incomplete` 0, `detokenization_or_api` 0

Llama's result is reported for information only (D-02); it is not gated. Every prompt also matched for this model.

## Divergence bisection (D-05)

No prompt diverged in any sequential block.

(Context: an earlier run of this same corpus against this same backend, before the harness fix described below, had shown a single mismatching prompt — `edge-08` — in each model's sequential sweep. That mismatch is now understood to have been a measurement artifact, not a real divergence; see the next section.)

## PAR-01 off-by-one investigation (Phase 6 scope expansion)

**CORRECTION (2026-10-07):** This section originally concluded (commit
`224fcb6`) that the single `edge-08` mismatch found in both models' sequential
sweeps was most likely ordinary GPU run-to-run floating-point
nondeterminism, "not a systematic frontend defect." **That conclusion was
wrong.** The real cause, found afterward by direct inspection of the raw
backend tap and fixed in commit `ae8feec`, was a bug in the measurement
harness itself: the comparison code was over-counting one frontend's output
tokens. The corrected harness, re-run against the same corpus and the same
backend, shows **zero** mismatches (see `## Criterion 2` and `## Divergence
bisection (D-05)` above). There is no real frontend parity bug and no
unexplained backend nondeterminism; the only genuine bug was in the test
harness, and it has been fixed and regression-tested. The rest of this
section is kept for the record, with the parts that were correct unchanged
and the parts that were wrong struck through in spirit (restated below,
not silently deleted) by this correction note.

**What the original investigation got right (still true):** Plan 06-08 Task 0
(user-approved scope expansion) checked whether either of the two `edge-08`
mismatches could be caused by the Rust frontend's own code — before accepting
Criterion 2's then-apparent FAIL at face value. It found, and this finding
still stands:

- `crates/rsg-server/tests/backend_finish_boundary.rs` (new, Mac-only, no GPU
  or network dependency) proved that `engine.rs`'s decode loop and
  `dispatch.rs`'s per-uid routing only ever branch on the wire's `finished`
  bit. A hand-built `DetokenizeMsg` sequence injected directly into the
  dispatcher's bound detok socket showed the engine never makes an
  independent "that id looks like EOS" call, and never second-guesses a
  `finished=true` reply arriving on the very first message for a uid.
- `crates/rsg-server/tests/http_chat.rs`'s pre-existing
  `chat_nonstream_response_shape`/`tracer_chat_stream_matches_upstream_framing`
  coverage already proved exact `max_tokens`-for-`max_tokens` delivery
  through the full real binary over real `ipc://` sockets.
- `python/rsglang/backend.py::run_scheduler` and `python/rsglang/launch.py`'s
  Python-frontend launch path both build `ServerArgs` from the identical
  upstream `parse_args(rest)` call and construct the identical unmodified
  `minisgl.scheduler:Scheduler` class — ruling out a frontend-specific
  backend-launch configuration difference.

These three findings are unaffected by this correction: the Rust frontend's
engine/dispatch pair genuinely has no independent stopping logic of its own,
and genuinely never second-guesses the backend's `finished` flag. That part
of the investigation was sound.

**What was wrong: the explanation for *why* the counts differed.** The
original investigation treated the backend tap's `output_ids` field as an
unimpeachable record of "what the backend decided to emit" and, finding a
one-token shortfall there with byte-identical inputs on both sides, reached
for GPU nondeterminism as the explanation (the same shape CONTEXT D-04 had
named as a plausible hard blocker). That reasoning skipped a question it
should have asked first: does `output_ids` in the JSON actually represent
what either frontend's real HTTP response contained?

The actual root cause, found by comparing the real HTTP response text both
frontends sent for `edge-08` against the JSON's `output_ids`/`ids_match`
field: **the HTTP response text was already byte-identical on both sides**
(`text_match` showed no difference) even on the run where `ids_match`
reported a mismatch. `output_ids` and `ids_match` are derived by
`python/rsglang/parity/sweep.py`'s `join_sequential`/`join_concurrent`
functions, which collected **every** backend-tap `detok` record for a uid,
unbounded. The scheduler's own pipelined/overlapped execution can emit one
extra `detok` record for a uid *after* it already sent `finished: true` —
observed for the corpus's last request in each sequential session, raced
against that session's own teardown `SIGINT`. That straggler record is
backend-internal bookkeeping: neither frontend's own response-finalization
logic ever consumes it, because each frontend already closed out the HTTP
response on the first `finished: true`. The unbounded join counted it anyway,
inflating the Python side's `output_ids` by exactly one token (Rust's
`engine.rs` returns immediately on the first `finished: true` and
structurally never sees the straggler at all), which is exactly the
one-token shortfall both models showed — not a real content discrepancy, and
not GPU nondeterminism.

**The fix (commit `ae8feec`):** `sweep.py` gained `_bounded_detoks()`, which
truncates each uid's sequence-sorted detok records at the first
`finished=True` record (inclusive), applied in both `join_sequential` and
`join_concurrent`. This matches what every real consumer (both frontends'
own response-finalization logic, and any real HTTP client) actually
receives. Two regression tests
(`test_bounded_detoks_discards_straggler_after_finished`,
`test_join_sequential_discards_straggler_detok_for_par01` in
`python/tests/test_parity_check.py`) prove the truncation directly against
the real raw-tap shape. **No production code change was applied anywhere** —
not in the Rust frontend (confirmed by the Task 0 findings above, which
still hold) and not in the shared backend. The bug was entirely in the
Python-side test harness that joins the backend tap into a comparable
record, and it has been fixed and is now regression-tested.

**Conclusion, corrected:** PAR-01 and PAR-02 both genuinely pass. The GPU
re-run reported in `## Criterion 2` above, taken after this fix (and after
the `--abort-timing deferred` default change, commit `de520b9`), shows
128/128 for both models with zero divergence anywhere in either sequential
sweep.

## Criterion 3: Concurrent-load match rate (PAR-02, informational)

From `concurrent["Qwen/Qwen3-0.6B"]` (`meta.gate_model`, concurrency 128 — Llama has no concurrent block; PAR-02 is measured once, at D-10's single fixed concurrency level, for the gate model only):

- **Status:** `ok` (reason: none)
- **Rust-vs-Python match rate:** 32.8% (42/128)
- **Python-vs-its-own-sequential-output match rate:** 28.9% (37/128)
- **Rust-vs-its-own-sequential-output match rate:** 23.4% (30/128)
- **unmatched_tap:** 0

**Reading:** the Rust-vs-Python concurrent match rate (42/128) sits in the same range as *each* frontend's own agreement with its sequential (one-at-a-time) output (37/128 for Python, 30/128 for Rust) — none of the three numbers is dramatically different from the others. This is consistent with D-10's framing: under concurrent load, GPU batch composition (which requests get batched together, in what order, with what padding) changes outputs for *both* frontends roughly equally, rather than one frontend diverging from its own single-request behavior much more than the other. This measurement is informational only (D-10) and is not a gate.

## Criterion 4: Cancellation stress and the abort-during-prefill bug (D-08)

From `abort_stress` (model: Qwen/Qwen3-0.6B):

### Run: `abort_timing=immediate`

- **stress_rc:** 0 · **stress_timed_out:** false · **canary_ok:** false
- **watch verdict:** `unhealthy` (samples 9, crashed 0, zombie 1, restarts 0, gpu_unlisted 1, nvsmi_errors 0)
- **aborts_by_class:** pending 1, pending_chunked 0, prefill_window 1, decode 27, not_found 1 (30 aborts total, out of 131 requests)
- **late_tokens_after_abort:** 17
- **double_free_uids:** none (0) · **double_free_in_prefill_window:** none (0) · **dup_free_slot_events:** 0
- **collisions:** 0 · **integrity_error:** none
- **failure_mode: crash**

The stress client's own captured output tail shows the scheduler rank-0 process raising a `KeyboardInterrupt` during shutdown, and the launcher's log shows `received SIGINT; exiting` immediately preceding it — consistent with the watcher's `unhealthy` verdict (one zombie sample, one GPU-unlisted sample) for this run.

### Run: `abort_timing=deferred`

- **stress_rc:** 0 · **stress_timed_out:** false · **canary_ok:** true
- **watch verdict:** `healthy` (samples 15, crashed 0, zombie 0, restarts 0, gpu_unlisted 0, nvsmi_errors 0)
- **aborts_by_class:** pending 0, pending_chunked 0, prefill_window 0, decode 28, not_found 0 (28 aborts total, out of 130 requests)
- **late_tokens_after_abort:** 12
- **double_free_uids:** none (0) · **double_free_in_prefill_window:** none (0) · **dup_free_slot_events:** 0
- **collisions:** 0 · **integrity_error:** none
- **failure_mode: none**

### Window probe (`abort_stress.probe`)

`status: ok`. 9 delays × 8 repeats = 72 trials, targeting `corpus:long-04`. `prefill_window_hits: 1`, `double_free_total: 0`, `collisions_total: 0`.

| delay_ms | trials | prefill_window hits | double_free | collisions |
|---|---|---|---|---|
| 0 | 8 | 0 | 0 | 0 |
| 1 | 8 | 1 | 0 | 0 |
| 2 | 8 | 0 | 0 | 0 |
| 3 | 8 | 0 | 0 | 0 |
| 5 | 8 | 0 | 0 | 0 |
| 8 | 8 | 0 | 0 | 0 |
| 13 | 8 | 0 | 0 | 0 |
| 21 | 8 | 0 | 0 | 0 |
| 34 | 8 | 0 | 0 | 0 |

**reproduced: yes**
**conclusive: yes**

**D-08(a) — does it reproduce, and under what trigger conditions:** Yes. The `immediate` abort-timing run landed 1 `prefill_window`-classified abort out of its 30 total aborts, versus 0 of 28 under `deferred`; the dedicated window probe, run only under `immediate` timing, also landed exactly 1 `prefill_window` hit out of 72 trials, and only at `delay_ms=1` — the narrowest tested delay above zero. The window is real but extremely narrow: 71 of 72 probe trials, spread across delays from 0 to 34 ms, missed it. `conclusive: yes` confirms the probe actually exercised the window at least once rather than running 72 trials that all missed it blindly.

**D-08(b) — crash vs. isolated corruption:** Under `immediate` timing, the observed failure mode is `crash` — the scheduler's own watcher verdict is `unhealthy` (one zombie sample, one GPU-unlisted `nvidia-smi` sample) and the stress client's captured output tail shows a `KeyboardInterrupt` traceback from the scheduler rank-0 process. This is the full-process-failure branch, not RESEARCH.md Pitfall 2's predicted "silent, isolated KV-page corruption on an otherwise-healthy, live scheduler": the tap evidence from both runs records **zero** `double_free_uids`, **zero** `dup_free_slot_events` and **zero** `collisions` — nothing in the collected evidence shows two live requests sharing a page slot or a corrupted-but-completed request. Under `deferred` timing, `failure_mode` is `none` (healthy watch, `canary_ok` true, same zero double-free/collision counts), isolating the crash specifically to the `immediate`-timing path. Given this run's own evidence, the abort-during-prefill condition manifests here as a process crash, not as the silently-corrupted-request failure mode RESEARCH.md's source-reading had predicted as more likely.

## Abort-timing decision (D-09)

**abort-timing default: deferred**

**Branch chosen: C (deep/structural) — route around with `--abort-timing deferred`, not a localized shared fix.**

The evidence above is unambiguous that the condition `abort_stress.reproduced=yes`/`conclusive=yes` holds: the `immediate` run's own failure mode is a scheduler process `crash` (watcher verdict `unhealthy`: 1 zombie sample, 1 gpu-unlisted sample out of 9; a `KeyboardInterrupt` traceback from the scheduler rank-0 process), absent entirely under `deferred` timing (`failure_mode: none`, watcher `healthy`). Zero `double_free_uids`, zero `dup_free_slot_events` and zero `collisions` were recorded in either run or in the dedicated window probe, so no small (≤30-line) fix to `scheduler.py`'s abort/finish bookkeeping is supported by the evidence — there is no double-free or collision to localize a fix around. The crash's own root cause is further masked in the captured traceback by what looks like a second interrupt arriving mid-print of the first, so the original fault is not even visible in this evidence. This is a deep/structural case (branch C), not a small localized one (branch B): the project-wide `--abort-timing` default is changed to `deferred` (`crates/rsg-server/src/main.rs`) rather than patching the vendored, pristine scheduler. `UPSTREAM.md`'s `## Known upstream issues` section records the same finding for anyone auditing the vendored tree.

**Python-baseline fairness note:** the frozen Python frontend (`vendor/mini-sglang/python/minisgl/server/api_server.py` lines 190-209) has no abort-timing switch at all — it aborts only after it observes a client disconnect while yielding a chunk, then sleeps 0.1 s before sending `AbortMsg`. There is nothing to set to `deferred` on the Python side; its own abort latency is fixed by its code shape. Phase 7's A/B benchmark design must state explicitly how its cancellation-stress scenario (scenario 1) treats this asymmetry — comparing the Rust frontend's `deferred` abort timing against the Python frontend's fixed chunk-plus-0.1s latency is not comparing the same knob on both sides, and the benchmark write-up should say so rather than imply a like-for-like setting.

## Known limits

- The tap and probe are diagnostic instruments that run only in parity sessions; they are not present in a normal production run of either frontend.
- The frontends notice a client disconnect only at their next write (05-CONTEXT D-03), which bounds how quickly either frontend can react to a cancellation relative to the backend's own processing.
- The concurrent-load rate in `## Criterion 3` depends on GPU batch composition, which this report does not control for beyond D-10's single fixed concurrency level.
- Llama results in `## Criterion 2` are reported for information only, per D-02; they do not gate PAR-01.

## Reproduce

```bash
uv pip install -e vendor/mini-sglang && uv pip install torch-c-dlpack-ext && uv pip install -e . \
  && uv pip install psutil==7.2.2 aiohttp==3.14.4
bash scripts/gpu_phase6_parity.sh
```

This re-runs the release build, `discover` for both frontends, the full
`parity_check.py run` (sequential sweeps for both models, the concurrent
sweep, and both abort-stress runs plus the window probe), `validate
--require-gpu`, `verdict --criterion 1..4`, and `check_upstream.py`, in
sequence, on a Linux GPU machine with CUDA 12.8 and one GPU visible to
`nvidia-smi`. It writes `docs/benchmarks/parity-report.json` even when a
criterion fails, because the sidecar is written before judgment.
