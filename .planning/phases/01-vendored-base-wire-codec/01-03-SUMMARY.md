---
phase: 01-vendored-base-wire-codec
plan: 03
subsystem: infra
tags: [launcher, multiprocessing, zmq, handshake, process-group, pytest, tracer]

requires:
  - phase: 01-vendored-base-wire-codec (plan 01-01)
    provides: "vendor/mini-sglang @ 9a91cfa, uv .venv importing minisgl on the Mac, pytest config (slow marker)"
  - phase: 01-vendored-base-wire-codec (plan 01-02)
    provides: "rsg-server binary: CLI roles, stdin handshake contract, log lines, exit codes 0/1/2/3; vendor/UPSTREAM_SHA"
provides:
  - "python -m rsglang.launch --frontend python|rust: one launch command for either frontend"
  - "rsglang.backend.run_scheduler: unmodified upstream Scheduler (injectable factory) that reports the handshake on the ready queue"
  - "rsglang.handshake: HANDSHAKE_VERSION=1, HANDSHAKE_KEYS order, read_upstream_sha, encode_handshake_line"
  - "rsglang.sockets: per-run /tmp/minisgl_{0..4}.rsg=<pid> paths, D-07 roles from ServerArgs, rsg-server argv"
  - "rsglang.testing.fake_scheduler.FakeScheduler: Mac stand-in on upstream ZmqPullQueue/ZmqPushQueue with a peer probe"
  - "E2E tracer test (slow) and fast launcher flag tests"
affects: [01-05-launcher-failure-paths, 01-06-integrity-gates, phase-03-transport, phase-05-http, phase-07-bench]

actuals:
  tokens: 6568    # chars/4 over added lines of the realized diff (26270 chars, 8 files)
  tasks: 2
  commits: 3
plan_head_before: 5a4438f22b8378e2a024e893fe4071e9138f5c4c
plan_head_after: cc0c2f7ef9116e19d3ebca4fd7fa446c33f29048

tech-stack:
  added: []
  patterns:
    - "Launcher-only flags via argparse(allow_abbrev=False).parse_known_args; the rest goes verbatim to upstream"
    - "Scheduler process = clone of upstream _run_scheduler plus a ready-queue envelope {kind: ready|error, rank, handshake|traceback}"
    - "Launcher is process-group leader; shutdown sets SIG_IGN only at shutdown time, then killpg(SIGINT), 10 s grace, per-child SIGKILL"
    - "Mac test seam: RSGLANG_SCHEDULER_FACTORY=module:attr, default minisgl.scheduler:Scheduler"
    - "Peer-presence proof on a connecting PUSH needs a separate ZMQ_IMMEDIATE probe socket"

key-files:
  created:
    - python/rsglang/launch.py
    - python/rsglang/backend.py
    - python/rsglang/sockets.py
    - python/rsglang/handshake.py
    - python/rsglang/testing/__init__.py
    - python/rsglang/testing/fake_scheduler.py
    - python/tests/test_launch_rust_e2e.py
    - python/tests/test_launch_args.py
  modified: []

key-decisions:
  - "Launcher SIGINT/SIGTERM handlers are installed before any child is spawned (not only in the supervise loop), so a stop during the readiness wait tears down the whole group instead of orphaning children"
  - "A KeyboardInterrupt in the scheduler process after it passed the ready point ends the process quietly (no error envelope, no re-raise); before that point it is reported as an error"
  - "Python mode execs python -m minisgl and never imports minisgl or parses upstream args in the launcher"

patterns-established:
  - "Launcher log contract: rsglang.launch: spawned rsg-server pid=N / spawned scheduler rank=R pid=N / backend ready; handshake sent to rsg-server / <name> exited with code C [before ready] / backend not ready after T s / exit code C"
  - "Only this run's five socket files are ever unlinked; suffix validated against ^[A-Za-z0-9._=-]+$"

requirements-completed: [BASE-02, BASE-03]

coverage:
  - id: D1
    description: "Rust-mode launch end to end on the Mac: launcher starts rsg-server before the fake scheduler, the handshake (max_seq_len=4096 eos_token_id=151645 page_size=16 max_running_req=8 num_pages=1024 + SHA) reaches rsg-server, Rust's bound _1 is seen by a ZMQ_IMMEDIATE probe, SIGTERM exits 0 with no child or socket file left"
    requirement: BASE-03
    verification:
      - kind: integration
        ref: "python/tests/test_launch_rust_e2e.py#test_rust_mode_handshake_reaches_rsg_server"
        status: pass
    human_judgment: false
  - id: D2
    description: "--frontend python execs python -m minisgl with upstream args unchanged; launcher-only flags stripped; no abbreviations; --shell-mode rejected in rust mode"
    requirement: BASE-02
    verification:
      - kind: unit
        ref: "python/tests/test_launch_args.py (7 tests)"
        status: pass
    human_judgment: false
  - id: D3
    description: "No vendored file touched (D-09): the rust-mode scheduler is the byte-identical upstream class"
    verification:
      - kind: other
        ref: "test \"$(git write-tree --prefix=vendor/mini-sglang/)\" = 02d3e4ad34ec00c88f549fd9d287a4588958d824"
        status: pass
    human_judgment: false
  - id: D4
    description: "Real-GPU confirmation: rust mode with the real upstream Scheduler reports real handshake values"
    verification: []
    human_judgment: true
    rationale: "Needs the Linux GPU box; the Mac cannot construct the real Scheduler. Scheduled as the end-of-phase human check in plan 01-05"

duration: 5min
completed: 2026-10-04
status: complete
---

# Phase 1 Plan 03: Launcher Tracer Summary

**`python -m rsglang.launch --frontend rust` now starts rsg-server and the upstream scheduler ranks in one process group. Rank 0's handshake (engine max_seq_len, eos, post-init page_size, max_running_req, num_pages, upstream SHA) reaches rsg-server on stdin, and SIGTERM tears everything down cleanly. This is proven on the Mac with a fake scheduler that uses upstream's real ZMQ queues. `--frontend python` execs the frozen `python -m minisgl`.**

## Performance

- **Duration:** about 5 min
- **Started:** 2026-10-04T02:57:10Z
- **Completed:** 2026-10-04T03:02:03Z
- **Tasks:** 2 (1 tracer, 1 TDD)
- **Files modified:** 8 (all created)

## Accomplishments

- One launch command for both frontends (BASE-02). Rust mode parses upstream args with upstream's `parse_args` and pins the suffix `.rsg=<launcher pid>`. It unlinks only this run's five socket files, becomes process-group leader, and spawns rsg-server *before* the scheduler ranks (D-10). The handshake line is written once rank 0 reports ready. Supervision follows, and shutdown uses SIG_IGN, then killpg(SIGINT), a 10 s grace period, then SIGKILL.
- The scheduler-process wrapper (BASE-03, D-09) is a clone of upstream `_run_scheduler`. The scheduler class comes from `RSGLANG_SCHEDULER_FACTORY` and defaults to upstream's `minisgl.scheduler:Scheduler`. Handshake values are read from the constructed scheduler: `engine.max_seq_len`, `cache_manager.page_size` after init, and `engine.num_pages`. Failures send a traceback envelope back to the launcher.
- The D-07 topology was checked with upstream's own queues. In the default topology the fake scheduler connects `_1` and Rust binds it, and a `ZMQ_IMMEDIATE` probe confirms the bound peer. I also checked `--num-tokenizer 2` by hand: Rust connected `_1`, the scheduler bound it, and the marker appeared.
- The e2e tracer passes in about 6–7 s and passed 5 consecutive runs. All 7 fast flag tests pass. The vendored tree hash is unchanged.

## Task Commits

1. **Task 1 (tracer): rust-mode launch end to end** - `d82d264` (feat). Tracer gate: interactive, end-of-phase mode, automated-only verify. I re-ran the verify and it passed before starting expansion.
2. **Task 2: --frontend python passthrough (TDD)**
   - `818ee07` test(01-03): failing tests (RED: the 4 python-mode tests failed on `assert 0 == 1`, because execv was never called against the Task 1 stub; the 3 parser/rust-mode tests already passed against Task 1's parser)
   - `cc0c2f7` feat(01-03): exec upstream launcher (GREEN: 7/7, e2e still passes)

## Files Created/Modified

- `python/rsglang/launch.py`: `main`, `build_parser`, `resolve_rust_bin`, `run_rust_mode`, `exec_python_frontend`
- `python/rsglang/backend.py`: `SCHEDULER_FACTORY_ENV`, `DEFAULT_SCHEDULER_FACTORY`, `resolve_scheduler_factory`, `extract_handshake`, `run_scheduler`
- `python/rsglang/handshake.py`: `HANDSHAKE_VERSION`, `HANDSHAKE_KEYS`, `repo_root`, `read_upstream_sha`, `encode_handshake_line`
- `python/rsglang/sockets.py`: `run_socket_paths`, `unlink_run_sockets`, `rust_endpoints`, `rust_cli_args`
- `python/rsglang/testing/__init__.py`: docstring-only package
- `python/rsglang/testing/fake_scheduler.py`: `FakeScheduler`, `FAKE_*` constants, `FACTORY_PATH`, `STATUS_DIR_ENV`
- `python/tests/test_launch_rust_e2e.py`: slow e2e tracer test
- `python/tests/test_launch_args.py`: 7 fast launcher tests

## Decisions Made

See `key-decisions` in the frontmatter. Everything else follows the plan.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Stop handlers installed before spawning, honored during the readiness wait**
- **Found during:** Task 1
- **Issue:** Step 10 installs the SIGINT/SIGTERM handlers only once supervision starts. A SIGTERM during the readiness wait (which can last up to 900 s) would therefore kill only the launcher with the default action and orphan the scheduler ranks and rsg-server.
- **Fix:** Install the stop-flag handlers right after the `setpgid` call and before any spawn. The readiness loop checks the flag and runs the same group shutdown with exit code 0. This is safe: Python-level handlers reset to default in exec'd and spawn children, and SIG_IGN is still set only inside shutdown.
- **Files modified:** python/rsglang/launch.py
- **Verification:** e2e test passes; manual SIGTERM run exits 0 with no leftover processes or sockets
- **Committed in:** d82d264

**2. [Rule 2 - Missing Critical] BrokenPipe on the handshake write**
- **Found during:** Task 1
- **Issue:** If rsg-server dies between the last poll and the handshake write, the write raises BrokenPipeError and the launcher would crash without shutting down the group.
- **Fix:** Catch it, log `rsg-server exited with code C before the handshake was sent`, and shut down with code 1.
- **Files modified:** python/rsglang/launch.py
- **Committed in:** d82d264

---

**Total deviations:** 2 auto-fixed (both Rule 2)
**Impact on plan:** Both deviations are small robustness additions inside the planned shutdown path. There is no scope creep, and 01-05 still owns the stderr tail, the SIGKILL escalation, and the parent watchdog.

## TDD Gate Compliance

Task 2: the RED commit `818ee07` comes before the GREEN commit `cc0c2f7`. During Task 1, `exec_python_frontend` was deliberately a stub (log plus return 2) so the RED tests would fail on their own assertions. No REFACTOR was needed.

## Issues Encountered

- `--frontend python` on the Mac reaches upstream's `python -m minisgl`, then fails importing `uvicorn`. This is expected: the Mac env deliberately excludes the API-server deps, and the GPU box has the full upstream install. The unit tests use an injected execv.
- `--help` is answered by the launcher's parser, so upstream's flag help is not reachable through `rsglang.launch`. `python -m minisgl --help` still works directly.
- Caveat for 01-05: if the launcher is already a process-group leader inside a shell pipeline (for example `... | tee log`), `killpg` also signals the other pipeline members. Interactive and pytest runs are unaffected.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- 01-05 can extend the failure paths in `run_rust_mode.shutdown`: the stderr tail, process-group SIGKILL escalation, and the scheduler ppid watchdog in `backend.run_scheduler`. It also owns the real-GPU end-of-phase check (D4).
- BASE-02 and BASE-03 are also declared by sibling plans, so `requirements.ready-ids` reported 0/2 ready, and REQUIREMENTS.md was not changed by this plan.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED

- All 8 key files present on disk.
- Commits d82d264, 818ee07, cc0c2f7 present in git log.
- `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py python/tests/test_launch_args.py -q` -> 8 passed; vendor tree hash 02d3e4ad... unchanged.
