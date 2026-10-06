---
status: diagnosed
trigger: "UAT test 3 / WR-02: parent watchdog records getppid() only after the scheduler child has booted; user said 修 (fix now). Diagnose only."
created: 2026-10-04T05:22:18Z
updated: 2026-10-04T05:48:00Z
goal: find_root_cause_only
---

## Current Focus

hypothesis: CONFIRMED - start_parent_watchdog() captures `parent = os.getppid()` at run_scheduler entry, which under the spawn start method runs only after the child interpreter boots and unpickles ServerArgs (importing torch, minisgl.utils.hf, minisgl.server.launch); a launcher SIGKILLed in that window has already reparented the child to launchd/init/subreaper, so the watchdog records that pid as "the parent" and never fires.
test: done - differential Mac repro (control kill after handshake vs kill 0-3.5 s after spawn) with a probe that reads the watchdog's closure `parent`.
expecting: n/a (confirmed: 5/5 in-window kills orphan the scheduler with watchdog_parent=1; 4/4 post-window kills clean)
next_action: return ROOT CAUSE FOUND to the orchestrator (goal: find_root_cause_only); fix belongs to the gap-closure plan for G-01-3
bug_class: Bohrbug (deterministic given the kill timing; window is a fixed startup interval of ~2.2-2.6 s on this Mac)
rca_branching:
  candidate_causes:
    - "code (CONFIRMED, root cause): backend.py:61 uses the post-boot getppid() as the reference pid instead of the launcher pid known at spawn time; run_scheduler (backend.py:74) has no launcher-pid parameter and launch.py:164 does not pass one"
    - "code/design (contributing): no OS-level parent-death signal is used - no PR_SET_PDEATHSIG on Linux, and the spawn pipe sentinel (multiprocessing.parent_process()) that is live from exec time is ignored - so detection depends on a 1 s getppid poll that starts late"
    - "config (not a defect): spawn start method forced at launch.py:156 creates the boot window; fork would shrink it but is unsafe with CUDA, so spawn stays"
    - "environment (trigger, not a defect): reparenting semantics - orphan's ppid becomes 1 (launchd/init) or a Linux subreaper (systemd --user, tini, docker --init); any reference captured after the death is wrong"
    - "verification gap (why not caught): python/tests/test_launch_rust_e2e.py:228-240 and scripts/gpu_phase1_check.sh step 4 both kill -9 the launcher only after 'handshake received', i.e., outside the window"
  and_gate: "yes for the symptom, no for the defect - the orphan needs (a) a launcher-only kill (kill -9 <pid>, OOM killer; a group kill takes the children too) AND (b) the kill landing in the ~2-3 s boot window AND (c) the late-capture code. (a) and (b) are external triggers; (c) is the only code defect, and removing it (explicit launcher pid + immediate re-check, optionally PDEATHSIG / sentinel) eliminates the orphan regardless of (a)/(b)."

## Symptoms

expected: The parent-death watchdog cannot miss a launcher that dies early; launcher pid passed explicitly to the scheduler child plus PR_SET_PDEATHSIG on Linux, so kill -9 of the launcher never leaves an orphaned scheduler.
actual: Code review (WR-02) reports the watchdog reads getppid() too late; user confirmed fix-now ("修").
errors: None reported (race found by code review). Relevant to scripts/gpu_phase1_check.sh step 4 (kill -9 of the launcher must leave no rsg-server or scheduler process).
reproduction: UAT test 3; WR-02 in .planning/phases/01-vendored-base-wire-codec/01-REVIEW.md
started: Discovered during code review, confirmed for fixing during UAT.

## Eliminated

- hypothesis: rsg-server has an analogous orphan window (launcher dies before rsg-server starts watching stdin)
  evidence: stdin EOF is level-persistent and rsg-server exits(3) on EOF both before and after the handshake (main.rs:138-190); empirically rsg-server was gone 0.05-0.10 s after the launcher SIGKILL in all 9 runs, including the 5 in-window runs where the scheduler was orphaned.
  timestamp: 2026-10-04T05:40:00Z

- hypothesis: the orphaned scheduler keeps rsg-server's stdin write end open, so rsg-server is orphaned too (AND-gate coupling)
  evidence: spawnv_passfds uses close_fds with an explicit passfds list and os.pipe/subprocess pipes are O_CLOEXEC, so spawn children do not inherit the stdin write end; empirically rsg-server exited within 0.08 s in every in-window run while the scheduler stayed alive.
  timestamp: 2026-10-04T05:40:00Z

- hypothesis: the multiprocessing resource_tracker survivor is an independent orphan source
  evidence: it survives only alongside an orphaned scheduler (it waits for EOF on a pipe whose write end every spawn child holds via tracker_fd); in the control run (survivors list printed) no process, tracker included, survived in the launcher's pgid once the scheduler exited.
  timestamp: 2026-10-04T05:40:00Z

## Evidence

- timestamp: 2026-10-04T05:22:30Z
  checked: .planning/debug/knowledge-base.md
  found: does not exist (no prior resolved sessions); no MemPalace query performed
  implication: no known-pattern candidate; investigate from scratch

- timestamp: 2026-10-04T05:24:00Z
  checked: python/rsglang/backend.py:54-75
  found: start_parent_watchdog() does `parent = os.getppid()` (line 61) and the thread exits only when `os.getppid() != parent` (line 66). It is called as the first statement of run_scheduler (line 75), with no launcher pid argument; run_scheduler's signature is (args, ready_queue, upstream_sha).
  implication: the reference pid is whatever the parent is at the moment run_scheduler starts executing, not the launcher pid known at spawn time.

- timestamp: 2026-10-04T05:24:30Z
  checked: python/rsglang/launch.py:156-170
  found: mp.set_start_method("spawn", force=True); each rank is mp.Process(target=backend.run_scheduler, args=(rank_args, ready_queue, upstream_sha), daemon=False) started from the launcher's main thread. The launcher pid is not in args.
  implication: child is a fresh interpreter; run_scheduler only runs after the child bootstraps.

- timestamp: 2026-10-04T05:26:00Z
  checked: CPython 3.12.12 multiprocessing/popen_spawn_posix.py:_launch and spawn.py:spawn_main/_main
  found: spawn = util.spawnv_passfds (fork+exec from the launcher process, close_fds with an explicit passfds list, no preexec hook). Child sequence: interpreter start -> spawn_main sets parent_sentinel = os.dup(pipe_handle) -> pickle.load(prep_data) -> prepare() re-imports the main module (rsglang.launch as __mp_main__) -> pickle.load(Process object) which imports rsglang.backend and minisgl.server.args (for ServerArgs) -> self._bootstrap(parent_sentinel) -> run_scheduler. The pickled Process object carries _parent_pid = launcher's os.getpid() captured at mp.Process() construction; multiprocessing.parent_process() in the child exposes it (.pid) together with a sentinel fd (the read end of the launcher->child pipe; the launcher keeps the write end parent_w open until its Popen object is finalized), so parent_process().is_alive()/.join() turns false/returns when the launcher dies, no matter when the child checks.
  implication: (a) there is a boot window between fork/exec and run_scheduler's first line; (b) the launcher pid is already available in the child without getppid(); (c) a race-free, cross-platform death signal (the spawn pipe sentinel) already exists from exec time; (d) no Python code can run in the child between fork and exec, so PR_SET_PDEATHSIG can only be set after exec, i.e., at the top of run_scheduler, and must be followed by a re-check against the launcher pid.

- timestamp: 2026-10-04T05:27:00Z
  checked: `python -X importtime -c "import minisgl.server.args"` with the project .venv (Mac, Python 3.12.12)
  found: cumulative 3.05 s (torch 1.16 s, minisgl.utils.hf 1.73 s, minisgl.server.launch 3.0 s) - all of it is triggered by unpickling ServerArgs inside the child BEFORE run_scheduler executes.
  implication: on this Mac the unprotected window is ~3 s per scheduler spawn (longer on a cold page cache / GPU box with CUDA-enabled torch).

- timestamp: 2026-10-04T05:28:00Z
  checked: python/tests/test_launch_rust_e2e.py:228-240 and scripts/gpu_phase1_check.sh step 4 (lines ~212-235)
  found: both kill -9 the launcher only AFTER "handshake received", i.e., long after run_scheduler started and the watchdog captured the (still correct) launcher pid.
  implication: neither the Mac e2e test nor GPU step 4 exercises the early window; both pass with the bug present, so they cannot detect WR-02 or its regression.

- timestamp: 2026-10-04T05:29:00Z
  checked: crates/rsg-server/src/main.rs:95-190 (stdin reader + select loops)
  found: after a non-blocking ZmqTransport::open, rsg-server spawns a dedicated stdin reader thread; both the pre-handshake and post-handshake select! loops exit(3) on StdinEvent::Eof/Error. The launcher holds the only write end of the stdin pipe (subprocess.Popen PIPE; later mp children use close_fds + passfds so they do not inherit it).
  implication: pipe EOF is persistent, so even a launcher that dies before the reader thread starts is detected immediately. rsg-server has no analogous late-capture window (to be confirmed empirically).

- timestamp: 2026-10-04T05:33:00Z
  checked: CONTROL repro - /tmp/wr02-probe/repro.py ... handshake 20 (fresh rsg-server built from this worktree into /tmp/wr02-target; probe factory /tmp/wr02-probe/probe_sched.py reads the watchdog thread's closure cell `parent`)
  found: handshake arrived 3.15 s after "spawned scheduler rank=0"; after SIGKILL of the launcher pid only: rsg-server gone in 0.10 s, scheduler gone in 1.0 s; probe {getppid_now: L, watchdog_parent: L, mp_parent_pid: L, mp_parent_alive: true} (L = launcher pid 74620); no survivors in the launcher's pgid.
  implication: the watchdog works when it starts while the launcher is alive (matches the existing e2e test).

- timestamp: 2026-10-04T05:35:00Z
  checked: WINDOW repro - repro.py ... 0 20 (SIGKILL launcher 0.0 s after "spawned scheduler rank=0")
  found: rsg-server gone in 0.05 s; scheduler STILL ALIVE after 20 s with ppid 1; probe {getppid_now: 1, watchdog_parent: 1, mp_parent_pid: 74817 (the launcher), mp_parent_alive: false}; probe (= run_scheduler/watchdog start) written 2.58 s after the kill; survivors in the dead launcher's pgid: the scheduler (ppid 1) and the multiprocessing resource_tracker (ppid 1).
  implication: CONFIRMED. The watchdog captured launchd's pid 1 as "the parent", so `os.getppid() != parent` can never become true; the scheduler is orphaned forever (on the GPU box it would go on to load weights and hold the GPU). In the same process, multiprocessing already knows the real launcher pid and already reports it dead.

- timestamp: 2026-10-04T05:40:00Z
  checked: delay sweep /tmp/wr02-probe/sweep.py (SIGKILL launcher N s after spawn; observe 8 s)
  found: |
    delay 0.0 -> ORPHAN, ppid 1, watchdog_parent 1, watchdog started 2.16 s after kill
    delay 0.0 -> ORPHAN, ppid 1, watchdog_parent 1, watchdog started 2.55 s after kill
    delay 1.0 -> ORPHAN, ppid 1, watchdog_parent 1, watchdog started 1.31 s after kill
    delay 2.0 -> ORPHAN, ppid 1, watchdog_parent 1, watchdog started 0.28 s after kill
    delay 2.4 -> clean, scheduler gone 0.83 s after kill, watchdog_parent = launcher
    delay 2.8 -> clean, scheduler gone 0.44 s after kill, watchdog_parent = launcher
    delay 3.5 -> clean, scheduler gone 0.73 s after kill, watchdog_parent = launcher
    rsg-server gone 0.06-0.08 s after kill in every run.
  implication: deterministic (Bohrbug): every kill landing before run_scheduler starts orphans the scheduler (5/5 incl. the single run above), every kill after it is clean (4/4 incl. control). The window on this Mac (warm cache, fake scheduler, CPU torch) is ~2.2-2.6 s per spawn; it is interpreter boot + prepare() + the ServerArgs-unpickle imports (torch, minisgl.utils.hf, minisgl.server.launch). On the GPU box with CUDA torch it is expected to be longer (projection, not measured).

- timestamp: 2026-10-04T05:42:00Z
  checked: processes left after all runs (ps filter on spawn_main/resource_tracker/rsg-server/rsglang.launch)
  found: nothing from these experiments (repro.py killpg's the dead launcher's group). One unrelated live launcher (pid 76334, using the main checkout's target/debug/rsg-server, parent 76332) belongs to another concurrent session and was left untouched.
  implication: experiments were clean; no interference with other sessions' sockets (suffixes are per-launcher-pid).

- timestamp: 2026-10-04T05:44:00Z
  checked: deterministic window-widening knob for a future test: PYTHONPATH=/tmp/wr02-probe/slowboot (sitecustomize.py sleeps WR02_SLOWBOOT_S only when "--multiprocessing-fork" is in sys.orig_argv); /tmp/wr02-probe/slowboot_check.py
  found: with WR02_SLOWBOOT_S=3 the spawn target ran 3.07 s after start(); the launcher and the resource tracker are unaffected (no --multiprocessing-fork in their argv); the venv has no existing sitecustomize.
  implication: a Mac e2e test can make the pre-run_scheduler window arbitrarily long and kill -9 the launcher inside it with no timing flakiness and no change to repo code.

- timestamp: 2026-10-04T05:45:00Z
  checked: grep for PDEATHSIG / prctl / parent_process / getppid in python/, crates/, scripts/, vendor/mini-sglang/python
  found: only backend.py:61 and :66 (the getppid watchdog). Upstream has no parent-death mechanism either. Scheduler ranks are spawned by p.start() from the launcher's main thread (launch.py:160-170, called from main()); rsglang.launch itself imports only stdlib + handshake/sockets (stdlib-only).
  implication: PR_SET_PDEATHSIG fires on death of the thread that forked the child; that is the launcher's main thread, which lives as long as the launcher, so PDEATHSIG is safe here. spawnv_passfds has no preexec hook, so the prctl must be issued in the child after exec (top of run_scheduler or earlier during __mp_main__ import) and must be followed by a getppid() == launcher_pid re-check to cover a death that happened before the prctl.

## Resolution

root_cause: |
  python/rsglang/backend.py:61 - start_parent_watchdog() takes its reference pid from os.getppid()
  at the moment run_scheduler (backend.py:74-75) starts, not from the launcher pid. Under the forced
  spawn start method (launch.py:156) run_scheduler only starts after the child has booted a fresh
  interpreter, re-imported rsglang.launch as __mp_main__, and unpickled ServerArgs (which imports
  torch, minisgl.utils.hf and minisgl.server.launch): ~2.2-2.6 s on this Mac, longer with CUDA torch.
  If the launcher is SIGKILLed (pid only) inside that window, the child has already been reparented
  to launchd/init/a subreaper, so `parent` is recorded as that pid (observed: 1), `os.getppid() != parent`
  can never become true, and the scheduler runs forever (on GPU: loads weights and holds the GPU).
  The launcher pid is never passed to the child (launch.py:164 args=(rank_args, ready_queue, upstream_sha)),
  and no OS-level parent-death mechanism (PR_SET_PDEATHSIG, or the multiprocessing spawn-pipe sentinel
  already open from exec time) is used. rsg-server is NOT affected: its stdin-EOF rule is level-persistent.
fix: (diagnose-only session - not applied)
verification: (diagnose-only session - root cause confirmed by differential repro, 5/5 in-window orphans vs 4/4 clean post-window kills)
files_changed: []
suggested_fix_direction: |
  1. launch.py:164 pass the launcher pid explicitly: args=(rank_args, ready_queue, upstream_sha, os.getpid())
     (equivalently multiprocessing.parent_process().pid in the child, which is the launcher's pid pickled
     at mp.Process() construction - explicit arg is clearer and injectable in a unit test).
  2. backend.py start_parent_watchdog(launcher_pid, ...): on Linux, prctl(PR_SET_PDEATHSIG=1, SIGKILL)
     via ctypes FIRST; then an immediate `if os.getppid() != launcher_pid: os._exit(1)` (covers a death
     before the prctl / before run_scheduler); then keep the polling thread comparing against launcher_pid
     (macOS has no PDEATHSIG; the thread is also a backstop on Linux).
     PDEATHSIG notes: fires on death of the THREAD that forked the child - here the launcher's main thread
     (p.start() in _run_rust_mode), which lives as long as the launcher; it must be set in the child after
     exec (spawnv_passfds has no preexec hook); it is preserved across exec for non-setuid binaries.
  3. Optional, cross-platform and event-driven: a daemon thread blocking on
     multiprocessing.parent_process().join() (waits on the spawn-pipe sentinel, readable the instant the
     launcher dies, even if it died before run_scheduler) -> os._exit(1). Caveat: relies on the launcher
     keeping its Process objects (the Popen holds parent_w open; launch.py keeps them in `ranks`) and never
     os.fork()ing after spawning ranks.
  Tests: (Mac) new e2e test - kill -9 the launcher right after "spawned scheduler rank=0", assert the
  scheduler pid is gone; make the window deterministic with a test-only PYTHONPATH sitecustomize.py that
  sleeps when "--multiprocessing-fork" in sys.orig_argv (proven: /tmp/wr02-probe/slowboot). (Mac) unit
  test - subprocess calls start_parent_watchdog(launcher_pid=<not its parent>) and must exit 1 promptly.
  (Linux-only, skipif) assert prctl(PR_GET_PDEATHSIG)==SIGKILL in the child / scheduler dies < 1 s poll.
  (GPU box) add an early-kill variant to gpu_phase1_check.sh step 4 (kill -9 immediately after
  "spawned scheduler rank=0"); the current step 4 kills after the handshake and passes with the bug present.
