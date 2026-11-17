//! «Escuchar en otro dispositivo»: this device passes what it plays to another one, which asks THIS device for the files
//! over the network (it does not need to have them). Apart from «Controlar» on purpose: they only share the transport.
//!
//! Here: the permission to read (a [`Tickets`] grant: one ticket, only the files of the queue that was passed, only while
//! that pass lasts) and serving one file by ranges. Built so it costs nothing while nobody reads: no timers, no threads of
//! its own; a read is the connection's thread, sends by chunks from disk, and ends with the range or the connection.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ring::rand::{SecureRandom, SystemRandom};

/// Reads open at once on one ticket: the file that sounds, a look at its end (an MP4 may keep its index there) and the
/// next one being loaded. More is somebody misbehaving.
pub const MAX_READS_PER_TICKET: usize = 3;
/// The size of what is read from disk and written to the network at a time.
pub const CHUNK: usize = 64 * 1024;

/// What can be read of one file of the queue that was passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFile {
    pub media: PathBuf,
    /// Its picture (`cover.jpg` of the playlist or the file's thumbnail), if it has one.
    pub cover: Option<PathBuf>,
}

/// Which of the two of a [`SharedFile`] is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Media,
    Cover,
}

/// Why a read was not let through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No such ticket (or no longer), or that file is not in its queue: the same answer for both, to say nothing.
    NotFound,
    /// [`MAX_READS_PER_TICKET`] are already open.
    Busy,
}

struct Grant {
    ticket: String,
    files: HashMap<String, SharedFile>,
    reads: Arc<AtomicUsize>,
}

/// The permission to read what was passed. ONE pass at a time: a new grant ends the one before (its reads in flight end
/// with their connection; no new one is let through).
#[derive(Default)]
pub struct Tickets {
    current: Mutex<Option<Grant>>,
}

impl Tickets {
    /// Allows reading these files (video id → files) with the ticket it gives back, and nothing else.
    pub fn grant(&self, files: impl IntoIterator<Item = (String, SharedFile)>) -> String {
        let ticket = random_ticket();
        let grant = Grant { ticket: ticket.clone(), files: files.into_iter().collect(), reads: Arc::new(AtomicUsize::new(0)) };
        *self.current.lock().unwrap() = Some(grant);
        ticket
    }

    /// The pass ended («Traer aquí», «Detener», passed to another device, Connect off).
    pub fn revoke(&self) {
        *self.current.lock().unwrap() = None;
    }

    /// Is there a pass going on?
    pub fn active(&self) -> bool {
        self.current.lock().unwrap().is_some()
    }

    /// Lets one read through: the path, and a guard that counts it as open until it is dropped.
    pub fn open(&self, ticket: &str, video_id: &str, part: Part) -> Result<(PathBuf, ReadGuard), Refusal> {
        let current = self.current.lock().unwrap();
        let grant = current.as_ref().filter(|grant| same(&grant.ticket, ticket)).ok_or(Refusal::NotFound)?;
        let file = grant.files.get(video_id).ok_or(Refusal::NotFound)?;
        let path = match part {
            Part::Media => file.media.clone(),
            Part::Cover => file.cover.clone().ok_or(Refusal::NotFound)?,
        };
        let reads = grant.reads.clone();
        if reads.fetch_add(1, Ordering::SeqCst) >= MAX_READS_PER_TICKET {
            reads.fetch_sub(1, Ordering::SeqCst);
            return Err(Refusal::Busy);
        }
        Ok((path, ReadGuard { reads }))
    }
}

/// One read counted as open; dropping it lets another one through.
pub struct ReadGuard {
    reads: Arc<AtomicUsize>,
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        self.reads.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Compared in a time that does not depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 32 random bytes in hex: travels in a query with no escaping.
fn random_ticket() -> String {
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("the system has random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Is it what [`Tickets::grant`] hands out?
pub fn is_ticket(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A `Range` that cannot be served (answered 416).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsatisfiable;

/// The bytes asked for by a `Range` header, inclusive, for a file of `len` bytes. `Ok(None)` = the whole file (no header);
/// `Err` = it cannot be served (416). Only one range (`bytes=a-b`, `bytes=a-`, `bytes=-n`): players never ask for more.
pub fn parse_range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, Unsatisfiable> {
    let Some(header) = header else { return Ok(None) };
    let spec = header.trim().strip_prefix("bytes=").ok_or(Unsatisfiable)?.trim();
    if spec.contains(',') || len == 0 {
        return Err(Unsatisfiable);
    }
    let (start, end) = spec.split_once('-').ok_or(Unsatisfiable)?;
    let number = |text: &str| text.trim().parse::<u64>().map_err(|_| Unsatisfiable);
    let (start, end) = match (start.trim().is_empty(), end.trim().is_empty()) {
        // The last n bytes.
        (true, false) => {
            let n = number(end)?;
            if n == 0 {
                return Err(Unsatisfiable);
            }
            (len.saturating_sub(n), len - 1)
        }
        (false, true) => (number(start)?, len - 1),
        (false, false) => (number(start)?, number(end)?.min(len - 1)),
        (true, true) => return Err(Unsatisfiable),
    };
    if start > end || start >= len {
        return Err(Unsatisfiable);
    }
    Ok(Some((start, end)))
}

/// The `Content-Type` of a file Soundify keeps, by its extension.
pub fn content_type(path: &Path) -> &'static str {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => "audio/mpeg",
        "m4a" | "alac" => "audio/mp4",
        "aac" => "audio/aac",
        "opus" | "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "flv" => "video/x-flv",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Answers a read of `path`: the whole file (200) or the range asked for (206), by chunks of [`CHUNK`], never all of it
/// in memory. `head_only` = only the head (a `HEAD`). Gives back the bytes of body written; an unsatisfiable range is
/// answered 416 here (with the length, so the player can ask again) and counts as 0.
pub fn serve_file(stream: &mut impl Write, path: &Path, range: Option<&str>, head_only: bool) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let kind = content_type(path);
    let Ok(range) = parse_range(range, len) else {
        let head = format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        stream.write_all(head.as_bytes())?;
        stream.flush()?;
        return Ok(0);
    };
    let (start, end, status) = match range {
        Some((start, end)) => (start, end, 206),
        None => (0, len.saturating_sub(1), 200),
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {count}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\nConnection: close\r\n",
        super::http::reason(status)
    );
    if status == 206 {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{len}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if head_only || count == 0 {
        stream.flush()?;
        return Ok(0);
    }
    file.seek(SeekFrom::Start(start))?;
    let mut left = count;
    let mut buffer = vec![0u8; CHUNK.min(count as usize)];
    while left > 0 {
        let want = buffer.len().min(left as usize);
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            // The file got shorter while it was read: what was promised cannot be given.
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the file got shorter"));
        }
        stream.write_all(&buffer[..read])?;
        left -= read as u64;
    }
    stream.flush()?;
    Ok(count)
}

// === THE DEVICE THAT RECEIVES ===

/// Where the pass that plays here is read from. The ticket never reaches the page: it asks `soundify-cast://` for a video
/// id and the app adds the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastSource {
    /// The device that passed it (its id).
    pub from: String,
    pub host: String,
    pub port: u16,
    /// Its certificate: nothing else is read from (the files go over TLS, like everything with a secret in it).
    pub fingerprint: String,
    pub ticket: String,
}

/// The pass this device plays now (one at a time: a new one replaces it).
#[derive(Default)]
pub struct Inbox {
    current: Mutex<Option<CastSource>>,
}

impl Inbox {
    pub fn set(&self, source: CastSource) {
        *self.current.lock().unwrap() = Some(source);
    }

    /// The device that passed it ended it; one that passed an older one cannot end the new one.
    pub fn end_from(&self, from: &str) -> bool {
        let mut current = self.current.lock().unwrap();
        if current.as_ref().is_some_and(|source| source.from == from) {
            *current = None;
            return true;
        }
        false
    }

    /// This device plays something else.
    pub fn clear(&self) {
        *self.current.lock().unwrap() = None;
    }

    pub fn current(&self) -> Option<CastSource> {
        self.current.lock().unwrap().clone()
    }
}

/// The most that one answer of the relay carries: the player asks again for what follows (it does with every answer
/// shorter than what it asked for), so the receiving device never holds more than this of a file in memory.
pub const RELAY_PIECE: u64 = 1024 * 1024;
/// A picture bigger than this is not passed along.
const MAX_COVER: u64 = 4 * 1024 * 1024;
const CONNECT_WAIT: Duration = Duration::from_secs(3);
const READ_WAIT: Duration = Duration::from_secs(10);
/// A device whose ticket has all its reads open (503) is asked again after a moment, a few times.
const BUSY_RETRIES: u32 = 3;
const BUSY_WAIT: Duration = Duration::from_millis(150);
const CRLF: &str = "\r\n";

/// What the relay answers the page with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relayed {
    pub status: u16,
    /// Names in lower case: `content-type`, `content-range`, `accept-ranges`, `content-length`.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Relayed {
    fn status(status: u16) -> Relayed {
        Relayed { status, headers: Vec::new(), body: Vec::new() }
    }
}

/// A video id that may go into a path as it is (YouTube's are letters, digits, `-` and `_`).
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The range asked of the device that shares, cut to [`RELAY_PIECE`]: `bytes=a-` and `bytes=a-b` become at most one
/// piece from `a`; a request with no range asks for the first piece; the last n bytes, at most one piece of them.
pub fn piece_of(range: Option<&str>) -> String {
    let last = RELAY_PIECE - 1;
    let first = format!("bytes=0-{last}");
    let Some(spec) = range.and_then(|r| r.trim().strip_prefix("bytes=")).map(str::trim) else { return first };
    let Some((start, end)) = spec.split_once('-').filter(|_| !spec.contains(',')) else { return first };
    match (start.trim().parse::<u64>(), end.trim().parse::<u64>()) {
        (Ok(start), Ok(end)) => format!("bytes={start}-{}", end.min(start.saturating_add(last))),
        (Ok(start), Err(_)) => format!("bytes={start}-{}", start.saturating_add(last)),
        (Err(_), Ok(n)) if start.trim().is_empty() => format!("bytes=-{}", n.min(RELAY_PIECE)),
        _ => first,
    }
}

/// Reads one piece of a file of the pass from the device that shares it, for the page's player.
pub fn relay(source: &CastSource, part: Part, video_id: &str, range: Option<&str>) -> Relayed {
    if !is_safe_id(video_id) || !is_ticket(&source.ticket) || !super::tls::is_fingerprint(&source.fingerprint) {
        return Relayed::status(404);
    }
    let Ok(ip) = source.host.parse::<IpAddr>() else { return Relayed::status(502) };
    let address = SocketAddr::new(ip, source.port);
    let (path, asked, limit) = match part {
        Part::Media => ("media", Some(piece_of(range)), RELAY_PIECE),
        Part::Cover => ("cover", None, MAX_COVER),
    };
    let mut request = format!("GET /connect/v1/{path}/{video_id}?t={} HTTP/1.1{CRLF}Host: {address}{CRLF}Connection: close{CRLF}", source.ticket);
    if let Some(asked) = &asked {
        request.push_str(&format!("Range: {asked}{CRLF}"));
    }
    request.push_str(CRLF);
    let mut tries = 0;
    loop {
        match fetch(address, &source.fingerprint, &request, limit) {
            Ok(relayed) if relayed.status == 503 && tries < BUSY_RETRIES => {
                tries += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            Ok(relayed) => return relayed,
            // The device that shares is not there (gone, asleep, another network): the player sees an error.
            Err(_) => return Relayed::status(502),
        }
    }
}

/// One request to the device that shares and its whole answer (at most `limit` bytes of body).
fn fetch(address: SocketAddr, fingerprint: &str, request: &str, limit: u64) -> io::Result<Relayed> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let (mut stream, _) = super::tls::connect(address, &super::tls::Expect::Pinned(fingerprint.to_string()), CONNECT_WAIT, READ_WAIT)?;
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    let mut data = Vec::new();
    match (&mut stream).take(limit + super::http::MAX_HEAD as u64).read_to_end(&mut data) {
        Ok(_) => {}
        // A TLS peer that closes after the answer without saying so: what arrived is the answer (its length is checked below).
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
        Err(e) => return Err(e),
    }
    let end = data.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| bad("no head"))?;
    let head = std::str::from_utf8(&data[..end]).map_err(|_| bad("head"))?;
    let status = head.split(' ').nth(1).and_then(|s| s.parse::<u16>().ok()).ok_or_else(|| bad("status"))?;
    let mut headers = Vec::new();
    let mut length = None;
    for line in head.split(CRLF).skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_string());
        match name.as_str() {
            "content-length" => length = value.parse::<usize>().ok(),
            "content-type" | "content-range" | "accept-ranges" => headers.push((name, value)),
            _ => {}
        }
    }
    let mut body = data.split_off(end + 4);
    if let Some(length) = length {
        if body.len() < length {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the answer was cut"));
        }
        body.truncate(length);
    }
    headers.push(("content-length".into(), body.len().to_string()));
    Ok(Relayed { status, headers, body })
}

// === THE RELAY FOR A NATIVE PLAYER (Android) ===

/// What a pass sounds through on Android: Media3 (the player that goes on with the screen off) reads plain HTTP, not the
/// app's own `soundify-cast://`. So a tiny server on 127.0.0.1 hands it the files of the pass, each answer the WHOLE range
/// it asked for (a player takes a shorter answer for the end of the file), fetched from the other device one piece at a
/// time and written on as it comes: never more than one piece in memory.
///
/// Only this phone can reach 127.0.0.1, but every app on it can: the paths carry a random secret, made when it starts.
/// Started the first time something is passed here; then it sleeps in `accept` (no timer, no polling).
pub struct LocalRelay {
    port: u16,
    secret: String,
}

impl LocalRelay {
    pub fn start(inbox: Arc<Inbox>) -> io::Result<LocalRelay> {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let secret = random_ticket();
        let wanted = format!("/{secret}/");
        std::thread::Builder::new().name("cast-local".into()).spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (inbox, wanted) = (inbox.clone(), wanted.clone());
                let _ = std::thread::Builder::new().name("cast-local-read".into()).spawn(move || {
                    let _ = serve_local(stream, &inbox, &wanted);
                });
            }
        })?;
        Ok(LocalRelay { port, secret })
    }

    /// `http://127.0.0.1:<port>/<secret>/`: a file of the pass is this plus `media/<videoId>`.
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.port, self.secret)
    }
}

fn serve_local(stream: std::net::TcpStream, inbox: &Inbox, wanted: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    // A player whose buffer is full stops reading: past this the connection is let go of (it asks again by range).
    stream.set_write_timeout(Some(Duration::from_secs(20)))?;
    let mut stream = super::resilient::Resilient::new(stream);
    let request = super::http::read_request(&mut stream)?;
    // The secret, compared in a time that does not tell how much of it was right.
    let id = request.path.get(..wanted.len()).filter(|prefix| same(prefix, wanted)).and_then(|_| request.path[wanted.len()..].strip_prefix("media/"));
    let (Some(id), Some(source)) = (id, inbox.current()) else {
        return super::http::respond(&mut stream, 404, "text/plain", &[], b"");
    };
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return super::http::respond(&mut stream, 405, "text/plain", &[], b"");
    }
    stream_range(&source, id, request.header("range"), request.method == "HEAD", &mut stream)
}

/// `bytes a-b` / `bytes a-` of a request (a suffix `bytes=-n` is left to one piece, as it comes).
fn wanted_range(range: Option<&str>) -> Option<(u64, Option<u64>)> {
    let spec = range?.trim().strip_prefix("bytes=")?.trim();
    let (start, end) = spec.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    Some((start, end.trim().parse::<u64>().ok()))
}

/// The whole range asked for, written on piece by piece as it arrives from the other device.
pub fn stream_range(source: &CastSource, id: &str, range: Option<&str>, head_only: bool, out: &mut impl Write) -> io::Result<()> {
    let ranged = range.is_some();
    let Some((start, end_asked)) = (if ranged { wanted_range(range) } else { Some((0, None)) }) else {
        // A suffix or a range of several parts: what one piece gives, as it comes.
        let piece = relay(source, Part::Media, id, range);
        return write_relayed(out, &piece);
    };
    let first = relay(source, Part::Media, id, Some(&format!("bytes={start}-")));
    if first.status != 206 {
        return write_relayed(out, &first);
    }
    let header = |name: &str| first.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
    let total = header("content-range").and_then(|r| r.rsplit('/').next().and_then(|t| t.trim().parse::<u64>().ok())).unwrap_or(0);
    if total == 0 {
        return write_relayed(out, &first);
    }
    let end = end_asked.unwrap_or(total - 1).min(total - 1);
    let length = end - start + 1;
    let kind = header("content-type").unwrap_or_else(|| "application/octet-stream".into());
    let status = if ranged { "206 Partial Content" } else { "200 OK" };
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\nConnection: close\r\n");
    if ranged {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    if head_only {
        return out.flush();
    }
    let mut at = start;
    let mut piece = first;
    loop {
        let take = piece.body.len().min((end + 1 - at) as usize);
        if take == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the other device sent nothing"));
        }
        out.write_all(&piece.body[..take])?;
        at += take as u64;
        if at > end {
            return out.flush();
        }
        piece = relay(source, Part::Media, id, Some(&format!("bytes={at}-{end}")));
        if piece.status != 206 {
            return Err(io::Error::other(format!("the other device answered {}", piece.status)));
        }
    }
}

fn write_relayed(out: &mut impl Write, relayed: &Relayed) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", relayed.status, super::http::reason(relayed.status));
    for (name, value) in &relayed.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    out.write_all(head.as_bytes())?;
    out.write_all(&relayed.body)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relay_asks_for_one_piece_at_a_time() {
        let last = RELAY_PIECE - 1;
        assert_eq!(piece_of(None), format!("bytes=0-{last}"));
        assert_eq!(piece_of(Some("bytes=0-")), format!("bytes=0-{last}"));
        assert_eq!(piece_of(Some("bytes=5000000-")), format!("bytes=5000000-{}", 5_000_000 + last));
        assert_eq!(piece_of(Some("bytes=10-20")), "bytes=10-20");
        assert_eq!(piece_of(Some("bytes=-500")), "bytes=-500");
        assert_eq!(piece_of(Some("bytes=-99999999")), format!("bytes=-{RELAY_PIECE}"));
        assert_eq!(piece_of(Some("nonsense")), format!("bytes=0-{last}"));
    }

    #[test]
    fn only_plain_video_ids_reach_a_path() {
        assert!(is_safe_id("dQw4w9WgXcQ") && is_safe_id("a-b_c"));
        for bad in ["", "../x", "a/b", "a?t=1", "a b", "ñ"] {
            assert!(!is_safe_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_pass_is_ended_only_by_the_device_that_started_it() {
        let inbox = Inbox::default();
        let source = |from: &str| CastSource { from: from.into(), host: "127.0.0.1".into(), port: 1, fingerprint: "0f".repeat(32), ticket: "ab".repeat(32) };
        inbox.set(source("phone"));
        assert!(!inbox.end_from("tablet"));
        assert!(inbox.current().is_some());
        assert!(inbox.end_from("phone"));
        assert!(inbox.current().is_none());
    }

    fn shared(name: &str) -> SharedFile {
        SharedFile { media: PathBuf::from(format!("/m/{name}.mp3")), cover: None }
    }

    #[test]
    fn a_ticket_opens_only_the_files_it_was_given_and_only_while_it_lasts() {
        let tickets = Tickets::default();
        let ticket = tickets.grant([("a".to_string(), shared("a"))]);
        assert!(is_ticket(&ticket));
        assert!(tickets.open(&ticket, "a", Part::Media).is_ok());
        assert_eq!(tickets.open(&ticket, "b", Part::Media).err(), Some(Refusal::NotFound));
        assert_eq!(tickets.open(&"0".repeat(64), "a", Part::Media).err(), Some(Refusal::NotFound));
        assert_eq!(tickets.open(&ticket, "a", Part::Cover).err(), Some(Refusal::NotFound), "no picture was given");
        // A new pass ends the one before.
        let newer = tickets.grant([("b".to_string(), shared("b"))]);
        assert_ne!(newer, ticket);
        assert_eq!(tickets.open(&ticket, "a", Part::Media).err(), Some(Refusal::NotFound));
        tickets.revoke();
        assert!(!tickets.active());
        assert_eq!(tickets.open(&newer, "b", Part::Media).err(), Some(Refusal::NotFound));
    }

    #[test]
    fn only_a_few_reads_are_open_at_once_and_a_finished_one_frees_its_place() {
        let tickets = Tickets::default();
        let ticket = tickets.grant([("a".to_string(), shared("a"))]);
        let first = tickets.open(&ticket, "a", Part::Media).unwrap();
        let _others: Vec<_> = (1..MAX_READS_PER_TICKET).map(|_| tickets.open(&ticket, "a", Part::Media).unwrap()).collect();
        assert_eq!(tickets.open(&ticket, "a", Part::Media).err(), Some(Refusal::Busy));
        drop(first);
        assert!(tickets.open(&ticket, "a", Part::Media).is_ok());
    }

    #[test]
    fn ranges_are_read_the_way_players_send_them() {
        assert_eq!(parse_range(None, 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=0-"), 100), Ok(Some((0, 99))));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Ok(Some((10, 19))));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Ok(Some((90, 99))), "an end past the file is cut");
        assert_eq!(parse_range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Ok(Some((0, 99))));
        for bad in ["bytes=100-", "bytes=20-10", "bytes=-0", "bytes=-", "items=0-1", "bytes=0-1,5-6", "bytes=a-b"] {
            assert_eq!(parse_range(Some(bad), 100), Err(Unsatisfiable), "{bad}");
        }
        assert_eq!(parse_range(Some("bytes=0-"), 0), Err(Unsatisfiable), "an empty file has no bytes to ask for");
    }

    #[test]
    fn a_range_is_served_with_its_head_and_only_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.mp3");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();

        let mut out = Vec::new();
        assert_eq!(serve_file(&mut out, &path, Some("bytes=100000-"), false).unwrap(), 100_000);
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let head = String::from_utf8_lossy(&out[..split]);
        assert!(head.starts_with("HTTP/1.1 206 Partial Content\r\n"), "{head}");
        assert!(head.contains("Content-Type: audio/mpeg\r\n") && head.contains("Content-Length: 100000\r\n"));
        assert!(head.contains("Content-Range: bytes 100000-199999/200000\r\n") && head.contains("Accept-Ranges: bytes\r\n"));
        assert_eq!(&out[split..], &data[100_000..]);

        let mut whole = Vec::new();
        assert_eq!(serve_file(&mut whole, &path, None, false).unwrap(), 200_000);
        assert!(whole.starts_with(b"HTTP/1.1 200 OK\r\n"));

        let mut head_only = Vec::new();
        assert_eq!(serve_file(&mut head_only, &path, None, true).unwrap(), 0);
        assert!(head_only.ends_with(b"\r\n\r\n"));

        let mut refused = Vec::new();
        assert_eq!(serve_file(&mut refused, &path, Some("bytes=999999-"), false).unwrap(), 0);
        assert!(String::from_utf8_lossy(&refused).contains("416") && String::from_utf8_lossy(&refused).contains("bytes */200000"));
    }

    #[test]
    fn the_type_follows_the_extension() {
        assert_eq!(content_type(Path::new("a/b.MP3")), "audio/mpeg");
        assert_eq!(content_type(Path::new("v.webm")), "video/webm");
        assert_eq!(content_type(Path::new("cover.jpg")), "image/jpeg");
        assert_eq!(content_type(Path::new("x")), "application/octet-stream");
    }
}
