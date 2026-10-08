//! Tokenizer asset loading, ported from `utils/hf.py::load_tokenizer()`.
//!
//! Fetches `tokenizer.json`, `tokenizer_config.json`, and `special_tokens_map.json` (D-06) from
//! the HF Hub repo named by a [`crate::ModelSpec`], falling back to `chat_template.json` when
//! `tokenizer_config.json` has no (or falsy) `chat_template` key — mirroring upstream's
//! `try/except Exception: pass` fallback exactly (a failed fallback leaves `chat_template: None`
//! rather than failing the whole load, so `.ok()` is used here instead of `?`).
//!
//! Gated models (D-04, Plan 04-04's Task 2) add one more concern: `meta-llama/*` repos 401 an
//! unauthenticated fetch. `load_model_assets` detects this *before* and *during* the fetch and
//! converts both into [`TokenizerError::GatedAccessUnavailable`] — never a hang, never a silent
//! fallback to a different model's assets.

use std::time::Duration;

use serde_json::Value;

use crate::{ModelSpec, TokenizerError};

/// The fetched tokenizer assets for one model: the HF `tokenizers::Tokenizer`, the raw
/// `tokenizer_config.json` / `special_tokens_map.json` values, and the resolved chat-template
/// string (if any), per D-05/D-06.
pub struct ModelAssets {
    pub tokenizer: tokenizers::Tokenizer,
    pub tokenizer_config: Value,
    pub special_tokens_map: Value,
    pub chat_template: Option<String>,
}

/// Bounded timeout applied to every HTTP request made while fetching a *gated* model's assets
/// (D-04 / T-04-07). An invalid-but-present token still reaches the Hub and gets a fast
/// 401/403 (neither status is in hf-hub's own retryable set, confirmed by reading
/// `hf-hub-1.0.0/src/retry.rs::is_transient_status` directly), but a genuinely unreachable
/// network (DNS hang, half-open TCP connect) has no such fast failure without a client-level
/// timeout. Ungated fetches (Qwen3) keep hf-hub's own default (untimed) client, unchanged from
/// Plan 04-01.
const GATED_FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// Mirrors hf-hub's own token-resolution precedence (`HFClientBuilder::build`'s private
/// `resolve_token()`, confirmed by reading `hf-hub-1.0.0/src/client.rs` directly): `HF_TOKEN` env
/// var, then the file named by `HF_TOKEN_PATH`, then `$HF_HOME/token`. `hf_hub::hf_home()` is the
/// one piece of that resolution order the crate exposes publicly — the other env var names are
/// hf-hub's own well-known, documented constants (its crate-level docs list the exact same
/// table), so this function reads them by their literal names rather than through a
/// crate-private API.
///
/// Also honors `HF_HUB_DISABLE_IMPLICIT_TOKEN`, matching hf-hub's own escape hatch: if set and
/// non-empty, no ambient credential counts, even if a token file exists on disk.
fn has_gated_credentials() -> bool {
    if std::env::var("HF_HUB_DISABLE_IMPLICIT_TOKEN").is_ok_and(|v| !v.is_empty()) {
        return false;
    }
    if std::env::var("HF_TOKEN").is_ok_and(|v| !v.is_empty()) {
        return true;
    }
    if let Ok(path) = std::env::var("HF_TOKEN_PATH")
        && std::fs::read_to_string(path).is_ok_and(|s| !s.trim().is_empty())
    {
        return true;
    }
    std::fs::read_to_string(hf_hub::hf_home().join("token")).is_ok_and(|s| !s.trim().is_empty())
}

/// `true` for the two hf-hub error variants an invalid/rejected token produces: the Hub either
/// requires authentication it didn't get (`401`, `AuthRequired`) or recognizes the credential but
/// denies the operation (`403`, `Forbidden` — the shape a gated repo returns to an authenticated
/// user who hasn't accepted the license).
fn is_auth_error(err: &hf_hub::HFError) -> bool {
    matches!(
        err,
        hf_hub::HFError::AuthRequired { .. } | hf_hub::HFError::Forbidden { .. }
    )
}

/// Converts an auth-shaped `hf_hub::HFError` on a *gated* model into
/// `TokenizerError::GatedAccessUnavailable`, leaving every other error (including auth errors on
/// an ungated model, which should never happen but must not be silently swallowed) to flow
/// through as the ordinary `TokenizerError::HfHub` conversion.
fn gated_or<T>(spec: ModelSpec, result: Result<T, hf_hub::HFError>) -> Result<T, TokenizerError> {
    match result {
        Ok(v) => Ok(v),
        Err(e) if spec.gated && is_auth_error(&e) => Err(TokenizerError::GatedAccessUnavailable {
            slug: spec.slug,
            detail: e.to_string(),
        }),
        Err(e) => Err(e.into()),
    }
}

/// Builds the blocking hf-hub client used to fetch `spec`'s assets. Gated models get a
/// `reqwest::Client` with [`GATED_FETCH_TIMEOUT`] wired in (`HFClientBuilder::client`, the
/// configuration surface hf-hub exposes for this — there is no separate per-request timeout
/// knob); ungated models keep hf-hub's own default, untimed client, unchanged from Plan 04-01.
fn build_client(spec: ModelSpec) -> Result<hf_hub::HFClientSync, TokenizerError> {
    if !spec.gated {
        return Ok(hf_hub::HFClientSync::new()?);
    }
    let http_client = reqwest::Client::builder()
        .timeout(GATED_FETCH_TIMEOUT)
        .build()
        .map_err(hf_hub::HFError::from)?;
    Ok(hf_hub::HFClientBuilder::new()
        .client(http_client)
        .build_sync()?)
}

/// Fetches and loads every asset for `spec` via hf-hub's blocking API.
///
/// `spec.repo_id` always comes from a hardcoded [`ModelSpec`] constant, never external or
/// request-controlled input — a fetch failure for the pinned repo id is a hard
/// [`TokenizerError`], never a masked success (no fallback to a cached/default repo).
///
/// For a gated `spec` (D-04): if no `HF_TOKEN`/cached credential is present at all, this returns
/// `TokenizerError::GatedAccessUnavailable` immediately, without attempting any network call. If
/// a credential is present but the Hub rejects it (401/403 — an invalid token, or a valid token
/// that hasn't accepted the gated repo's license), the same error variant is returned instead of
/// letting the underlying `hf_hub::HFError` surface as a generic fetch failure — both paths are
/// distinguishable only by this one error variant, so callers (Plan 04-06's tests) can match on
/// it to skip cleanly rather than failing the build.
pub fn load_model_assets(spec: ModelSpec) -> Result<ModelAssets, TokenizerError> {
    if spec.gated && !has_gated_credentials() {
        return Err(TokenizerError::GatedAccessUnavailable {
            slug: spec.slug,
            detail: "no HF_TOKEN and no cached credentials".to_string(),
        });
    }

    let (owner, name) = spec.repo_id.split_once('/').unwrap_or_else(|| {
        panic!(
            "ModelSpec {:?}: repo_id {:?} has no '/'",
            spec.slug, spec.repo_id
        )
    });

    let client = build_client(spec)?;
    let repo = client.model(owner, name);

    let tokenizer_json_path =
        gated_or(spec, repo.download_file().filename("tokenizer.json").send())?;
    let tokenizer_config_path = gated_or(
        spec,
        repo.download_file()
            .filename("tokenizer_config.json")
            .send(),
    )?;

    let tokenizer_config: Value =
        serde_json::from_str(&std::fs::read_to_string(&tokenizer_config_path)?)?;

    // `special_tokens_map.json` is not present in every repo (e.g. Qwen3-0.6B has none — its
    // special tokens live entirely in tokenizer_config.json); `AutoTokenizer.from_pretrained`
    // handles its absence gracefully, so this fetch is best-effort, like the chat_template
    // fallback below, not a hard requirement.
    let special_tokens_map: Value = repo
        .download_file()
        .filename("special_tokens_map.json")
        .send()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);

    let chat_template = match tokenizer_config.get("chat_template") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => repo
            .download_file()
            .filename("chat_template.json")
            .send()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|v| {
                v.get("chat_template")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
    };

    let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_json_path)?;

    Ok(ModelAssets {
        tokenizer,
        tokenizer_config,
        special_tokens_map,
        chat_template,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::Instant;

    use super::*;
    use crate::LLAMA_3_2_1B_INSTRUCT;

    /// Serializes every [`EnvGuard`] across this test binary's threads (07-10: `cargo test
    /// --workspace` runs every `#[test]` fn in this module concurrently by default, and two
    /// `EnvGuard`s mutating the same process-global `HF_*` env vars at once -- e.g.
    /// `gated_access_unavailable_with_blank_token_file` and
    /// `gated_access_unavailable_when_implicit_token_disabled` -- can interleave their
    /// mutate/restore cycles, intermittently making one see the other's env state and get `Ok(_)`
    /// instead of the expected `GatedAccessUnavailable`. A `Mutex` held for the `EnvGuard`'s own
    /// lifetime, not a one-off lock, is the fix: it serializes the whole save-mutate-run-restore
    /// cycle, not just the mutation. Poisoning (a prior guard's holder panicked mid-test) is
    /// recovered from via `into_inner`: the lock here only ever protects mutual exclusion, never
    /// data integrity, so a poisoned `()` is as good as an unpoisoned one.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Isolates every env var `has_gated_credentials`/hf-hub's own token resolution reads, so a
    /// test can assert "no credentials" behavior even on the author's own Mac, where a real
    /// cached token lives at `~/.cache/huggingface/token`. `HF_HOME` is pointed at a fresh empty
    /// tempdir (no `token` file inside) for the duration of the guard; every touched var is
    /// restored on drop, following the same pattern hf-hub's own
    /// `client::tests::token_precedence::EnvGuard` uses.
    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
        _hf_home: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn new() -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let hf_home = tempfile::tempdir().expect("tempdir for HF_HOME");
            let keys = [
                "HF_TOKEN",
                "HF_TOKEN_PATH",
                "HF_HOME",
                "HF_HUB_DISABLE_IMPLICIT_TOKEN",
            ];
            let saved = keys.iter().map(|k| (*k, std::env::var(*k).ok())).collect();
            for k in keys {
                unsafe { std::env::remove_var(k) };
            }
            unsafe { std::env::set_var("HF_HOME", hf_home.path()) };
            Self {
                saved,
                _hf_home: hf_home,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => unsafe { std::env::set_var(k, val) },
                    None => unsafe { std::env::remove_var(k) },
                }
            }
        }
    }

    /// Asserts `result` is `Err(TokenizerError::GatedAccessUnavailable { .. })` without requiring
    /// `ModelAssets: Debug` (it holds a `tokenizers::Tokenizer`, which doesn't implement it) --
    /// `{:?}`-formatting the whole `Result` would need that on the `Ok` side even when the
    /// actual value is `Err`.
    fn assert_gated_unavailable(result: Result<ModelAssets, TokenizerError>) {
        match result {
            Err(TokenizerError::GatedAccessUnavailable { .. }) => {}
            Err(other) => panic!("expected GatedAccessUnavailable, got other error: {other}"),
            Ok(_) => panic!("expected GatedAccessUnavailable, got Ok(_)"),
        }
    }

    /// D-04's clean-skip path, exercised directly: with every credential source cleared (not
    /// just `HF_TOKEN` — `HF_HOME` is redirected to a token-file-free tempdir too, since the
    /// author's real machine has a cached token this test must not see), `load_model_assets`
    /// must return `GatedAccessUnavailable` without ever reaching the network, and therefore
    /// well under any plausible network timeout. Run with `--test-threads=1`: `EnvGuard` mutates
    /// process-global env vars, so a concurrently-running test could observe a torn-down state.
    #[test]
    fn gated_access_unavailable_without_token() {
        let _guard = EnvGuard::new();

        let start = Instant::now();
        let result = load_model_assets(LLAMA_3_2_1B_INSTRUCT);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(5),
            "load_model_assets should fail fast with no credentials, took {elapsed:?}"
        );
        match &result {
            Err(TokenizerError::GatedAccessUnavailable { slug, .. }) => {
                assert_eq!(*slug, "llama-3.2-1b-instruct");
            }
            Err(other) => panic!("expected GatedAccessUnavailable, got other error: {other}"),
            Ok(_) => panic!("expected GatedAccessUnavailable, got Ok(_)"),
        }
    }

    /// `HF_TOKEN_PATH` pointing at a file containing only whitespace must count as "no
    /// credential", matching `has_gated_credentials`'s `.trim().is_empty()` check (and hf-hub's
    /// own `resolve_token`, which applies the same trim before testing emptiness).
    #[test]
    fn gated_access_unavailable_with_blank_token_file() {
        let _guard = EnvGuard::new();
        let dir = tempfile::tempdir().expect("tempdir for token file");
        let path = dir.path().join("token");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(b"   \n")
            .unwrap();
        unsafe { std::env::set_var("HF_TOKEN_PATH", &path) };

        let result = load_model_assets(LLAMA_3_2_1B_INSTRUCT);
        assert_gated_unavailable(result);
    }

    /// `HF_HUB_DISABLE_IMPLICIT_TOKEN` must suppress a real, present `HF_TOKEN` — mirrors
    /// hf-hub's own `resolve_token` short-circuit exactly (confirmed in `client.rs`).
    #[test]
    fn gated_access_unavailable_when_implicit_token_disabled() {
        let _guard = EnvGuard::new();
        unsafe { std::env::set_var("HF_TOKEN", "some-token-value") };
        unsafe { std::env::set_var("HF_HUB_DISABLE_IMPLICIT_TOKEN", "1") };

        let result = load_model_assets(LLAMA_3_2_1B_INSTRUCT);
        assert_gated_unavailable(result);
    }

    /// One-time canonical spot-check (Plan 04-04 Task 2 / RESEARCH.md Open Question 2): fetches
    /// `meta-llama/Llama-3.2-1B-Instruct`'s real `tokenizer_config.json` using the author's local
    /// gated credentials and confirms the four facts RESEARCH.md assumed from a third-party
    /// mirror (`unsloth/Llama-3.2-1B-Instruct`) against the canonical repo: `chat_template`
    /// contains `{{- bos_token }}` and a `strftime_now(` call, `clean_up_tokenization_spaces` is
    /// `true`, and `add_bos_token`'s actual value (see below -- it diverges from the mirror).
    ///
    /// **Live run on 2026-10-06 (human-granted gated access), findings recorded in
    /// `04-04-SUMMARY.md` per this plan's own instructions:** three of the four facts matched
    /// RESEARCH.md's mirror-sourced assumptions exactly. The fourth, `add_bos_token`, diverged:
    /// the canonical repo's `tokenizer_config.json` does **not** set this key at all (`None`, not
    /// `Some(true)` as the `unsloth` mirror explicitly set it) -- confirmed by a direct read of
    /// the cached canonical file. This does not weaken Pitfall 2's double-BOS risk: the canonical
    /// `tokenizer.json`'s own `post_processor` (a `Sequence` containing a `TemplateProcessing`
    /// step that unconditionally prepends the `<|begin_of_text|>` special token to both `single`
    /// and `pair` encodings) drives BOS insertion directly, independent of the
    /// `tokenizer_config.json`-level `add_bos_token` flag entirely -- the Rust `tokenizers` crate
    /// applies this same post-processor automatically via `Tokenizer::encode(_, true)`, so the
    /// double-BOS mechanism Pitfall 2 describes is confirmed real via a different file than
    /// RESEARCH.md assumed, not refuted. Plan 04-05/04-06 should rely on the post-processor
    /// mechanism (and this test's own `bos_count_via_post_processor_...` check below), not on
    /// `add_bos_token`'s presence in `tokenizer_config.json`, which this live fetch proved absent.
    ///
    /// `#[ignore]`: this is a one-time human-run verification, not a continuously-run parity
    /// test (Plan 04-06 owns the real TOK-04 parity suite, which follows D-04's skip-clean
    /// pattern for every run). Run manually with:
    /// `cargo test -p rsg-tokenizer --lib -- --ignored canonical_llama_config_spot_check`
    #[test]
    #[ignore = "network: requires a local HF_TOKEN with accepted-license access to the gated \
                meta-llama/Llama-3.2-1B-Instruct repo; run manually to reverify RESEARCH.md's \
                mirror-sourced assumptions (Open Question 2)"]
    fn canonical_llama_config_spot_check() {
        let assets = load_model_assets(LLAMA_3_2_1B_INSTRUCT)
            .expect("expected a successful fetch with HF_TOKEN configured per D-02");

        let chat_template = assets
            .chat_template
            .as_deref()
            .expect("expected tokenizer_config.json to have a non-empty chat_template");
        assert!(
            chat_template.contains("{{- bos_token }}"),
            "canonical chat_template does not manually prepend bos_token as RESEARCH.md's \
             mirror-sourced Pitfall 2 assumed"
        );
        assert!(
            chat_template.contains("strftime_now("),
            "canonical chat_template does not call strftime_now as RESEARCH.md's mirror-sourced \
             Pitfall 3 assumed"
        );

        // Diverges from RESEARCH.md's mirror-sourced assumption (`Some(true)`, explicitly set by
        // the `unsloth` mirror): the canonical `tokenizer_config.json` does not set this key at
        // all. Recorded as an authoritative correction in 04-04-SUMMARY.md -- see this test's own
        // doc comment above for why the double-BOS mechanism still holds regardless.
        let add_bos_token = assets
            .tokenizer_config
            .get("add_bos_token")
            .and_then(Value::as_bool);
        assert_eq!(
            add_bos_token, None,
            "canonical tokenizer_config.json now sets add_bos_token (expected it to be absent, \
             per the live 2026-10-06 verification) -- if this changed, re-check whether \
             Plan 04-05/04-06's BOS-handling assumptions (driven by tokenizer.json's \
             post_processor, not this field) still hold"
        );

        let clean_up = assets
            .tokenizer_config
            .get("clean_up_tokenization_spaces")
            .and_then(Value::as_bool);
        assert_eq!(
            clean_up,
            Some(true),
            "canonical clean_up_tokenization_spaces differs from RESEARCH.md's mirror-sourced \
             assumption"
        );
    }

    /// Confirms Pitfall 2's double-BOS mechanism directly against the live canonical tokenizer,
    /// independent of the `add_bos_token` divergence found above: `Tokenizer::encode(_, true)`
    /// (the HF default `add_special_tokens=true` upstream's `tokenize.py` always uses) must
    /// already prepend exactly one BOS token via `tokenizer.json`'s own post-processor, for a
    /// prompt that does *not* itself contain BOS text. Combined with the chat template's own
    /// `{{- bos_token }}` prepend (confirmed in `canonical_llama_config_spot_check`), rendering
    /// through the full chat-template-then-encode pipeline the way Plan 04-05 will is therefore
    /// expected to double up BOS -- precisely the case D-10's "assert BOS appears exactly once"
    /// test exists to catch.
    #[test]
    #[ignore = "network: requires a local HF_TOKEN with accepted-license access to the gated \
                meta-llama/Llama-3.2-1B-Instruct repo; run manually alongside \
                canonical_llama_config_spot_check"]
    fn canonical_llama_post_processor_prepends_bos_once() {
        let assets = load_model_assets(LLAMA_3_2_1B_INSTRUCT)
            .expect("expected a successful fetch with HF_TOKEN configured per D-02");

        let bos_id = assets
            .tokenizer
            .token_to_id("<|begin_of_text|>")
            .expect("expected <|begin_of_text|> to be a known special token");

        let encoding = assets
            .tokenizer
            .encode("hello", true)
            .expect("expected encode(_, add_special_tokens=true) to succeed");
        let ids = encoding.get_ids();

        let bos_count = ids.iter().filter(|&&id| id == bos_id).count();
        assert_eq!(
            bos_count, 1,
            "expected tokenizer.json's post-processor to prepend exactly one BOS token via \
             add_special_tokens=true, got {bos_count} in {ids:?}"
        );
        assert_eq!(
            ids[0], bos_id,
            "expected the post-processor-inserted BOS token to be first, got {ids:?}"
        );
    }
}
