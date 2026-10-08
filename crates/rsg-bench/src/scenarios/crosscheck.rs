//! D-04 third-party cross-checks: `vllm bench serve --backend openai-chat`
//! and `python -m sglang.benchmark.serving --backend sglang-oai-chat`
//! against each arm's freshly launched server, with key-tolerant,
//! defensive parsing of the tool's own result file (T-07-17). Scenario 2
//! only: neither tool can inject mid-stream cancellations.

use std::collections::BTreeMap;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::cmdline;
use crate::orchestrator::{Lifecycle, MeasuredWindow, TrialContext, TrialMeasurement, TrialRunner};

/// Which third-party benchmark client to cross-check against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Vllm,
    Sglang,
}

impl Tool {
    /// The lowercase name used in the session's scenario string
    /// (`crosscheck_vllm` / `crosscheck_sglang`) and the default result
    /// file name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tool::Vllm => "vllm",
            Tool::Sglang => "sglang",
        }
    }
}

/// `vllm bench serve`'s default argv template (RESEARCH A3: flag names from
/// docs.vllm.ai v0.22.1, not re-verified on the GPU box). `--tool-cmd`
/// overrides this entirely.
pub const VLLM_TEMPLATE: &str = "{vllm} bench serve --backend openai-chat --endpoint /v1/chat/completions --base-url http://127.0.0.1:{port} --model {model} --dataset-name random --random-input-len {input_len} --random-output-len {output_len} --num-prompts {num_prompts} --max-concurrency {concurrency} --percentile-metrics ttft,tpot,itl,e2el --metric-percentiles 50,90,99 --seed {seed} --save-result --result-dir {out_dir} --result-filename {out_file}";

/// `python -m sglang.benchmark.serving`'s default argv template (RESEARCH
/// A3: CLAUDE.md, not re-verified on the GPU box). `--tool-cmd` overrides
/// this entirely.
pub const SGLANG_TEMPLATE: &str = "{sglang_python} -m sglang.benchmark.serving --backend sglang-oai-chat --base-url http://127.0.0.1:{port} --model {model} --dataset-name random --random-input-len {input_len} --random-output-len {output_len} --num-prompts {num_prompts} --max-concurrency {concurrency} --seed {seed} --output-file {out_dir}/{out_file}";

fn default_template(tool: Tool) -> &'static str {
    match tool {
        Tool::Vllm => VLLM_TEMPLATE,
        Tool::Sglang => SGLANG_TEMPLATE,
    }
}

/// Cross-check's own CLI flags (flattened alongside
/// [`crate::orchestrator::SessionArgs`]).
#[derive(Debug, Clone, clap::Args)]
pub struct CrossArgs {
    #[arg(long, value_enum)]
    pub tool: Tool,
    /// Overrides the tool's default argv template entirely.
    #[arg(long)]
    pub tool_cmd: Option<String>,
    /// Path to the `vllm` CLI (its own venv), required for `--tool vllm`
    /// unless `--tool-cmd` is given.
    #[arg(long)]
    pub vllm_bin: Option<String>,
    /// Path to the Python interpreter that has `sglang` installed (its own
    /// venv), required for `--tool sglang` unless `--tool-cmd` is given.
    #[arg(long)]
    pub sglang_python: Option<String>,
    #[arg(long, default_value_t = 512)]
    pub num_prompts: u32,
    #[arg(long, default_value_t = 64)]
    pub concurrency: u32,
    #[arg(long, default_value_t = 32)]
    pub input_len: u32,
    #[arg(long, default_value_t = 32)]
    pub output_len: u32,
    #[arg(long, default_value_t = 1800.0)]
    pub tool_timeout_s: f64,
}

/// Requires `--vllm-bin`/`--sglang-python` for the chosen tool, unless
/// `--tool-cmd` overrides the template entirely (the harness never installs
/// either tool: T-07-16).
pub fn validate_cross_args(args: &CrossArgs) -> anyhow::Result<()> {
    if args.tool_cmd.is_some() {
        return Ok(());
    }
    let has_path = match args.tool {
        Tool::Vllm => args.vllm_bin.is_some(),
        Tool::Sglang => args.sglang_python.is_some(),
    };
    if !has_path {
        anyhow::bail!(
            "crosscheck --tool {:?} requires --vllm-bin, --sglang-python, or --tool-cmd",
            args.tool
        );
    }
    Ok(())
}

/// One tool's parsed result: the chosen tool and every numeric metric
/// [`parse_tool_result`] kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossCheckResult {
    pub tool: Tool,
    pub metrics: BTreeMap<String, f64>,
}

/// Parses a third-party tool's result file (T-07-17, defensive): the whole
/// document as one JSON object first (vllm's `--save-result` shape); if
/// that fails (e.g. a JSONL stream of progress-then-final records), the
/// last non-empty line as its own JSON object (sglang's shape). Keeps every
/// numeric top-level field whose lowercase key contains `ttft`, `itl`,
/// `tpot` or `e2e` and ends in `_ms`, plus `request_throughput`,
/// `output_throughput` and `completed`. A document with no TTFT field, or
/// one that never parses to a JSON object at all, is an `Err` -- never a
/// crash.
pub fn parse_tool_result(tool: Tool, text: &str) -> anyhow::Result<CrossCheckResult> {
    let value = parse_document_or_last_line(text)?;
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("cross-check tool result is not a JSON object"))?;
    let metrics = extract_metrics(obj);
    if !metrics.keys().any(|k| k.to_lowercase().contains("ttft")) {
        anyhow::bail!("cross-check tool result has no TTFT field");
    }
    Ok(CrossCheckResult { tool, metrics })
}

/// Tries the whole trimmed text as one JSON document first; on failure
/// (e.g. multiple JSON values back to back, JSONL-style), falls back to
/// the last non-empty line.
fn parse_document_or_last_line(text: &str) -> anyhow::Result<serde_json::Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(value);
    }
    let last_line = trimmed
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("cross-check tool result is empty"))?;
    serde_json::from_str(last_line.trim())
        .map_err(|e| anyhow::anyhow!("failed to parse cross-check tool result as JSON: {e}"))
}

const NAMED_METRIC_KEYS: [&str; 3] = ["request_throughput", "output_throughput", "completed"];

fn extract_metrics(obj: &serde_json::Map<String, serde_json::Value>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for (key, value) in obj {
        let Some(num) = value.as_f64() else { continue };
        let lower = key.to_lowercase();
        let matches_latency_pattern = lower.ends_with("_ms")
            && (lower.contains("ttft")
                || lower.contains("itl")
                || lower.contains("tpot")
                || lower.contains("e2e"));
        let is_named = NAMED_METRIC_KEYS.contains(&key.as_str());
        if matches_latency_pattern || is_named {
            out.insert(key.clone(), num);
        }
    }
    out
}

/// Spawns `argv[0] argv[1..]` with stdout discarded and stderr captured on
/// a background thread (so a chatty tool never blocks on a full pipe
/// buffer), polling for exit up to `timeout` and killing on expiry.
/// Returns `Ok(())` on a zero exit, `Err` naming the last 20 stderr lines
/// otherwise.
fn run_cross_tool(argv: &[String], timeout: Duration) -> anyhow::Result<()> {
    let (argv0, rest) = argv
        .split_first()
        .context("empty cross-check tool command")?;
    let mut cmd = std::process::Command::new(argv0);
    cmd.args(rest);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn cross-check tool: {}", argv.join(" ")))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .context("cross-check tool stderr not piped")?;

    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let stderr_text = reader
                        .join()
                        .ok()
                        .and_then(|()| rx.recv().ok())
                        .unwrap_or_default();
                    anyhow::bail!(
                        "cross-check tool timed out after {timeout:?}; last stderr:\n{}",
                        tail_lines(&stderr_text, 20)
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => anyhow::bail!("wait on cross-check tool: {e}"),
        }
    };

    let stderr_text = reader
        .join()
        .ok()
        .and_then(|()| rx.recv().ok())
        .unwrap_or_default();
    if !status.success() {
        anyhow::bail!(
            "cross-check tool exited {:?}; last stderr:\n{}",
            status.code(),
            tail_lines(&stderr_text, 20)
        );
    }
    Ok(())
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// A [`TrialRunner`] that spawns the chosen tool against the launched
/// arm's server (D-04).
pub struct CrossRunner {
    pub args: CrossArgs,
}

impl TrialRunner for CrossRunner {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::HarnessManaged
    }

    fn workload(&self) -> serde_json::Value {
        serde_json::json!({
            "tool": self.args.tool,
            "num_prompts": self.args.num_prompts,
            "concurrency": self.args.concurrency,
            "input_len": self.args.input_len,
            "output_len": self.args.output_len,
            "tool_timeout_s": self.args.tool_timeout_s,
        })
    }

    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement> {
        let model = ctx
            .model_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("TrialContext has no model_id"))?;

        let template = self
            .args
            .tool_cmd
            .clone()
            .unwrap_or_else(|| default_template(self.args.tool).to_string());
        let out_file = format!("{}-result.json", self.args.tool.as_str());

        let mut values: BTreeMap<&str, String> = BTreeMap::new();
        values.insert("port", ctx.port.to_string());
        values.insert("model", model);
        values.insert("num_prompts", self.args.num_prompts.to_string());
        values.insert("concurrency", self.args.concurrency.to_string());
        values.insert("input_len", self.args.input_len.to_string());
        values.insert("output_len", self.args.output_len.to_string());
        values.insert("seed", ctx.seed.to_string());
        values.insert("out_dir", ctx.trial_dir.to_string_lossy().into_owned());
        values.insert("out_file", out_file.clone());
        if let Some(vllm) = &self.args.vllm_bin {
            values.insert("vllm", vllm.clone());
        }
        if let Some(sglang_python) = &self.args.sglang_python {
            values.insert("sglang_python", sglang_python.clone());
        }

        let argv = cmdline::render(&template, &values)?;

        let start_unix_ns = unix_ns_now();
        run_cross_tool(
            &argv,
            Duration::from_secs_f64(self.args.tool_timeout_s.max(0.0)),
        )?;
        let end_unix_ns = unix_ns_now();

        let result_path = ctx.trial_dir.join(&out_file);
        let text = std::fs::read_to_string(&result_path).with_context(|| {
            format!(
                "read cross-check tool result file {}",
                result_path.display()
            )
        })?;
        let result = parse_tool_result(self.args.tool, &text)?;

        Ok(TrialMeasurement {
            windows: vec![MeasuredWindow {
                label: "crosscheck".to_string(),
                start_unix_ns,
                end_unix_ns,
                requests: Vec::new(),
                histograms: None,
            }],
            result: serde_json::to_value(result)?,
        })
    }
}

fn unix_ns_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
