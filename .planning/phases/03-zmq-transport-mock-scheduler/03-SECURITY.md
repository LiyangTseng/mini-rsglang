---
phase: "03"
slug: "zmq-transport-mock-scheduler"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-06"
---

# Phase 03 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| frontend process ↔ mock-scheduler process | Bytes cross the ipc:// PUSH/PULL sockets in both directions; each side decodes frames the other produced | rsg-wire BackendMsg/TokenizerMsg bytes |
| test harness ↔ mock-scheduler | CLI arguments, stdin liveness, stdout handshake line, observe file, and (03-03) misbehavior config flags | process args, handshake JSON, plain-text observe lines |
| scheduler (or mock) → rx-zmq dispatcher → per-request consumers | Peer-controlled detok frames, including uids the frontend never issued, arbitrary/malformed bytes, and a slow or absent consumer | TokenizerMsg frames routed by uid |
| many async callers → tx-zmq writer → scheduler | Concurrent producers (submit/abort from different tasks) share one queue to the single backend socket | BackendMsg frames, FIFO-ordered |
| mock-scheduler stdout → rsg-server stdin | The handshake line is relayed verbatim, as the launcher relays the real backend's | Handshake JSON (version, upstream_sha, max_seq_len, eos_token_id, page_size, max_running_req, num_pages) |
| cargo ↔ crates.io | proptest and rustc-hash are fetched and compiled into the dev/test build (03-01 only; no new packages in 03-02 through 03-06) | Rust crate source + Cargo.lock checksums |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-03-01 | Denial of Service | mock-scheduler decode of backend frames | low | mitigate | `decode_backend`/`to_i32_vec` matched on `Result`; an undecodable frame logs and exits 4 deliberately (`undecodable_frame_exits_4`, verified) | closed |
| T-03-02 | Tampering | ipc socket files and observe files in /tmp | low | accept | Same-user dev machine; paths unique per test process/counter; harness `Drop` removes them; no secrets carried | closed |
| T-03-03 | Denial of Service | orphaned mock-scheduler after a crashed test run | low | mitigate | stdin-EOF guard exits 3 when parent goes away (`stdin_eof_exits_3`, verified); harness `Drop` kills and waits on the child | closed |
| T-03-04 | Tampering | wire schema drift through the mock | medium | mitigate | Mock and harness use only rsg-wire's existing types; readiness via stdout, observation via side file, never a new wire tag/field | closed |
| T-03-05 | Denial of Service | rx-zmq decode of detok frames | medium | mitigate | `decode_tokenizer` matched on `Result`; malformed frame logged at warn and skipped, thread keeps routing | closed |
| T-03-06 | Denial of Service | slow consumer blocking routing for every uid | medium | mitigate | Per-uid broadcast channel never blocks on send; full channel overwrites oldest token (D-05); proven end to end in 03-05 | closed |
| T-03-07 / T-03-12 | Tampering | message order to the scheduler (abort overtaking its submit) | high | mitigate | Single tx-zmq thread exclusively owns the only backend PUSH half; `WriterHandle::abort` requires a `Submitted` ticket obtainable only after its own submit enqueued; `grep -rl send_backend` pins the single write path; 64-case property test proves it under random concurrency through a real mock subprocess | closed |
| T-03-08 / T-03-18 | Spoofing | replies for uids the frontend never registered | low | mitigate | Missing route drops the reply silently and is counted in `unknown_uid`, without touching any other route | closed |
| T-03-09 | Tampering | misbehavior configuration (a typo silently disables a fault) | medium | mitigate | Strict parsing/validation exits 2 before the handshake: unpaired flags, overlapping uids, malformed/reversed ranges, batch size 0 — each with its own test | closed |
| T-03-10 | Repudiation | injected fault mistaken for a real backend defect | low | mitigate | Every injected behavior logs a fixed message + uid to stderr; observe file records every message received | closed |
| T-03-11 | Denial of Service | huge uid ranges on the CLI | low | mitigate | Ranges stored as intervals and looked up without expanding into a set | closed |
| T-03-13 | Denial of Service | aborts silently lost when the writer thread has died | medium | mitigate | `abort` returns `Err(WriterClosed)` after the writer stops (`abort_after_writer_stopped_returns_writer_closed`, verified) | closed |
| T-03-14 | Denial of Service | leaked mock processes/ipc files from proptest cases or shrinking | low | mitigate | Each case's `MockScheduler` `Drop` kills, waits, and removes its files; mock's stdin-EOF guard ends any orphan | closed |
| T-03-15 | Repudiation | silent gaps in a uid's token stream (backpressure drop misattributed to backend) | medium | mitigate | In-band `Dropped(n)`, per-uid `dropped()` counter, structured warn (D-06); proven by `lagged_stream_reports_dropped_event_counter_and_warning` and the end-to-end tracer | closed |
| T-03-16 | Denial of Service | malformed or hostile detok frames | medium | mitigate | Decode matched on `Result`; bad frames counted in `malformed_frames` and skipped, thread keeps routing (`malformed_frame_is_skipped_and_dispatcher_keeps_routing`) | closed |
| T-03-17 | Denial of Service | route-table growth from aborted/abandoned requests | low | mitigate | Routes removed on finished, on failed send to a dropped stream (`closed_route`), and by explicit `deregister`; unknown uids never create state | closed |
| T-03-19 | Denial of Service | late tokens after an abort hitting the full transport | medium | mitigate | `tracer_late_tokens_after_abort_are_dropped_and_counted` proves they are dropped, counted in `unknown_uid`, and isolated from other uids with no panic | closed |
| T-03-20 | Repudiation | mock-derived timings presented as frontend performance results | medium | mitigate | Prohibition in must_haves; mock's module doc states delays are synthetic; Phase 3 tests assert behavior/order only, never speed — confirmed by human sign-off in 03-UAT.md test 2 | closed |
| T-03-21 | Tampering | handshake relay accepting a wrong-SHA or malformed line | low | accept | rsg-server's Phase 1 parser already rejects bad lines (exit 2); this phase only proves the mock's line is valid, never widens the parser | closed |
| T-03-22 | Denial of Service | gate runs leaving mock or rsg-server processes and ipc files | low | mitigate | Every spawned child killed in `Drop`; mock's stdin-EOF guard ends orphans; harness `Drop` removes ipc files | closed |
| T-03-SC | Tampering | cargo installs (proptest 1.11.0, rustc-hash 2.1.3) and the Mac Python venv bootstrap | high | mitigate | Both crates rated OK/Approved in the 03-RESEARCH.md Package Legitimacy Audit (crates.io registry, rust-lang/proptest-rs orgs), pinned in `[workspace.dependencies]` with `Cargo.lock` checksums; no new packages introduced in 03-02 through 03-06; `scripts/bootstrap_mac_env.sh` (03-06) installs only from the existing sha256-hashed `requirements-mac.txt` (Phase 1 human-gated lock) | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on (high) count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

Register origin: `register_authored_at_plan_time: true` — every one of the 6 plans (03-01 through 03-06) authored a `<threat_model>` block at planning time; this register consolidates them verbatim rather than being reconstructed retroactively. ASVS level 1: L1 grep/read-depth verification is sufficient at `threats_open: 0`, so this audit was performed directly against the already-completed phase verification (03-VERIFICATION.md, independently re-ran `cargo test`, `cargo clippy`, `scripts/check_all.sh --offline`, and the single-write-path `grep`) and the code review (03-REVIEW.md, 0 Critical) rather than re-spawning a separate auditor subagent over the same evidence.

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-03-01 | T-03-02 | Same-user Mac dev machine; ipc/observe files in /tmp carry no secrets and are cleaned up by harness `Drop` | LiyangTseng (project owner, via phase conventions) | 2026-10-06 |
| AR-03-02 | T-03-21 | Handshake-line validation is Phase 1's parser contract (unchanged this phase); widening it is explicitly out of scope | LiyangTseng (project owner, via phase conventions) | 2026-10-06 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-06 | 23 | 23 | 0 | Claude (gsd-secure-phase, ASVS L1 short-circuit — no auditor subagent spawned; verified against 03-VERIFICATION.md + 03-REVIEW.md evidence) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-06
