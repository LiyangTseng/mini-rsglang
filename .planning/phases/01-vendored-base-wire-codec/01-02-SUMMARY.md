---
phase: 01-vendored-base-wire-codec
plan: 02
subsystem: infra
tags: [rust, cargo-workspace, zmq, tokio, clap, serde_json, tracing, handshake]

requires:
  - phase: none
    provides: "Wave-1 plan; needs no Python env and no vendored tree (vendor/UPSTREAM_SHA is created here)"
provides:
  - "Cargo virtual workspace (resolver 3, edition 2024) with every Phase 1 crate pinned in [workspace.dependencies]"
  - "rust-toolchain.toml pinned to 1.99.0 (zmq-sys builds on it: assumption A2 confirmed)"
  - "vendor/UPSTREAM_SHA single-source SHA 9a91cfafe754aa85daee49998176275667eb58f2"
  - "rsg-server skeleton binary: CLI flags, ZMQ sockets per role, stdin JSON handshake, exit codes 0/1/2/3"
  - "handshake.rs: Handshake (deny_unknown_fields), parse_handshake, HandshakeError, EXPECTED_UPSTREAM_SHA, HANDSHAKE_VERSION"
  - "transport.rs: Role, Endpoint, Transport trait, ZmqTransport::open"
affects: [01-03 launcher, 01-04 rsg-wire, 01-05, phase-03 transport]

plan_head_before: bfc0bbd6d9bccdd899b790c867bcee1982ebc30c
plan_head_after: 3da12c9598c52d69d94be39c4754febd4d1064ed

actuals:
  tokens: 7007    # chars/4 over added lines in the realized diff, excluding the generated Cargo.lock
  tasks: 2
  commits: 4

tech-stack:
  added: [zmq 0.10.0 (zmq-sys 0.12, bundled libzmq 4.3.4), tokio 1.53, clap 4.6.7, serde 1.0.229, serde_json 1.0.151, tracing 0.1.44, tracing-subscriber 0.3.23, anyhow 1.0.104]
  patterns:
    - "stdin read on a dedicated std::thread feeding tokio mpsc (never tokio::io::stdin)"
    - "exit via std::process::exit so no destructor can hang shutdown; sockets use linger 0"
    - "ZMQ behind a small Transport trait; bind-or-connect per endpoint Role mirrors upstream utils/mp.py"
    - "SHA compiled in with include_str!(\"../../../vendor/UPSTREAM_SHA\").trim_ascii() as a const"

key-files:
  created:
    - Cargo.toml
    - rust-toolchain.toml
    - Cargo.lock
    - vendor/UPSTREAM_SHA
    - crates/rsg-server/Cargo.toml
    - crates/rsg-server/src/main.rs
    - crates/rsg-server/src/handshake.rs
    - crates/rsg-server/src/transport.rs
    - crates/rsg-server/tests/cli.rs
  modified: []

key-decisions:
  - "Toolchain stays on 1.99.0: zmq-sys 0.12 / bundled libzmq built and all tests passed on it (A2 confirmed on macOS arm64)"
  - "SIGINT is handled with a signal(SignalKind::interrupt()) stream installed before sockets open, instead of a per-select tokio::signal::ctrl_c() future, so a SIGINT that lands before the first poll still exits 0"
  - "A stdin read error is treated like EOF (exit 3), before and after the handshake"

patterns-established:
  - "Exit-code contract: 0 signal, 1 startup failure, 2 bad handshake, 3 stdin EOF"
  - "Log-message contract consumed by plans 01-03/01-05: rsg-server starting, sockets ready, awaiting handshake on stdin, handshake received, handshake rejected: ..., launcher went away (stdin EOF), idle until SIGINT/SIGTERM or stdin EOF"

requirements-completed: [BASE-02, BASE-03]

coverage:
  - id: D1
    description: "Cargo workspace, 1.99.0 toolchain pin and vendor/UPSTREAM_SHA single source"
    requirement: BASE-02
    verification:
      - kind: unit
        ref: "crates/rsg-server/src/handshake.rs#tests::expected_sha_is_vendor_file"
        status: pass
      - kind: other
        ref: "test \"$(cat vendor/UPSTREAM_SHA)\" = 9a91cfafe754aa85daee49998176275667eb58f2"
        status: pass
    human_judgment: false
  - id: D2
    description: "Handshake parsing enforces schema (deny_unknown_fields, all keys required), version 1 and exact SHA equality"
    requirement: BASE-03
    verification:
      - kind: unit
        ref: "cargo test -p rsg-server --bin rsg-server handshake::tests (8 tests)"
        status: pass
    human_judgment: false
  - id: D3
    description: "ZmqTransport binds or connects PUSH/PULL per role and moves frames with raw zmq peers"
    requirement: BASE-02
    verification:
      - kind: unit
        ref: "cargo test -p rsg-server --bin rsg-server transport::tests (3 tests)"
        status: pass
    human_judgment: false
  - id: D4
    description: "rsg-server process contract: logs handshake values, exit 0 on SIGINT/SIGTERM, 2 on bad handshake, 3 on stdin EOF"
    requirement: BASE-03
    verification:
      - kind: integration
        ref: "cargo test -p rsg-server --test cli (9 tests)"
        status: pass
      - kind: other
        ref: "cargo clippy -p rsg-server --all-targets -- -D warnings"
        status: pass
    human_judgment: false

duration: 6min
completed: 2026-10-04
status: complete
---

# Phase 1 Plan 02: Rust Workspace and rsg-server Skeleton Summary

**Cargo workspace pinned to Rust 1.99.0 plus an `rsg-server` skeleton that binds/connects its two ZMQ sockets per role flags, reads one JSON handshake line from stdin, refuses a mismatched upstream SHA (exit 2), and exits 3 on stdin EOF and 0 on SIGINT/SIGTERM**

## Performance

- **Duration:** about 6 min
- **Started:** 2026-10-04T02:41:38Z
- **Completed:** 2026-10-04T02:47:25Z
- **Tasks:** 2
- **Files modified:** 9 (all created)

## Accomplishments

- Virtual cargo workspace (`resolver = "3"`, `members = ["crates/*"]`, edition 2024). Every crate version this phase uses is pinned in `[workspace.dependencies]`, so later plans (rsg-wire in 01-04) never edit the root manifest.
- `rust-toolchain.toml` pins 1.99.0. rustup installed it automatically, and `zmq-sys`'s bundled libzmq built on it, which confirms research assumption A2. No fallback to 1.95.0 was needed.
- `vendor/UPSTREAM_SHA` is the single-source SHA. It is compiled into `EXPECTED_UPSTREAM_SHA` via `include_str!(...).trim_ascii()` as a `const`.
- The `handshake.rs` parser enforces T-01-04: `deny_unknown_fields`, all seven keys required (`eos_token_id` may be `null`), `handshake_version == 1`, and full-string SHA equality. The mismatch error names both SHAs.
- `transport.rs` adds the `Transport` trait and `ZmqTransport::open`: PUSH backend, PULL detokenizer, `linger 0`, and bind or connect per `Role`.
- `main.rs` handles stdin on a dedicated `std::thread` → tokio mpsc and a `select!` over stdin, SIGINT and SIGTERM. It exits with codes 0/1/2/3 through `std::process::exit` (T-01-05), and it never calls `send_backend`.
- 11 unit tests and 9 process-level CLI tests pass. Five reruns of the CLI suite were stable (about 0.6 s each). Clippy passes with `-D warnings`.

## Task Commits

1. **Task 1: Cargo workspace, SHA single source, handshake parser and ZMQ transport**
   - `120541f` test(01-02): failing handshake and transport tests (RED, against `todo!()` stubs)
   - `6d83a45` feat(01-02): handshake parser and ZMQ transport (GREEN)
2. **Task 2: stdin handshake lifecycle, exit-code contract and CLI integration tests**
   - `0cd859a` test(01-02): failing CLI exit-code contract tests (RED: all 9 failed on log or exit timeouts)
   - `3da12c9` feat(01-02): stdin lifecycle and exit-code contract (GREEN)

## Files Created/Modified

- `Cargo.toml`: virtual workspace, workspace package metadata, pinned workspace dependencies
- `rust-toolchain.toml`: 1.99.0, rustfmt + clippy, minimal profile
- `Cargo.lock`: committed resolution (tokio resolved to 1.53.2 under the `1.53.1` caret requirement)
- `vendor/UPSTREAM_SHA`: `9a91cfafe754aa85daee49998176275667eb58f2`
- `crates/rsg-server/Cargo.toml`: binary crate, all deps `workspace = true`, no thiserror
- `crates/rsg-server/src/handshake.rs`: `HANDSHAKE_VERSION`, `EXPECTED_UPSTREAM_SHA`, `Handshake`, `eos_display`, `HandshakeError`, `parse_handshake`, plus 8 unit tests
- `crates/rsg-server/src/transport.rs`: `Role`, `Endpoint`, `Transport`, `ZmqTransport::open`, plus 3 unit tests using raw zmq peers at `ipc:///tmp/rsgt-PID-*`
- `crates/rsg-server/src/main.rs`: `Cli`, `StdinEvent`, `EXIT_*` constants, stdin reader thread, signal handling, handshake lifecycle
- `crates/rsg-server/tests/cli.rs`: 9 process-level tests (stderr drain thread, 20 s deadlines, kill on timeout, socket-file cleanup)

## Decisions Made

- Kept the toolchain at 1.99.0, since A2 held.
- SIGINT uses a `signal(SignalKind::interrupt())` stream instead of `tokio::signal::ctrl_c()`. Both handlers are installed right after logging is initialized, before the sockets open, so a signal that arrives during startup is caught and exits 0 instead of taking the default action. The behavior matches the plan; only the API differs.
- A stdin read error is handled the same way as EOF (exit 3, "launcher went away (stdin EOF)"), following the plan's `Eof or Error` grouping.

## Deviations from Plan

None in behavior. The only change in mechanics is the SIGINT API choice above. The plan's `tokio::signal::ctrl_c()` would also work; the persistent stream registers its handler earlier.

**Total deviations:** 0 auto-fixed.
**Impact on plan:** None.

## TDD Gate Compliance

Both tasks followed RED → GREEN with separate commits (`test(01-02)` → `feat(01-02)`). Task 1's RED tests failed on `todo!()` panics in stubs that compiled, not on assertions. The tests were the intended targets, and only the trivially true `expected_sha_is_vendor_file` passed at RED. Task 2's RED tests failed on their own assertions (log or exit-code timeouts). No REFACTOR commits were needed; `cargo fmt` formatting went into the GREEN commits.

## Issues Encountered

- A RED run of `backend_connect_delivers_to_raw_pull_peer` left a stale `/tmp/rsgt-<pid>-c` socket file behind, because the stub panicked before cleanup. It was removed by hand. GREEN runs leave no files.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 01-03 (launcher tracer) can spawn `target/debug/rsg-server` with `--backend-addr/--backend-role connect --detok-addr/--detok-role bind --model --run-id` and write the handshake line. The exit-code and log-message contracts above are what it should assert on.
- Plan 01-04 (rsg-wire) can add `crates/rsg-wire` with `rmp-serde`, `serde_bytes` and `thiserror`, all already declared in `[workspace.dependencies]`, and read the same `vendor/UPSTREAM_SHA`.
- The GPU Linux box needs a C/C++ toolchain for `zmq-sys` (assumption A3, still unverified).

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED

- All 9 key files present on disk.
- Commits 120541f, 6d83a45, 0cd859a, 3da12c9 present in git log.
- `cargo test --workspace` (11 + 9 pass), `cargo clippy -p rsg-server --all-targets -- -D warnings`, and the vendor SHA check all pass.
