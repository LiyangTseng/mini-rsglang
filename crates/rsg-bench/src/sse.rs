//! Hand-written `data: ` line splitter.
//!
//! CLAUDE.md rules out `eventsource-stream` (unmaintained since 2022) and
//! notes that `/generate`'s own framing is a single `\n`, not spec SSE — a
//! generic SSE crate would mis-parse it anyway. This splitter buffers bytes
//! across chunk boundaries, strips one trailing `\r`, and only ever looks at
//! lines that start with `data: `; everything else (blank lines, `:`
//! comments, other fields) is ignored.

/// One parsed `data: ` payload, or the terminal `[DONE]` sentinel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseData {
    Payload(String),
    Done,
}

/// Buffers a byte stream across chunk boundaries and yields complete
/// `data: ` lines as they complete.
#[derive(Debug, Default)]
pub struct SseLineSplitter {
    buf: Vec<u8>,
}

impl SseLineSplitter {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Appends `chunk`, splits on `\n`, and returns every complete `data: `
    /// line found in this call plus any carried over from a previous call.
    /// An incomplete trailing line stays buffered for the next `push` or for
    /// `finish`.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseData> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop(); // drop the trailing '\n' itself
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(data) = Self::parse_line(&line) {
                out.push(data);
            }
        }
        out
    }

    /// Processes a final line with no trailing newline, if any bytes remain
    /// buffered.
    pub fn finish(mut self) -> Option<SseData> {
        if self.buf.last() == Some(&b'\r') {
            self.buf.pop();
        }
        Self::parse_line(&self.buf)
    }

    fn parse_line(line: &[u8]) -> Option<SseData> {
        const PREFIX: &[u8] = b"data: ";
        if !line.starts_with(PREFIX) {
            return None;
        }
        let rest = &line[PREFIX.len()..];
        if rest == b"[DONE]" {
            return Some(SseData::Done);
        }
        Some(SseData::Payload(String::from_utf8_lossy(rest).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_push_yields_payload_then_done() {
        let mut s = SseLineSplitter::new();
        let out = s.push(b"data: hello\n\ndata: [DONE]\n");
        assert_eq!(
            out,
            vec![SseData::Payload("hello".to_string()), SseData::Done]
        );
    }

    #[test]
    fn crlf_is_stripped() {
        let mut s = SseLineSplitter::new();
        let out = s.push(b"data: hi\r\n");
        assert_eq!(out, vec![SseData::Payload("hi".to_string())]);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let mut s = SseLineSplitter::new();
        let out = s.push(b": keep-alive\n\ndata: ok\n");
        assert_eq!(out, vec![SseData::Payload("ok".to_string())]);
    }

    #[test]
    fn finish_processes_an_unterminated_trailing_line() {
        let mut s = SseLineSplitter::new();
        let mut out = s.push(b"data: a\n");
        // No trailing '\n' on this last line.
        out.extend(s.push(b"data: [DONE]"));
        assert_eq!(out, vec![SseData::Payload("a".to_string())]);
        assert_eq!(s.finish(), Some(SseData::Done));
    }
}

#[cfg(test)]
mod chunk_boundary_proptests {
    use super::*;
    use proptest::prelude::*;

    fn payload_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 _-]{0,24}"
    }

    proptest! {
        /// However the byte stream is cut into chunks, and whether lines end
        /// in CRLF or LF, the yielded sequence is exactly the payload list
        /// followed by `Done`.
        #[test]
        fn yields_payloads_then_done_regardless_of_chunk_boundaries(
            payloads in prop::collection::vec(payload_strategy(), 0..8),
            use_crlf in any::<bool>(),
            cut_points in prop::collection::vec(0usize..256, 0..24),
        ) {
            let newline = if use_crlf { "\r\n" } else { "\n" };
            let mut bytes = Vec::new();
            for p in &payloads {
                bytes.extend_from_slice(format!("data: {p}{newline}{newline}").as_bytes());
            }
            bytes.extend_from_slice(format!("data: [DONE]{newline}").as_bytes());

            let mut cuts: Vec<usize> = cut_points
                .into_iter()
                .map(|c| c % (bytes.len() + 1))
                .collect();
            cuts.sort_unstable();
            cuts.dedup();

            let mut splitter = SseLineSplitter::new();
            let mut out = Vec::new();
            let mut start = 0;
            for cut in cuts {
                if cut > start {
                    out.extend(splitter.push(&bytes[start..cut]));
                    start = cut;
                }
            }
            out.extend(splitter.push(&bytes[start..]));
            if let Some(last) = splitter.finish() {
                out.push(last);
            }

            let mut expected: Vec<SseData> = payloads.into_iter().map(SseData::Payload).collect();
            expected.push(SseData::Done);
            prop_assert_eq!(out, expected);
        }
    }
}
