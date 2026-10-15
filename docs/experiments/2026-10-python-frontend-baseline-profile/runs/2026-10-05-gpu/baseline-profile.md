# Python Frontend Baseline Profile

BENCH-01's measured baseline for the frozen Python frontend, read directly from
`docs/benchmarks/baseline-profile.json` (the sidecar `scripts/baseline_profile.py run`
writes). Every number below is copied from that file; none are estimated. Where the
JSON carries a `null` for a metric, this report says so explicitly rather than filling
in a number.

## Run

- **Date:** 2026-10-06T03:17:14Z (`meta.created_utc`)
- **GPU:** NVIDIA GeForce RTX 3050 (`meta.gpu`)
- **Model:** Qwen/Qwen3-0.6B
- **Git commit:** `d4272b3f42f59ca1e7dd5a2f73ec4550dc1549ff` (`meta.git_commit`; `meta.git_dirty` is `false` — the GPU-box checkout was clean at run time)
- **Upstream mini-sglang SHA:** `9a91cfafe754aa85daee49998176275667eb58f2`
- **py-spy:** version `py-spy 0.4.2`, sampling at 100 Hz, flags `--nonblocking --format speedscope`
- **Clock:** `clock_gettime(CLOCK_MONOTONIC)`
- **Python:** 3.12.3, platform `linux`, `meta.mode` = `run`

**Instrumentation caveat:** every number in this report was taken with py-spy
`--nonblocking` sampling, a `gc.callbacks` hook, and `tracemalloc` all active inside
every one of the three frontend/backend processes for the whole scenario. That
instrumentation has real overhead — absolute latencies here are higher than an
uninstrumented run would show (Scenario 3 below measures this directly: the
instrumented ready time is roughly 3.5x the uninstrumented `hyperfine` mean). Phase 7's
benchmarks measure uninstrumented. This report exists for *attribution* — where time
and memory go, and how GC lines up with tail latency — not as a headline speed number.

## Topology and GIL framing

The frozen Python frontend, run with the default `--num-tokenizer 0` and `--tp-size 1`,
is exactly three OS processes, each with its own interpreter and its own GIL:

- **api_server** — FastAPI/uvicorn, HTTP handling.
- **tokenizer** — one combined worker doing *both* tokenize and detokenize, because
  `--num-tokenizer 0` shares the tokenizer process (`share_tokenizer=True`).
- **scheduler** — the backend process (TP rank 0), shared by both frontends.

**There is no cross-process GIL to contend for.** Tokenize and detokenize run
sequentially inside the *same* process and the *same* GIL; HTTP handling is a separate
process with its own, independent GIL. "GIL contention between tokenize, detokenize and
HTTP handling" is therefore not a number that exists in this topology — reporting one
would invent a metric, not measure one. What *can* be measured, and is reported per
scenario below, is each process's own CPU-active and GIL-held percentage, plus the
tokenize-vs-detokenize split of time inside the tokenizer process:

| Scenario | Process | CPU-active % | GIL-held % |
|---|---|---|---|
| s1 (cancel) | api_server | 55.25% | 49.06% |
| s1 (cancel) | scheduler | 93.13% | 35.81% |
| s1 (cancel) | tokenizer | 23.71% | 23.08% |
| s2 (saturation) | api_server | 42.93% | 44.12% |
| s2 (saturation) | scheduler | 52.02% | 39.18% |
| s2 (saturation) | tokenizer | 29.96% | 27.20% |
| s3 (coldstart) | api_server | 28.40% | 29.89% |
| s3 (coldstart) | scheduler | 31.84% | 32.71% |
| s3 (coldstart) | tokenizer | 32.72% | 32.72% |

**Tokenize vs detokenize split inside the tokenizer process** (share of that process's
own active samples):

| Scenario | tokenize share | detokenize share |
|---|---|---|
| s1 (cancel) | 5.93% | 40.01% |
| s2 (saturation) | 6.59% | 11.43% |
| s3 (coldstart) | 10.46% | 0.57% |

In every scenario, detokenize (one call per generated token) dominates tokenize (one
call per request) inside the tokenizer process — expected, since s1 and s2 each decode
many tokens per request while tokenizing the prompt happens once.

## Scenario 1: 128 concurrent agents with cancellations

**Requests:** sent 2012, completed 1528, cancelled 484, failed 0. TTFT p50 98.1 ms, p90
227.5 ms, **P99 TTFT: 968.4 ms**, max 1069.1 ms. RPS: 12.19.

**GC per role:**

| Role | Count | Total pause (ms) | P99 pause (ms) |
|---|---|---|---|
| api_server | 3097 | 1833.68 | 1.09 |
| scheduler | 11 | 2.29 | 0.77 |
| tokenizer | 12 | 3.38 | 1.11 |

**GC-to-P99 overlap (spike threshold = P99 TTFT, 968.4 ms):**

| Role | Spike requests | Spike overlap | Non-spike requests | Non-spike overlap |
|---|---|---|---|---|
| frontend (api_server + tokenizer) | 21 | 100.0% | 1985 | 96.4% |
| scheduler | 21 | 100.0% | 1985 | 5.7% |

*Reading:* frontend GC overlaps almost every request regardless of spike status (100%
spike vs 96.4% non-spike) — api_server alone collects 3097 times over the run, so GC
pauses are so frequent that overlap stops being a useful discriminator for the frontend.
The scheduler's GC overlap is sharply spike-specific instead (100% spike vs only 5.7%
non-spike), making **scheduler-process GC the metric worth watching against P99 TTFT**
in this scenario, not frontend GC.

**Memory per role** (RSS start/end/growth, tracemalloc peak, top 3 allocation sites):

| Role | RSS start | RSS end | RSS growth | tracemalloc peak |
|---|---|---|---|---|
| api_server | 811.8 MB | 971.5 MB | 159.7 MB | 163.2 MB |
| scheduler | 2580.7 MB | 2910.9 MB | 330.2 MB | 211.8 MB |
| tokenizer | 914.0 MB | 1048.2 MB | 134.1 MB | 139.4 MB |

Top 3 allocation sites:
- **api_server:** `<frozen importlib._bootstrap_external>:753` (77.20 MB, 580844 allocs); `<frozen importlib._bootstrap>:488` (5.42 MB, 57111 allocs); `/usr/lib/python3.12/linecache.py:137` (2.86 MB, 32225 allocs)
- **scheduler:** `<frozen importlib._bootstrap_external>:753` (99.78 MB, 709966 allocs); `<frozen importlib._bootstrap>:488` (10.70 MB, 99070 allocs); `/usr/lib/python3.12/dataclasses.py:473` (4.60 MB, 44104 allocs)
- **tokenizer:** `<frozen importlib._bootstrap_external>:753` (68.66 MB, 496838 allocs); `<frozen importlib._bootstrap>:488` (5.34 MB, 56359 allocs); `/usr/lib/python3.12/linecache.py:137` (2.83 MB, 31801 allocs)

Across all three roles, the single largest tracked allocation site is CPython's own
module-import machinery (`importlib._bootstrap_external:753`), not application code —
this is import-time cost, not per-request cost.

**Whole-tree RSS/PSS:** RSS 4322.3 MB (ready) → 4914.1 MB (end); PSS 3761.8 MB (ready) →
4349.3 MB (end).

**CPU per role** (cpu_active_pct, gil_held_pct — see the Topology table above) and
per-request IPC/serialization cost for api_server and the tokenizer:

| Role | ipc_zmq ms/request | serde ms/request |
|---|---|---|
| api_server | 4.23 | 2.54 |
| tokenizer | 6.66 | 4.35 |

## Scenario 2: 32-token short-prompt saturation

**Requests:** sent 512, completed 512, cancelled 0, failed 0. TTFT p50 2161.1 ms, p90
3823.9 ms, **P99 TTFT: 3844.1 ms**, max 3846.3 ms. RPS: 55.18.

**GC per role:**

| Role | Count | Total pause (ms) | P99 pause (ms) |
|---|---|---|---|
| api_server | 229 | 138.23 | 0.90 |
| scheduler | 72 | 139.10 | 132.86 |
| tokenizer | 69 | 4.75 | 0.81 |

**GC-to-P99 overlap (spike threshold = P99 TTFT, 3844.1 ms):**

| Role | Spike requests | Spike overlap | Non-spike requests | Non-spike overlap |
|---|---|---|---|---|
| frontend (api_server + tokenizer) | 6 | 100.0% | 506 | 100.0% |
| scheduler | 6 | 100.0% | 506 | 100.0% |

*Reading:* both frontend and scheduler GC overlap 100% of spike *and* non-spike
requests in this scenario, so overlap rate itself is not a discriminator here — GC is
dense enough relative to this short, bursty 9.3 s window that nearly every request
touches a pause. More telling is the scheduler's single 132.86 ms GC pause (its P99 and
max pause are the same value, i.e. one outlier collection) inside a ~9.3 s window — that
one pause is a plausible direct contributor to the 3844.1 ms P99 TTFT tail, independent
of the overlap-rate statistic.

**Memory per role** (RSS start/end/growth, tracemalloc peak, top 3 allocation sites):

| Role | RSS start | RSS end | RSS growth | tracemalloc peak |
|---|---|---|---|---|
| api_server | 812.0 MB | 986.6 MB | 174.6 MB | 173.0 MB |
| scheduler | 2579.9 MB | 2935.2 MB | 355.3 MB | 211.7 MB |
| tokenizer | 925.1 MB | 1059.4 MB | 134.3 MB | 138.1 MB |

Top 3 allocation sites:
- **api_server:** `<frozen importlib._bootstrap_external>:753` (77.20 MB, 580848 allocs); `<frozen importlib._bootstrap>:488` (5.43 MB, 57112 allocs); `/usr/lib/python3.12/linecache.py:137` (2.86 MB, 32225 allocs)
- **scheduler:** `<frozen importlib._bootstrap_external>:753` (99.76 MB, 709849 allocs); `<frozen importlib._bootstrap>:488` (10.70 MB, 99082 allocs); `/usr/lib/python3.12/dataclasses.py:473` (4.60 MB, 44063 allocs)
- **tokenizer:** `<frozen importlib._bootstrap_external>:753` (68.66 MB, 496840 allocs); `<frozen importlib._bootstrap>:488` (5.34 MB, 56366 allocs); `/usr/lib/python3.12/linecache.py:137` (2.83 MB, 31801 allocs)

Same pattern as Scenario 1: import machinery dominates every role's top allocation site.

**Whole-tree RSS/PSS:** RSS 4332.7 MB (ready) → 4908.7 MB (end); PSS 3757.7 MB (ready) →
4260.6 MB (end).

**CPU per role** (see the Topology table above) and per-request IPC/serialization cost
for api_server and the tokenizer:

| Role | ipc_zmq ms/request | serde ms/request |
|---|---|---|
| api_server | 0.51 | 0.23 |
| tokenizer | 0.80 | 0.53 |

## Scenario 3: Cold start and host RAM

**Requests:** sent 1, completed 1, cancelled 0, failed 0. TTFT p50/p90/**P99 TTFT: 1179.6
ms**/max all equal 1179.6 ms (a single request). RPS: 0.10.

Because this scenario sends exactly one request, its per-request IPC/serde figures below
are not comparable to Scenario 1/2's — they divide a single process's sampled time by
`requests_completed=1`, not by a meaningful request count.

**GC per role:**

| Role | Count | Total pause (ms) | P99 pause (ms) |
|---|---|---|---|
| api_server | 735 | 332.25 | 1.60 |
| scheduler | 1004 | 1002.23 | 3.39 |
| tokenizer | 635 | 280.72 | 3.56 |

**GC-to-P99 overlap (spike threshold = P99 TTFT, 1179.6 ms):**

| Role | Spike requests | Spike overlap | Non-spike requests | Non-spike overlap |
|---|---|---|---|---|
| frontend (api_server + tokenizer) | 1 | 100.0% | 0 | n/a (no non-spike requests — only one request was sent) |
| scheduler | 1 | 100.0% | 0 | n/a (no non-spike requests — only one request was sent) |

*Reading:* with only one request in this scenario, spike-vs-non-spike GC overlap is not
a statistically meaningful comparison; it is reported here only because the JSON reports
it, not as a finding.

This scenario's GC counts (735/1004/635 collections) are far higher than Scenario 1 or
2's despite only one HTTP request, because this window includes the server's *boot*
(`include_boot=True`) — model load, import, and CUDA init, not per-request work. The
scheduler's max single pause here is 432.76 ms, by far the largest GC pause recorded in
any scenario.

**Memory per role** (RSS start/end/growth, tracemalloc peak, top 3 allocation sites):

| Role | RSS start | RSS end | RSS growth | tracemalloc peak |
|---|---|---|---|---|
| api_server | 807.6 MB | 1041.5 MB | 233.9 MB | 155.6 MB |
| scheduler | 2578.1 MB | 2885.3 MB | 307.3 MB | 211.3 MB |
| tokenizer | 915.4 MB | 1129.0 MB | 213.6 MB | 138.0 MB |

Top 3 allocation sites:
- **api_server:** `<frozen importlib._bootstrap_external>:753` (77.22 MB, 581026 allocs); `<frozen importlib._bootstrap>:488` (5.42 MB, 57113 allocs); `/usr/lib/python3.12/linecache.py:137` (2.86 MB, 32225 allocs)
- **scheduler:** `<frozen importlib._bootstrap_external>:753` (99.76 MB, 709860 allocs); `<frozen importlib._bootstrap>:488` (10.70 MB, 99079 allocs); `/usr/lib/python3.12/dataclasses.py:473` (4.60 MB, 44062 allocs)
- **tokenizer:** `<frozen importlib._bootstrap_external>:753` (68.66 MB, 496839 allocs); `<frozen importlib._bootstrap>:488` (5.34 MB, 56364 allocs); `/usr/lib/python3.12/linecache.py:137` (2.83 MB, 31801 allocs)

**Whole-tree RSS/PSS (this instrumented session):** RSS 4316.8 MB (ready) → 5074.1 MB
(end); PSS 3681.9 MB (ready) → 4439.5 MB (end).

**CPU per role** (see the Topology table above) and per-request IPC/serialization cost
for api_server and the tokenizer (caveat above — divided by 1 request):

| Role | ipc_zmq ms/request | serde ms/request |
|---|---|---|
| api_server | 130.00 | 0.00 |
| tokenizer | 40.00 | 20.00 |

**Cold start (hyperfine, 3 timed runs after 1 warmup, uninstrumented):**
mean 16.156 s ± 1.180 s (stddev); median 16.245 s; min 14.934 s; max 17.289 s.

**Self-timed ready seconds** (per run, uninstrumented `coldstart-once` wrapper):
16.042 s, 17.032 s, 14.567 s — consistent with the hyperfine timings above.

**Tree PSS at ready** (per run, uninstrumented): 3118.5 MB, 3122.9 MB, 3119.8 MB.

**Instrumented boot time:** the one instrumented first-request session (py-spy +
gc.callbacks + tracemalloc all active through boot) took 56.18 s to become ready
(`params.instrumented_ready_s`), roughly **3.5x** the uninstrumented hyperfine mean of
16.16 s — the concrete number behind this report's instrumentation caveat in the Run
section above. End-to-end startup (weight load + CUDA init dominates) and the
front-half-only instrumentation delta should be reported separately in Phase 7, as the
uninstrumented hyperfine numbers are the ones that matter for a headline cold-start
comparison.

## Radix cache share (input to RADIX-01)

The scheduler process's share of sampled time spent inside the radix cache
(`scheduler/cache.py`'s `match_req`/`cache_req` and `kvcache/radix_cache.py`'s
`match_prefix`/`insert_prefix`/`evict`/`_tree_walk`), against total scheduler-process
sampled time (D-10):

| Scenario | Radix samples | Scheduler samples | Share |
|---|---|---|---|
| s1 (cancel) | 194 | 12253 | **1.58%** |
| s2 (saturation) | 6 | 790 | **0.76%** |
| s3 (coldstart) | 5 | 512 | **0.98%** |

**Recommendation for RADIX-01's "meaningful share" condition:** across all three
scenarios, radix-cache time is under 2% of the scheduler's sampled time — nowhere close
to a plurality or even a double-digit-percent share of scheduler time in any scenario
measured. RADIX-01 gates the Rust radix cache port on radix time being a "meaningful
share of scheduler time"; at these measured levels, the scheduler is spending roughly
50-100x more sampled time on things other than the radix cache (CUDA-sync waits, batch
scheduling, ZMQ IPC, serde — see the Known blind spots section below for why some of
that "other" time is itself uncertain). This report's recommendation is that **radix
time, as measured here, does not clear a "meaningful share" bar** — but this is a
recommendation for the project author's decision, not an automatic verdict: the
threshold itself ("meaningful" = what percentage) is not numerically defined in
REQUIREMENTS.md, and a different workload (e.g. one with much higher prefix-reuse rate
than this phase's synthetic scenarios) could show a materially higher share.

## Inputs to Phase 7 benchmark design

**(a) Metrics Phase 7 must capture**, each tied to a measured finding in this report:

- **Frontend GC counts alongside P99 TTFT, per process, not pooled.** Scenario 1 shows
  *why* this must be per-process: frontend (api_server+tokenizer) GC overlap is
  uniformly high regardless of spike status (100%/96.4%), but scheduler GC overlap is
  sharply spike-specific (100%/5.7%). A single pooled "GC overlap rate" metric would
  have hidden the scheduler's much stronger signal. BENCH-08 should be read as
  "per-process", not "per-frontend".
- **Tokenizer GIL-held percentage, tracked across `--num-tokenizer` settings.** Measured
  here, the tokenizer is far from GIL saturation in Scenario 2 (27.20% GIL-held, 29.96%
  CPU-active) — this single-GPU, 32-token, Qwen3-0.6B workload does not push the shared
  tokenize+detokenize worker close to its own ceiling. That does not make the metric
  unnecessary: BENCH-07 explicitly compares the Python frontend's default
  `--num-tokenizer` setting against its best one, and this is exactly the number that
  would reveal whether splitting tokenize/detokenize into separate processes helps.
- **Whole-process-tree PSS at ready, reported separately from end-to-end PSS.**
  Scenario 3's uninstrumented tree-PSS-at-ready values (3118.5-3122.9 MB) are the
  cold, idle baseline; growth after serving a request pushes PSS to 4260-4655 MB across
  the three scenarios. Phase 7's cold-start comparison (BENCH-05) should report both
  numbers, not just one, exactly as this report does.
- **Scheduler's single-largest GC pause per scenario, not just P99.** Scenario 2's
  scheduler P99 pause (132.86 ms) is also its *only* large pause (max equals P99) inside
  a 9.3 s window — a single outlier that a mean or even a P90 would miss entirely. This
  argues for reporting max pause alongside P99 in Phase 7, not P99 alone.

**(b) Frontend vs. backend attribution:**

- **Creditable to the frontend** (api_server + tokenizer processes, what a Rust frontend
  could change): the `ipc_zmq`/`serde` per-request costs (api_server: 4.23/2.54 ms in
  s1, 0.51/0.23 ms in s2; tokenizer: 6.66/4.35 ms in s1, 0.80/0.53 ms in s2),
  tokenize/detokenize bucket time inside the tokenizer process, the `http_stack`/
  `api_handlers` bucket time inside api_server, and the frontend-process GC pauses
  (api_server's 3097 collections / 1833.68 ms total pause in s1 is by far the largest GC
  load of any single process in any scenario).
- **Belongs to the shared backend** (scheduler process, paid identically by both
  frontends, not something a Rust frontend changes): scheduler CPU-active percentage
  (93.13% in s1 — the busiest process in the busiest scenario), the radix-cache share
  quantified above, and scheduler GC pauses (including the two largest single pauses
  recorded anywhere in this report: 132.86 ms in s2, 432.76 ms in s3's boot window).
  Phase 7 should report scheduler-side numbers as a shared cost floor neither frontend
  can improve, not credit or blame them to whichever frontend is under test.

## Known blind spots

- **Native radix kernel time is not split out.** Without `--native`, py-spy attributes
  time spent inside the native radix kernel (`kernel/radix.py`'s `fast_compare_key`, a
  C++ extension) to its calling Python frame (`match_prefix` and similar) — so native
  kernel time is still counted inside the radix share above, but cannot be distinguished
  from the surrounding Python frame's own time.
- **CUDA-sync spin time reads as active scheduler CPU time.** Time the scheduler process
  spends spinning on a CUDA synchronization point looks identical to py-spy as genuine
  on-CPU Python work — the scheduler's high CPU-active percentages (up to 93.13% in s1)
  likely include some of this, not pure Python/radix/IPC work.
- **py-spy sample drops under load make every percentage a lower bound.** `--nonblocking`
  sampling can miss samples when a process is under heavy load exactly when a sample
  would otherwise fire, so every percentage in this report (CPU-active, GIL-held, bucket
  shares, radix share) should be read as "at least this much", not an exact figure.
- **Instrumentation overhead inflates every absolute latency.** As noted in the Run
  section and demonstrated directly in Scenario 3 (56.18 s instrumented boot vs 16.16 s
  uninstrumented hyperfine mean, roughly 3.5x), py-spy + gc.callbacks + tracemalloc
  running simultaneously in every process add real overhead. Use this report for
  *attribution* (where time and memory go, relative to each other) rather than as a
  source of headline absolute numbers; Phase 7's uninstrumented runs are the source for
  those.
- **Some mid-run allocation snapshots are missing.** The sidecar's own `warnings` field
  flags that no mid-run allocation snapshot was captured for the scheduler in Scenario 1,
  and for all three roles in Scenario 3 (`s1_cancel: no allocation snapshot from
  scheduler`; `s3_coldstart: no allocation snapshot from scheduler`; `s3_coldstart: no
  allocation snapshot from api_server`; `s3_coldstart: no allocation snapshot from
  tokenizer`). The tracemalloc current/peak and top-allocation-site figures reported
  above for those role/scenario combinations come from whichever snapshot(s) the sidecar
  did capture, not a guaranteed mid-run sample.

## Reproduce

```bash
uv pip install -e vendor/mini-sglang && uv pip install torch-c-dlpack-ext && uv pip install -e . \
  && uv pip install py-spy==0.4.2 psutil==7.2.2 aiohttp==3.14.4 \
  && cargo install hyperfine --version 1.20.0 --locked
bash scripts/gpu_phase2_profile.sh
```

This re-runs preflight, `discover`, `run` (which writes
`docs/benchmarks/baseline-profile.json`), `validate --require-gpu`, and
`check_upstream.py` in sequence on a Linux GPU machine with CUDA 12.8 and one GPU
visible to `nvidia-smi`.
