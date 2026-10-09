# Findings: does a drop-in Rust frontend rewrite actually help?

**TL;DR: it's not a uniform win, and the split is informative.** Holding the
GPU backend fixed and identical for both frontends: cold start and host RAM
are a clear, large Rust win; raw throughput is statistically tied; tail
latency under concurrent or saturated load is dramatically *worse* for
Rust — not a rounding error, an order of magnitude. The root cause for that
last one is not obviously a mistake in the Rust implementation; the
evidence points at the ZMQ process-boundary transport itself, which is the
one piece of the original architecture this project could not change (the
vendored Python scheduler must stay frozen and unmodified — see the repo's
`CLAUDE.md`). That conclusion is independently corroborated by SGLang's own
production Rust-migration team, who hit the same wall and abandoned ZMQ for
an in-process, zero-copy transport.

## Situation

mini-sglang's Python frontend (HTTP ingress, tokenizer, detokenizer) talks
to its GPU scheduler over ZMQ PUSH/PULL sockets using a fixed MessagePack
wire format. The premise this project tests: if the GPU backend is left
completely untouched, does rewriting *only* the frontend in Rust reduce
host-overhead latency in the scenarios where host overhead — not GPU compute
— dominates request time?

## Task

Build a Rust frontend that is byte-compatible with the existing wire
protocol (so the same, unmodified Python scheduler process can't tell which
frontend it's talking to), verify it produces identical output to the
Python frontend on the same backend (differential testing, not just unit
tests), and build a benchmark harness that isolates and measures exactly
three scenarios engineered to be host-overhead-bound rather than GPU-bound:

1. **S1** — 128 concurrent agents, dynamic requests with mid-stream
   cancellations. Measures P50/P90/P99 time-to-first-token (TTFT).
2. **S2** — 32-token short-prompt saturation. Measures the maximum
   sustainable requests/second (RPS) and the TTFT saturation curve as load
   ramps.
3. **S3** — cold start and host RAM (process-tree RSS/PSS), separating
   frontend-only readiness from end-to-end readiness (which includes GPU
   weight loading, identical for both frontends).

## Action

**Real GPU results (NVIDIA RTX 3050, `scripts/gpu_phase7_bench.sh`, 5 rounds
per arm, Welch's 95% CI). Full tables: `docs/benchmarks/frontend-benchmarks.md`.**

Along the way, four real, reproducible bugs were found and fixed while
getting this benchmark to actually run on real GPU hardware (not just
pass in CI with a mock backend):

- `sysinfo` on Linux surfaces each userland OS thread of a multi-threaded
  process (tokio's worker threads) as its own pid-like entry. Two places in
  the harness that walked the process tree by raw pid/pgid treated every
  thread as a distinct process, inflating both RSS totals (summed once per
  thread instead of once per process) and process-group membership counts.
- The harness's log-watching loop truncated a `String` at a raw byte
  offset to find a readiness marker; the server's log contains multi-byte
  UTF-8 characters (tqdm's progress-bar glyphs during CUDA graph capture),
  and the cut reliably landed mid-character for one frontend's specific log
  timing, panicking `String::drain`.
- Two missing/misconfigured dependencies on the GPU box itself (`ninja` not
  on `$PATH`, the system CUDA toolkit several major versions behind what
  PyTorch/flashinfer were built against) silently prevented the real
  backend from ever reaching a ready state, for every trial, for hours,
  before being root-caused.
- `standard_throughput` (BENCH-06, upstream's own `bench_simple.py`
  workload — 64 concurrent requests, prompts up to 8192 tokens,
  deliberately backend/GPU-bound rather than host-overhead-bound) failed
  5/5 rust trials, every round, with a "Connection error" from the Python
  load driver. Root cause: `rsg-server`'s own backend-inactivity watchdog
  (60s default, reset per token) force-failed requests the backend was
  still legitimately working on — on this 8GB GPU, Python's own measured
  p99 TTFT in the *identical* workload is ~100 **seconds**, and Python's
  frontend has no equivalent watchdog. `rsglang.launch --frontend rust`
  never forwarded `--backend-timeout-ms` to `rsg-server` at all (no way to
  raise it for a workload that needs to); fixed by adding the passthrough,
  mirroring the existing `--abort-timing` one exactly.

## Result

**Cold start & host RAM (S3): Rust wins, clearly.** Frontend RSS: **490 MB**
(Rust) vs. 1.48 GB (Python, default `--num-tokenizer`) vs. 3.03 GB (Python,
sweep-tuned) — a 67–84% reduction, statistically significant
(p < 0.05, CIs exclude zero). End-to-end cold start: 9.13s (Rust) vs. 9.67s
/ 10.74s (Python) — 5.6–15.0% faster, significant against the tuned
Python config.

**Raw throughput: close to tied, with one small but real exception.** S1's
RPS delta was small (~3%) but *did* reach statistical significance (CIs
exclude zero) — Rust processed marginally fewer requests/sec under
cancellation load. S2's peak RPS delta was smaller still (~0.3%) and also
technically significant given the large sample size, but not practically
meaningful. Once the backend-timeout mismatch above was fixed,
`standard_throughput`'s throughput and tail-latency deltas are both
genuinely tied (every CI includes zero) between Rust and Python —
confirming this is a parity result, not a frontend-dependent one, exactly
as expected for a backend-bound workload.

**Tail latency under load (S1/S2): dramatically worse for Rust — the
headline negative result.**

| Scenario | Metric | Python (best) | Rust | Delta |
|---|---|---|---|---|
| S1 (cancellation, 128 agents) | P99 TTFT | 430.8ms | **4687.5ms** | **+988%**, CI [913%, 1063%] |
| S2 (saturation, rate=60) | P99 TTFT | 126.1ms | **1016.2ms** | ~8x |
| S2 (saturation, rate=80) | P99 TTFT | 159.3ms | 373.5ms | ~2.3x |

S1's P99 TTFT delta is enormous and tightly bounded — not noise. In S2, the
degradation is sharpest at exactly one load level (rate=60); Python stays
smooth across the whole ramp.

### Root cause investigation

Reading `rsg-server`'s request-lifecycle code (`engine.rs`, `dispatch.rs`,
`writer.rs`) found a well-engineered implementation — Drop-based
cancellation, a `biased` `select!` prioritizing aborts, lock-free per-uid
routing, an already-fixed prior bug (a tokenizer-decoder clone that could
starve sibling requests' own polls). No obvious "the Rust code blocked the
runtime" bug surfaced.

What did surface: every submit and every abort for every in-flight request
funnels through **one** dedicated OS thread (`tx-zmq`) and **one** bounded
queue, which batches whatever's pending and sends it over the ZMQ PUSH
socket to the Python scheduler. If the scheduler falls behind draining its
PULL socket under load, the PUSH send blocks — and that block propagates
backward through the bounded queue into every subsequent request's own
`submit().await`.

A standalone micro-benchmark (`crates/zmq-vs-channel-spike/`, not part of
the production harness) tested this directly: the same ZMQ transport,
socket options, and real wire-format frame sizes `rsg-server` actually
uses, against an equivalent in-process bounded channel, under matched
backpressure. Under a cancel-storm load pattern, the two transports failed
in qualitatively different ways — the in-process channel degraded
uniformly and predictably (bounded by the drain rate), while ZMQ stayed
near-instant at p50/p90/p99 but hit **1.48–1.65 second** maximums: rare,
catastrophic stalls rather than uniform slowdown. That "usually instant,
occasionally catastrophic" shape is consistent with — not proof of — the
real benchmark's measured pattern (tied RPS, a P99 blown out by outliers).
Full write-up: `crates/zmq-vs-channel-spike/README.md`.

### Independent corroboration

This isn't a theory invented to excuse a bad number. SGLang's own
production Rust-migration effort
([sgl-project/sglang#23206](https://github.com/sgl-project/sglang/issues/23206))
replaced ZMQ with bounded in-process channels, per-core SPSC returns, and
pinned-memory zero-copy tensor handoff — explicitly to keep the Rust↔Python
boundary off the hot path. Their own risk register separately flags
UTF-8-boundary handling under streaming as a known hazard class requiring
dedicated boundary tests — independent confirmation that the second bug
found above is a recognized failure mode in this exact domain, not
something specific to this project's inexperience.

The difference: SGLang's team can patch the Python scheduler they own.
This project's explicit constraint — the vendored backend stays frozen and
unmodified, so the Rust and Python frontends are compared fairly against
the literal same process — rules that fix out. The measured regression is
likely the honest cost of that constraint, not a defect to be optimized
away within it.

## Future work

- **Confirm the writer-queue-depth hypothesis with live data.** `rsg-server`
  now exposes `rsg_writer_queue_depth` on `/metrics` (added alongside this
  report). The next step is a short, targeted load test correlating that
  gauge's timeseries against TTFT spikes from a real run, turning "the
  micro-benchmark's shape is consistent with this" into a direct
  measurement on the real system.
- **In-process, zero-copy IPC.** The architecturally complete fix —
  removing the ZMQ hop entirely, the way SGLang's own migration did —
  requires patching the vendored Python scheduler, which is out of this
  project's current scope by design (see `CLAUDE.md`'s "fair comparison"
  constraint). This is the natural next milestone if that constraint is
  deliberately revisited, not a bug to fix within the current one.
- **Rust radix-tree KV cache.** SGLang's own Rust radix-tree integration
  (`sgl-project/sglang#20415`) splits the tree behind a
  `UnifiedTreeCoreInterface`: Rust owns match/insert/evict/LRU, Python
  keeps the KV pool/allocator, and the tree returns a deferred-action list
  for Python to execute. The same upstream-patching constraint that rules
  out in-process IPC rules this out today; it's recorded here as the
  reference pattern to follow if that ever changes.
