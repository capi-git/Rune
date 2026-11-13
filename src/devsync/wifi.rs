//! Sync over Wi-Fi (docs/sync-wifi.md): the Mac serves its own sync folder
//! to the iPhones that are paired with it, over HTTPS with a certificate of
//! its own. mobile/scripts/wifi-mac.py does the same in Python and is the
//! reference; the iPhone's side is mobile/Core/Sources/RuneCore/Wifi*.swift.
//!
//! What is kept, in `<state dir>/wifi/` (the folder and the secrets in it
//! only the user can read): `cert.der` and `key.der` (made once; a new one
//! means every iPhone pairs again), `port` (picked once), and `phones.json`
//! (the SHA-256 of each paired iPhone's token, with its device id and name;
//! never the token).
//!
//! The listener is plain threads blocked in `accept` and `read`: it takes no
//! power assertion, so it never keeps the Mac awake. Nothing is answered
//! without a known token except `POST /v1/pair`.

// The app does not start the server yet (Settings → Sync wires it up).


mod http;

use super::format::{self, SyncPath};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long a pairing code works.
const CODE_LIFE_MS: i64 = 10 * 60 * 1000;
/// The port tried first, the first time.
const FIRST_PORT: u16 = 47815;
/// The largest file an iPhone may send.
const PUT_LIMIT: u64 = 256 << 20;
/// The largest device file an iPhone may send.
const DEVICE_FILE_LIMIT: u64 = 1 << 20;
/// A body up to this length is read away before a refusal; a longer one
/// closes the connection instead.
const DRAIN_LIMIT: u64 = 64 << 10;
/// Connections open at once; more are closed at once.
const CONNECTION_LIMIT: usize = 32;
/// A connection that says nothing for this long is closed.
const IDLE: Duration = Duration::from_secs(30);
const JSON: &str = "application/json";

/// What the server needs to know.
pub struct Config {
    /// The folder served: `devsync::own_folder(db)`.
    pub folder: PathBuf,
    /// The Mac's sync state: `devsync::state_dir(db)`.
    pub state: PathBuf,
    /// The Mac's device id in the sync files.
    pub device: String,
    /// The Mac's name, for "Connect to …?" on the iPhone.
    pub name: String,
}

/// A QR code to show: the pairing link and the code itself.
#[derive(Debug, Clone)]
pub struct Offer {
    #[allow(dead_code)] // what the QR code holds; tests read it
    pub link: String,
    /// An `<svg>` with a `viewBox` and no size: dark modules are filled with
    /// `currentColor`, the quiet zone (4 modules) is included and is clear.
    pub qr_svg: String,
    /// When the code stops working (ms since 1970).
    pub expires_ms: i64,
}

/// An iPhone that is paired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub device: String,
    pub name: String,
    pub at_ms: i64,
}

/// The listener. It stops when dropped.
pub struct Server {
    shared: Arc<Shared>,
    accepts: Vec<(JoinHandle<()>, Vec<SocketAddr>)>,
    port: u16,
}

impl Server {
    /// Start listening on all networks (IPv4 and IPv6), off the UI thread, on
    /// the port this Mac keeps (47815 when it was free the first time).
    /// `on_put` is called after an iPhone wrote a file into the folder.
    pub fn start(cfg: Config, on_put: Arc<dyn Fn() + Send + Sync>) -> Result<Server, String> {
        let shared = Shared::new(cfg, on_put, None)?;
        let kept = kept_port(&shared.wifi);
        let listener = match kept {
            Some(port) => listen_everywhere(port).map_err(|e| format!("Sync over Wi-Fi cannot listen on port {port}: {e}"))?,
            None => listen_everywhere(FIRST_PORT).or_else(|_| listen_everywhere(0)).map_err(|e| format!("Sync over Wi-Fi cannot listen: {e}"))?,
        };
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        if kept != Some(port) {
            write_private(&shared.wifi.join("port"), format!("{port}\n").as_bytes()).map_err(|e| format!("Cannot keep the port: {e}"))?;
        }
        let wake = vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port), SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)];
        Ok(Server::run(shared, vec![(listener, wake)], port))
    }

    /// Start on this computer only (127.0.0.1 and ::1), on `port` (0: any
    /// free one), with "localhost" in the link; the port is not kept. For
    /// tests, which must not open the Mac to the network.
    /// For tests and self-test runs: this Mac only.
    pub fn start_local(cfg: Config, port: u16, on_put: Arc<dyn Fn() + Send + Sync>) -> Result<Server, String> {
        let shared = Shared::new(cfg, on_put, Some(vec!["localhost".to_string()]))?;
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| e.to_string())?;
        let port = v4.local_addr().map_err(|e| e.to_string())?.port();
        let mut listeners = vec![(v4, vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)])];
        if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            listeners.push((v6, vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)]));
        }
        Ok(Server::run(shared, listeners, port))
    }

    fn run(shared: Arc<Shared>, listeners: Vec<(TcpListener, Vec<SocketAddr>)>, port: u16) -> Server {
        let accepts = listeners
            .into_iter()
            .map(|(listener, wake)| {
                let shared = shared.clone();
                let handle = std::thread::Builder::new()
                    .name("wifi-sync-accept".into())
                    .spawn(move || accept_loop(listener, shared))
                    .expect("a thread for sync over Wi-Fi");
                (handle, wake)
            })
            .collect();
        Server { shared, accepts, port }
    }

    /// A new pairing code (10 minutes, one use; it replaces the one before),
    /// as the link of the QR code and the QR code itself.
    pub fn offer(&self) -> Result<Offer, String> {
        self.shared.offer(self.port)
    }

    /// Kill the open code (the window that showed it closed).
    pub fn withdraw(&self) {
        *lock(&self.shared.code) = None;
    }

    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop listening and close every connection.
    pub fn stop(self) {}

    fn halt(&mut self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        *lock(&self.shared.code) = None;
        for (handle, wake) in self.accepts.drain(..) {
            // A connection wakes the thread blocked in `accept`; it sees
            // `stopping` and ends. When none gets through, it is left to end
            // with the next connection.
            let woke = wake.iter().any(|addr| TcpStream::connect_timeout(addr, Duration::from_secs(1)).is_ok());
            if woke {
                let _ = handle.join();
            }
        }
        for (_, stream) in lock(&self.shared.open).drain() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The iPhones paired with this Mac, the first paired first.
pub fn paired(state: &Path) -> Vec<Paired> {
    let _guard = lock(&PHONES);
    let mut list: Vec<Paired> = load_phones(&wifi_dir(state)).into_values().map(|p| Paired { device: p.device, name: p.name, at_ms: p.at }).collect();
    list.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then_with(|| a.device.cmp(&b.device)));
    list
}

/// Forget an iPhone: its token stops working at once.
pub fn forget(state: &Path, device: &str) -> Result<(), String> {
    let wifi = wifi_dir(state);
    let _guard = lock(&PHONES);
    let mut phones = load_phones(&wifi);
    let before = phones.len();
    phones.retain(|_, p| p.device != device);
    if phones.len() != before {
        save_phones(&wifi, &phones).map_err(|e| format!("Cannot forget the iPhone: {e}"))?;
    }
    Ok(())
}

/// The fingerprint of the Mac's certificate (made the first time): SHA-256 of
/// its DER form, 64 lower-case hex characters.
#[allow(dead_code)] // for a later "which Mac is this" line in Settings
pub fn fingerprint(state: &Path) -> Result<String, String> {
    let (cert, _) = certificate(&wifi_dir(state))?;
    Ok(sha256_hex(&cert))
}

/// The Mac's name as the user set it (System Settings → General → About).
pub fn computer_name() -> String {
    scutil("ComputerName").unwrap_or_else(|| "Mac".into())
}

// MARK: The server's state

struct Code {
    text: String,
    until_ms: i64,
}

struct Shared {
    cfg: Config,
    /// `<state>/wifi`
    wifi: PathBuf,
    fingerprint: String,
    tls: Arc<rustls::ServerConfig>,
    /// Hosts for the link in place of this Mac's names (tests).
    hosts: Option<Vec<String>>,
    code: Mutex<Option<Code>>,
    on_put: Arc<dyn Fn() + Send + Sync>,
    stopping: AtomicBool,
    active: AtomicUsize,
    next_id: AtomicU64,
    /// The open connections, to close them on stop.
    open: Mutex<HashMap<u64, TcpStream>>,
}

impl Shared {
    fn new(cfg: Config, on_put: Arc<dyn Fn() + Send + Sync>, hosts: Option<Vec<String>>) -> Result<Arc<Shared>, String> {
        if !format::valid_device(&cfg.device) {
            return Err("The Mac has no device id for sync yet.".into());
        }
        let wifi = wifi_dir(&cfg.state);
        fs::create_dir_all(&cfg.folder).map_err(|e| format!("Cannot make the sync folder: {e}"))?;
        let (cert, key) = certificate(&wifi)?;
        let fingerprint = sha256_hex(&cert);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
            .map_err(|e| format!("The certificate cannot be used: {e}"))?;
        Ok(Arc::new(Shared {
            cfg,
            wifi,
            fingerprint,
            tls: Arc::new(tls),
            hosts,
            code: Mutex::new(None),
            on_put,
            stopping: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            next_id: AtomicU64::new(0),
            open: Mutex::new(HashMap::new()),
        }))
    }

    fn offer(&self, port: u16) -> Result<Offer, String> {
        let hosts = match &self.hosts {
            Some(h) => h.clone(),
            None => lan_hosts(),
        };
        if hosts.is_empty() {
            return Err("This Mac is on no network now.".into());
        }
        let code = random_text(16);
        let expires_ms = now_ms() + CODE_LIFE_MS;
        let link = pair_link(&hosts, port, &self.fingerprint, &code, &self.cfg.name);
        let qr_svg = qr_svg(&link)?;
        *lock(&self.code) = Some(Code { text: code, until_ms: expires_ms });
        Ok(Offer { link, qr_svg, expires_ms })
    }

    // MARK: Requests

    /// Answer one request whose head was read. Returns whether the
    /// connection stays open.
    fn respond<S: Read + Write>(&self, head: &http::Head, conn: &mut BufReader<S>) -> io::Result<bool> {
        let path = head.path.as_deref().unwrap_or("");
        if head.method == "POST" && path == "/v1/pair" {
            return self.pair(head, conn);
        }
        let Some(phone) = head.token.as_deref().and_then(|t| find_phone(&self.wifi, t)) else {
            return refuse(conn, head, 401);
        };
        match (head.method.as_str(), path) {
            ("GET", "/v1/files") => {
                let kept = drain(conn, head.length)?;
                let body = self.listing();
                http::answer(conn.get_mut(), 200, JSON, &body, head.close || !kept)
            }
            ("GET", p) if p.starts_with("/v1/file/") => self.send_file(&p["/v1/file/".len()..], head, conn),
            ("PUT", p) if p.starts_with("/v1/file/") => self.receive_file(&phone, &p["/v1/file/".len()..], head, conn),
            ("DELETE", "/v1/pair") => {
                let kept = drain(conn, head.length)?;
                let _guard = lock(&PHONES);
                let mut phones = load_phones(&self.wifi);
                if let Some(t) = head.token.as_deref() {
                    phones.remove(&sha256_hex(t.as_bytes()));
                }
                let status = if save_phones(&self.wifi, &phones).is_ok() { 204 } else { 500 };
                http::answer(conn.get_mut(), status, JSON, b"", head.close || !kept)
            }
            _ => refuse(conn, head, 404),
        }
    }

    /// `POST /v1/pair`
    fn pair<S: Read + Write>(&self, head: &http::Head, conn: &mut BufReader<S>) -> io::Result<bool> {
        if head.length > 4096 {
            return http::answer(conn.get_mut(), 403, JSON, b"", true);
        }
        let mut body = Vec::new();
        (&mut *conn).take(head.length).read_to_end(&mut body)?;
        if body.len() as u64 != head.length {
            return Ok(false);
        }
        let (status, answer) = match self.try_pair(&body) {
            Ok(answer) => (200, answer),
            Err(status) => (status, Vec::new()),
        };
        http::answer(conn.get_mut(), status, JSON, &answer, head.close)
    }

    fn try_pair(&self, body: &[u8]) -> Result<Vec<u8>, u16> {
        let request: serde_json::Value = serde_json::from_slice(body).map_err(|_| 403u16)?;
        let text = |key: &str| request.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (code, device) = (text("code"), text("device"));
        let mut slot = lock(&self.code);
        let good = slot.as_ref().is_some_and(|c| same(c.text.as_bytes(), code.as_bytes()) && now_ms() < c.until_ms);
        if !good || !format::valid_device(&device) || device == self.cfg.device {
            return Err(403);
        }
        let token = random_text(32);
        let phone = Phone { device: device.clone(), name: clean(&text("name"), 80), kind: clean(&text("kind"), 20), at: now_ms() };
        {
            let _guard = lock(&PHONES);
            let mut phones = load_phones(&self.wifi);
            // An iPhone that pairs again takes the place of its earlier entry.
            phones.retain(|_, p| p.device != device);
            phones.insert(sha256_hex(token.as_bytes()), phone);
            save_phones(&self.wifi, &phones).map_err(|_| 500u16)?;
        }
        *slot = None; // used
        let answer = serde_json::json!({"token": token, "device": self.cfg.device, "name": self.cfg.name});
        Ok(answer.to_string().into_bytes())
    }

    /// `GET /v1/files`: every file of the folder of the four kinds, with the
    /// device files whole. A few directory reads: it is asked every 10 s.
    fn listing(&self) -> Vec<u8> {
        let root = &self.cfg.folder;
        let mut devices = Vec::new();
        let mut files: Vec<(String, u64)> = Vec::new();
        for (name, _) in regular_files(&root.join("devices")) {
            let rel = format!("devices/{name}");
            if matches!(SyncPath::parse(&rel), Some(SyncPath::Device(_))) {
                if let Ok(value) = fs::read(root.join(&rel)).map_err(|_| ()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|_| ())) {
                    devices.push(value);
                }
            }
        }
        for kind in ["snapshots", "changes"] {
            for device in subfolders(&root.join(kind)) {
                for (name, size) in regular_files(&root.join(kind).join(&device)) {
                    let rel = format!("{kind}/{device}/{name}");
                    if SyncPath::parse(&rel).is_some() {
                        files.push((rel, size));
                    }
                }
            }
        }
        for (name, size) in regular_files(&root.join("files")) {
            let rel = format!("files/{name}");
            if SyncPath::parse(&rel).is_some() {
                files.push((rel, size));
            }
        }
        files.sort();
        let files: Vec<_> = files.into_iter().map(|(path, size)| serde_json::json!({"path": path, "size": size})).collect();
        serde_json::json!({
            "format": format::FORMAT,
            "device": self.cfg.device,
            "name": self.cfg.name,
            "devices": devices,
            "files": files,
        })
        .to_string()
        .into_bytes()
    }

    /// `GET /v1/file/<path>`
    fn send_file<S: Read + Write>(&self, rel: &str, head: &http::Head, conn: &mut BufReader<S>) -> io::Result<bool> {
        let Some(path) = SyncPath::parse(rel) else { return refuse(conn, head, 404) };
        let full = self.cfg.folder.join(path.path());
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&full);
        let Ok((file, length)) = file.and_then(|f| {
            let meta = f.metadata()?;
            if meta.is_file() { Ok((f, meta.len())) } else { Err(io::ErrorKind::NotFound.into()) }
        }) else {
            return refuse(conn, head, 404);
        };
        let kept = drain(conn, head.length)?;
        let out = conn.get_mut();
        http::write_head(out, 200, "application/octet-stream", length, head.close || !kept)?;
        let sent = io::copy(&mut file.take(length), out)?;
        out.flush()?;
        // A file that got shorter on the way (it never should): the client
        // sees the connection close before the length it was promised.
        Ok(sent == length && !head.close && kept)
    }

    /// `PUT /v1/file/<path>`: an iPhone writes only its own files (and
    /// pictures); a file other than a device file is written once.
    fn receive_file<S: Read + Write>(&self, phone: &Phone, rel: &str, head: &http::Head, conn: &mut BufReader<S>) -> io::Result<bool> {
        let Some(path) = SyncPath::parse(rel) else { return refuse(conn, head, 403) };
        if path.device().is_some_and(|d| d != phone.device) {
            return refuse(conn, head, 403);
        }
        let is_device_file = matches!(path, SyncPath::Device(_));
        if head.length > PUT_LIMIT || (is_device_file && head.length > DEVICE_FILE_LIMIT) {
            return http::answer(conn.get_mut(), 413, JSON, b"", true);
        }
        let full = self.cfg.folder.join(path.path());
        if !is_device_file {
            if let Ok(meta) = fs::symlink_metadata(&full) {
                // Written once: the same file again is fine, another is not.
                let status = if meta.is_file() && meta.len() == head.length { 204 } else { 409 };
                io::copy(&mut (&mut *conn).take(head.length), &mut io::sink())?;
                return http::answer(conn.get_mut(), status, JSON, b"", head.close);
            }
        }

        // Written outside the folder first, then moved in.
        let temp = self.wifi.join(format!("put-{}", random_hex(8)));
        let written = (|| -> io::Result<u64> {
            let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)?;
            let n = io::copy(&mut (&mut *conn).take(head.length), &mut file)?;
            file.sync_all()?;
            Ok(n)
        })();
        let n = match written {
            Ok(n) => n,
            Err(e) => {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
        };
        if n != head.length {
            let _ = fs::remove_file(&temp);
            return Ok(false); // the iPhone went away
        }
        let status = self.place(&temp, &full, &path, phone, head.length);
        let _ = fs::remove_file(&temp);
        if status == 204 {
            (self.on_put)();
        }
        http::answer(conn.get_mut(), status, JSON, b"", head.close)
    }

    /// Move a received file into the folder. Returns the status to answer.
    fn place(&self, temp: &Path, full: &Path, path: &SyncPath, phone: &Phone, length: u64) -> u16 {
        if let SyncPath::Device(_) = path {
            // A device file must be one, and of the iPhone that sends it.
            let ok = fs::read(temp)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .is_some_and(|v| v.get("device").and_then(|d| d.as_str()) == Some(phone.device.as_str()));
            if !ok {
                return 400;
            }
        }
        if let Some(parent) = full.parent() {
            if fs::create_dir_all(parent).is_err() {
                return 500;
            }
        }
        match path {
            SyncPath::Device(_) => {
                if fs::rename(temp, full).is_ok() {
                    204
                } else {
                    500
                }
            }
            // A link fails when the name is taken: two sends at once cannot
            // replace a file.
            _ => match fs::hard_link(temp, full) {
                Ok(()) => 204,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match fs::symlink_metadata(full) {
                    Ok(meta) if meta.is_file() && meta.len() == length => 204,
                    _ => 409,
                },
                Err(_) => 500,
            },
        }
    }
}

/// Answer with an empty body, after reading away a small body (a long one
/// closes the connection instead).
fn refuse<S: Read + Write>(conn: &mut BufReader<S>, head: &http::Head, status: u16) -> io::Result<bool> {
    let kept = drain(conn, head.length)?;
    http::answer(conn.get_mut(), status, JSON, b"", head.close || !kept)
}

/// Read away a body that is not used. Returns false when it is too long to
/// read away: the connection is then closed after the answer.
fn drain<S: Read>(conn: &mut BufReader<S>, length: u64) -> io::Result<bool> {
    if length == 0 {
        return Ok(true);
    }
    if length > DRAIN_LIMIT {
        return Ok(false);
    }
    let n = io::copy(&mut (&mut *conn).take(length), &mut io::sink())?;
    Ok(n == length)
}

// MARK: Connections

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let tcp = match stream {
            Ok(tcp) => tcp,
            Err(e) if matches!(e.kind(), io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted) => continue,
            Err(_) => {
                // Out of files, most likely: wait a little.
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        if shared.active.fetch_add(1, Ordering::SeqCst) >= CONNECTION_LIMIT {
            shared.active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let shared2 = shared.clone();
        let spawned = std::thread::Builder::new().name("wifi-sync-conn".into()).spawn(move || {
            connection(&shared2, tcp);
            shared2.active.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            shared.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn connection(shared: &Shared, tcp: TcpStream) {
    let _ = tcp.set_read_timeout(Some(IDLE));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(60)));
    let _ = tcp.set_nodelay(true);
    let id = shared.next_id.fetch_add(1, Ordering::SeqCst);
    if let Ok(clone) = tcp.try_clone() {
        lock(&shared.open).insert(id, clone);
    }
    if !shared.stopping.load(Ordering::SeqCst) {
        if let Ok(tls) = rustls::ServerConnection::new(shared.tls.clone()) {
            let mut stream = serve(shared, rustls::StreamOwned::new(tls, tcp));
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
    }
    lock(&shared.open).remove(&id);
}

/// Answer requests on one connection until it closes. Gives the stream back.
fn serve<S: Read + Write>(shared: &Shared, stream: S) -> S {
    let mut conn = BufReader::with_capacity(16 << 10, stream);
    while !shared.stopping.load(Ordering::SeqCst) {
        let head = match http::read_head(&mut conn) {
            Ok(Some(head)) => head,
            Ok(None) | Err(http::HeadError::Io(_)) => break,
            Err(http::HeadError::Bad) => {
                let _ = http::answer(conn.get_mut(), 400, JSON, b"", true);
                break;
            }
        };
        match shared.respond(&head, &mut conn) {
            Ok(true) => {}
            _ => break,
        }
    }
    conn.into_inner()
}

/// One socket for IPv4 and IPv6 on every network.
fn listen_everywhere(port: u16) -> io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let v6 = (|| -> io::Result<TcpListener> {
        let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_only_v6(false)?;
        socket.set_reuse_address(true)?;
        socket.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port).into())?;
        socket.listen(64)?;
        Ok(socket.into())
    })();
    match v6 {
        Ok(listener) => Ok(listener),
        // IPv6 is off on this Mac: IPv4 alone.
        Err(e) if e.kind() != io::ErrorKind::AddrInUse => {
            let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
            socket.set_reuse_address(true)?;
            socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).into())?;
            socket.listen(64)?;
            Ok(socket.into())
        }
        Err(e) => Err(e),
    }
}

fn kept_port(wifi: &Path) -> Option<u16> {
    fs::read_to_string(wifi.join("port")).ok()?.trim().parse().ok().filter(|&p| p != 0)
}

// MARK: Paired iPhones

/// One lock for `phones.json` in this process (the server and Settings).
static PHONES: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Phone {
    device: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    at: i64,
}

/// `phones.json`: SHA-256 of the token (hex) → the iPhone.
fn load_phones(wifi: &Path) -> BTreeMap<String, Phone> {
    fs::read(wifi.join("phones.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_phones(wifi: &Path, phones: &BTreeMap<String, Phone>) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(phones).map_err(io::Error::other)?;
    write_private(&wifi.join("phones.json"), &text)
}

/// The paired iPhone that holds this token. Every entry is compared, each in
/// constant time.
fn find_phone(wifi: &Path, token: &str) -> Option<Phone> {
    if token.is_empty() {
        return None;
    }
    let hash = sha256_hex(token.as_bytes());
    let _guard = lock(&PHONES);
    let mut found = None;
    for (key, phone) in load_phones(wifi) {
        if same(key.as_bytes(), hash.as_bytes()) {
            found = Some(phone);
        }
    }
    found
}

// MARK: The certificate

static CERTIFICATE: Mutex<()> = Mutex::new(());

/// The certificate (DER) and its private key (PKCS #8 DER), made the first
/// time: self-signed, ECDSA P-256 with SHA-256.
fn certificate(wifi: &Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    let _guard = lock(&CERTIFICATE);
    let (cert_path, key_path) = (wifi.join("cert.der"), wifi.join("key.der"));
    if let (Ok(cert), Ok(key)) = (fs::read(&cert_path), fs::read(&key_path)) {
        if !cert.is_empty() && !key.is_empty() {
            return Ok((cert, key));
        }
    }
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(|e| e.to_string())?;
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).map_err(|e| e.to_string())?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params.distinguished_name.push(rcgen::DnType::CommonName, "Rune");
    // The iPhone checks the fingerprint, not the dates.
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2126, 1, 1);
    let cert = params.self_signed(&key).map_err(|e| e.to_string())?;
    let (cert, key) = (cert.der().to_vec(), key.serialize_der());
    write_private(&key_path, &key).map_err(|e| format!("Cannot keep the key: {e}"))?;
    write_private(&cert_path, &cert).map_err(|e| format!("Cannot keep the certificate: {e}"))?;
    Ok((cert, key))
}

// MARK: The link and the QR code

/// `rune://pair?v=1&host=…&port=…&cert=…&code=…&name=…` (A Mac before the
/// name Rune gave `notion2://pair?…` with the same fields; the iPhone reads
/// both.)
fn pair_link(hosts: &[String], port: u16, fingerprint: &str, code: &str, name: &str) -> String {
    let mut link = String::from("rune://pair?v=1");
    for host in hosts {
        link.push_str(&format!("&host={}", http::percent_encode(host)));
    }
    link.push_str(&format!("&port={port}&cert={fingerprint}&code={}&name={}", http::percent_encode(code), http::percent_encode(name)));
    link
}

/// The QR code of a text as an SVG: one path in `currentColor`, with the
/// quiet zone of 4 modules around it.
fn qr_svg(text: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 4;
    let mut d = String::new();
    for y in 0..width {
        let mut x = 0;
        while x < width {
            if colors[y * width + x] == qrcode::Color::Dark {
                let start = x;
                while x < width && colors[y * width + x] == qrcode::Color::Dark {
                    x += 1;
                }
                d.push_str(&format!("M{} {}h{}v1h-{}z", start + quiet, y + quiet, x - start, x - start));
            } else {
                x += 1;
            }
        }
    }
    let size = width + 2 * quiet;
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" shape-rendering="crispEdges" role="img" aria-label="QR code"><path fill="currentColor" d="{d}"/></svg>"#
    ))
}

/// Where an iPhone finds this Mac: its local network name, then its IPv4
/// addresses now (the one with the default route first). At most four.
fn lan_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    if let Some(name) = scutil("LocalHostName") {
        hosts.push(format!("{name}.local"));
    }
    for address in ipv4_addresses() {
        hosts.push(address.to_string());
    }
    let mut seen = std::collections::HashSet::new();
    hosts.retain(|h| valid_host(h) && seen.insert(h.clone()));
    hosts.truncate(4);
    hosts
}

/// Letters, digits, dots and hyphens (a name or an IPv4 address).
fn valid_host(h: &str) -> bool {
    (1..=253).contains(&h.len()) && !h.starts_with(['.', '-']) && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

fn ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut list = Vec::new();
    // The address of the default route; nothing is sent.
    if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        if socket.connect(("192.0.2.1", 9)).is_ok() {
            if let Ok(SocketAddr::V4(a)) = socket.local_addr() {
                list.push(*a.ip());
            }
        }
    }
    // SAFETY: getifaddrs gives a list that stays valid until freeifaddrs;
    // each entry is read before that, and only through its own pointers.
    unsafe {
        let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut first) == 0 {
            let mut entry = first;
            while !entry.is_null() {
                let e = &*entry;
                let flags = e.ifa_flags as libc::c_int;
                let up = flags & libc::IFF_UP != 0 && flags & libc::IFF_RUNNING != 0 && flags & libc::IFF_LOOPBACK == 0;
                if up && !e.ifa_addr.is_null() && (*e.ifa_addr).sa_family as libc::c_int == libc::AF_INET {
                    let sin = &*(e.ifa_addr as *const libc::sockaddr_in);
                    list.push(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
                }
                entry = e.ifa_next;
            }
            libc::freeifaddrs(first);
        }
    }
    list.retain(|a| !a.is_loopback() && !a.is_link_local() && !a.is_unspecified());
    list.dedup();
    list
}

fn scutil(key: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/scutil").args(["--get", key]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

// MARK: Small things

fn wifi_dir(state: &Path) -> PathBuf {
    state.join("wifi")
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Equal, in a time that depends only on the lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    std::hint::black_box(a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y))) == 0
}

fn random_bytes(n: usize) -> Vec<u8> {
    use ring::rand::SecureRandom;
    let mut bytes = vec![0u8; n];
    ring::rand::SystemRandom::new().fill(&mut bytes).expect("the system's random numbers");
    bytes
}

/// `n` random bytes as base64 for URLs, without padding (`A-Z a-z 0-9 - _`).
fn random_text(n: usize) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(n))
}

fn random_hex(n: usize) -> String {
    random_bytes(n).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// A name from the iPhone: no control characters, at most `max` characters.
fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// Write a file only the user can read (its folder too), whole or not at all.
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no folder"))?;
    if !dir.is_dir() {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let temp = dir.join(format!(".{name}.{}", random_hex(6)));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// The regular files of a folder (no links), with their lengths.
fn regular_files(dir: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?; // the entry itself, not where a link points
            let name = e.file_name().into_string().ok()?;
            (meta.is_file() && !name.starts_with('.')).then_some((name, meta.len()))
        })
        .collect()
}

/// The folders of a folder whose names are device ids.
fn subfolders(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            (e.file_type().ok()?.is_dir() && format::valid_device(&name)).then_some(name)
        })
        .collect()
}

#[cfg(test)]
mod tests;
