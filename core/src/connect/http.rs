//! The least HTTP/1.1 the Connect server needs: read one request (head and a small
//! body), answer it and close. No library: three routes and a WebSocket upgrade do
//! not justify an async runtime, and what comes in from the network is bounded
//! here (size of the head, of the body, how long it may take).

use std::io::{self, Read, Write};
use std::net::TcpStream;

/// The head (request line and headers) may not be longer than this.
pub const MAX_HEAD: usize = 8 * 1024;
/// A body may not be longer than this (a PIN request is a few dozen bytes).
pub const MAX_BODY: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Without the query.
    pub path: String,
    /// Names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// The `Bearer` token of the `Authorization` header.
    pub fn bearer(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ").map(str::trim)
    }

    /// Does it ask to become a WebSocket?
    pub fn wants_websocket(&self) -> bool {
        let has = |name: &str, word: &str| self.header(name).is_some_and(|v| v.to_ascii_lowercase().split(',').any(|part| part.trim() == word));
        has("connection", "upgrade") && has("upgrade", "websocket") && self.header("sec-websocket-key").is_some()
    }
}

/// Reads one request from the stream. The caller has set read timeouts.
pub fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    let mut data = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(end) = find(&data, b"\r\n\r\n") {
            break end;
        }
        if data.len() > MAX_HEAD {
            return Err(invalid("the head is too long"));
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed before the request ended"));
        }
        data.extend_from_slice(&chunk[..read]);
    };
    let mut request = parse_head(&data[..head_end])?;
    let mut body = data[head_end + 4..].to_vec();
    let length = match request.header("content-length") {
        Some(value) => value.trim().parse::<usize>().map_err(|_| invalid("bad Content-Length"))?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(invalid("the body is too long"));
    }
    while body.len() < length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed before the body ended"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    request.body = body;
    Ok(request)
}

/// The request line and the headers (without the blank line at the end).
pub fn parse_head(head: &[u8]) -> io::Result<Request> {
    let text = std::str::from_utf8(head).map_err(|_| invalid("the head is not text"))?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(version), None) = (first.next(), first.next(), first.next(), first.next()) else {
        return Err(invalid("bad request line"));
    };
    if !version.starts_with("HTTP/1.") || method.is_empty() || !target.starts_with('/') {
        return Err(invalid("bad request line"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(|| invalid("bad header"))?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let path = target.split('?').next().unwrap_or(target).to_string();
    Ok(Request { method: method.to_string(), path, headers, body: Vec::new() })
}

/// Answers with a JSON body and closes.
pub fn respond_json(stream: &mut TcpStream, status: u16, body: &serde_json::Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(body).map_err(io::Error::other)?;
    respond(stream, status, "application/json", &[], &bytes)
}

pub fn respond(stream: &mut TcpStream, status: u16, content_type: &str, extra: &[(&str, &str)], body: &[u8]) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        reason(status),
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// The answer that turns the connection into a WebSocket.
pub fn respond_switching(stream: &mut TcpStream, key: &str) -> io::Result<()> {
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let head = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_gives_method_path_without_query_and_lowercase_headers() {
        let request = parse_head(b"GET /connect/v1/info?x=1 HTTP/1.1\r\nHost: a\r\nAuthorization: Bearer  abc \r\nX-Thing:  y").unwrap();
        assert_eq!((request.method.as_str(), request.path.as_str()), ("GET", "/connect/v1/info"));
        assert_eq!(request.header("host"), Some("a"));
        assert_eq!(request.bearer(), Some("abc"));
        assert_eq!(request.header("x-thing"), Some("y"));
    }

    #[test]
    fn a_bad_request_line_is_refused() {
        for head in ["", "GET", "GET /x", "GET x HTTP/1.1", "GET /x HTTP/2.0", "GET /x HTTP/1.1 extra"] {
            assert!(parse_head(head.as_bytes()).is_err(), "{head:?}");
        }
        assert!(parse_head(b"GET /x HTTP/1.1\r\nno colon here").is_err());
    }

    #[test]
    fn a_websocket_upgrade_needs_all_three_headers() {
        let upgrade = parse_head(b"GET /ws HTTP/1.1\r\nConnection: keep-alive, Upgrade\r\nUpgrade: WebSocket\r\nSec-WebSocket-Key: abc").unwrap();
        assert!(upgrade.wants_websocket());
        let plain = parse_head(b"GET /ws HTTP/1.1\r\nConnection: close").unwrap();
        assert!(!plain.wants_websocket());
        let no_key = parse_head(b"GET /ws HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket").unwrap();
        assert!(!no_key.wants_websocket());
    }

    #[test]
    fn the_authorization_header_must_be_a_bearer() {
        let basic = parse_head(b"GET / HTTP/1.1\r\nAuthorization: Basic abc").unwrap();
        assert_eq!(basic.bearer(), None);
    }
}
