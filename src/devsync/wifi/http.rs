//! The little HTTP/1.1 that sync over Wi-Fi needs: one request head at a
//! time, bodies only with a `Content-Length`, keep-alive, and a
//! `Content-Length` on every answer (docs/sync-wifi.md, "Requests").

use std::io::{self, BufRead, Read, Write};

/// Longest request head taken (request line and headers together).
const HEAD_LIMIT: u64 = 16 << 10;
const HEADER_COUNT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    /// The path, percent-decoded, without the query. `None` when it does not
    /// decode to UTF-8 (no route takes it).
    pub path: Option<String>,
    /// The token of `Authorization: Bearer <token>`.
    pub token: Option<String>,
    /// The length of the body (0 without `Content-Length`).
    pub length: u64,
    /// The client asked to close after this answer (or speaks HTTP/1.0).
    pub close: bool,
}

#[derive(Debug)]
pub enum HeadError {
    /// Not a request this server takes: answer 400 and close.
    Bad,
    /// The connection broke or timed out: just close.
    Io(#[allow(dead_code)] io::Error),
}

impl From<io::Error> for HeadError {
    fn from(e: io::Error) -> Self {
        HeadError::Io(e)
    }
}

/// Read the next request head. `Ok(None)`: the client closed between requests.
pub fn read_head<R: BufRead>(reader: &mut R) -> Result<Option<Head>, HeadError> {
    let mut limited = reader.take(HEAD_LIMIT);
    let mut lines: Vec<String> = Vec::new();
    loop {
        let mut raw = Vec::new();
        let n = limited.read_until(b'\n', &mut raw)?;
        if n == 0 {
            // End of the stream: clean between requests, broken inside one.
            return if lines.is_empty() { Ok(None) } else { Err(HeadError::Bad) };
        }
        if raw.last() != Some(&b'\n') {
            return Err(HeadError::Bad); // the head is too long
        }
        raw.pop();
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }
        let line = String::from_utf8(raw).map_err(|_| HeadError::Bad)?;
        if line.is_empty() {
            if lines.is_empty() {
                continue; // an empty line before a request is allowed
            }
            break;
        }
        if lines.len() > HEADER_COUNT {
            return Err(HeadError::Bad);
        }
        lines.push(line);
    }

    let mut parts = lines[0].split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(HeadError::Bad);
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) || !target.starts_with('/') {
        return Err(HeadError::Bad);
    }
    let mut close = match version {
        "HTTP/1.1" => false,
        "HTTP/1.0" => true,
        _ => return Err(HeadError::Bad),
    };
    let mut length: Option<u64> = None;
    let mut token = None;
    for line in &lines[1..] {
        let Some((name, value)) = line.split_once(':') else { return Err(HeadError::Bad) };
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                let n: u64 = value.parse().map_err(|_| HeadError::Bad)?;
                if length.is_some_and(|l| l != n) {
                    return Err(HeadError::Bad);
                }
                length = Some(n);
            }
            // Bodies come with a length only (URLSession sends one).
            "transfer-encoding" => return Err(HeadError::Bad),
            "connection" => {
                let v = value.to_ascii_lowercase();
                if v.split(',').any(|t| t.trim() == "close") {
                    close = true;
                } else if v.split(',').any(|t| t.trim() == "keep-alive") {
                    close = false;
                }
            }
            "authorization" => {
                if let Some(t) = value.strip_prefix("Bearer ") {
                    token = Some(t.trim().to_string());
                }
            }
            _ => {}
        }
    }
    let raw_path = target.split(['?', '#']).next().unwrap_or("");
    Ok(Some(Head { method: method.to_string(), path: percent_decode(raw_path), token, length: length.unwrap_or(0), close }))
}

/// `%XX` decoded; `None` for a bad escape or a result that is not UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Every byte outside `A-Z a-z 0-9 - . _ ~` as `%XX` (for the pairing link).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Content Too Large",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

/// The head of an answer whose body of `length` bytes follows.
pub fn write_head<W: Write>(w: &mut W, status: u16, kind: &str, length: u64, close: bool) -> io::Result<()> {
    let connection = if close { "Connection: close\r\n" } else { "" };
    write!(w, "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {length}\r\nCache-Control: no-store\r\n{connection}\r\n", reason(status))
}

/// A whole answer. Returns whether the connection stays open.
pub fn answer<W: Write>(w: &mut W, status: u16, kind: &str, body: &[u8], close: bool) -> io::Result<bool> {
    write_head(w, status, kind, body.len() as u64, close)?;
    w.write_all(body)?;
    w.flush()?;
    Ok(!close)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(text: &str) -> Result<Option<Head>, HeadError> {
        read_head(&mut io::Cursor::new(text.as_bytes().to_vec()))
    }

    #[test]
    fn a_request_head_is_read() {
        let h = head("PUT /v1/file/files/a%2Epng?x=1 HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer abc\r\nContent-Length: 12\r\n\r\nbody").unwrap().unwrap();
        assert_eq!((h.method.as_str(), h.path.as_deref(), h.token.as_deref(), h.length, h.close), ("PUT", Some("/v1/file/files/a.png"), Some("abc"), 12, false));
        let h = head("GET / HTTP/1.0\r\n\r\n").unwrap().unwrap();
        assert!(h.close);
        let h = head("GET / HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap().unwrap();
        assert!(h.close);
        assert!(head("").unwrap().is_none());
        for bad in [
            "GET / HTTP/2\r\n\r\n",
            "GET  / HTTP/1.1\r\n\r\n",
            "get / HTTP/1.1\r\n\r\n",
            "GET x HTTP/1.1\r\n\r\n",
            "PUT / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "PUT / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            "PUT / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
            "GET / HTTP/1.1\r\nHost",
        ] {
            assert!(matches!(head(bad), Err(HeadError::Bad)), "{bad:?}");
        }
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(20_000));
        assert!(matches!(head(&long), Err(HeadError::Bad)));
    }

    #[test]
    fn percent_signs_are_decoded_and_encoded() {
        assert_eq!(percent_decode("/a%2e%2E/%41").as_deref(), Some("/a../A"));
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%ff"), None);
        assert_eq!(percent_encode("MacBook Pro/é"), "MacBook%20Pro%2F%C3%A9");
    }
}
