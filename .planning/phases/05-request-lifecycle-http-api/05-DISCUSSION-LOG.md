# Phase 5: Request Lifecycle & HTTP API - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-06
**Phase:** 05-request-lifecycle-http-api
**Areas discussed:** Abort timing, API parity verification, Non-streaming disconnect gap, 128-agent stress harness scope

---

## Abort timing (LIFE-05)

| Option | Description | Selected |
|--------|-------------|----------|
| Server-wide CLI flag | One `--abort-timing immediate\|deferred` flag at startup, same for every request; matches the launcher's existing CLI-flag convention; keeps the Phase 6 fairness comparison simplest (one global mode per benchmark run) | ✓ |
| Per-request override | A request-level field/header lets a client pick per call, CLI flag sets only the default | |
| You decide | Claude picks based on what keeps Phase 6's fairness comparison simplest | |

**User's choice:** Server-wide CLI flag
**Notes:** None — chosen directly.

---

## API parity verification (API-01)

| Option | Description | Selected |
|--------|-------------|----------|
| Golden fixtures from a live Python run | Spin up the real Python frontend, capture its actual HTTP/SSE responses, diff Rust's output byte-for-byte — same pattern as Phase 1's wire codec and Phase 4's tokenizer fixtures | ✓ |
| Hand-port from reading api_server.py | Read the source directly and replicate response-building logic without live fixtures; no automated regression guard | |
| You decide | Claude picks based on what's practical without a GPU on the Mac | |

**User's choice:** Golden fixtures from a live Python run
**Notes:** None — chosen directly.

---

## Non-streaming disconnect gap (LIFE-02)

| Option | Description | Selected |
|--------|-------------|----------|
| Accept the limitation, document it | Non-streaming requests abort as soon as hyper notices the drop (on its next write); matches the AbortGuard+Drop pattern as designed; CLAUDE.md already flags verifying this with a disconnect test | ✓ |
| Add a lightweight liveness check | Periodically probe the connection even for non-streaming requests to catch a disconnect sooner; more plumbing, no prior art cited | |
| You decide | Claude picks after checking what hyper actually exposes for this case | |

**User's choice:** Accept the limitation, document it
**Notes:** None — chosen directly.

---

## 128-agent stress harness scope (LIFE-03)

| Option | Description | Selected |
|--------|-------------|----------|
| Minimal, Phase-5-only harness | Just enough (mock-scheduler's fixed delays + randomized abort-after-N-tokens in the test driver) to prove LIFE-03's no-leak/no-stuck-connection criterion; Phase 7 builds its own full harness later, unconstrained | ✓ |
| Shared foundation for Phase 7 | Design Phase 5's stress test as a first version of the eventual benchmark-stack load generator; couples Phase 5 to benchmark-harness decisions not yet made | |
| You decide | Claude picks based on keeping Phase 5 focused on its own correctness goal | |

**User's choice:** Minimal, Phase-5-only harness
**Notes:** None — chosen directly.

---

## Claude's Discretion

- Exact FSM implementation shape (actor task structuring beyond CLAUDE.md's named pattern)
- `/metrics` label cardinality and TTFT histogram bucket boundaries beyond API-02's required counters
- Exact backend-unresponsive timeout duration/config surface for LIFE-04
- Whether the overlong-prompt 400 check happens before or after invoking Phase 4's tokenizer
- Exact SSE keep-alive interval for queued-request abort, if implemented at all in this phase

## Deferred Ideas

None — discussion stayed within phase scope.
