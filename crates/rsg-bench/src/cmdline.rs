//! Template splitting and `{name}` substitution for launch command lines
//! (T-07-14): templates are split into argv tokens *before* any
//! substitution happens, so a substituted value can never inject a new
//! token or shell syntax -- there is no shell involved at all.

use std::collections::BTreeMap;

/// Splits `s` on whitespace into argv tokens.
///
/// Single-quoted segments are literal (no escape processing inside).
/// Double-quoted segments allow `\"` and `\\` escapes. A backslash outside
/// any quote escapes the next character. Quoted and unquoted text can be
/// adjacent within the same token (e.g. `--flag="a b"` stays one token).
/// An unterminated quote is an error.
pub fn split_template(s: &str) -> anyhow::Result<Vec<String>> {
    let chars: Vec<char> = s.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
            i += 1;
            continue;
        }
        in_token = true;
        match c {
            '\'' => {
                i += 1;
                loop {
                    match chars.get(i) {
                        None => anyhow::bail!("unterminated single quote in template: {s:?}"),
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            current.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '"' => {
                i += 1;
                loop {
                    match chars.get(i) {
                        None => anyhow::bail!("unterminated double quote in template: {s:?}"),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') if matches!(chars.get(i + 1), Some('"') | Some('\\')) => {
                            current.push(chars[i + 1]);
                            i += 2;
                        }
                        Some(&ch) => {
                            current.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '\\' => {
                i += 1;
                match chars.get(i) {
                    None => anyhow::bail!(
                        "trailing backslash with nothing to escape in template: {s:?}"
                    ),
                    Some(&ch) => {
                        current.push(ch);
                        i += 1;
                    }
                }
            }
            _ => {
                current.push(c);
                i += 1;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Splits `template` first ([`split_template`]), then replaces every
/// `{name}` placeholder inside each resulting token with `values[name]`.
/// `{{` and `}}` are literal braces. An unknown placeholder is an error
/// naming it.
pub fn render(template: &str, values: &BTreeMap<&str, String>) -> anyhow::Result<Vec<String>> {
    split_template(template)?
        .into_iter()
        .map(|tok| render_token(&tok, values))
        .collect()
}

fn render_token(token: &str, values: &BTreeMap<&str, String>) -> anyhow::Result<String> {
    let chars: Vec<char> = token.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                out.push('{');
                i += 2;
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                out.push('}');
                i += 2;
            }
            '{' => {
                let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '}') else {
                    anyhow::bail!("unterminated placeholder in template token {token:?}");
                };
                let name: String = chars[i + 1..i + 1 + rel_end].iter().collect();
                let value = values.get(name.as_str()).ok_or_else(|| {
                    anyhow::anyhow!("unknown placeholder {{{name}}} in template token {token:?}")
                })?;
                out.push_str(value);
                i += rel_end + 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Fixed, case-insensitive substrings that mark a flag/env name as
/// secret-suggestive (T-07-12). No regex crate.
const SECRET_SUBSTRINGS: [&str; 8] = [
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "auth",
    "credential",
    "cookie",
];

/// Whether `name` contains (case-insensitively) any [`SECRET_SUBSTRINGS`]
/// entry.
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    SECRET_SUBSTRINGS.iter().any(|s| lower.contains(s))
}

/// Redacts secret-looking values from an argv list (T-07-12):
/// - the value token immediately after a `--flag`/`-f` whose name matches
///   [`is_secret_name`];
/// - the value half of a `--flag=value` (or `-f=value`) token whose flag
///   name matches;
/// - the value half of a bare `NAME=VALUE` token whose name matches.
///
/// A token that itself contains internal whitespace is treated as its
/// own mini-argv and redacted recursively: the harness's own argv
/// (`std::env::args()`) sees a user's quoted `--python-cmd "<cmd> --api-key
/// ..."` value as *one* opaque token (a shell delivers it that way), so
/// scanning only at top-level token boundaries would miss a secret
/// embedded inside it.
pub fn redact_argv(argv: &[String]) -> Vec<String> {
    redact_flat(argv)
        .into_iter()
        .map(|tok| {
            if tok.split_whitespace().count() > 1 {
                let pieces: Vec<String> = tok.split_whitespace().map(str::to_string).collect();
                redact_flat(&pieces).join(" ")
            } else {
                tok
            }
        })
        .collect()
}

/// The token-boundary-only redaction pass, applied both to a real argv
/// list and (by [`redact_argv`]) to the whitespace-split pieces of any
/// single blob token.
fn redact_flat(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut redact_next = false;
    for tok in tokens {
        if redact_next {
            out.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }

        let (prefix, flag_body) = if let Some(rest) = tok.strip_prefix("--") {
            ("--", rest)
        } else if let Some(rest) = tok.strip_prefix('-') {
            ("-", rest)
        } else {
            ("", tok.as_str())
        };

        if let Some((name, _)) = flag_body.split_once('=') {
            if is_secret_name(name) {
                out.push(format!("{prefix}{name}=<redacted>"));
                continue;
            }
            out.push(tok.clone());
            continue;
        }

        if !prefix.is_empty() && is_secret_name(flag_body) {
            out.push(tok.clone());
            redact_next = true;
            continue;
        }

        out.push(tok.clone());
    }
    out
}

/// Characters safe to leave an argv token unquoted for POSIX `sh`, mirroring
/// Python's `shlex.quote` (`_find_unsafe` treats `[^\w@%+=:,./-]` as unsafe,
/// where `\w` is letters/digits/underscore) (T-07-19).
const SHELL_SAFE_PUNCTUATION: &str = "@%+=:,./_-";

/// Quotes one token for POSIX `sh`, the equivalent of Python's
/// `shlex.quote` (T-07-19): the empty string becomes `''`; a token made
/// only of `[A-Za-z0-9@%+=:,./_-]` is returned unchanged; anything else is
/// wrapped in single quotes, with each embedded `'` replaced by `'\''`
/// (close the quote, an escaped literal quote, reopen).
pub fn shell_quote(token: &str) -> String {
    if token.is_empty() {
        return "''".to_string();
    }
    let is_safe = |c: char| c.is_ascii_alphanumeric() || SHELL_SAFE_PUNCTUATION.contains(c);
    if token.chars().all(is_safe) {
        return token.to_string();
    }
    let mut out = String::with_capacity(token.len() + 2);
    out.push('\'');
    for c in token.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Quotes every token ([`shell_quote`]) and joins them with a single space,
/// the equivalent of Python's `shlex.join` (T-07-19). `hyperfine`'s own
/// `--conclude <stop_cmd>`/timed `<once_cmd>` strings are run through *its*
/// shell, so building them this way -- never by plain string concatenation
/// -- means no harness-built value is ever reinterpreted as shell syntax.
pub fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| shell_quote(t))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(
            split_template("a b  c").unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn single_quotes_are_literal() {
        assert_eq!(
            split_template("'a b' c").unwrap(),
            vec!["a b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn double_quotes_allow_escapes() {
        assert_eq!(
            split_template(r#""a \"b\" c""#).unwrap(),
            vec!["a \"b\" c".to_string()]
        );
    }

    #[test]
    fn unterminated_quote_errors() {
        assert!(split_template("'a").is_err());
        assert!(split_template("\"a").is_err());
    }

    #[test]
    fn render_replaces_known_placeholders() {
        let mut values = BTreeMap::new();
        values.insert("port", "1919".to_string());
        let out = render("--port {port}", &values).unwrap();
        assert_eq!(out, vec!["--port".to_string(), "1919".to_string()]);
    }

    #[test]
    fn render_unknown_placeholder_errors() {
        let values = BTreeMap::new();
        let err = render("--port {port}", &values).unwrap_err();
        assert!(err.to_string().contains("port"));
    }

    #[test]
    fn render_literal_braces() {
        let values = BTreeMap::new();
        let out = render("{{literal}}", &values).unwrap();
        assert_eq!(out, vec!["{literal}".to_string()]);
    }

    #[test]
    fn is_secret_name_matches_case_insensitively() {
        assert!(is_secret_name("API-KEY"));
        assert!(is_secret_name("hf_token"));
        assert!(!is_secret_name("model"));
    }

    #[test]
    fn redact_argv_handles_flag_value_and_inline_forms() {
        let argv = vec![
            "--api-key".to_string(),
            "sekrit".to_string(),
            "HF_TOKEN=abc".to_string(),
            "--model".to_string(),
            "qwen".to_string(),
        ];
        let out = redact_argv(&argv);
        assert_eq!(
            out,
            vec![
                "--api-key".to_string(),
                "<redacted>".to_string(),
                "HF_TOKEN=<redacted>".to_string(),
                "--model".to_string(),
                "qwen".to_string(),
            ]
        );
    }
}
