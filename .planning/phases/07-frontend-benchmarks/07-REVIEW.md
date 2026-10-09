---
phase: 07-frontend-benchmarks
reviewed: 2026-10-07T00:00:00Z
depth: standard
files_reviewed: 61
files_reviewed_list:
  - Cargo.toml
  - crates/rsg-bench/Cargo.toml
  - crates/rsg-bench/src/lib.rs
  - crates/rsg-bench/src/sse.rs
  - crates/rsg-bench/src/client.rs
  - crates/rsg-bench/src/metrics.rs
  - crates/rsg-bench/src/procs.rs
  - crates/rsg-bench/src/bin/bench-stub.rs
  - crates/rsg-bench/src/bin/rsg-mock-stack.rs
  - crates/rsg-bench/src/rng.rs
  - crates/rsg-bench/src/loadgen.rs
  - crates/rsg-bench/src/stats.rs
  - crates/rsg-bench/src/gclog.rs
  - crates/rsg-bench/src/roles.rs
  - crates/rsg-bench/src/memory.rs
  - crates/rsg-bench/src/cmdline.rs
  - crates/rsg-bench/src/manifest.rs
  - crates/rsg-bench/src/orchestrator.rs
  - crates/rsg-bench/src/main.rs
  - crates/rsg-bench/src/report.rs
  - crates/rsg-bench/src/scenarios/mod.rs
  - crates/rsg-bench/src/scenarios/s1_cancel.rs
  - crates/rsg-bench/src/scenarios/s2_saturation.rs
  - crates/rsg-bench/src/scenarios/sweep.rs
  - crates/rsg-bench/src/scenarios/crosscheck.rs
  - crates/rsg-bench/src/scenarios/s3_coldstart.rs
  - crates/rsg-bench/src/scenarios/standard_throughput.rs
  - crates/rsg-bench/tests/common/mod.rs
  - crates/rsg-bench/tests/tracer.rs
  - crates/rsg-bench/tests/teardown.rs
  - crates/rsg-bench/tests/loadgen_cancel.rs
  - crates/rsg-bench/tests/metrics.rs
  - crates/rsg-bench/tests/gclog.rs
  - crates/rsg-bench/tests/memory_pss_gate.rs
  - crates/rsg-bench/tests/s1_report_schema.rs
  - crates/rsg-bench/tests/orchestrator_alternation.rs
  - crates/rsg-bench/tests/manifest_schema.rs
  - crates/rsg-bench/tests/s2_curve.rs
  - crates/rsg-bench/tests/sweep.rs
  - crates/rsg-bench/tests/crosscheck_parse.rs
  - crates/rsg-bench/tests/coldstart.rs
  - crates/rsg-bench/tests/hyperfine_parse.rs
  - crates/rsg-bench/tests/s3_coldstart_e2e.rs
  - crates/rsg-bench/tests/throughput_runner.rs
  - crates/rsg-bench/tests/report_render.rs
  - crates/rsg-bench/tests/mock_stack.rs
  - crates/rsg-bench/tests/fixtures/vllm_result.json
  - crates/rsg-bench/tests/fixtures/sglang_result.jsonl
  - crates/rsg-bench/tests/fixtures/fake_bench_tool.sh
  - crates/rsg-bench/tests/fixtures/hyperfine_export.json
  - crates/rsg-bench/tests/fixtures/fake_standard_throughput.py
  - crates/rsg-tokenizer/src/loader.rs
  - python/rsglang/profiling/hook.py
  - python/rsglang/bench/__init__.py
  - python/rsglang/bench/standard_throughput.py
  - python/tests/test_hook_gc_only.py
  - python/tests/test_bench_simple_reuse.py
  - python/tests/test_gpu_bench_script.py
  - python/tests/test_baseline_profile.py
  - python/tests/test_gpu_profile_script.py
  - scripts/gpu_phase7_bench.sh
  - scripts/bench_mac_devpass.sh
findings:
  critical: 0
  warning: 3
  info: 3
  total: 6
status: issues_found
---

# Phase 7: Code Review Report

**Reviewed:** 2026-10-07T00:00:00Z
**Depth:** standard
**Files Reviewed:** 61
**Status:** issues_found

## Summary

Reviewed the `rsg-bench` crate (orchestrator, load generator, process lifecycle,
manifest/report pipeline, scenario runners), `rsg-tokenizer`'s loader, the
Python hook/throughput-driver modules, and the two shell wrappers that drive
the harness end to end.

The code is unusually disciplined for a benchmarking tool: nearly every
function that touches subprocess lifecycle, file I/O, or env-var parsing has
an explicit doc comment calling out the edge case it defends against
("never a crash", "never a fabricated value", symlink refusal, atomic
writes, leader-identity checks before `killpg`, etc.), and most of those
claims hold up under tracing. I did not find a data-loss, injection, or
authentication-class bug. The issues below are narrower: two correctness
gaps where an edge case the rest of the crate is careful about was missed
(an unvalidated CLI range that can panic/balloon, and an EOF case that can
misclassify a backend failure as a success), plus a few lower-impact
robustness/consistency nits.

## Warnings

### WR-01: Unvalidated `--prompt-words-max` can panic or balloon into a near-u32::MAX random range

**File:** `crates/rsg-bench/src/scenarios/s2_saturation.rs:138`
**Issue:**
`S2Args.prompt_words_max` (`crates/rsg-bench/src/scenarios/s2_saturation.rs:36-39`,
default `32`) flows unchecked into `let prompt_words = (1, self.args.prompt_words_max);`
and from there into every `make_prompt` call
(`crates/rsg-bench/src/loadgen.rs:34-40`):

```rust
pub fn make_prompt(rng: &mut SplitMix64, min_words: u32, max_words: u32) -> String {
    let n = rng.range_inclusive_u32(min_words, max_words);
    ...
}
```

`range_inclusive_u32` (`crates/rsg-bench/src/rng.rs:49-54`) only guards the
`lo > hi` case with `debug_assert!`, which compiles out in release builds
(the shape `cargo build --release -p rsg-bench` in
`scripts/gpu_phase7_bench.sh` uses):

```rust
pub fn range_inclusive_u32(&mut self, lo: u32, hi: u32) -> u32 {
    debug_assert!(lo <= hi, "range_inclusive_u32: lo {lo} > hi {hi}");
    let span = u64::from(hi - lo) + 1;
    lo + (self.next_u64() % span) as u32
}
```

If an operator passes `--prompt-words-max 0` (or any value `< 1`, since the
lower bound is hard-coded to `1`), `hi - lo` underflows `u32`:
- In a **debug** build, this panics immediately with "attempt to subtract
  with overflow" (overflow checks are on by default in the `dev` profile,
  independent of the `debug_assert!`), crashing the harness mid-session.
- In a **release** build, the subtraction silently wraps to `u32::MAX`,
  making `span` ≈ `2^32`; `n` can then land anywhere up to `u32::MAX`, and
  `make_prompt`'s `(0..n).map(...).collect::<Vec<_>>().join(" ")`
  (`crates/rsg-bench/src/loadgen.rs:36-39`) will try to build a prompt with
  up to ~4 billion words, effectively hanging/exhausting memory rather than
  failing cleanly.

Every other list-shaped CLI input in this crate (`validate_levels` for
`--rates`/`--concurrency`, `validate_candidates` for the sweep) is validated
before use; this one path was missed. `S1Args` doesn't have this exposure
only because its `prompt_words` tuple is hard-coded (`(16, 128)` in
`scenarios/s1_cancel.rs:58`), not because the underlying primitive is safe.

**Fix:** Validate `prompt_words_max >= 1` (and generally `min_words <=
max_words`) before constructing the tuple, e.g. in
`S2Runner::run_trial` right after reading `self.args.prompt_words_max`:

```rust
if self.args.prompt_words_max < 1 {
    anyhow::bail!("--prompt-words-max must be >= 1, got {}", self.args.prompt_words_max);
}
let prompt_words = (1, self.args.prompt_words_max);
```

and/or make `range_inclusive_u32` itself return a `Result`/clamp instead of
relying on a caller-side `debug_assert!` that is absent from release builds.

### WR-02: A clean EOF with no `[DONE]` is always `Outcome::Completed`, even when the connection closed because the backend died

**File:** `crates/rsg-bench/src/client.rs:225-258` (`read_stream`)
**Issue:**
```rust
let chunk = match resp.chunk().await {
    Ok(Some(c)) => c,
    Ok(None) => break 'read, // EOF without an explicit [DONE]: still Completed.
    Err(e) => { outcome = Outcome::Failed; error = Some(e.to_string()); break 'read; }
};
```
`resp.chunk().await` returning `Ok(None)` is only an unambiguous "the
response body finished cleanly" signal when the HTTP framing explicitly
delimits the body (`Content-Length`, or a terminating zero-size chunk under
`Transfer-Encoding: chunked`). For a server that streams with
`Connection: close` and **no** `Content-Length`/chunked framing, the body is
delimited purely by the TCP connection closing, so `hyper` cannot
distinguish "the server finished and closed politely" from "the server
process died mid-stream and the kernel closed the socket" — both arrive at
the client as `Ok(None)`. `crates/rsg-bench/src/bin/bench-stub.rs:261`
builds exactly that ambiguous framing for its own chat-completion response
head (`"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n...\r\nConnection: close\r\n\r\n"`,
no `Content-Length`/chunked encoding), so a `bench-stub`/mock-backed trial
cannot tell a mid-stream crash from a normal finish via this code path
either.

The practical blast radius is bounded today — `report.rs`'s
`comparison_valid` gate already refuses to call anything measured against a
`mock`/`stub` backend a real frontend comparison — but the classification
bug is in `client.rs` itself, not in the mock fixture, and it silently
inflates `counts.completed` (and therefore `rps`) for a request that
produced zero tokens and zero `[DONE]`.

**Fix:** Treat an `Ok(None)` that occurred before any `[DONE]` payload *and*
before the `max_tokens`-implied end as suspect rather than automatically
`Completed` — e.g. carry the number of chunks actually read and compare it
against the request's `max_tokens`, or at minimum record a distinguishable
outcome (`Outcome::Failed` or a dedicated `Outcome::TruncatedEof`) when EOF
arrives with zero chunks observed, so a dead backend doesn't register as a
0-token "success".

### WR-03: `probe_upstream_sha` rejects an otherwise-valid uppercase-hex SHA

**File:** `crates/rsg-bench/src/manifest.rs:224-230`
**Issue:**
```rust
fn probe_upstream_sha(repo_root: &Path) -> Option<String> {
    let path = repo_root.join("vendor").join("UPSTREAM_SHA");
    let text = std::fs::read_to_string(path).ok()?;
    let sha = text.trim();
    let valid = sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    valid.then(|| sha.to_string())
}
```
A syntactically valid 40-character SHA that happens to use uppercase hex
digits (e.g. hand-edited, or written by a tool other than `git rev-parse`,
which always emits lowercase) is silently treated as if the file didn't
exist (`upstream_sha: None` in the manifest), with no warning recorded.
Every other defensive-parsing path in this file (and `gclog.rs`,
`memory.rs`) prefers "accept the value and record it" over "silently drop
it", making this one inconsistent.

**Fix:** Either accept any hex-digit case (`b.is_ascii_hexdigit()` alone)
or, if case is meant to be meaningful, surface the rejection as a `warnings`
entry rather than a silent `None`.

## Info

### IN-01: `redact_argv` normalizes internal whitespace in multi-word tokens

**File:** `crates/rsg-bench/src/cmdline.rs:172-184`
**Issue:** When a single argv token contains internal whitespace (e.g. a
quoted `--python-cmd "<cmd> --api-key ..."` value delivered as one token),
`redact_argv` re-joins the redacted pieces with a single space
(`redact_flat(&pieces).join(" ")`), collapsing any original multiple spaces
or tabs. This only affects the *recorded* manifest/`harness_argv` value
(never the argv actually executed), so it's cosmetic, but a reader
diffing the recorded command against what was actually run could be
confused by the whitespace normalization.
**Fix:** Low priority; if exact round-tripping matters, split on whitespace
runs while preserving the original separators, or note in the manifest
schema docs that recorded argv whitespace is normalized.

### IN-02: `also_best` is `true` by default even when no sweep ever ran

**File:** `crates/rsg-bench/src/orchestrator.rs:52`
**Issue:** `let also_best = best_nt.is_none_or(|v| v == default_nt);` makes
`python-default.also_best == true` whenever `--python-best-num-tokenizer`
is simply not passed, not only when a sweep explicitly confirmed the
default is the best candidate. `report.rs:844-851` then emits "Python's
default --num-tokenizer setting is also its best-performing one" for every
run that never swept at all, which overstates what was actually verified.
**Fix:** Consider a third state (`also_best: Option<bool>`, or a
`best_source: "swept" | "assumed"` field) so the report sentence can
distinguish "a sweep confirmed this" from "no sweep was requested".

### IN-03: `is_secret_name`/redaction allowlist is a fixed substring list

**File:** `crates/rsg-bench/src/cmdline.rs:141-150`
**Issue:** `SECRET_SUBSTRINGS` (`key`, `token`, `secret`, `password`,
`passwd`, `auth`, `credential`, `cookie`) is a reasonable first pass but
will not catch every credential-shaped flag an operator might add later
(e.g. `api_id`, `bearer`, `apikey` is covered via `key` but a flag literally
named `secret-value` is fine while something like `x-signature` would slip
through). Given this feeds the manifest's redaction guarantee (T-07-12),
a future caller adding a new `--*-cmd` template with a differently-named
credential flag could leak it into a committed manifest.
**Fix:** No action required now; flagging so the allowlist gets revisited
whenever a new launch-template flag is introduced.

---

_Reviewed: 2026-10-07T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
