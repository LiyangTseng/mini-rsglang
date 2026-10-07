# Phase 4: Tokenizer & Detokenizer Parity - Pattern Map

**Mapped:** 2026-10-06
**Files analyzed:** 10
**Analogs found:** 10 / 10 (all via Phase 1's `rsg-wire` precedent; no prior tokenizer/detokenizer code exists)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `crates/rsg-tokenizer/Cargo.toml` | config | — | `crates/rsg-wire/Cargo.toml` | role-match |
| `crates/rsg-tokenizer/src/lib.rs` | module root | transform | `crates/rsg-wire/src/lib.rs` | role-match |
| `crates/rsg-tokenizer/src/loader.rs` | service (asset loader) | file-I/O | `vendor/mini-sglang/python/minisgl/utils/hf.py` (`load_tokenizer`) | exact (direct port) |
| `crates/rsg-tokenizer/src/template.rs` | utility (template env setup) | transform | none in-repo; `vendor/.../tokenizer/tokenize.py` (call site) + RESEARCH.md Pattern 2 | role-match (cross-language) |
| `crates/rsg-tokenizer/src/encode.rs` | service (CRUD-like: text→ids) | request-response | `vendor/mini-sglang/python/minisgl/tokenizer/tokenize.py` (`TokenizeManager.tokenize`) | exact (direct port) |
| `crates/rsg-tokenizer/src/detokenize.rs` | service (stateful stream decoder) | streaming | `vendor/mini-sglang/python/minisgl/tokenizer/detokenize.py` (`DetokenizeManager.detokenize`, `DecodeStatus`) | exact (direct port) |
| `crates/rsg-tokenizer/tests/token_ids.rs` | test | file-I/O (fixture-driven) | `crates/rsg-wire/tests/fixtures.rs` | exact |
| `crates/rsg-tokenizer/tests/chat_templates.rs` | test | file-I/O (fixture-driven) | `crates/rsg-wire/tests/fixtures.rs` | exact |
| `crates/rsg-tokenizer/tests/detokenize_streams.rs` | test | file-I/O (fixture-driven) | `crates/rsg-wire/tests/fixtures.rs` | exact |
| `scripts/gen_tokenizer_fixtures.py` | utility (fixture generator) | batch / file-I/O | `scripts/gen_wire_fixtures.py` | exact |
| `scripts/check_all.sh` (modified: add a step) | config (CI gate script) | batch | `scripts/check_all.sh` (itself, existing steps 3/5) | exact |

## Pattern Assignments

### `crates/rsg-tokenizer/Cargo.toml` (config)

**Analog:** `crates/rsg-wire/Cargo.toml`

**Full pattern** (entire file, 16 lines) — workspace-relative deps, no pinned versions in the crate itself (versions live in root `Cargo.toml` `[workspace.dependencies]`):
```toml
[package]
name = "rsg-wire"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
rmp-serde.workspace = true
serde.workspace = true
serde_bytes.workspace = true
thiserror.workspace = true

[dev-dependencies]
serde_json.workspace = true
```
**Apply to `rsg-tokenizer`:** same shape, but `tokenizers`, `minijinja`, `minijinja-contrib`, `hf-hub` are **new** crates not yet in root `Cargo.toml`'s `[workspace.dependencies]` (root `Cargo.toml` lines 11-22 list only `anyhow`, `clap`, `rmp-serde`, `serde`, `serde_bytes`, `serde_json`, `thiserror`, `tokio`, `tracing`, `tracing-subscriber`, `zmq`). The planner's first task for this crate must add these four (plus `chrono` or `time` for `strftime_now`, per RESEARCH.md Pitfall 3) to root `Cargo.toml`'s `[workspace.dependencies]` first, then reference them `.workspace = true` here — do not pin versions directly in the crate `Cargo.toml`, mirroring the existing convention. The crate is auto-discovered by the `members = ["crates/*"]` glob (root `Cargo.toml` line 3) — no root-level registration needed beyond the dependency pins.

---

### `crates/rsg-tokenizer/src/lib.rs` (module root, transform)

**Analog:** `crates/rsg-wire/src/lib.rs`

**Module doc-comment pattern** (lines 1-21) — a `//!` header stating the upstream source file(s) ported, the parity contract, and the specific encoding/decoding rules that byte-equality depends on:
```rust
//! The msgpack codec for the mini-sglang scheduler boundary.
//!
//! Upstream (`vendor/mini-sglang`, `message/utils.py`) serializes every message as a map whose
//! first key is `"__type__"` ...
//!
//! Decoding ignores unknown keys; the decode-then-re-encode fixture tests catch schema drift.
```
**Apply:** `rsg-tokenizer/src/lib.rs` should open with the same shape of doc comment, naming the three upstream files it ports (`tokenize.py`, `detokenize.py`, `utils/hf.py`), and re-export the public surface (`pub mod loader; pub mod template; pub mod encode; pub mod detokenize;` plus flattened re-exports like `pub use encode::encode_chat_prompt;`).

**Error type pattern** (lines 44-55) — one `thiserror`-based enum per crate, not per module:
```rust
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("msgpack encode failed: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode failed: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("tensor dtype is {got:?}, expected {TENSOR_DTYPE_INT32:?}")]
    TensorDtype { got: String },
    #[error("tensor buffer length {len} is not a multiple of 4")]
    TensorLength { len: usize },
}
```
**Apply:** define `TokenizerError` in `lib.rs` with `#[from]` variants for `tokenizers::Error`, `minijinja::Error`, `hf_hub` errors, `serde_json::Error`, plus explicit variants for parity-specific failures (e.g. `MissingChatTemplate`, `BosCountMismatch` if that assertion lives in library code rather than only tests).

**Constants-from-file pattern** (line 27):
```rust
pub const UPSTREAM_SHA: &str = include_str!("../../../vendor/UPSTREAM_SHA").trim_ascii();
```
**Apply:** reuse `UPSTREAM_SHA` from `rsg-wire` (depend on it, or duplicate the `include_str!` — prefer depending on `rsg_wire::UPSTREAM_SHA` if cross-crate reuse is cheap) rather than re-deriving it, to keep "single source" per the existing doc comment's stated intent.

**Unit test module pattern** (lines 190 to end) — `#[cfg(test)] mod tests` inside `lib.rs` for crate-internal invariants (constants, type tags), separate from the fixture-driven integration tests in `tests/*.rs`. Apply the same split: fast structural tests in `lib.rs`, fixture-byte/text-parity tests in `tests/`.

---

### `crates/rsg-tokenizer/src/loader.rs` (service, file-I/O)

**Analog:** `vendor/mini-sglang/python/minisgl/utils/hf.py:load_tokenizer()` (full function, 11 lines, quoted in RESEARCH.md Architecture Pattern 1 and reproduced here for the plan to port verbatim)

**Core pattern to port** (exact source, read directly):
```python
# vendor/mini-sglang/python/minisgl/utils/hf.py
def load_tokenizer(model_path: str) -> PreTrainedTokenizerBase:
    tokenizer = AutoTokenizer.from_pretrained(model_path)
    # Some Mistral models store chat_template in a separate JSON file
    if not getattr(tokenizer, "chat_template", None):
        try:
            path = hf_hub_download(repo_id=model_path, filename="chat_template.json")
            with open(path, "r", encoding="utf-8") as f:
                tokenizer.chat_template = json.load(f)["chat_template"]
        except Exception:
            pass
    return tokenizer
```
**Rust port shape:** fetch `tokenizer.json` + `tokenizer_config.json` + `special_tokens_map.json` (D-06 adds this third file) via `hf-hub`'s `HFClient`; parse `tokenizer_config.json` as `serde_json::Value`; if its `chat_template` key is missing/null/empty, fall back to downloading `chat_template.json` and reading its `"chat_template"` field, swallowing errors exactly as the Python `except Exception: pass` does (i.e. leave `chat_template: None` rather than propagating an error) — this is the one place where **not** propagating a `Result` error mirrors upstream's actual behavior.

**Error-swallowing semantics to preserve:** the Python `try/except Exception: pass` means a failed `chat_template.json` fallback silently leaves the tokenizer without a chat template, rather than failing the whole load. The Rust equivalent should use `.ok()` on the fallback download/parse rather than `?`, consistent with `rsg-wire`'s practice of making every genuine failure an explicit typed error (`WireError` variants) while this specific upstream branch is intentionally best-effort.

**D-04 skip-clean behavior (Llama-specific):** when `HF_TOKEN`/gated access is unavailable, the loader (or its caller) must detect this and let the caller skip cleanly — no existing analog in-repo for "skip a test with a clear log," but Rust's standard pattern is `#[ignore]` with a printed reason, or an early `return Ok(None)` / a dedicated `TokenizerError::GatedAccessUnavailable` variant that call sites in `tests/*.rs` match on to skip rather than fail.

---

### `crates/rsg-tokenizer/src/template.rs` (utility, transform)

**No in-repo analog** — this is genuinely new (no prior Jinja/minijinja usage anywhere in the Rust workspace). Follow RESEARCH.md Architecture Pattern 2 verbatim for the `Environment` setup:
```rust
let mut env = minijinja::Environment::new();
minijinja_contrib::add_to_environment(&mut env);
env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
env.add_function("raise_exception", |msg: String| -> Result<minijinja::Value, minijinja::Error> {
    Err(minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, msg))
});
env.add_function("strftime_now", |fmt: String| -> String {
    chrono::Local::now().format(&fmt).to_string()
});
```
**Cross-cutting note:** per RESEARCH.md Pitfall 3, the fixture generator and the Rust test harness must both freeze `strftime_now`'s clock to the same fixed instant when generating/checking Llama fixtures — production code keeps the real clock. This means `template.rs` should expose the `strftime_now` registration as injectable (e.g. take a `now: impl Fn() -> DateTime` parameter or a cfg/test-only override), not hardcode `chrono::Local::now()` unconditionally, so tests can substitute a fixed instant.

**Style precedent to follow (from `rsg-wire`):** keep this as its own module (`template.rs`) separate from `encode.rs`, matching the existing convention of one concern per file rather than one monolithic `lib.rs` (the wire crate is small enough to be one file, but RESEARCH.md's recommended structure already splits `rsg-tokenizer` into `loader.rs` / `encode.rs` / `template.rs` / `detokenize.rs`, which this plan should follow exactly).

---

### `crates/rsg-tokenizer/src/encode.rs` (service, request-response)

**Analog:** `vendor/mini-sglang/python/minisgl/tokenizer/tokenize.py:17-30` (`TokenizeManager.tokenize`, read directly — exact source):
```python
if isinstance(msg.text, list):
    prompt = self.tokenizer.apply_chat_template(
        msg.text,
        tokenize=False,
        add_generation_prompt=True,
    )
    assert isinstance(prompt, str)
else:
    prompt = msg.text
input_ids: torch.Tensor = self.tokenizer.encode(prompt, return_tensors="pt")
```
**Rust port shape:**
```rust
let prompt = match text {
    PromptInput::Chat(messages) => template.render_chat(messages, /* bos_token, eos_token */)?,
    PromptInput::Raw(s) => s,
};
let encoding = tokenizer.encode(prompt, true)?; // add_special_tokens = true, matches Python default
let ids: &[u32] = encoding.get_ids();
```
**Exact call-site parameters to preserve (parity-critical, confirmed by direct source read):**
- `apply_chat_template(..., tokenize=False, add_generation_prompt=True)` — no `tools`, no `date_string` kwarg passed; this is why Llama's `strftime_now` branch always fires (Pitfall 3).
- `tokenizer.encode(prompt, return_tensors="pt")` uses the HF default `add_special_tokens=True` → Rust's `Tokenizer::encode(prompt, true)`.
- D-10's BOS-exactly-once assertion belongs in the test file, not in `encode.rs` itself, unless the plan decides the loader should de-duplicate — RESEARCH.md Pitfall 2 only specifies the test, not a fix, so default to asserting rather than silently correcting (parity means matching the oracle's actual two-BOS-then-whatever-downstream-does-with-it behavior, if that's what upstream truly produces end to end — verify against the oracle before adding any de-dup logic not present upstream).

---

### `crates/rsg-tokenizer/src/detokenize.rs` (service, streaming)

**Analog:** `vendor/mini-sglang/python/minisgl/tokenizer/detokenize.py:54-111` (`DetokenizeManager.detokenize`, `DecodeStatus`) — **must be ported byte-for-byte**, already quoted verbatim in RESEARCH.md Architecture Pattern 3. Full source re-confirmed by direct read this session:
```python
@dataclass
class DecodeStatus:
    decoded_ids: List[int]
    decoded_str: str
    read_offset: int
    surr_offset: int
    sent_offset: int

class DetokenizeManager:
    def detokenize(self, msgs: List[DetokenizeMsg]) -> List[str]:
        read_ids, surr_ids = [], []
        for msg in msgs:
            if msg.uid not in self.decode_map:
                self.decode_map[msg.uid] = DecodeStatus(
                    decoded_ids=[], decoded_str="", read_offset=0, surr_offset=0, sent_offset=0,
                )
            s = self.decode_map[msg.uid]
            if not (msg.finished and msg.next_token == self.eos_token_id):
                s.decoded_ids.append(msg.next_token)
            read_ids.append(s.decoded_ids[s.surr_offset :])
            surr_ids.append(s.decoded_ids[s.surr_offset : s.read_offset])
        read_texts = self.tokenizer.batch_decode(read_ids)
        surr_texts = self.tokenizer.batch_decode(surr_ids)
        incremental_strs = []
        for msg, read_str, surr_str in zip(msgs, read_texts, surr_texts, strict=True):
            s = self.decode_map[msg.uid]
            new_text = read_str[len(surr_str):]
            if len(new_text) > 0 and not new_text.endswith("�"):
                output_str = s.decoded_str + new_text
                s.decoded_str = output_str
                s.surr_offset = s.read_offset
                s.read_offset = len(s.decoded_ids)
            else:
                new_text = find_printable_text(new_text)
                output_str = s.decoded_str + new_text
            incremental_output = output_str[s.sent_offset:]
            s.sent_offset = len(output_str)
            incremental_strs.append(incremental_output)
            if msg.finished:
                del self.decode_map[msg.uid]
        return incremental_strs

def _is_chinese_char(cp: int):
    if ((cp >= 0x4E00 and cp <= 0x9FFF) or (cp >= 0x3400 and cp <= 0x4DBF)
        or (cp >= 0x20000 and cp <= 0x2A6DF) or (cp >= 0x2A700 and cp <= 0x2B73F)
        or (cp >= 0x2B740 and cp <= 0x2B81F) or (cp >= 0x2B820 and cp <= 0x2CEAF)
        or (cp >= 0xF900 and cp <= 0xFAFF) or (cp >= 0x2F800 and cp <= 0x2FA1F)):
        return True
    return False

def find_printable_text(text: str):
    if text.endswith("\n"):
        return text
    elif len(text) > 0 and _is_chinese_char(ord(text[-1])):
        return text
    elif len(text) > 1 and _is_chinese_char(ord(text[-2])):
        return text[:-1]
    else:
        return text[: text.rfind(" ") + 1]
```
**Rust port notes (parity-critical, from RESEARCH.md Pitfalls 1/4/5, do not re-derive — apply directly):**
- State struct: `FxHashMap<i64, DecodeStatus>` (use `rustc_hash::FxHashMap` per CLAUDE.md's stack guidance), `.remove(&uid)` on `msg.finished`, mirroring `del self.decode_map[msg.uid]`.
- `batch_decode([read_ids],[surr_ids])` → `tokenizer.decode_batch(&[&read_ids, &surr_ids], false)` (`skip_special_tokens=false`, matching upstream's default).
- **Character, not byte, slicing:** `read_str[len(surr_str):]` must become `.chars().skip(surr_str.chars().count())` or an explicit `.char_indices()`-based byte offset — never `&read_str[surr_str.len()..]`.
- `"�"` check → `new_text.ends_with('\u{FFFD}')`.
- If `clean_up_tokenization_spaces` is true for the model (Llama), apply the exact 10-step `.replace(...)` chain (RESEARCH.md Pitfall 5, quoted there verbatim from `transformers` `tokenization_utils_base.py`) to the decoded text, in order, as a Python-side-equivalent post-step — this does **not** exist inside the Rust `tokenizers` core and must be hand-ported as a plain `String` method chain.
- Do not use `tokenizers::DecodeStream` (RESEARCH.md Pitfall 4 — missing EOS-exclusion and CJK fallback).

**No in-repo Rust analog exists for this stateful-map pattern** — `rsg-wire` is purely stateless encode/decode. The closest structural precedent for "a map keyed by request id with explicit removal on completion" is the FSM design described in RESEARCH.md/CLAUDE.md for Phase 5 (not yet implemented), so this file is the first concrete instance of that shape in the codebase; subsequent Phase 5 files should look to this one as their analog, not the reverse.

---

### `crates/rsg-tokenizer/tests/{token_ids,chat_templates,detokenize_streams}.rs` (test, file-I/O)

**Analog:** `crates/rsg-wire/tests/fixtures.rs` (full file, 137 lines) + `crates/rsg-wire/tests/common/mod.rs` (fixture-dir helper, hex helper, manifest parser)

**Manifest-driven fixture test pattern** (lines 1-59 of `fixtures.rs`):
```rust
mod common;
use std::path::PathBuf;
use rsg_wire::{SamplingParams, Tensor, WIRE_TYPE_TAGS};
use serde_json::Value;

fn manifest_cases(manifest: &Value) -> &Vec<Value> { manifest["cases"].as_array().expect(...) }
fn field<'a>(case: &'a Value, key: &str) -> &'a str { ... }
fn read_fixture(file: &str) -> Vec<u8> { std::fs::read(common::fixtures_dir().join(file))... }

#[test]
fn every_fixture_roundtrips_byte_exact() {
    let manifest = common::manifest();
    for case in manifest_cases(&manifest) {
        let bytes = read_fixture(field(case, "file"));
        let again = reencode(...);
        assert_eq!(common::hex(&again), common::hex(&bytes), "case {name}: ...");
    }
}
```
**Apply:** add a `tests/common/mod.rs` to `rsg-tokenizer` with a `fixtures_dir()` helper pointing at `fixtures/tokenizer/` (not `fixtures/wire/`), and a `manifest()`/model-slug-aware variant since D-11 stores per-model subdirectories (`fixtures/tokenizer/{model_slug}/token_ids.json` etc.) rather than one flat manifest. Each of the three test files loads its own JSON fixture (not msgpack — these are plain-text/id-array JSON per D-08/D-11, so use `serde_json::from_str` directly instead of `rsg_wire`'s binary+manifest split) and asserts Rust's computed value equals the fixture's recorded value, case by case, with the case `name` included in every `assert_eq!` failure message (critical ergonomic pattern — every assertion above names the case).

**D-04 skip-clean pattern for Llama cases:** no existing analog (Phase 1 has no "skip if env missing" case — its fixtures are always generatable offline from vendored code). New pattern: check for `HF_TOKEN`/gated-access availability at the top of each Llama-parametrized test and `return` early with a `println!`/`eprintln!` log plus (if available) `#[ignore]`-equivalent runtime skip, never a hard failure. Qwen3 cases have no such guard (D-04: "always runs regardless").

---

### `scripts/gen_tokenizer_fixtures.py` (utility, batch/file-I/O)

**Analog:** `scripts/gen_wire_fixtures.py` (full file, 319 lines) — direct structural template, confirmed by CONTEXT.md D-07 and RESEARCH.md's Recommended Project Structure as the explicit precedent to mirror.

**CLI/exit-code contract** (lines 299-318, `main()`):
```python
def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=FIXTURES_DIR, help="output directory")
    parser.add_argument("--check", action="store_true", help="regenerate into a temp dir and byte-diff")
    args = parser.parse_args(argv)
    try:
        if args.check:
            return check(args.out)
        count = generate(args.out)
    except EnvError as exc:
        print(f"gen_wire_fixtures: error: {exc}", file=sys.stderr)
        return 2
    print(f"gen_wire_fixtures: wrote {count} cases to {args.out}")
    return 0
```
**Apply verbatim-shaped:** same `--out`/`--check` flags, same exit codes (0 ok, 1 fixtures differ, 2 environment error), same `EnvError` exception class for "cannot import/reach upstream or HF" failures. D-04's clean-skip requirement for Llama maps onto this generator too: if gated access is unavailable, the generator should skip writing Llama fixtures (with a clear log) rather than raising `EnvError`, while still writing/checking Qwen3 fixtures — this is a deliberate *deviation* from `gen_wire_fixtures.py`'s all-or-nothing `EnvError` model, since Phase 1 had no notion of a partially-unavailable oracle.

**Upstream-import-and-origin-check pattern** (lines 52-87, `_load_upstream()`):
```python
def _load_upstream():
    sys.path.insert(0, str(VENDOR_PY))
    try:
        import msgpack, numpy, torch, minisgl
        from minisgl.core import SamplingParams
        from minisgl.message import (...)
        from minisgl.message.utils import serialize_type
    except ImportError as exc:
        raise EnvError(f"cannot import upstream message classes: {exc}") from exc
    origin = Path(list(minisgl.__path__)[0]).resolve()
    if not origin.is_relative_to(VENDOR_PY.resolve()):
        raise EnvError(f"minisgl resolved to {origin}, not the vendored tree under {VENDOR_PY}")
    return {...}
```
**Apply:** `gen_tokenizer_fixtures.py` should import `AutoTokenizer`, `apply_chat_template` plumbing, and `minisgl.tokenizer.detokenize.{DetokenizeManager, DecodeStatus, find_printable_text, _is_chinese_char}` from the **vendored tree** the same way, with the same origin-check guard against accidentally importing a pip-installed `transformers`/`minisgl` instead of the pinned vendored one — though note tokenizer loading pulls from HF Hub (not purely the vendored tree), so the origin check applies only to the `minisgl.tokenizer.detokenize` import, not to `AutoTokenizer` itself.

**Case-table-as-data pattern** (lines 90-199, `build_cases()`) and **manifest-with-summary pattern** (lines 202-264, `_summarize()`/`generate()`): apply directly — one Python list of `(name, ..., value)` tuples in a fixed order that both the generator and the Rust test's case table must agree on (cf. `case_tables_agree()` test in `fixtures.rs` lines 74-82), and a JSON manifest recording enough metadata (here: token ids array, rendered prompt string, or incremental text chunks, rather than msgpack hex/length/sha256) for the Rust side to assert against directly.

**Check-mode regenerate-and-diff pattern** (lines 267-296, `check()`): regenerate into a `tempfile.TemporaryDirectory()`, diff file-by-file and manifest-key-by-key, print one `DIFF <name>: <reason>` line per difference, return 1 if any diffs, 0 otherwise. Apply directly, adapted for JSON-text diffing instead of binary-byte diffing (and freezing the clock for Llama's `strftime_now`, per RESEARCH.md Pitfall 3 — the check-mode regeneration must use the same frozen instant as the original generation, or every Llama chat-template fixture will spuriously diff).

---

### `scripts/check_all.sh` (modified: add one step)

**Analog:** itself — existing steps 3 (`gen_wire_fixtures.py --check`) and 5 (`check_upstream.py`), lines 25-26 and 29-30:
```bash
step 3 "fixture freshness (gen_wire_fixtures.py --check)"
"$PYTHON" scripts/gen_wire_fixtures.py --check
```
**Apply:** add a new numbered step (renumber the trailing steps and the `[N/5]` → `[N/6]` counter in the `step()` calls and the header comment) immediately after/alongside step 3, e.g.:
```bash
step 4 "tokenizer fixture freshness (gen_tokenizer_fixtures.py --check)"
"$PYTHON" scripts/gen_tokenizer_fixtures.py --check
```
Keep the `set -euo pipefail` / stop-at-first-failure semantics (top of file, line 7) and the existing `step()` helper (line 19) unchanged — do not introduce a parallel or alternate gate mechanism per RESEARCH.md's note that this phase "adds a step... parallel to the existing fixture-freshness step," not a new script.

## Shared Patterns

### Upstream-source-citation doc comments
**Source:** `crates/rsg-wire/src/lib.rs` lines 1-21
**Apply to:** every new `rsg-tokenizer` module (`loader.rs`, `encode.rs`, `detokenize.rs`, `template.rs`) — each should open with a `//!` comment naming the exact upstream Python file and function/class it ports, and stating the parity contract in one sentence, exactly as `rsg-wire`'s header does for `message/utils.py`.

### Single typed error enum per crate via `thiserror`
**Source:** `crates/rsg-wire/src/lib.rs` lines 44-55 (`WireError`)
**Apply to:** `rsg-tokenizer`'s `TokenizerError` in `lib.rs`, with `#[from]` conversions for every external error type (`tokenizers::Error`, `minijinja::Error`, `serde_json::Error`, hf-hub's error type) plus named variants for parity-specific failure modes.

### Fixture dir + manifest/case-table triad
**Source:** `crates/rsg-wire/tests/common/mod.rs` (whole file) + `scripts/gen_wire_fixtures.py` `build_cases()`/`generate()` + `crates/rsg-wire/tests/fixtures.rs` `case_tables_agree()`
**Apply to:** all three new test files plus `gen_tokenizer_fixtures.py` — keep the Python case table and the Rust-side expectations in the same order, verified by an explicit `case_tables_agree`-style test, per D-11's `fixtures/tokenizer/{model_slug}/*.json` layout (JSON text, not msgpack binary, since there is no byte-exactness requirement here — text/id-array equality instead).

### Mac-only generation, `--check` regen-and-diff, exit codes 0/1/2
**Source:** `scripts/gen_wire_fixtures.py` `main()`, `check()`, `EnvError`
**Apply to:** `scripts/gen_tokenizer_fixtures.py` end-to-end, with the one deviation noted above (D-04's partial-skip for ungated-Llama environments, which `gen_wire_fixtures.py` has no analog for since it never has a partially-unavailable oracle).

### CI gate step ordering (`scripts/check_all.sh`)
**Source:** `scripts/check_all.sh` whole file
**Apply to:** insert the new tokenizer-fixture-freshness step without altering the stop-at-first-failure, numbered-step convention.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `crates/rsg-tokenizer/src/template.rs` (minijinja `Environment` setup, custom globals) | utility | transform | No prior Jinja/minijinja usage anywhere in the Rust workspace; the only in-repo reference is the Python call site (`apply_chat_template`) plus RESEARCH.md's own code sketch (Architecture Pattern 2), which should be treated as the primary source for this file instead of a codebase analog |
| `crates/rsg-tokenizer/src/detokenize.rs` (per-uid stateful map with explicit removal on finish) | service | streaming | No existing Rust code in this repo manages a request-keyed map with completion-triggered removal; `rsg-wire` is purely stateless. The Python oracle (`detokenize.py`) is the only real analog, already fully quoted above |

## Metadata

**Analog search scope:** `crates/` (both existing crates, all files), `scripts/` (all 8 scripts), `vendor/mini-sglang/python/minisgl/{tokenizer,utils}/` (the three canonical-ref files named in CONTEXT.md), root `Cargo.toml`
**Files scanned:** 14 read in full or targeted sections (rsg-wire: `lib.rs`, `Cargo.toml`, `tests/fixtures.rs`, `tests/common/mod.rs`; scripts: `gen_wire_fixtures.py`, `check_all.sh`; root `Cargo.toml`; CONTEXT.md, RESEARCH.md for Phase 4 — vendor Python files already quoted verbatim in RESEARCH.md, not re-read)
**Pattern extraction date:** 2026-10-06
**Tracked-source gate:** all 10 analog paths confirmed via `git ls-files` (non-empty output for every path) — none are gitignored mirrors.
