//! Process-group launch, readiness polling, and teardown for a
//! frontend-under-test subprocess (D-01, T-07-01/T-07-02).
//!
//! Mirrors `python/rsglang/profiling/procs.py`'s `launch_server`/`wait_ready`/
//! `teardown`: a dedicated process group at spawn (`setpgid`-equivalent via
//! `CommandExt::process_group(0)`), `SIGINT` then `SIGKILL` at teardown, and
//! "already gone" (`ESRCH`/`EPERM`) treated as success, never an error.

use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid as SysPid, ProcessStatus, ProcessesToUpdate, System, ThreadKind};

/// What to launch and how, before any process-group-specific wiring is
/// applied.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub argv: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub log_path: PathBuf,
}

/// A launched subprocess, in its own process group (`pgid == leader_pid`).
#[derive(Debug)]
pub struct ServerHandle {
    pub pgid: i32,
    pub leader_pid: i32,
    pub launched_at: Instant,
    pub launched_unix_ns: u64,
    pub log_path: PathBuf,
    child: std::process::Child,
    reaped: bool,
}

/// What `teardown` observed.
#[derive(Debug, Clone)]
pub struct TeardownReport {
    pub graceful: bool,
    pub survivors: Vec<i32>,
}

fn unix_ns_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Fails if port `port` on `127.0.0.1` is already answering connections,
/// so a stale server is never mistaken for a freshly launched one under
/// test (T-07-15 seam).
pub fn ensure_port_free(port: u16) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse()?;
    match std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
        Ok(_) => anyhow::bail!("port {port} already in use"),
        Err(_) => Ok(()),
    }
}

/// Spawns `spec.argv` in a fresh process group, with stdout/stderr appended
/// to `spec.log_path`. Checks `ensure_port_free` first.
pub fn launch(spec: &LaunchSpec, port: u16) -> anyhow::Result<ServerHandle> {
    ensure_port_free(port)?;

    if let Some(parent) = spec.log_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create log dir {}", parent.display()))?;
    }
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&spec.log_path)
        .with_context(|| format!("open log file {}", spec.log_path.display()))?;
    let log_file_err = log_file.try_clone().context("clone log file handle")?;

    let (argv0, rest) = spec
        .argv
        .split_first()
        .context("LaunchSpec.argv must not be empty")?;

    let mut cmd = Command::new(argv0);
    cmd.args(rest)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(log_file_err)
        .process_group(0); // new process group; leader = this child's own pid

    for (k, v) in &spec.env_set {
        cmd.env(k, v);
    }
    for k in &spec.env_remove {
        cmd.env_remove(k);
    }

    let launched_at = Instant::now();
    let launched_unix_ns = unix_ns_now();
    let child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", spec.argv.join(" ")))?;
    let leader_pid = child.id() as i32;

    Ok(ServerHandle {
        pgid: leader_pid,
        leader_pid,
        launched_at,
        launched_unix_ns,
        log_path: spec.log_path.clone(),
        child,
        reaped: false,
    })
}

fn tail_log(log_path: &std::path::Path, n: usize) -> String {
    let Ok(text) = std::fs::read_to_string(log_path) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Polls `GET {base}/v1/models` every `poll` until it returns 200, and
/// returns the elapsed time since `handle.launched_at`. Fails early (with a
/// log tail) if the leader has already exited, and fails on `timeout`.
pub async fn wait_ready(
    handle: &ServerHandle,
    client: &reqwest::Client,
    port: u16,
    timeout: Duration,
    poll: Duration,
) -> anyhow::Result<Duration> {
    let url = format!("{}/v1/models", crate::client::local_base_url(port));
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return Ok(handle.launched_at.elapsed());
        }

        if leader_exited(handle) {
            let tail = tail_log(&handle.log_path, 40);
            anyhow::bail!(
                "leader exited before ready; last lines of {}:\n{tail}",
                handle.log_path.display()
            );
        }

        if Instant::now() > deadline {
            anyhow::bail!("server not ready after {:?}", timeout);
        }

        tokio::time::sleep(poll).await;
    }
}

/// Whether the leader process is gone or a zombie. Never reaps: reaping
/// would free the pgid number for reuse before teardown finishes signalling
/// it.
pub fn leader_exited(handle: &ServerHandle) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(
        ProcessesToUpdate::Some(&[SysPid::from_u32(handle.leader_pid as u32)]),
        true,
    );
    match sys.process(SysPid::from_u32(handle.leader_pid as u32)) {
        None => true,
        Some(p) => p.status() == ProcessStatus::Zombie,
    }
}

/// Live, non-zombie pids whose process group equals `pgid`.
pub fn group_members_alive(pgid: i32) -> Vec<i32> {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.processes()
        .keys()
        .filter_map(|&sys_pid| {
            let pid = sys_pid.as_u32() as i32;
            let proc = sys.process(sys_pid)?;
            if proc.status() == ProcessStatus::Zombie {
                return None;
            }
            // On Linux, sysinfo surfaces each userland OS thread of a
            // multi-threaded process (e.g. tokio worker threads) as its own
            // pid-like entry; such a thread shares its process's pgid, so
            // without this filter every thread of any tokio multi-threaded
            // binary in the group would be double-counted as a distinct
            // member.
            if matches!(proc.thread_kind(), Some(ThreadKind::Userland)) {
                return None;
            }
            let got_pgid = nix::unistd::getpgid(Some(Pid::from_raw(pid)))
                .ok()?
                .as_raw();
            (got_pgid == pgid).then_some(pid)
        })
        .collect()
}

/// `SIGINT` the group, poll for `grace` for it to quiesce, `SIGKILL` any
/// survivor, then reap the leader. Non-reaping until that final `SIGKILL`,
/// so the leader's zombie keeps `pgid` reserved for the whole grace period.
pub async fn teardown(mut handle: ServerHandle, grace: Duration) -> anyhow::Result<TeardownReport> {
    let pgid = handle.pgid;
    check_signalable_pgid(pgid)?;
    safe_killpg(pgid, Signal::SIGINT);

    let deadline = Instant::now() + grace;
    let mut graceful = true;
    loop {
        if group_members_alive(pgid).is_empty() {
            break;
        }
        if Instant::now() > deadline {
            graceful = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if !graceful {
        safe_killpg(pgid, Signal::SIGKILL);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let _ = handle.child.wait();
    handle.reaped = true;

    Ok(TeardownReport {
        graceful,
        survivors: group_members_alive(pgid),
    })
}

/// T-07-01: rejects `pgid <= 1` (process group 0/1, or any bare pid of 0/1)
/// and the harness's own `getpgrp()`. The leader is never reaped before the
/// final `SIGKILL` in [`teardown`], so its zombie keeps `pgid` reserved for
/// the whole grace period and a recycled id is never signalled by mistake.
pub fn check_signalable_pgid(pgid: i32) -> anyhow::Result<()> {
    if pgid <= 1 {
        anyhow::bail!("refusing to signal pgid {pgid} (<= 1)");
    }
    let own = nix::unistd::getpgrp().as_raw();
    if pgid == own {
        anyhow::bail!("refusing to signal the harness's own process group ({pgid})");
    }
    Ok(())
}

/// `killpg` guarded by [`check_signalable_pgid`]. ESRCH/EPERM ("already
/// gone") are not errors (Phase 2 precedent); an unsafe pgid is silently
/// skipped rather than signalled.
fn safe_killpg(pgid: i32, sig: Signal) {
    if check_signalable_pgid(pgid).is_ok() {
        let _ = signal::killpg(Pid::from_raw(pgid), sig);
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        safe_killpg(self.pgid, Signal::SIGKILL);
        let _ = self.child.wait();
    }
}

/// A detached server group (T-07-20): enough identity to safely signal it
/// later from a *separate* process invocation (`coldstart-stop`) once the
/// harness's own [`ServerHandle`]/`Child` is gone -- the leader's pgid plus
/// the leader's own process start time, so a since-recycled pgid is never
/// mistaken for this group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedGroup {
    pub pgid: i32,
    pub leader_start_time: u64,
    pub run: u32,
    pub hook_dir: Option<PathBuf>,
}

impl ServerHandle {
    /// Consumes `self` *without* killing anything (marks it reaped so
    /// [`Drop`] becomes a no-op), returning a [`DetachedGroup`] that can
    /// signal this group from a later, separate process invocation
    /// (T-07-20). Fails if the leader has already vanished -- there would
    /// be no start time to record, and nothing to detach.
    pub fn detach(mut self, run: u32, hook_dir: Option<PathBuf>) -> anyhow::Result<DetachedGroup> {
        let leader_start_time = process_start_time(self.leader_pid)
            .context("leader process vanished before it could be detached")?;
        self.reaped = true; // Drop becomes a no-op; the pgid now outlives this handle.
        Ok(DetachedGroup {
            pgid: self.pgid,
            leader_start_time,
            run,
            hook_dir,
        })
    }
}

/// `sysinfo`'s process start time (seconds since boot) for `pid`, or `None`
/// if the process does not exist. The leader-identity check in
/// [`stop_detached`] compares this against the value recorded at
/// [`ServerHandle::detach`] time (T-07-20): a recycled pid/pgid will almost
/// never show the same start time.
pub fn process_start_time(pid: i32) -> Option<u64> {
    let mut sys = System::new();
    sys.refresh_processes(
        ProcessesToUpdate::Some(&[SysPid::from_u32(pid as u32)]),
        true,
    );
    sys.process(SysPid::from_u32(pid as u32))
        .map(|p| p.start_time())
}

/// Live, non-zombie pids whose process group equals `pgid` *and* whose own
/// start time is at or after `min_start_time` (T-07-20): never counts a
/// member of a group whose pgid number has since been recycled for an
/// unrelated process tree that merely happens to share the number.
fn group_members_matching(pgid: i32, min_start_time: u64) -> Vec<i32> {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.processes()
        .iter()
        .filter_map(|(&sys_pid, proc)| {
            let pid = sys_pid.as_u32() as i32;
            if proc.status() == ProcessStatus::Zombie {
                return None;
            }
            let got_pgid = nix::unistd::getpgid(Some(Pid::from_raw(pid)))
                .ok()?
                .as_raw();
            if got_pgid != pgid {
                return None;
            }
            (proc.start_time() >= min_start_time).then_some(pid)
        })
        .collect()
}

/// Signals each pid individually (never `killpg`, since the pgid *number*
/// itself may already be recycled to an unrelated group) matching
/// [`group_members_matching`], guarded the same way [`safe_killpg`] guards
/// a pgid.
fn signal_matching_members(pgid: i32, min_start_time: u64, sig: Signal) {
    for pid in group_members_matching(pgid, min_start_time) {
        if check_signalable_pgid(pid).is_ok() {
            let _ = signal::kill(Pid::from_raw(pid), sig);
        }
    }
}

/// Tears down a [`DetachedGroup`] recorded by a prior process's
/// [`ServerHandle::detach`] (T-07-20). Verifies leader identity before
/// trusting `killpg`: the pid equal to `group.pgid` must still exist with
/// exactly the `leader_start_time` recorded at detach time. When that
/// holds, behaves like [`teardown`] (`killpg` SIGINT, poll for `grace`,
/// `killpg` SIGKILL on survivors). When it does *not* hold -- the leader
/// already exited and, since it is not this process's child, was reaped by
/// init rather than left as a zombie reserving the pgid -- signals only
/// pids individually matching [`group_members_matching`], never `killpg`
/// on the (possibly-recycled) pgid number itself.
pub async fn stop_detached(
    group: &DetachedGroup,
    grace: Duration,
) -> anyhow::Result<TeardownReport> {
    let pgid = group.pgid;
    let leader_present = process_start_time(pgid) == Some(group.leader_start_time);

    if leader_present {
        check_signalable_pgid(pgid)?;
        safe_killpg(pgid, Signal::SIGINT);
    } else {
        signal_matching_members(pgid, group.leader_start_time, Signal::SIGINT);
    }

    let deadline = Instant::now() + grace;
    let mut graceful = true;
    loop {
        if group_members_matching(pgid, group.leader_start_time).is_empty() {
            break;
        }
        if Instant::now() > deadline {
            graceful = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if !graceful {
        if leader_present {
            safe_killpg(pgid, Signal::SIGKILL);
        } else {
            signal_matching_members(pgid, group.leader_start_time, Signal::SIGKILL);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(TeardownReport {
        graceful,
        survivors: group_members_matching(pgid, group.leader_start_time),
    })
}
