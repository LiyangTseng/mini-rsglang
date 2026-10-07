//! Role attribution without py-spy (D-15). Every pid in a spawned process
//! tree is classified into a benchmark [`Role`] using its process name
//! (`sysinfo`, or the Rust frontend's own argv[0]) and/or phase 07-02's
//! `hook.py` `proc` records (the Python multiprocessing process name) -- no
//! sampling profiler is attached to any process.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::gclog::HookLog;

/// Which frontend is under test. Also a `clap::ValueEnum` for the harness
/// CLI (`--frontend python|rust`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum FrontendKind {
    Python,
    Rust,
}

/// A benchmark role, attributed to one pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    ApiServer,
    Tokenizer,
    Scheduler,
    Launcher,
    RustFrontend,
    Other,
}

/// The coarser grouping BENCH-08's report tables use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Frontend,
    Launcher,
    Scheduler,
    Other,
}

/// `ApiServer`/`Tokenizer`/`RustFrontend` roll up to `Frontend` (the
/// Rust-vs-Python-comparable surface); everything else maps 1:1.
pub fn group_of(role: Role) -> Group {
    match role {
        Role::ApiServer | Role::Tokenizer | Role::RustFrontend => Group::Frontend,
        Role::Scheduler => Group::Scheduler,
        Role::Launcher => Group::Launcher,
        Role::Other => Group::Other,
    }
}

/// `mock-scheduler`'s own process name (Phase 3), recognized the same way a
/// real Python scheduler's `-scheduler`-suffixed hook name is.
pub const MOCK_SCHEDULER_PROCESS_NAME: &str = "mock-scheduler";

/// Classifies one pid. Order of precedence (07-RESEARCH / 07-CONTEXT D-15):
///
/// 1. An explicit process-name match wins first: the Rust frontend binary's
///    own name (`rust_frontend_process_name`, default `rsg-server`) gives
///    [`Role::RustFrontend`]; [`MOCK_SCHEDULER_PROCESS_NAME`] gives
///    [`Role::Scheduler`].
/// 2. A hook-reported multiprocessing name (07-02's `proc` records): a
///    `-scheduler` suffix gives [`Role::Scheduler`]; a name containing
///    `tokenizer` gives [`Role::Tokenizer`] (this covers
///    `minisgl-detokenizer-0`).
/// 3. The pgid leader falls back to [`Role::ApiServer`] for
///    [`FrontendKind::Python`] (the launcher execs into the frontend, same
///    pid) or [`Role::Launcher`] for [`FrontendKind::Rust`] (the launcher
///    spawns a separate frontend child).
/// 4. Anything else is [`Role::Other`].
pub fn classify(
    pid: i32,
    leader_pid: i32,
    kind: FrontendKind,
    hook_name: Option<&str>,
    process_name: Option<&str>,
    rust_frontend_process_name: &str,
) -> Role {
    if let Some(name) = process_name {
        if name == rust_frontend_process_name {
            return Role::RustFrontend;
        }
        if name == MOCK_SCHEDULER_PROCESS_NAME {
            return Role::Scheduler;
        }
    }
    if let Some(name) = hook_name {
        if name.ends_with("-scheduler") {
            return Role::Scheduler;
        }
        if name.contains("tokenizer") {
            return Role::Tokenizer;
        }
    }
    if pid == leader_pid {
        return match kind {
            FrontendKind::Python => Role::ApiServer,
            FrontendKind::Rust => Role::Launcher,
        };
    }
    Role::Other
}

/// Every known pid's role, for one trial.
#[derive(Debug, Clone, Default)]
pub struct RoleMap {
    pub roles: BTreeMap<i32, Role>,
}

impl RoleMap {
    /// Classifies every pid known from `hook.start_pids`, `process_names`'
    /// keys, and `leader_pid` itself (so the leader is always classified,
    /// even when it wrote no hook file and has no separately-sampled
    /// process name).
    pub fn build(
        leader_pid: i32,
        kind: FrontendKind,
        hook: &HookLog,
        process_names: &BTreeMap<i32, String>,
        rust_frontend_process_name: &str,
    ) -> RoleMap {
        let mut pids: BTreeSet<i32> = hook.start_pids.iter().copied().collect();
        pids.extend(process_names.keys().copied());
        pids.insert(leader_pid);

        let mut roles = BTreeMap::new();
        for pid in pids {
            let hook_name = hook.proc_names.get(&pid).map(String::as_str);
            let process_name = process_names.get(&pid).map(String::as_str);
            let role = classify(
                pid,
                leader_pid,
                kind,
                hook_name,
                process_name,
                rust_frontend_process_name,
            );
            roles.insert(pid, role);
        }
        RoleMap { roles }
    }

    /// [`Role::Other`] for any pid not classified by [`RoleMap::build`].
    pub fn role_of(&self, pid: i32) -> Role {
        self.roles.get(&pid).copied().unwrap_or(Role::Other)
    }

    /// Pids (sorted, `BTreeMap` iteration order) whose role maps to `group`
    /// via [`group_of`].
    pub fn pids_in(&self, group: Group) -> Vec<i32> {
        self.roles
            .iter()
            .filter(|(_, role)| group_of(**role) == group)
            .map(|(pid, _)| *pid)
            .collect()
    }
}
