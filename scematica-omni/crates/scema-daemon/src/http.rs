//! A minimal HTTP/1.1 server, on `std` and nothing else.
//!
//! ## Why hand-rolled rather than axum
//!
//! Two reasons, and the second is the real one.
//!
//! The first is consistency: `scema-world` takes two dependencies because it is the wire
//! format a reimplementer has to match, and `alchem-link` ships a whole terminal toolkit on
//! the standard library for the same reason. A loopback JSON server for a known client is
//! squarely in that class.
//!
//! The second is that pulling a full async HTTP stack into this workspace would pull
//! `hyper` → `rustls`/`tokio`, and the moment omni carries a TLS stack somebody will try to
//! path-depend it from the bot workspace and rediscover the `zeroize`/`curve25519-dalek`
//! conflict the root `Cargo.toml` documents at length. A server that speaks `Content-Length`
//! HTTP/1.1 to a client on the same machine does not need any of it.
//!
//! ## What this deliberately does not implement
//!
//! Chunked transfer encoding, keep-alive, pipelining, compression, TLS, HTTP/2. Every
//! response closes the connection. Anything a browser or `curl` sends to a localhost JSON
//! API works; a general-purpose server this is not, and it must never be exposed to a
//! network — see [`crate::routes`] for the bind rule.
//!
//! ## Limits are enforced, not assumed
//!
//! [`MAX_HEADER_BYTES`] and [`MAX_BODY_BYTES`] are checked while reading, not after. An
//! unbounded read from a socket is a memory exhaustion bug that looks like a hang, and the
//! client here can be any local process.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Request line plus headers may not exceed this.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// Body cap. A `WorldState` from a large page is the biggest thing this carries.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Concurrent connections. Beyond this, new connections are accepted and immediately
/// answered `503` rather than queued — an agent daemon that stops responding under load is
/// worse than one that says it is busy.
pub const MAX_CONNECTIONS: usize = 32;
/// Per-connection read/write timeout. A client that opens a socket and says nothing must
/// not hold a slot forever.
pub const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Path only, query stripped and decoded into [`Request::query`].
    pub path: String,
    pub query: BTreeMap<String, String>,
    /// Header names lowercased, so lookup does not depend on what the client capitalised.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    /// Path segments, empty ones dropped: `/decisions/abc/verify` → `["decisions", "abc", "verify"]`.
    pub fn segments(&self) -> Vec<&str> {
        self.path.split('/').filter(|s| !s.is_empty()).collect()
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// Extra headers, sent verbatim.
    pub extra: Vec<(String, String)>,
}

impl Response {
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Response {
            status,
            content_type: "application/json; charset=utf-8".into(),
            body: body.into(),
            extra: vec![],
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Response {
            status,
            content_type: "text/plain; charset=utf-8".into(),
            body: body.into().into_bytes(),
            extra: vec![],
        }
    }

    /// A JSON error with a stable shape, so a client never has to parse prose.
    pub fn error(status: u16, code: &str, message: impl std::fmt::Display) -> Self {
        let body = serde_json::json!({ "error": code, "message": message.to_string() });
        Response::json(status, body.to_string())
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        421 => "Misdirected Request",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    // A malformed escape is kept literally rather than dropped. Dropping it
                    // would silently change the path being requested.
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(raw: &str) -> BTreeMap<String, String> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(p), String::new()),
        })
        .collect()
}

/// Read one request. `Ok(None)` means the peer closed without sending anything.
fn read_request(stream: &mut BufReader<&TcpStream>) -> Result<Option<Request>, Response> {
    let mut head = Vec::new();
    let mut line = Vec::new();

    loop {
        line.clear();
        let n = stream
            .read_until(b'\n', &mut line)
            .map_err(|e| Response::error(400, "read_failed", e))?;
        if n == 0 {
            if head.is_empty() {
                return Ok(None);
            }
            return Err(Response::error(400, "truncated", "connection closed mid-headers"));
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEADER_BYTES {
            return Err(Response::error(
                413,
                "headers_too_large",
                format!("request head exceeds {MAX_HEADER_BYTES} bytes"),
            ));
        }
        // Blank line terminates the head.
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }

    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| Response::error(400, "empty_request", "no request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| Response::error(400, "bad_request_line", request_line))?
        .to_string();
    let target = parts
        .next()
        .ok_or_else(|| Response::error(400, "bad_request_line", request_line))?;

    let (raw_path, raw_query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };

    let mut headers = BTreeMap::new();
    for l in lines {
        if l.is_empty() {
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY_BYTES {
        return Err(Response::error(
            413,
            "body_too_large",
            format!("{len} bytes exceeds the {MAX_BODY_BYTES} byte limit"),
        ));
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        stream
            .read_exact(&mut body)
            .map_err(|e| Response::error(400, "body_read_failed", e))?;
    }

    Ok(Some(Request {
        method,
        path: percent_decode(raw_path),
        query: parse_query(raw_query),
        headers,
        body,
    }))
}

/// Write a response, dropping the body if the request was a `HEAD`.
///
/// Stripped **here**, in the one place every response passes through, rather than by each
/// branch deciding for itself whether its answer carries a body. A branch-by-branch rule is one
/// a new route silently opts out of, and the whole point of a `HEAD` is that it is the request
/// somebody sends when they are not supposed to receive the content.
///
/// `Content-Length` still reports what a `GET` would return — that is what the header means on
/// a `HEAD`, and zeroing it would make the response a lie about the resource rather than an
/// accurate description of a body that is deliberately absent.
fn write_response(stream: &mut TcpStream, r: &Response, is_head: bool) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        r.status,
        reason(r.status),
        r.content_type,
        r.body.len()
    );
    // No `Access-Control-Allow-Origin`, ever, and no `OPTIONS` handler. See the note in
    // `crate::routes`: the browser extension reaches this from its service worker with
    // host permissions, which is not subject to CORS, so a web page gets no way to read a
    // response even if it manages to send a request.
    head.push_str("X-Content-Type-Options: nosniff\r\n");
    for (k, v) in &r.extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if !is_head {
        stream.write_all(&r.body)?;
    }
    stream.flush()
}

/// The bind address. **Loopback only, and not configurable.**
///
/// This process reads the operator's filesystem and answers questions about it. Binding it
/// to anything routable turns a local tool into an unauthenticated file-disclosure service
/// on the network, and the one thing that reliably happens to a `--bind` flag is that
/// somebody sets it to `0.0.0.0` to reach it from another machine. The port is
/// configurable; the interface is not.
pub fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

/// Serve until the process is killed.
///
/// `handler` runs on a worker thread per connection and must not panic; a panic is caught
/// and answered `500` so one bad request cannot take the daemon down.
pub fn serve<H>(listener: TcpListener, handler: H) -> std::io::Result<()>
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    let live = Arc::new(AtomicUsize::new(0));

    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

        if live.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            // The request was never read, so the method is unknown. Sending the body is the
            // safe side of that: this response describes the server's own load and discloses
            // nothing about any resource.
            let _ = write_response(
                &mut stream,
                &Response::error(503, "busy", "too many concurrent connections"),
                false,
            );
            continue;
        }

        let handler = Arc::clone(&handler);
        let live = Arc::clone(&live);
        live.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            handle_connection(&mut stream, handler.as_ref());
            live.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

fn handle_connection<H>(stream: &mut TcpStream, handler: &H)
where
    H: Fn(Request) -> Response,
{
    let mut is_head = false;
    let response = {
        let mut reader = BufReader::new(&*stream);
        match read_request(&mut reader) {
            Ok(None) => return,
            Ok(Some(req)) => {
                // Captured before the handler runs, so a route that does not know about `HEAD`
                // — which is all of them, deliberately — still cannot answer one with a body.
                is_head = req.method == "HEAD";
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(req)))
                    .unwrap_or_else(|_| {
                        Response::error(500, "handler_panicked", "the request handler panicked")
                    })
            }
            Err(e) => e,
        }
    };
    let _ = write_response(stream, &response, is_head);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_string_is_decoded_not_passed_through() {
        let q = parse_query("a=one%20two&b&c=%2Fpath");
        assert_eq!(q.get("a").map(String::as_str), Some("one two"));
        assert_eq!(q.get("b").map(String::as_str), Some(""));
        assert_eq!(q.get("c").map(String::as_str), Some("/path"));
    }

    #[test]
    fn a_malformed_escape_is_kept_literally_not_dropped() {
        // Dropping it would silently change which path was requested, and a path is a
        // security decision here.
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn segments_ignore_empty_components() {
        let r = Request {
            method: "GET".into(),
            path: "//decisions//abc/verify/".into(),
            query: BTreeMap::new(),
            headers: BTreeMap::new(),
            body: vec![],
        };
        assert_eq!(r.segments(), vec!["decisions", "abc", "verify"]);
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer x".to_string());
        let r = Request {
            method: "GET".into(),
            path: "/".into(),
            query: BTreeMap::new(),
            headers,
            body: vec![],
        };
        assert_eq!(r.header("Authorization"), Some("Bearer x"));
    }

    #[test]
    fn the_bind_address_is_always_loopback() {
        // Asserted rather than trusted to review. This process answers questions about the
        // operator's filesystem.
        assert!(loopback(7842).ip().is_loopback());
    }

    #[test]
    fn a_response_never_carries_a_cors_allow_header() {
        // The rule that keeps a malicious page from reading an answer even if it manages to
        // send a request. Checked on the serialised bytes, since that is what a browser sees.
        let listener = TcpListener::bind(loopback(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve(listener, |_req| Response::json(200, "{}")).unwrap();
        });
        let mut s = TcpStream::connect(loopback(port)).unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.starts_with("HTTP/1.1 200"), "{buf}");
        assert!(
            !buf.to_lowercase().contains("access-control-allow"),
            "a CORS allow header would let any web page read this: {buf}"
        );
    }

    #[test]
    fn head_never_returns_a_body() {
        // On every route, without exception, because the stripping is in `write_response` and
        // not in any handler. A handler that answers a HEAD with content is the normal case —
        // none of them know the method — so the guarantee has to sit below all of them.
        let listener = TcpListener::bind(loopback(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve(listener, |_req| {
                Response::json(200, r#"{"secret":"this must not be written"}"#)
            })
            .unwrap();
        });
        let mut s = TcpStream::connect(loopback(port)).unwrap();
        s.write_all(b"HEAD /world/abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();

        assert!(buf.starts_with("HTTP/1.1 200"), "{buf}");
        assert!(!buf.contains("this must not be written"), "a HEAD returned a body: {buf}");
        // Headers still describe what a GET would return — that is what Content-Length means
        // on a HEAD, and zeroing it would misdescribe the resource rather than the body.
        assert!(buf.contains("Content-Length: 37"), "the length stopped describing the resource: {buf}");
        assert!(buf.ends_with("\r\n\r\n"), "something followed the headers: {buf:?}");
    }

    #[test]
    fn the_same_route_still_returns_a_body_to_a_get() {
        // The other half: a stripping rule that also strips GETs is not a fix, and nothing
        // above would notice, because every assertion in the HEAD test would still pass.
        let listener = TcpListener::bind(loopback(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve(listener, |_req| Response::json(200, r#"{"ok":true}"#)).unwrap();
        });
        let mut s = TcpStream::connect(loopback(port)).unwrap();
        s.write_all(b"GET /world/abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.contains(r#"{"ok":true}"#), "a GET lost its body: {buf}");
    }

    #[test]
    fn an_oversized_body_is_refused_by_the_declared_length() {
        // Refused from Content-Length, before any of it is read into memory.
        let listener = TcpListener::bind(loopback(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve(listener, |_req| Response::json(200, "{}")).unwrap();
        });
        let mut s = TcpStream::connect(loopback(port)).unwrap();
        let head = format!(
            "POST /observe HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        s.write_all(head.as_bytes()).unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.starts_with("HTTP/1.1 413"), "{buf}");
    }

    #[test]
    fn a_panicking_handler_answers_500_rather_than_killing_the_daemon() {
        let listener = TcpListener::bind(loopback(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve(listener, |_req| panic!("boom")).unwrap();
        });
        // Silence the panic message; the behaviour under test is the response.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let mut s = TcpStream::connect(loopback(port)).unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        std::panic::set_hook(prev);
        assert!(buf.starts_with("HTTP/1.1 500"), "{buf}");

        // And the daemon is still serving.
        let mut s2 = TcpStream::connect(loopback(port)).unwrap();
        s2.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut buf2 = String::new();
        s2.read_to_string(&mut buf2).unwrap();
        assert!(buf2.starts_with("HTTP/1.1 500"), "{buf2}");
    }

    #[test]
    fn a_body_is_read_in_full_before_the_handler_sees_it() {
        let listener = TcpListener::bind(loopback(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve(listener, |req| {
                Response::text(200, String::from_utf8_lossy(&req.body).to_string())
            })
            .unwrap();
        });
        let mut s = TcpStream::connect(loopback(port)).unwrap();
        let payload = r#"{"locator":"."}"#;
        s.write_all(
            format!(
                "POST /observe HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{}",
                payload.len(),
                payload
            )
            .as_bytes(),
        )
        .unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.ends_with(payload), "{buf}");
    }
}
