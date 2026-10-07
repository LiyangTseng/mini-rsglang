//! BENCH-06's standard-inference-throughput workload (D-12): per trial,
//! spawns 07-03's own `python -m rsglang.bench.standard_throughput` driver
//! (bench_simple-shaped, reusing upstream's client helpers), reads its
//! schema-tagged JSON output, and reports one [`MeasuredWindow`] spanning
//! the driver's own `[t_start_unix, t_end_unix]` window -- the harness never
//! times this itself, so the measured interval always matches what the
//! driver actually benchmarked.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::cmdline;
use crate::orchestrator::{Lifecycle, MeasuredWindow, TrialContext, TrialMeasurement, TrialRunner};

/// The schema tag 07-03's driver writes (`python/rsglang/bench/standard_throughput.py`'s
/// `SCHEMA`). [`parse_throughput_output`] rejects any document whose
/// `schema` field is not exactly this string.
pub const THROUGHPUT_SCHEMA: &str = "rsglang.bench.standard_throughput/1";

/// `rsg-bench throughput` flags (flattened alongside
/// [`crate::orchestrator::SessionArgs`]).
#[derive(Debug, Clone, clap::Args)]
pub struct ThroughputArgs {
    #[arg(
        long,
        default_value = "{python} -m rsglang.bench.standard_throughput --port {port} --out {out} --seed {seed}"
    )]
    pub throughput_cmd: String,
    #[arg(long, default_value_t = 3600.0)]
    pub throughput_timeout_s: f64,
}

/// `avg`/`p50`/`p90`/`p99`/`max`, matching 07-03's `summarize()` `_stats`
/// shape exactly (field names and all).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Stats5 {
    pub avg: f64,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
}

/// 07-03's `summarize()` output shape, field-for-field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputSummary {
    pub num_requests: u64,
    pub num_tokens: u64,
    pub duration_s: f64,
    pub throughput_tok_s: f64,
    pub throughput_req_s: f64,
    pub ttft_ms: Stats5,
    pub tpot_ms: Stats5,
    pub e2e_s: Stats5,
}

/// A parsed, schema-checked throughput driver output document.
#[derive(Debug, Clone)]
pub struct ThroughputOutput {
    pub model: String,
    pub t_start_unix: f64,
    pub t_end_unix: f64,
    pub summary: ThroughputSummary,
}

/// Parses one throughput driver output document (T-07-10-style defensive
/// parsing: every field is checked explicitly, never a panic). Rejects a
/// `schema` that is not exactly [`THROUGHPUT_SCHEMA`], and any missing
/// required field, naming it.
pub fn parse_throughput_output(text: &str) -> anyhow::Result<ThroughputOutput> {
    let doc: serde_json::Value =
        serde_json::from_str(text).context("parse throughput output as JSON")?;

    let schema = doc
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("throughput output missing 'schema'"))?;
    if schema != THROUGHPUT_SCHEMA {
        anyhow::bail!(
            "throughput output has unexpected schema {schema:?}, expected {THROUGHPUT_SCHEMA:?}"
        );
    }

    let model = doc
        .get("model")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("throughput output missing 'model'"))?
        .to_string();
    let t_start_unix = doc
        .get("t_start_unix")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("throughput output missing 't_start_unix'"))?;
    let t_end_unix = doc
        .get("t_end_unix")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("throughput output missing 't_end_unix'"))?;
    let summary_value = doc
        .get("summary")
        .ok_or_else(|| anyhow::anyhow!("throughput output missing 'summary'"))?;
    let summary: ThroughputSummary =
        serde_json::from_value(summary_value.clone()).context("parse throughput output 'summary'")?;

    Ok(ThroughputOutput {
        model,
        t_start_unix,
        t_end_unix,
        summary,
    })
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Spawns the rendered throughput-driver `argv` with `PYTHONPATH` set to
/// `<python_dir>` prepended to any inherited value (otherwise the process
/// inherits the harness's own environment unchanged), polling for exit up
/// to `timeout` and killing on expiry -- the same technique
/// `crosscheck::run_cross_tool`/`s3_coldstart::run_hyperfine` already
/// established for a long-lived external subprocess. A non-zero exit is an
/// `Err` naming the last 40 lines of its stderr.
fn run_throughput_driver(argv: &[String], python_dir: &Path, timeout: Duration) -> anyhow::Result<()> {
    let (argv0, rest) = argv.split_first().context("empty throughput command")?;
    let mut cmd = std::process::Command::new(argv0);
    cmd.args(rest);

    let mut pythonpath = python_dir.to_string_lossy().into_owned();
    if let Ok(existing) = std::env::var("PYTHONPATH") {
        pythonpath.push(':');
        pythonpath.push_str(&existing);
    }
    cmd.env("PYTHONPATH", pythonpath);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn throughput driver: {}", argv.join(" ")))?;
    let mut stdout_pipe = child.stdout.take().context("throughput driver stdout not piped")?;
    let mut stderr_pipe = child.stderr.take().context("throughput driver stderr not piped")?;

    let (tx_out, rx_out) = std::sync::mpsc::channel();
    let reader_out = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        let _ = tx_out.send(buf);
    });
    let (tx_err, rx_err) = std::sync::mpsc::channel();
    let reader_err = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        let _ = tx_err.send(buf);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let stderr_text = reader_err.join().ok().and_then(|()| rx_err.recv().ok()).unwrap_or_default();
                    anyhow::bail!(
                        "throughput driver timed out after {timeout:?}; last stderr:\n{}",
                        tail_lines(&stderr_text, 40)
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => anyhow::bail!("wait on throughput driver: {e}"),
        }
    };

    let _ = reader_out.join().ok().and_then(|()| rx_out.recv().ok());
    let stderr_text = reader_err.join().ok().and_then(|()| rx_err.recv().ok()).unwrap_or_default();
    if !status.success() {
        anyhow::bail!(
            "throughput driver exited {:?}; stderr tail:\n{}",
            status.code(),
            tail_lines(&stderr_text, 40)
        );
    }
    Ok(())
}

/// A [`TrialRunner`] over 07-03's `python -m rsglang.bench.standard_throughput`
/// driver (D-12), one subprocess call per trial.
pub struct ThroughputRunner {
    pub args: ThroughputArgs,
}

impl TrialRunner for ThroughputRunner {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::HarnessManaged
    }

    fn workload(&self) -> serde_json::Value {
        serde_json::json!({
            "throughput_cmd": self.args.throughput_cmd,
            "throughput_timeout_s": self.args.throughput_timeout_s,
        })
    }

    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement> {
        let out_path = ctx.trial_dir.join("throughput.json");

        let mut values: BTreeMap<&str, String> = BTreeMap::new();
        values.insert("python", ctx.cfg.python.clone());
        values.insert("port", ctx.port.to_string());
        values.insert("out", out_path.to_string_lossy().into_owned());
        values.insert("seed", ctx.seed.to_string());
        let argv = cmdline::render(&self.args.throughput_cmd, &values)?;

        let python_dir = ctx.cfg.repo_root.join("python");
        let timeout = Duration::from_secs_f64(self.args.throughput_timeout_s.max(0.0));
        run_throughput_driver(&argv, &python_dir, timeout)?;

        let text = std::fs::read_to_string(&out_path)
            .with_context(|| format!("read throughput output {}", out_path.display()))?;
        let output = parse_throughput_output(&text)?;

        let start_unix_ns = (output.t_start_unix * 1e9).round().max(0.0) as u64;
        let end_unix_ns = (output.t_end_unix * 1e9).round().max(0.0) as u64;

        let result = serde_json::json!({
            "model": output.model,
            "summary": output.summary,
        });

        Ok(TrialMeasurement {
            windows: vec![MeasuredWindow {
                label: "throughput".to_string(),
                start_unix_ns,
                end_unix_ns,
                requests: Vec::new(),
                histograms: None,
            }],
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_doc() -> serde_json::Value {
        serde_json::json!({
            "schema": THROUGHPUT_SCHEMA,
            "params": {"seed": 42},
            "model": "fast",
            "t_start_unix": 100.0,
            "t_end_unix": 101.0,
            "summary": {
                "num_requests": 8,
                "num_tokens": 80,
                "duration_s": 1.0,
                "throughput_tok_s": 80.0,
                "throughput_req_s": 8.0,
                "ttft_ms": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
                "tpot_ms": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
                "e2e_s": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
            },
        })
    }

    #[test]
    fn parses_valid_document() {
        let text = valid_doc().to_string();
        let out = parse_throughput_output(&text).expect("valid document parses");
        assert_eq!(out.model, "fast");
        assert_eq!(out.summary.throughput_tok_s, 80.0);
    }
}
