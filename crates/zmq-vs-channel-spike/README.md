# zmq-vs-channel-spike

Standalone, throwaway spike. Not part of the production workspace's
dependency graph and not wired into `rsg-bench`'s real harness. Measures the
raw enqueue latency of one ZMQ PUSH send (same transport/options/`ipc://`
scheme as `rsg-server`'s production `tx-zmq` writer, real msgpack frame
sizes from `rsg-wire`) against one `std::sync::mpsc::sync_channel` send,
under three load conditions, for both an `AbortBackendMsg`-sized frame (S1's
cancel storm) and a 32-token `UserMsg` frame (S2's saturation scenario).

Run with `cargo run --release -p zmq-vs-channel-spike`.

## What this can and cannot prove

This isolates the transport **hop** in isolation. It cannot by itself prove
the transport is *the* cause of the real GPU benchmark's measured
Rust-frontend P99 TTFT regression (that system has many more layers: HTTP,
the FSM, the codec, the real Python scheduler's own CPU/GIL behavior). It
can only show whether the transport's own failure mode under backpressure is
*consistent with* being a meaningful contributor to that shape of
regression.

## Finding

Measured on the Mac dev machine (not the Linux GPU box — not re-verified
there within this spike's time box; absolute numbers may differ cross-
platform, though the qualitative shape below stems from each transport's
queueing design, not OS specifics).

**Drained (receiver keeps up):** both transports are fast, but ZMQ's own
tail is already far fatter than the in-process channel's — p99 74-230us vs
2-3us, even with nothing backed up. The PUSH socket's send hands off to
libzmq's own I/O thread; the in-process channel never leaves userspace.

**Cancel storm (sender fires as fast as possible, receiver drains at a
fixed, deliberately slow 2ms/msg rate — simulating a scheduler that's
itself CPU-bound):** this is the cleanest, most controlled scenario, and the
two transports fail in qualitatively different ways:
- `std::sync::mpsc`: uniformly elevated once backed up — every send pays
  roughly the drain rate (p50 2.5-3ms, p99 6-9.6ms), bounded and predictable.
- ZMQ PUSH/PULL: p50/p90/p99 stay near **zero** (most sends return
  instantly), but the max blows up to **1.5-1.65 seconds** — a rare,
  catastrophic stall rather than a uniformly elevated cost.

That "usually instant, occasionally catastrophic" shape is a plausible
mechanism for exactly the real GPU benchmark's pattern: RPS statistically
tied between frontends, but P99 TTFT ~772% worse for Rust with a very wide
confidence interval — a few catastrophic outliers inflate P99 without
denting mean throughput. This is consistent with (not proof of) the ZMQ
transport hop being a meaningful contributor to the measured regression,
and matches SGLang's own public Rust-migration RFC (`sgl-project/sglang`
issue #23206), which abandoned ZMQ for in-process channels + zero-copy
citing the same class of Rust↔Python boundary perf cliff.

The "backed up" (receiver fully idle until a barrier, then drains in a
spin-loop) scenario's results are included in the raw output but are not
load-bearing in this summary — the drainer's startup timing wasn't
controlled tightly enough across both transports to call it clean.
