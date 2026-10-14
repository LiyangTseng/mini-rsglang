# mini-rsglang Glossary

What each term means *in this system*. No implementation details; see the code
and `docs/RESULTS.md` for those.

## System boundary

**Upstream**:
The original mini-sglang, pinned at commit `9a91cfa` and vendored unmodified under `vendor/`.
_Avoid_: "the original", "the Python version" (both collide with Python frontend)

**Frontend**:
The user-facing layer: HTTP API, tokenize, detokenize, request lifecycle. The only part mini-rsglang rewrites in Rust.
_Avoid_: "server" (too vague)

**Backend**:
Scheduler, KV cache management, model forward and kernels. Frozen upstream Python/CUDA code in this repo.
_Avoid_: "engine" (`Engine` is one class inside the backend)

**Frozen backend**:
The rule that `vendor/` is never patched, so both frontends are compared against the identical backend.

**Wire contract**:
The ZMQ + msgpack message format between frontend and backend. The Rust frontend must match the Python frontend byte-for-byte; an extra key crashes the scheduler.

**Parity**:
Given the same backend and input, the Rust frontend's output is identical to the Python frontend's.

## Serving metrics

**TTFT / TPOT**:
Time to first token (dominated by prefill and frontend overhead) / time per output token after the first (dominated by decode).

**P50 / P99**:
Median / 99th-percentile latency over a run. The project's open problem is the Rust frontend's P99 TTFT tail under concurrent load.

**Tail spike**:
The small fraction of requests whose TTFT is one to two orders of magnitude above P50 (e.g. 4.7 s vs 75 ms in the cancellation scenario).

## Benchmark vocabulary

**Arm**:
One frontend configuration in an A/B run: `rust`, `python-default` (`--num-tokenizer=0`), or `python-best` (`--num-tokenizer=2`).

**Cancellation scenario (S1)**:
128 concurrent agents sending random requests with mid-stream cancellations. Where the P99 TTFT spike is largest.

**Saturation scenario (S2)**:
Request rate ramped until the server saturates; P99 TTFT is tracked per rate step.

**Cold start scenario (S3)**:
End-to-end startup time and frontend RSS.

**Mock stack**:
`rsg-mock-stack`: the Rust frontend paired with `mock-scheduler` (fixed prefill/decode delays), no GPU needed. Measures frontend overhead in isolation.

## Frontend internals

**Lifecycle stage**:
One step a request passes through in the Rust frontend between HTTP ingress and first token: encode, decoder build, submit (writer queue + ZMQ send), backend wait, first token emit.

**Writer thread**:
The single OS thread (`tx-zmq`) that owns the PUSH socket to the scheduler and drains a bounded queue of outgoing messages.

## Relationships

- An **Upstream** consists of a **Frontend** and a **Backend**
- mini-rsglang = Rust **Frontend** + Upstream **Backend**, talking over the **Wire contract**
- Every benchmark compares **Arms** against one shared, **Frozen backend**
