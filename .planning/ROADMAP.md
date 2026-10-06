# Roadmap: mini-rsglang

## Overview

The project starts by vendoring mini-sglang @ `9a91cfa`. A single launcher then runs the shared backend with either the frozen Python frontend or a Rust one, and the Rust msgpack codec is locked byte-for-byte to upstream's encoder. While the Rust work proceeds on the Mac, the Python frontend's host overhead is profiled on the GPU machine to inform benchmark design. On the Mac, the Rust frontend is built bottom-up against one minimal mock scheduler, in three steps:

1. Ordered ZMQ transport.
2. Tokenizer and detokenizer parity.
3. The request-lifecycle FSM and HTTP API, which serve the first full request.

The Rust frontend then runs against the real backend on the GPU machine. Output parity with the Python frontend there gates the three-scenario benchmark comparison.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Vendored Base & Wire Codec** - Pinned mini-sglang, one launcher for both frontends with a readiness handshake, and a byte-exact Rust msgpack codec (completed 2026-10-05)
- [ ] **Phase 2: Python Frontend Baseline Profile** - Measure the Python frontend's host overhead on the GPU machine (parallel track, does not gate)
- [ ] **Phase 3: ZMQ Transport & Mock Scheduler** - Ordered ZMQ transport plus one minimal Rust mock scheduler for GPU-free development
- [ ] **Phase 4: Tokenizer & Detokenizer Parity** - Token ids, chat templates and streamed text identical to Python for Qwen3-0.6B and one Llama-3.x model
- [ ] **Phase 5: Request Lifecycle & HTTP API** - First full request on the Mac: lifecycle FSM, cancellation, and endpoints that match the Python frontend
- [ ] **Phase 6: GPU End-to-End Parity** - Rust and Python frontends produce identical output on the real backend
- [ ] **Phase 7: Frontend Benchmarks** - Reproducible Python-vs-Rust comparison across the three scenarios, plus a throughput regression check

## Phase Details

### Phase 1: Vendored Base & Wire Codec

**Goal**: The repo holds a pinned, attributed copy of mini-sglang. One launch command runs the shared backend with either frontend, and the backend reports a readiness handshake. The Rust msgpack codec is byte-exact with upstream for all 7 message types.
**Depends on**: Nothing (first phase)
**Requirements**: BASE-01, BASE-02, BASE-03, WIRE-01, WIRE-02
**Success Criteria** (what must be TRUE):
  1. The repo contains mini-sglang @ `9a91cfa` with its MIT LICENSE and copyright notice. `UPSTREAM.md` names the source commit and lists every vendored file that has been modified.
  2. On the GPU machine, one launch command with `--frontend python` serves a chat completion through the unmodified Python frontend. With `--frontend rust`, it starts the same backend plus the Rust frontend binary, which at this stage is a skeleton that connects and reads the handshake.
  3. The backend reports `max_seq_len`, `eos_token_id`, `page_size` and `max_running_req` at readiness, and the Rust frontend logs the values it received. The Python frontend keeps working unchanged against that same backend code.
  4. For each of the 7 upstream message types, the bytes the Rust codec produces equal golden fixtures exported from upstream's Python encoder, checked by a test that runs on the Mac.
  5. Every message the Rust codec emits decodes through upstream's real Python decoder (`cls(**kwargs)`) without error.

**Plans:** 13/13 plans complete

Plans:
- [x] 01-12-PLAN.md

**Wave 1**
- [x] 01-01-PLAN.md — Vendor mini-sglang @ 9a91cfa with UPSTREAM.md; package-legitimacy gate; hash-pinned Mac env bootstrap (wave 1)
- [x] 01-02-PLAN.md — Cargo workspace + rsg-server skeleton: CLI roles, stdin handshake, SHA refusal, exit codes (wave 1)

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 01-03-PLAN.md — Tracer: `python -m rsglang.launch --frontend rust` end to end on the Mac (fake scheduler on upstream queues + real rsg-server); `--frontend python` passthrough (wave 2)
- [x] 01-04-PLAN.md — rsg-wire codec + golden fixtures from upstream's encoder for all 8 wire tags and boundaries (wave 2)

**Wave 3** *(blocked on Wave 2 completion)*
- [x] 01-05-PLAN.md — Launcher failure contract (D-12), seam unit tests, GPU verification script with end-of-phase human check (wave 3)
- [x] 01-06-PLAN.md — check_upstream.py frozen-tier gate, WIRE-02 decode check, check_all.sh phase gate (wave 3)

**Wave 4** *(gap closure)*
- [x] 01-07-PLAN.md — G-01-2 / CR-01: group SIGINT (Ctrl-C) to the rust-mode launcher exits 0 with no failure report; stop re-checked after every ready_queue.get (wave 4)

**Wave 5** *(gap closure; blocked on Wave 4 completion)*
- [x] 01-08-PLAN.md — G-01-3 / WR-02: launcher pid passed explicitly to the scheduler watchdog plus Linux PR_SET_PDEATHSIG; early kill -9 leaves no orphan; GPU check step 4b (wave 5)

**Wave 6** *(gap closure; blocked on Wave 5 completion)*
- [x] 01-09-PLAN.md — G-01-7-WR07 / WR-08: GPU-orphan check cannot false-PASS (nvidia-smi failure, SIGPIPE under pipefail); start_session polls for setsid and cleans up on failure; Mac tests with stubbed nvidia-smi/setsid (wave 6)

**Wave 7** *(gap closure; blocked on Wave 6 completion)*
- [x] 01-10-PLAN.md — G-01-7-WR06 / WR-09: prctl failure degrades to the polling watchdog and watchdog startup failures reach the launcher as an error envelope; watchdog exit test proves the watchdog caused the exit (wave 7)

**Wave 8** *(gap closure; blocked on Wave 7 completion)*
- [x] 01-11-PLAN.md — G-01-7-WR04 / WR-01: rsg-server rejects a handshake without eos_token_id (exit 2); rust mode rejects abbreviated --shell-mode via the parsed run_shell flag (wave 8)

**Wave 10** *(gap closure; blocked on Wave 9 completion)*
- [x] 01-13-PLAN.md — G-01-8 / G-01-9 (WR-01, IN-01 of the 2026-10-04 review): rust mode reports the shell-mode rejection before a missing rsg-server binary (upstream args parsed once, before binary resolution); a missing prctl symbol degrades to the polling watchdog (wave 10)

### Phase 2: Python Frontend Baseline Profile

**Goal**: Measured numbers show where the frozen Python frontend spends host-side time and memory in each of the three benchmark scenarios, so the numbers can inform benchmark design and attribution.
**Depends on**: Nothing (needs only mini-sglang @ `9a91cfa` on the GPU machine, the same code BASE-01 vendors)
**Parallelization**: Runs on the GPU machine alongside the Mac-side build work. Its findings feed the benchmark design but never decide whether the project continues.
**Requirements**: BENCH-01
**Success Criteria** (what must be TRUE):
  1. A profiling report from the GPU machine quantifies, for each of the three scenarios, the Python frontend's GC pauses (count, duration, and correlation with P99 TTFT spikes) and its memory allocation and resident-memory growth.
  2. The report quantifies GIL contention between tokenization, detokenization and HTTP handling, and the per-request serialization and IPC cost across the Python frontend's process hops.
  3. The report records the share of scheduler time spent in the radix cache, as the input to the v2 radix decision.
  4. The profiling run is scripted so it can be repeated on the GPU machine. Its findings are written down as concrete inputs to the benchmark design: which metrics to capture, and which effects can be credited to the frontend.

**Plans:** 8/9 plans executed

Plans:

**Wave 1**
- [x] 02-01-PLAN.md — Package gate (blocking-human) for psutil, aiohttp, py-spy, hyperfine; pin psutil + aiohttp into the hashed Mac lock (wave 1)
- [x] 02-02-PLAN.md — Env-gated sitecustomize hook (gc.callbacks + tracemalloc), led by the Pitfall 3 spawn/exec pre-flight tracer (wave 1)

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 02-03-PLAN.md — Phase tracer: `baseline_profile.py discover` end to end on the Mac (launch, /v1/models ready, psutil children, py-spy dump role-ID, hook active, validated JSON, clean teardown) (wave 2)

**Wave 3** *(blocked on Wave 2 completion)*
- [x] 02-04-PLAN.md — Full baseline-profile.json schema validation + require_gpu gate (wave 3)
- [x] 02-05-PLAN.md — Analysis: radix share (D-09/D-10), CPU/GIL %, IPC/serde buckets, GC stats, GC-to-P99 correlation, memory summaries (wave 3)
- [x] 02-06-PLAN.md — Scenario drivers: 128-agent aiohttp cancellations, bench_simple-adapted 32-token saturation, hyperfine cold start helpers (wave 3)
- [x] 02-07-PLAN.md — GPU-box wrapper `scripts/gpu_phase2_profile.sh` with preflight and privilege probe (wave 3)

**Wave 4** *(blocked on Wave 3 completion)*
- [x] 02-08-PLAN.md — `baseline_profile.py run`: per-scenario sessions with py-spy active+GIL passes, all 3 scenarios, script-written sidecar (wave 4)

**Wave 5** *(blocked on Wave 4 completion)*
- [ ] 02-09-PLAN.md — GPU run (human) + hand-written docs/benchmarks/baseline-profile.md tied to the JSON (wave 5)

### Phase 3: ZMQ Transport & Mock Scheduler

**Goal**: The Rust frontend exchanges messages with a scheduler over ZMQ `ipc://` in strict per-request order. One minimal Rust mock scheduler lets it run end-to-end on the Mac, including the backend misbehaviors the cancellation tests need.
**Depends on**: Phase 1
**Requirements**: WIRE-03, MOCK-01
**Success Criteria** (what must be TRUE):
  1. On the Mac, the Rust transport connects to the mock scheduler over ZMQ `ipc://`, receives its readiness handshake, and submits requests. Each request's token replies are routed back to it by uid.
  2. Under concurrent load, the scheduler never sees an abort before the submit it cancels, because every outgoing message goes through a single ordered writer.
  3. The mock can be configured to send late tokens after an abort, silently drop overlong prompts, and batch replies for several requests in one message. A test exercises each behavior.
  4. The transport drops replies for unknown uids, such as late tokens after an abort, without crashing. A slow consumer on one request does not stall replies for the others.

**Plans**: TBD

### Phase 4: Tokenizer & Detokenizer Parity

**Goal**: Rust produces the same token ids from requests, and the same text from token streams, as the Python frontend, for Qwen3-0.6B and one Llama-3.x model. Parity is proven entirely on the Mac.
**Depends on**: Phase 1
**Parallelization**: Does not use the transport, so it can be planned and executed alongside the ZMQ transport and mock work.
**Requirements**: TOK-01, TOK-02, TOK-03, TOK-04
**Success Criteria** (what must be TRUE):
  1. On a test corpus, Rust tokenization produces token ids identical to the Python frontend's for Qwen3-0.6B.
  2. For every chat conversation in the corpus, Rust chat-template rendering produces a prompt string identical to the Python frontend's for Qwen3-0.6B.
  3. Given the same token streams, the Rust incremental detokenizer and the Python frontend produce identical streamed text. This includes CJK and emoji split across tokens, with no UTF-8 breakage and no panics.
  4. Criteria 1-3 also pass for one Llama-3.x model, including its BOS and space-cleanup cases.

**Plans**: TBD

### Phase 5: Request Lifecycle & HTTP API

**Goal**: The first full request runs on the Mac: a client calls the Rust frontend's HTTP API backed by the mock scheduler. Every request ends in exactly one terminal state (finished, cancelled or failed), and none are leaked.
**Depends on**: Phase 3, Phase 4
**Requirements**: LIFE-01, LIFE-02, LIFE-03, LIFE-04, LIFE-05, API-01, API-02
**Success Criteria** (what must be TRUE):
  1. On the Mac, a client can call `/v1/chat/completions` (streaming and non-streaming), `/generate`, `/v1/models` and `/v1` on the Rust frontend backed by the mock. The responses match recorded Python-frontend responses in format and SSE framing.
  2. When a client disconnects mid-request, streaming or not, the backend gets an abort right away. Tokens that arrive after the abort are dropped and counted.
  3. A stress test of 128 concurrent requests with random cancellations against the mock ends with no leaked requests and no stuck connections. Every request reaches exactly one terminal state: finished, cancelled or failed.
  4. An overlong prompt gets an immediate 400, and a request whose backend stops responding times out with an error instead of hanging. Abort timing is configurable: immediate by default, or deferred until the first token.
  5. `/health` and `/health/ready` respond, and `/metrics` exposes the request count, the cancellation count and a TTFT histogram.

**Plans**: TBD

### Phase 6: GPU End-to-End Parity

**Goal**: On the GPU machine, serving through the Rust frontend produces the same output as the Python frontend on the same backend.
**Depends on**: Phase 5
**Requirements**: PAR-01, PAR-02
**Success Criteria** (what must be TRUE):
  1. On the GPU machine, `--frontend rust` serves Qwen3-0.6B through the real backend and answers every endpoint delivered on the Mac.
  2. With greedy decoding (temperature 0) and one request at a time, the Rust and Python frontends produce identical output on at least 100 prompts for Qwen3-0.6B, which is the hard gate. The same comparison is run and reported for the Llama-3.x model.
  3. Under concurrent load, the Rust-vs-Python output match rate is measured and reported. It is informational only, because GPU batch composition affects the results.
  4. The 128-request cancellation stress test also completes against the real backend without crashing or wedging the scheduler. The run records whether the suspected abort-during-prefill bug reproduces, which settles the abort-timing setting used in the benchmarks.

**Plans**: TBD

### Phase 7: Frontend Benchmarks

**Goal**: A reproducible harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios over the Python frontend on the same backend, and shows that standard throughput does not regress.
**Depends on**: Phase 2, Phase 6
**Requirements**: BENCH-02, BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08
**Success Criteria** (what must be TRUE):
  1. The Rust load generator drives either frontend open-loop, can cancel requests mid-stream, and records TTFT, P99 and RPS.
  2. Each of the three scenarios has a Python-vs-Rust report:
     - 128 concurrent agents with random cancellations, reporting P99 TTFT.
     - 32-token short-prompt saturation, reporting an RPS-vs-latency curve.
     - Frontend cold-start time and frontend memory, with end-to-end startup reported separately.
  3. A standard-inference throughput run shows the Rust frontend does not regress versus the Python frontend. ±2% is a reference target, not a hard gate.
  4. Every comparison comes from alternating A/B runs and reports the Python frontend at both its default and its best `--num-tokenizer` setting. Results carry confidence intervals and a run manifest that is enough to reproduce the run.
  5. Every scenario report shows frontend memory usage and Python GC pause counts next to TTFT, P99 and RPS, so P99 spikes can be compared against GC pauses.

**Plans**: TBD

## Progress

**Execution Order:**
- **Critical path:** 1 → 3 → 5 → 6 → 7.
- **Phase 2:** runs on the GPU machine in parallel with 1, 3 and 4, and must finish before 7.
- **Phase 4:** runs in parallel with 3, and must finish before 5.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Vendored Base & Wire Codec | 13/13 | Complete    | 2026-10-05 |
| 2. Python Frontend Baseline Profile | 8/9 | In Progress|  |
| 3. ZMQ Transport & Mock Scheduler | 0/TBD | Not started | - |
| 4. Tokenizer & Detokenizer Parity | 0/TBD | Not started | - |
| 5. Request Lifecycle & HTTP API | 0/TBD | Not started | - |
| 6. GPU End-to-End Parity | 0/TBD | Not started | - |
| 7. Frontend Benchmarks | 0/TBD | Not started | - |
