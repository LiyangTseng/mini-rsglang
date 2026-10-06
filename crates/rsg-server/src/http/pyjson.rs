//! Python `json.dumps(ensure_ascii=True)` string escaping, and the
//! streaming chat-completions chunk builder that depends on it.
//!
//! `/v1/chat/completions`'s streaming responses must be byte-identical to
//! upstream's `f"data: {json.dumps(chunk)}\n\n".encode()`
//! (`vendor/mini-sglang/python/minisgl/server/api_server.py:160-188`).
//! `serde_json`'s compact encoder does not escape non-ASCII characters by
//! default (Starlette's own `JSONResponse` doesn't either, which is why the
//! non-streaming response in `chat.rs` uses plain `serde_json`/`axum::Json`
//! instead) — CPython's `json.dumps` with its default `ensure_ascii=True`
//! does, and every streaming chunk's JSON text must go through this module,
//! never through `serde_json`, to match.

/// Appends a quoted JSON string literal for `s` to `out`, exactly as
/// CPython's `json.dumps(s)` with `ensure_ascii=True` would encode it:
/// - `"` becomes `\"` and `\` becomes `\\`;
/// - U+000A, U+000D, U+0009, U+0008 and U+000C become `\n`, `\r`, `\t`,
///   `\b` and `\f`;
/// - every other char below U+0020 becomes `\u00XX` (lowercase hex);
/// - U+0020 to U+007E are copied literally, including `/`;
/// - U+007F and every other BMP char become `\uXXXX` (lowercase hex);
/// - chars above U+FFFF become a UTF-16 surrogate pair `\uXXXX\uXXXX`
///   (lowercase hex).
pub fn push_ascii_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        let cp = c as u32;
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            _ if cp < 0x20 => {
                out.push_str(&format!("\\u{cp:04x}"));
            }
            _ if (0x20..=0x7E).contains(&cp) => out.push(c),
            _ if cp <= 0xFFFF => {
                out.push_str(&format!("\\u{cp:04x}"));
            }
            _ => {
                let v = cp - 0x10000;
                let high = 0xD800 + (v >> 10);
                let low = 0xDC00 + (v & 0x3FF);
                out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
            }
        }
    }
    out.push('"');
}

/// Builds the JSON text (without the leading `data: ` prefix) of one
/// streaming `/v1/chat/completions` chunk, matching upstream's
/// `stream_chat_completions` exactly:
/// `{"id": "cmpl-<uid>", "object": "text_completion.chunk", "choices":
/// [{"delta": <DELTA>, "index": 0, "finish_reason": <null or "stop">}]}`.
///
/// `role` is `true` only for the very first chunk of a request. `content`
/// is `None` for an empty/absent increment (upstream's
/// `if ack.incremental_output:` is falsy for an empty string) — DELTA is
/// then `{}` (or `{"role": "assistant"}` if `role` is also set). `content`
/// is escaped with [`push_ascii_json_str`], never `serde_json`.
/// `finish_stop` set gives `finish_reason: "stop"`; otherwise `null`.
pub fn chat_stream_chunk(uid: i64, role: bool, content: Option<&str>, finish_stop: bool) -> String {
    let mut delta = String::from("{");
    let mut first = true;
    if role {
        delta.push_str("\"role\": \"assistant\"");
        first = false;
    }
    if let Some(c) = content {
        if !first {
            delta.push_str(", ");
        }
        delta.push_str("\"content\": ");
        push_ascii_json_str(&mut delta, c);
    }
    delta.push('}');

    let finish_reason = if finish_stop { "\"stop\"" } else { "null" };

    format!(
        "{{\"id\": \"cmpl-{uid}\", \"object\": \"text_completion.chunk\", \"choices\": [{{\"delta\": {delta}, \"index\": 0, \"finish_reason\": {finish_reason}}}]}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escaped(s: &str) -> String {
        let mut out = String::new();
        push_ascii_json_str(&mut out, s);
        out
    }

    #[test]
    fn escapes_like_python_json_dumps() {
        assert_eq!(escaped("é"), "\"\\u00e9\"");
        assert_eq!(escaped("😀"), "\"\\ud83d\\ude00\"");
        assert_eq!(escaped("a\nb"), "\"a\\nb\"");
        assert_eq!(escaped("\u{7f}"), "\"\\u007f\"");
        assert_eq!(escaped("/"), "\"/\"");
        assert_eq!(escaped("\0"), "\"\\u0000\"");
        assert_eq!(escaped("\u{8}\u{c}"), "\"\\b\\f\"");
        assert_eq!(escaped("\"\\"), "\"\\\"\\\\\"");
        assert_eq!(escaped("你"), "\"\\u4f60\"");
        assert_eq!(escaped("\r\t"), "\"\\r\\t\"");
        assert_eq!(escaped("\u{2028}"), "\"\\u2028\"");
    }

    #[test]
    fn chunk_shapes() {
        assert_eq!(
            chat_stream_chunk(3, true, None, false),
            r#"{"id": "cmpl-3", "object": "text_completion.chunk", "choices": [{"delta": {"role": "assistant"}, "index": 0, "finish_reason": null}]}"#
        );
        assert_eq!(
            chat_stream_chunk(3, false, None, true),
            r#"{"id": "cmpl-3", "object": "text_completion.chunk", "choices": [{"delta": {}, "index": 0, "finish_reason": "stop"}]}"#
        );
        assert_eq!(
            chat_stream_chunk(3, true, Some("hi"), false),
            r#"{"id": "cmpl-3", "object": "text_completion.chunk", "choices": [{"delta": {"role": "assistant", "content": "hi"}, "index": 0, "finish_reason": null}]}"#
        );
        assert_eq!(
            chat_stream_chunk(3, false, Some("é"), false),
            r#"{"id": "cmpl-3", "object": "text_completion.chunk", "choices": [{"delta": {"content": "\u00e9"}, "index": 0, "finish_reason": null}]}"#
        );
    }
}
