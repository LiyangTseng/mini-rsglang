//! Chat-template rendering: a minijinja `Environment` with the two HF chat-template globals
//! (`raise_exception`, `strftime_now`) that `minijinja-contrib` does not supply, plus
//! `render_chat`, which renders a model's chat template fresh on every call — never cached.
//!
//! Mirrors `tokenize.py`'s `apply_chat_template(msg.text, tokenize=False,
//! add_generation_prompt=True)` call exactly: no `tools`, no `date_string` kwarg.

use serde::Serialize;

use crate::TokenizerError;

/// Builds a minijinja `Environment` with `minijinja-contrib`'s pycompat support wired in, plus
/// the two HF chat-template globals neither `minijinja` nor `minijinja-contrib` provide:
/// `raise_exception` (a template-authored guard clause) and `strftime_now` (today's date).
///
/// `minijinja_contrib::add_to_environment` registers filters/globals but intentionally does NOT
/// register pycompat method support (its own doc comment says so), so
/// `set_unknown_method_callback` must be wired separately.
///
/// `now_override` controls `strftime_now`'s clock (RESEARCH.md Open Question 1, resolved on this
/// plan): `None` calls the real clock (`chrono::Local::now()`), exactly matching Python's
/// `datetime.now().strftime(fmt)` for Llama's chat template — this is the only value production
/// code may pass. `Some(fixed)` makes `strftime_now` return `fixed` verbatim, ignoring its `fmt`
/// argument entirely (sufficient because the one template in scope — Llama's — only ever calls
/// it with `"%d %b %Y"`); this is reserved for fixture-generation and test code paths, so a
/// committed Llama chat-template fixture stays byte-exact across calendar days instead of
/// embedding whatever day it happened to be generated on.
pub fn build_environment(now_override: Option<String>) -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);

    env.add_function("raise_exception", raise_exception);

    // `transformers`' own chat-template Jinja environment overrides Jinja2's built-in `tojson`
    // filter with `json.dumps(x, ensure_ascii=False, indent=None, separators=None,
    // sort_keys=False)` (confirmed directly from `transformers/utils/chat_template_utils.py`).
    // Python's `json.dumps` default separators (when `indent` is `None`) are `(', ', ': ')` --
    // a space after every comma and colon. minijinja's own built-in `tojson` filter instead
    // calls `serde_json::to_string`, which is fully compact (no separator spaces) unless an
    // `indent` argument is passed. Qwen3's template calls `tojson` with no `indent` argument
    // (`tool | tojson`, `tool_call.arguments | tojson`), so the two filters disagree on
    // whitespace for every rendered JSON value. Overriding the registration below (inserted
    // after `Environment::new()`'s own default `tojson`, so this replaces it) matches the real
    // oracle's separators exactly.
    env.add_filter("tojson", tojson);

    // Plan 04-04 Task 2: `strftime_now` now calls the real clock in production
    // (`now_override: None`), with a test/fixture-generation-only override that returns a fixed
    // string regardless of `fmt` -- never the default, per this plan's
    // `must_haves.prohibitions` ("no silent default clock-freeze").
    env.add_function("strftime_now", move |fmt: String| -> String {
        match &now_override {
            Some(fixed) => fixed.clone(),
            None => chrono::Local::now().format(&fmt).to_string(),
        }
    });

    env
}

/// The `raise_exception` HF chat-template global: a template-authored guard clause that must
/// surface as `Err(TokenizerError::Template(...))` rather than panicking.
fn raise_exception(msg: String) -> Result<minijinja::Value, minijinja::Error> {
    Err(minijinja::Error::new(
        minijinja::ErrorKind::InvalidOperation,
        msg,
    ))
}

/// A `serde_json::ser::Formatter` matching Python `json.dumps`'s default separators (a space
/// after every `,` and `:`) instead of `serde_json`'s fully compact defaults. All other
/// `Formatter` methods (object/array braces, string/number encoding) use the trait's defaults,
/// which already match `json.dumps`'s non-pretty-printed output exactly.
struct PySeparatorsFormatter;

impl serde_json::ser::Formatter for PySeparatorsFormatter {
    fn begin_array_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
}

/// `tojson` filter override matching the real oracle's behavior: `transformers`' own
/// chat-template Jinja environment replaces Jinja2's built-in `tojson` with
/// `json.dumps(x, ensure_ascii=False, indent=None, separators=None, sort_keys=False)`
/// (`transformers/utils/chat_template_utils.py`), NOT minijinja's own default `tojson` filter
/// (`serde_json::to_string`, fully compact with no separator spaces, plus an HTML-escaping
/// post-process step Python's override never applies).
fn tojson(value: minijinja::Value) -> Result<minijinja::Value, minijinja::Error> {
    let mut out = Vec::<u8>::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PySeparatorsFormatter);
    value.serialize(&mut ser).map_err(|err| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "cannot serialize to JSON",
        )
        .with_source(err)
    })?;
    let s = String::from_utf8(out).map_err(|err| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "tojson produced invalid utf-8",
        )
        .with_source(err)
    })?;
    Ok(minijinja::Value::from_safe_string(s))
}

/// Renders `template_src` against `messages`, matching `tokenize.py`'s
/// `apply_chat_template(msg.text, tokenize=False, add_generation_prompt=True)` call exactly: no
/// `tools` kwarg, no `date_string` kwarg, ever passed.
///
/// Uses [`minijinja::Environment::render_str`], which parses and renders the template in one
/// step without storing it on `env` — `env` is taken by shared reference and never mutated
/// here, so every call renders fresh from the current `messages` value. This matters because a
/// later model's template (Llama, Plan 04-04) embeds the render-time clock via `strftime_now`;
/// a cached render would silently serve a stale date instead of surfacing real behavior (see
/// this plan's `must_haves.prohibitions`).
pub fn render_chat(
    env: &minijinja::Environment<'static>,
    template_src: &str,
    messages: &serde_json::Value,
    bos_token: Option<&str>,
    eos_token: Option<&str>,
) -> Result<String, TokenizerError> {
    let rendered = env.render_str(
        template_src,
        minijinja::context! {
            messages => messages,
            add_generation_prompt => true,
            bos_token => bos_token,
            eos_token => eos_token,
        },
    )?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_exception_in_template_returns_templated_error_with_message() {
        let env = build_environment(None);
        let result = render_chat(
            &env,
            "{{ raise_exception('boom') }}",
            &serde_json::json!([]),
            None,
            None,
        );
        let err = result.expect_err("expected an Err from a raise_exception call, not a panic");
        match err {
            TokenizerError::Template(e) => {
                assert!(
                    e.to_string().contains("boom"),
                    "error message should mention 'boom', got: {e}"
                );
            }
            other => panic!("expected TokenizerError::Template, got {other:?}"),
        }
    }

    /// `now_override: Some(fixed)` must win regardless of the template's own `fmt` argument --
    /// this is the knob fixture generation and the Rust test harness use to keep a committed
    /// Llama chat-template fixture byte-exact across calendar days (RESEARCH.md Open Question 1,
    /// must_haves.truths on this plan).
    #[test]
    fn strftime_now_override_returns_fixed_string_regardless_of_format() {
        let env = build_environment(Some("06 Oct 2026".to_string()));
        let rendered = render_chat(
            &env,
            "{{ strftime_now('%d %b %Y') }}",
            &serde_json::json!([]),
            None,
            None,
        )
        .expect("strftime_now override must not error");
        assert_eq!(rendered, "06 Oct 2026");

        // Also prove the override ignores `fmt` entirely, not just coincidentally matching it.
        let env2 = build_environment(Some("06 Oct 2026".to_string()));
        let rendered2 = render_chat(
            &env2,
            "{{ strftime_now('%Y-%m-%d') }}",
            &serde_json::json!([]),
            None,
            None,
        )
        .expect("strftime_now override must not error");
        assert_eq!(rendered2, "06 Oct 2026");
    }

    /// `now_override: None` must call the real clock -- asserted structurally (output changes
    /// between two distinct frozen `now_override` values and differs from either when `None`
    /// is used), without asserting an exact wall-clock string (which would make this test
    /// flaky/date-sensitive, exactly the problem `now_override` exists to avoid in fixtures).
    #[test]
    fn strftime_now_without_override_calls_the_real_clock() {
        let env = build_environment(None);
        let rendered = render_chat(
            &env,
            "{{ strftime_now('%Y') }}",
            &serde_json::json!([]),
            None,
            None,
        )
        .expect("strftime_now (real clock) must not error");
        // Sanity: a real year string is 4 ASCII digits, never the frozen fixture string, and
        // never empty.
        assert_eq!(
            rendered.len(),
            4,
            "expected a 4-digit year, got {rendered:?}"
        );
        assert!(
            rendered.chars().all(|c| c.is_ascii_digit()),
            "expected a 4-digit year, got {rendered:?}"
        );
    }

    /// RESEARCH.md Assumptions Log A3, spot-checked once (not assumed indefinitely):
    /// `chrono::Local::now().format("%d %b %Y")` must byte-match Python's
    /// `datetime(...).strftime("%d %b %Y")` for the same calendar date, across several months so
    /// a month-abbreviation mismatch (locale, capitalization, etc.) would be caught. Expected
    /// strings below were computed once via `python3 -c "from datetime import date; print(date(Y,
    /// M, D).strftime('%d %b %Y'))"` (English/C locale, matching Python's default) and hardcoded
    /// here -- this test never invokes Python itself.
    #[test]
    fn chrono_strftime_matches_python_format() {
        let cases: &[(chrono::NaiveDate, &str)] = &[
            // python3 -c "from datetime import date; print(date(2026, 1, 5).strftime('%d %b %Y'))"
            // -> "05 Jan 2026"
            (
                chrono::NaiveDate::from_ymd_opt(2026, 1, 5).expect("valid date"),
                "05 Jan 2026",
            ),
            // python3 -c "from datetime import date; print(date(2026, 7, 31).strftime('%d %b %Y'))"
            // -> "31 Jul 2026"
            (
                chrono::NaiveDate::from_ymd_opt(2026, 7, 31).expect("valid date"),
                "31 Jul 2026",
            ),
            // python3 -c "from datetime import date; print(date(2026, 12, 25).strftime('%d %b %Y'))"
            // -> "25 Dec 2026"
            (
                chrono::NaiveDate::from_ymd_opt(2026, 12, 25).expect("valid date"),
                "25 Dec 2026",
            ),
        ];

        for (date, expected) in cases {
            let formatted = date.format("%d %b %Y").to_string();
            assert_eq!(
                &formatted, expected,
                "chrono's \"%d %b %Y\" formatting of {date:?} diverges from Python's strftime"
            );
        }
    }
}
