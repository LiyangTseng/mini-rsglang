//! A raw HTTP/1.1 test client on `tokio::net::TcpStream`, so tests see the
//! exact response bytes (framing, chunk boundaries) and can disconnect a
//! request mid-stream deliberately. Not a general-purpose HTTP client —
//! just enough of HTTP/1.1 (status line, headers, Content-Length or
//! chunked bodies) for this phase's tests.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// A fully-read HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// `true` only if the body was read to its proper end — the
    /// Content-Length byte count, or chunked's terminating zero-size chunk
    /// — before the connection closed or a read timed out.
    pub complete: bool,
}

impl HttpResponse {
    /// The first header value matching `name`, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Body-framing, parsed from the response headers.
#[derive(Clone, Copy, Debug)]
enum Framing {
    ContentLength(usize),
    Chunked,
    /// Neither header present: there is no in-band terminator, so
    /// completion can only ever be inferred from a clean EOF.
    None,
}

/// Writes one request line, headers and (if given) body onto `w`. Generic
/// over the writer half so [`send`] can write on a split-off write half
/// concurrently with reading the response — some tests send bodies large
/// enough that the server responds (and stops reading) before the whole
/// body has been written, and a sequential write-then-read would deadlock
/// waiting for TCP window space the server will never drain.
async fn write_request<W: AsyncWrite + Unpin>(
    w: &mut W,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> std::io::Result<()> {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(b) = body {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req.push_str("\r\n");
    w.write_all(req.as_bytes()).await?;
    if let Some(b) = body {
        w.write_all(b).await?;
    }
    w.flush().await?;
    Ok(())
}

/// Reads the status line and headers, consuming the trailing blank line.
async fn read_head<R: AsyncBufRead + Unpin>(
    r: &mut R,
) -> std::io::Result<(u16, Vec<(String, String)>, Framing)> {
    let mut status_line = String::new();
    r.read_line(&mut status_line).await?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut headers = Vec::new();
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).await?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().ok();
            }
            if k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked") {
                chunked = true;
            }
            headers.push((k, v));
        }
    }

    let framing = if chunked {
        Framing::Chunked
    } else if let Some(n) = content_length {
        Framing::ContentLength(n)
    } else {
        Framing::None
    };
    Ok((status, headers, framing))
}

/// Reads up to `buf.len()` bytes, retrying short reads, stopping at the
/// first `Ok(0)` (EOF) or error. Returns how many bytes were actually
/// filled — never panics or assumes success on a truncated stream.
async fn read_best_effort<R: AsyncRead + Unpin>(r: &mut R, buf: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]).await {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => break,
        }
    }
    filled
}

/// One decoded HTTP chunk, or how the chunked stream ended.
enum ChunkResult {
    Data(Vec<u8>),
    /// The terminating zero-size chunk (and any trailers) was read.
    End,
    /// The stream ended or was malformed before a proper terminator.
    Error,
}

async fn read_one_chunk<R: AsyncBufRead + Unpin>(r: &mut R) -> ChunkResult {
    let mut size_line = String::new();
    match r.read_line(&mut size_line).await {
        Ok(0) | Err(_) => return ChunkResult::Error,
        Ok(_) => {}
    }
    let size_str = size_line.trim().split(';').next().unwrap_or("");
    let size = match usize::from_str_radix(size_str, 16) {
        Ok(s) => s,
        Err(_) => return ChunkResult::Error,
    };
    if size == 0 {
        // Trailers, if any, until the blank line.
        loop {
            let mut trailer = String::new();
            match r.read_line(&mut trailer).await {
                Ok(0) | Err(_) => return ChunkResult::End,
                Ok(_) => {
                    if trailer == "\r\n" || trailer == "\n" {
                        return ChunkResult::End;
                    }
                }
            }
        }
    }
    let mut data = vec![0u8; size];
    if read_best_effort(r, &mut data).await < size {
        return ChunkResult::Error;
    }
    let mut crlf = [0u8; 2];
    let _ = read_best_effort(r, &mut crlf).await;
    ChunkResult::Data(data)
}

async fn read_chunked_to_end<R: AsyncBufRead + Unpin>(r: &mut R) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    loop {
        match read_one_chunk(r).await {
            ChunkResult::Data(d) => out.extend_from_slice(&d),
            ChunkResult::End => return (out, true),
            ChunkResult::Error => return (out, false),
        }
    }
}

async fn read_body_to_end<R: AsyncBufRead + Unpin>(r: &mut R, framing: Framing) -> (Vec<u8>, bool) {
    match framing {
        Framing::ContentLength(n) => {
            let mut buf = vec![0u8; n];
            let filled = read_best_effort(r, &mut buf).await;
            let complete = filled == n;
            buf.truncate(filled);
            (buf, complete)
        }
        Framing::Chunked => read_chunked_to_end(r).await,
        Framing::None => {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf).await;
            (buf, true)
        }
    }
}

/// Sends one request and reads the whole response (headers plus body)
/// before returning. Writes the request concurrently with reading the
/// response (via `tokio::join!`, on the split halves of the same
/// connection): a body large enough to trip a server-side size limit gets
/// its response (and the connection closed) before the whole body is
/// written, and writing/reading sequentially would deadlock waiting for
/// TCP window space the server will never drain.
///
/// Splitting (rather than reading/writing both ends of one `TcpStream`
/// directly) matters here for a subtler reason: `TcpStream`'s own
/// `AsyncRead`/`AsyncWrite` impls borrow `&mut self`, so a naive
/// `tokio::join!` over the same stream can't borrow it mutably twice at
/// once. The split halves are two distinct owned values, so both sides of
/// the join can each hold their own `&mut`. Keep both halves alive in this
/// function's own scope (not moved into a detached `tokio::spawn`) until
/// after the response has been read: `OwnedWriteHalf` shuts down the
/// connection's write direction when it drops, and doing that before the
/// server has finished writing its response is read by some servers as an
/// early client disconnect, discarding the in-flight response.
pub async fn send(addr: SocketAddr, method: &str, path: &str, body: Option<&[u8]>) -> HttpResponse {
    let stream = TcpStream::connect(addr).await.expect("connect");
    let (read_half, mut write_half) = stream.into_split();

    let write_fut = write_request(&mut write_half, method, path, body);

    let mut r = BufReader::new(read_half);
    let read_fut = async {
        let (status, headers, framing) = read_head(&mut r).await.expect("read head");
        let (resp_body, complete) = if method.eq_ignore_ascii_case("HEAD") {
            (Vec::new(), true)
        } else {
            read_body_to_end(&mut r, framing).await
        };
        (status, headers, resp_body, complete)
    };

    // A write error (e.g. the peer closed early) is expected whenever the
    // server rejects the request before reading the whole body; the
    // response read concurrently on the other half is this function's
    // actual result.
    let (_write_result, (status, headers, resp_body, complete)) = tokio::join!(write_fut, read_fut);

    HttpResponse {
        status,
        headers,
        body: resp_body,
        complete,
    }
}

/// A request whose response headers have been read, but whose body is
/// consumed incrementally (and can be abandoned mid-stream by dropping the
/// `OpenStream`, closing the socket).
pub struct OpenStream {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    reader: BufReader<TcpStream>,
    framing: Framing,
    remaining: Option<usize>,
    ended: bool,
    completed: bool,
}

impl OpenStream {
    /// Connects, sends the request, and returns once the response headers
    /// have been read.
    pub async fn open(
        addr: SocketAddr,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> OpenStream {
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        write_request(&mut stream, method, path, body)
            .await
            .expect("write request");
        let mut reader = BufReader::new(stream);
        let (status, headers, framing) = read_head(&mut reader).await.expect("read head");
        let remaining = match framing {
            Framing::ContentLength(n) => Some(n),
            _ => None,
        };
        OpenStream {
            status,
            headers,
            reader,
            framing,
            remaining,
            ended: false,
            completed: false,
        }
    }

    /// The first header value matching `name`, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Returns the next decoded chunk, or `None` on EOF, the terminator, or
    /// a `timeout` expiry (a timeout leaves the stream usable — it is not
    /// itself treated as the terminator).
    pub async fn next_chunk(&mut self, timeout_dur: Duration) -> Option<Vec<u8>> {
        if self.ended {
            return None;
        }
        timeout(timeout_dur, self.next_chunk_inner())
            .await
            .unwrap_or_default()
    }

    async fn next_chunk_inner(&mut self) -> Option<Vec<u8>> {
        match self.framing {
            Framing::ContentLength(_) => {
                let remaining = self.remaining.unwrap_or(0);
                if remaining == 0 {
                    self.ended = true;
                    self.completed = true;
                    return None;
                }
                let mut buf = vec![0u8; remaining.min(65536)];
                let n = read_best_effort(&mut self.reader, &mut buf).await;
                if n == 0 {
                    self.ended = true;
                    return None;
                }
                buf.truncate(n);
                let left = remaining - n;
                self.remaining = Some(left);
                if left == 0 {
                    self.ended = true;
                    self.completed = true;
                }
                Some(buf)
            }
            Framing::Chunked => match read_one_chunk(&mut self.reader).await {
                ChunkResult::Data(d) => Some(d),
                ChunkResult::End => {
                    self.ended = true;
                    self.completed = true;
                    None
                }
                ChunkResult::Error => {
                    self.ended = true;
                    None
                }
            },
            Framing::None => {
                let mut buf = vec![0u8; 65536];
                let n = read_best_effort(&mut self.reader, &mut buf).await;
                if n == 0 {
                    self.ended = true;
                    self.completed = true;
                    return None;
                }
                buf.truncate(n);
                Some(buf)
            }
        }
    }

    /// Collects every remaining chunk (each bounded by `timeout_dur`) into
    /// one [`HttpResponse`].
    pub async fn read_to_end(mut self, timeout_dur: Duration) -> HttpResponse {
        let mut body = Vec::new();
        while let Some(data) = self.next_chunk(timeout_dur).await {
            body.extend_from_slice(&data);
        }
        HttpResponse {
            status: self.status,
            headers: self.headers,
            body,
            complete: self.completed,
        }
    }
}
