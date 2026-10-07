//! `rsg-bench`: the Phase 7 Python-vs-Rust frontend benchmark harness (D-01,
//! D-08). It launches a frontend-under-test in its own process group, drives
//! load against its OpenAI-style HTTP surface, records TTFT/ITL/E2E in
//! `hdrhistogram`, and tears the process group down without leaking or
//! over-reaching.

pub mod client;
pub mod cmdline;
pub mod gclog;
pub mod loadgen;
pub mod manifest;
pub mod memory;
pub mod metrics;
pub mod orchestrator;
pub mod procs;
pub mod report;
pub mod rng;
pub mod roles;
pub mod scenarios;
pub mod sse;
pub mod stats;
