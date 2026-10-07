//! Scenario-specific [`crate::orchestrator::TrialRunner`] implementations.
//! Each scenario adds only a `TrialRunner` and a CLI subcommand (D-08) --
//! alternation, launch/teardown, observation and manifest writing live
//! once in `orchestrator.rs`/`manifest.rs`.

pub mod crosscheck;
pub mod s1_cancel;
pub mod s2_saturation;
pub mod s3_coldstart;
pub mod standard_throughput;
pub mod sweep;
