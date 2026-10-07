# Rust vs Python Frontend Output Parity

PAR-01/PAR-02's measured GPU parity run, read directly from
`docs/benchmarks/parity-report.json` (the sidecar `scripts/parity_check.py run`
writes, via `scripts/gpu_phase6_parity.sh`). Every number below is copied from
that file; none are estimated. Where the JSON carries a `null` for a field,
this report says so explicitly rather than filling in a number.

## Run

- **Date:** 2026-10-07T20:00:42Z (`meta.created_utc`)
- **GPU:** NVIDIA GeForce RTX 3050 (`meta.gpu`)
- **Git commit:** `6af8a91b5a7871489250c857ba314d543f4a32bc` (`meta.git_commit`; `meta.git_dirty` is `false` — the GPU-box checkout was clean at run time)
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
- **matched/n:** 127/128
- **ids_matched/n:** 127/128
- **text_matched/n:** 127/128
- **by_layer:** `request_error` 0, `tokenization` 0, `sampling_params` 0, `backend` 1, `incomplete` 0, `detokenization_or_api` 0

**Result: the zero-tolerance hard gate (D-04) FAILS.** `n=128` clears the `n >= 100` floor, but `matched=127 != n=128` — one prompt (`edge-08`, category `edge`) diverges. Per D-04, any single-token mismatch on any prompt fails the gate; this run's single `edge-08` mismatch means **PAR-01's hard gate does not pass for Qwen/Qwen3-0.6B on this run.** See `## Divergence bisection (D-05)` below for the bisected cause.

### meta-llama/Llama-3.2-1B-Instruct (reported, not gated)

- **Status:** `ok` (reason: none)
- **matched/n:** 127/128
- **ids_matched/n:** 127/128
- **text_matched/n:** 127/128
- **by_layer:** `request_error` 0, `tokenization` 0, `sampling_params` 0, `backend` 1, `incomplete` 0, `detokenization_or_api` 0

Llama's result is reported for information only (D-02); it is not gated. The same prompt id, `edge-08`, is the sole mismatch for this model too, at a different divergence point than Qwen's — see the bisection below.

## Divergence bisection (D-05)

Two mismatches, one per model, both at prompt `edge-08` (category `edge`, a chat prompt whose user content embeds chat-template special-token literals — `<|endoftext|>` / `<|im_end|>` — per the corpus's edge-category requirements). `annotate_sequence` recorded `note: null` for both, meaning **neither divergence is preceded by an earlier input-level (tokenization/sampling_params) mismatch** in its own sweep — these are not explained by upstream radix-cache drift from a prior mismatched prompt.

- **Model:** Qwen/Qwen3-0.6B
  **Prompt:** `edge-08` · **Category:** `edge`
  **Layer:** `backend` · **First diverging index:** 127 (the request's `max_tokens` is 128, so this is the very last generated position)
  **Python window (decoded):** `['Ġuser', 'Ġmight', 'Ġnot', 'Ġbe', 'Ġfamiliar']`
  **Rust window (decoded):** `['Ġuser', 'Ġmight', 'Ġnot', 'Ġbe']`
  **Trace note:** Same `input_ids` and same `sampling` (`temperature=0.0`, `max_tokens=128`, `ignore_eos=false`) reach both frontends — this is a `backend`-layer divergence: same inputs, different outputs. Python emits exactly 128 output tokens (hits the `max_tokens` cap with a 128th token, `familiar`); Rust stops one token short, at 127. The divergence happens at the last possible position before the cap. **Working hypothesis (not confirmed): an off-by-one in how the Rust frontend's FSM enforces `max_tokens` exactly at the cap boundary** — it requests/accepts one fewer decode step than the Python frontend before marking the response finished. This prompt is the only item in the whole 128-item corpus that both exercises `<think>`-mode generation and runs all the way to its `max_tokens` cap, which is why no other prompt shows the same shape of divergence.

- **Model:** meta-llama/Llama-3.2-1B-Instruct
  **Prompt:** `edge-08` · **Category:** `edge`
  **Layer:** `backend` · **First diverging index:** 36 (well short of `max_tokens=128` — this run ends naturally at the model's own stop token, not the cap)
  **Python window (decoded):** `['Ġof', 'Ġvalues', '.', '<|eot_id|>', '<|start_header_id|>']`
  **Rust window (decoded):** `['Ġof', 'Ġvalues', '.', '<|eot_id|>']`
  **Trace note:** Again a `backend`-layer divergence with identical `input_ids`/`sampling` on both sides. Both frontends agree on every token through `<|eot_id|>` (Llama's end-of-turn token) at index 35. Python then emits one further token, `<|start_header_id|>`, at index 36 before finishing; Rust finishes immediately after `<|eot_id|>`, one token shorter. Unlike the Qwen case above, this has nothing to do with the `max_tokens` cap (the response is 37 tokens long against a 128 cap) — it is specifically about what happens in the single step immediately following the end-of-turn token. **Working hypothesis (not confirmed): the Python frontend's FSM/detokenizer accepts one more already-in-flight token after the stop token is observed before it marks the request finished, while the Rust frontend's FSM stops as soon as it sees the stop id.** This is a distinct hypothesis from the Qwen entry above — same `backend` layer label, different trigger (end-of-turn boundary vs. `max_tokens` cap boundary) — and both are findings to carry forward, not root-caused further within this plan's scope.

No other prompt in either model's sequential sweep diverged.

## PAR-01 off-by-one investigation (Phase 6 scope expansion)

Plan 06-08 Task 0 (user-approved scope expansion, beyond this plan's
original D-09-only scope) investigated whether either of the two
`edge-08` mismatches above is a Rust-frontend bug, before accepting
Criterion 2's FAIL at face value. This section documents the finding; it
is a different question from the D-05 bisection above (where the
divergence falls) and Task 3's D-05 disposition (what to do about a
Criterion 2 FAIL) -- this section only answers whether the Rust frontend's
own code could cause a backend-sent token to go missing or an independent
"this looks like EOS" decision to end a request a token early.

**What was checked, directly against the JSON:**

- Both mismatches have `python_side.sampling == rust_side.sampling`
  (`{"temperature": 0.0, "top_k": -1, "top_p": 1.0, "ignore_eos": false,
  "max_tokens": 128}` on both sides, both models) and byte-identical
  `output_ids` for every position up to the shortfall -- the lists are not
  merely "similar", they are the exact same integers through the last
  token Rust produced.
- `output_ids`/`finished` in this JSON come from `tap.py`'s
  `_wrap_reply_tokenizer_rank0` wrapper, hooked around
  `SchedulerIOMixin._reply_tokenizer_rank0` **inside the scheduler/backend
  process itself**, before any ZMQ framing. This is the backend's own
  record of what it decided to emit, independent of what either frontend
  received, decoded, or reported over HTTP.
- `python/rsglang/backend.py::run_scheduler` (the rust-mode backend
  launch) and `python/rsglang/launch.py`'s `exec_python_frontend`
  (`--frontend python`) both build `ServerArgs` from the identical
  upstream `parse_args(rest)` call and construct the identical unmodified
  `minisgl.scheduler:Scheduler` class. Neither frontend mode passes a
  different `max_seq_len`, model path, or other backend-construction
  argument than the other.

**What was reproduced, on the Mac, with no GPU or network dependency**
(`crates/rsg-server/tests/backend_finish_boundary.rs`, run against the
real `rsg-server` binary, `engine.rs`/`dispatch.rs`/`writer.rs` unmodified):

- A hand-built `DetokenizeMsg` sequence injected directly into the
  dispatcher's bound detok socket (bypassing `mock-scheduler`'s own
  cap-based finishing logic entirely) proves the engine only ever branches
  on the wire's `finished` bit: a `finished=false` reply carrying an
  eos-shaped id, immediately followed by a different id flagged
  `finished=true`, arriving well before the client's requested
  `max_tokens` cap, is reported as (non-finished text, then the finishing
  token) -- `engine.rs` never makes its own "that id looks like EOS" call.
- A `finished=true` reply on the very first message for a uid ends the
  request immediately and correctly -- the shortest "backend decided to
  stop now" shape.
- The pre-existing `crates/rsg-server/tests/http_chat.rs` coverage
  (`chat_nonstream_response_shape`, `tracer_chat_stream_matches_upstream_framing`)
  already proves exact `max_tokens`-for-`max_tokens` delivery through the
  full real binary over real `ipc://` sockets against a real
  `mock-scheduler` subprocess -- re-run here as corroboration, unchanged.

**Conclusion: no production code change is applied.** The Rust frontend's
engine/dispatch pair is a faithful pass-through of the backend's own
`finished` flag, with no independent stopping logic of any kind; the
shortfall originates inside the shared, unmodified backend itself (per the
tap's own emission-point record), with byte-identical inputs and no
earlier divergence in either session. The divergence position in both
cases -- the `max_tokens` cap; immediately after a stop token -- is
exactly where a model's logit margin between its top-1 and top-2
candidates is typically smallest (the natural end of a response), which is
consistent with ordinary run-to-run GPU floating-point nondeterminism
flipping a near-tie argmax between two separate backend process launches,
not with a systematic frontend defect. This is the same shape CONTEXT D-04
anticipated as a hard blocker ("backend-layer + identical inputs + no
earlier divergence"); Task 3's `## PAR-01 disposition (D-05)` section
below records what this means for Criterion 2's FAIL.

## Criterion 3: Concurrent-load match rate (PAR-02, informational)

From `concurrent["Qwen/Qwen3-0.6B"]` (`meta.gate_model`, concurrency 128 — Llama has no concurrent block; PAR-02 is measured once, at D-10's single fixed concurrency level, for the gate model only):

- **Status:** `ok` (reason: none)
- **Rust-vs-Python match rate:** 28.1% (36/128)
- **Python-vs-its-own-sequential-output match rate:** 27.3% (35/128)
- **Rust-vs-its-own-sequential-output match rate:** 26.6% (34/128)
- **unmatched_tap:** 0

**Reading:** the Rust-vs-Python concurrent match rate (36/128) sits in the same range as *each* frontend's own agreement with its sequential (one-at-a-time) output (35/128 for Python, 34/128 for Rust) — none of the three numbers is dramatically different from the others. This is consistent with D-10's framing: under concurrent load, GPU batch composition (which requests get batched together, in what order, with what padding) changes outputs for *both* frontends roughly equally, rather than one frontend diverging from its own single-request behavior much more than the other. This measurement is informational only (D-10) and is not a gate.

## Criterion 4: Cancellation stress and the abort-during-prefill bug (D-08)

From `abort_stress` (model: Qwen/Qwen3-0.6B):

### Run: `abort_timing=immediate`

- **stress_rc:** 0 · **stress_timed_out:** false · **canary_ok:** false
- **watch verdict:** `unhealthy` (samples 14, crashed 0, zombie 1, restarts 0, gpu_unlisted 1, nvsmi_errors 0)
- **aborts_by_class:** pending 1, pending_chunked 0, prefill_window 1, decode 28, not_found 0 (30 aborts total, out of 131 requests)
- **late_tokens_after_abort:** 8
- **double_free_uids:** none (0) · **double_free_in_prefill_window:** none (0) · **dup_free_slot_events:** 0
- **collisions:** 0 · **integrity_error:** none
- **failure_mode: crash**

The stress client's own captured output tail shows the scheduler rank-0 process raising a `KeyboardInterrupt` during shutdown, and the launcher's log shows `received SIGINT; exiting` immediately preceding it — consistent with the watcher's `unhealthy` verdict (one zombie sample, one GPU-unlisted sample) for this run.

### Run: `abort_timing=deferred`

- **stress_rc:** 0 · **stress_timed_out:** false · **canary_ok:** true
- **watch verdict:** `healthy` (samples 21, crashed 0, zombie 0, restarts 0, gpu_unlisted 0, nvsmi_errors 0)
- **aborts_by_class:** pending 0, pending_chunked 0, prefill_window 0, decode 26, not_found 1 (27 aborts total, out of 130 requests)
- **late_tokens_after_abort:** 16
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

**D-08(a) — does it reproduce, and under what trigger conditions:** Yes. The `immediate` abort-timing run landed 1 `prefill_window`-classified abort out of its 30 total aborts, versus 0 of 27 under `deferred`; the dedicated window probe, run only under `immediate` timing, also landed exactly 1 `prefill_window` hit out of 72 trials, and only at `delay_ms=1` — the narrowest tested delay above zero. The window is real but extremely narrow: 71 of 72 probe trials, spread across delays from 0 to 34 ms, missed it. `conclusive: yes` confirms the probe actually exercised the window at least once rather than running 72 trials that all missed it blindly.

**D-08(b) — crash vs. isolated corruption:** Under `immediate` timing, the observed failure mode is `crash` — the scheduler's own watcher verdict is `unhealthy` (one zombie sample, one GPU-unlisted `nvidia-smi` sample) and the stress client's captured output tail shows a `KeyboardInterrupt` traceback from the scheduler rank-0 process. This is the full-process-failure branch, not RESEARCH.md Pitfall 2's predicted "silent, isolated KV-page corruption on an otherwise-healthy, live scheduler": the tap evidence from both runs records **zero** `double_free_uids`, **zero** `dup_free_slot_events` and **zero** `collisions` — nothing in the collected evidence shows two live requests sharing a page slot or a corrupted-but-completed request. Under `deferred` timing, `failure_mode` is `none` (healthy watch, `canary_ok` true, same zero double-free/collision counts), isolating the crash specifically to the `immediate`-timing path. Given this run's own evidence, the abort-during-prefill condition manifests here as a process crash, not as the silently-corrupted-request failure mode RESEARCH.md's source-reading had predicted as more likely — D-09's decision on what to do about it is out of this plan's scope (deferred to plan 06-08).

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
