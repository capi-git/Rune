//! Tests of sync over Wi-Fi: the rules of docs/sync-wifi.md on a folder in
//! the temporary folder, most of them without a network (requests go
//! through `serve` as bytes), one through TLS on 127.0.0.1. Nothing listens
//! on any other network.

use super::*;
use std::sync::atomic::AtomicUsize;

const MAC: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PHONE: &str = "cccccccccccccccccccccccccccccccc";
const OTHER: &str = "dddddddddddddddddddddddddddddddd";

/// A request and an answer as bytes.
struct Duplex {
    input: io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Read for Duplex {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for Duplex {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

/// The answers in a stream of bytes; each must have a `Content-Length`.
fn answers(mut bytes: &[u8]) -> Vec<Answer> {
    let mut list = Vec::new();
    while !bytes.is_empty() {
        let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").expect("a whole head");
        let head = std::str::from_utf8(&bytes[..end]).unwrap();
        let mut lines = head.split("\r\n");
        let status: u16 = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
        let headers: Vec<(String, String)> = lines.map(|l| l.split_once(": ").map(|(a, b)| (a.to_ascii_lowercase(), b.to_string())).unwrap()).collect();
        let length: usize = headers.iter().find(|(k, _)| k == "content-length").expect("Content-Length on every answer").1.parse().unwrap();
        let body = bytes[end + 4..end + 4 + length].to_vec();
        bytes = &bytes[end + 4 + length..];
        list.push(Answer { status, headers, body });
    }
    list
}

fn request(method: &str, path: &str, token: Option<&str>, body: &[u8]) -> Vec<u8> {
    let mut r = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
    if let Some(t) = token {
        r.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    if !body.is_empty() {
        r.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    r.push_str("\r\n");
    let mut bytes = r.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

struct Scene {
    base: PathBuf,
    shared: Arc<Shared>,
    puts: Arc<AtomicUsize>,
}

impl Scene {
    fn new() -> Scene {
        let base = std::env::temp_dir().join(format!("rune-wifi-test-{}", random_hex(8)));
        let state = base.join("Sync");
        let folder = state.join("sync-v1");
        fs::create_dir_all(folder.join("devices")).unwrap();
        fs::write(folder.join(format!("devices/{MAC}.json")), format!(r#"{{"format":1,"device":"{MAC}","name":"Test Mac","kind":"mac","seq":2}}"#)).unwrap();
        let puts = Arc::new(AtomicUsize::new(0));
        let counter = puts.clone();
        let cfg = Config { folder, state, device: MAC.into(), name: "Test Mac".into() };
        let shared = Shared::new(cfg, Arc::new(move || _ = counter.fetch_add(1, Ordering::SeqCst)), Some(vec!["localhost".into()])).unwrap();
        Scene { base, shared, puts }
    }

    fn folder(&self) -> &Path {
        &self.shared.cfg.folder
    }

    fn state(&self) -> &Path {
        &self.shared.cfg.state
    }

    /// Send requests on one connection; give the answers.
    fn send(&self, bytes: Vec<u8>) -> Vec<Answer> {
        let out = serve(&self.shared, Duplex { input: io::Cursor::new(bytes), output: Vec::new() });
        answers(&out.output)
    }

    fn ask(&self, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> Answer {
        let mut list = self.send(request(method, path, token, body));
        assert_eq!(list.len(), 1, "{method} {path}");
        list.remove(0)
    }

    fn code(&self) -> String {
        let offer = self.shared.offer(47815).unwrap();
        let code = offer.link.split('&').find_map(|p| p.strip_prefix("code=")).unwrap().to_string();
        code
    }

    fn pair_with(&self, code: &str, device: &str) -> Answer {
        let body = serde_json::json!({"code": code, "device": device, "name": "Test iPhone", "kind": "iphone"}).to_string();
        self.ask("POST", "/v1/pair", None, body.as_bytes())
    }

    /// Pair the phone; give its token.
    fn pair(&self, device: &str) -> String {
        let answer = self.pair_with(&self.code(), device);
        assert_eq!(answer.status, 200);
        answer.json()["token"].as_str().unwrap().to_string()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn a_code_pairs_one_iphone_once() {
    let scene = Scene::new();
    let offer = scene.shared.offer(47815).unwrap();
    let fp = fingerprint(scene.state()).unwrap();
    assert!(offer.link.starts_with(&format!("notion2://pair?v=1&host=localhost&port=47815&cert={fp}&code=")), "{}", offer.link);
    assert!(offer.link.ends_with("&name=Test%20Mac"));
    assert!(offer.expires_ms > now_ms() + 9 * 60 * 1000);
    let code = offer.link.split('&').find_map(|p| p.strip_prefix("code=")).unwrap().to_string();
    assert!(code.len() >= 16 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "{code}");

    // A wrong code, a device id that is not one, the Mac's own id: nothing.
    for (c, d) in [("aaaaaaaaaaaaaaaaaaaaaa", PHONE), (code.as_str(), "CCCC"), (code.as_str(), MAC)] {
        let a = scene.pair_with(c, d);
        assert_eq!((a.status, a.body.len()), (403, 0));
    }
    assert_eq!(scene.ask("POST", "/v1/pair", None, b"not json").status, 403);

    let a = scene.pair_with(&code, PHONE);
    assert_eq!(a.status, 200);
    let json = a.json();
    let token = json["token"].as_str().unwrap();
    assert_eq!(token.len(), 43);
    assert!(token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    assert_eq!((json["device"].as_str(), json["name"].as_str()), (Some(MAC), Some("Test Mac")));

    // Used: it works no more.
    assert_eq!(scene.pair_with(&code, OTHER).status, 403);

    // Only the hash of the token is kept, in a file only the user can read.
    let kept = fs::read_to_string(scene.state().join("wifi/phones.json")).unwrap();
    assert!(!kept.contains(token));
    assert!(kept.contains(&sha256_hex(token.as_bytes())));
    for name in ["phones.json", "key.der"] {
        let mode = fs::metadata(scene.state().join("wifi").join(name)).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{name}");
    }
    assert_eq!(fs::metadata(scene.state().join("wifi")).unwrap().permissions().mode() & 0o077, 0);
    let list = paired(scene.state());
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].device.as_str(), list[0].name.as_str()), (PHONE, "Test iPhone"));
}

#[test]
fn a_new_code_kills_the_old_one_and_so_do_time_and_withdraw() {
    let scene = Scene::new();
    let old = scene.code();
    let new = scene.code();
    assert_ne!(old, new);
    assert_eq!(scene.pair_with(&old, PHONE).status, 403);

    lock(&scene.shared.code).as_mut().unwrap().until_ms = now_ms() - 1;
    assert_eq!(scene.pair_with(&new, PHONE).status, 403);

    let code = scene.code();
    *lock(&scene.shared.code) = None; // what withdraw does
    assert_eq!(scene.pair_with(&code, PHONE).status, 403);
    assert!(paired(scene.state()).is_empty());
}

#[test]
fn nothing_is_answered_without_a_known_token() {
    let scene = Scene::new();
    let token = scene.pair(PHONE);
    let file = format!("/v1/file/devices/{MAC}.json");
    for token in [None, Some("x".repeat(43).as_str()), Some("")] {
        for (method, path) in [("GET", "/v1/files"), ("GET", file.as_str()), ("PUT", "/v1/file/files/a.png"), ("DELETE", "/v1/pair"), ("GET", "/"), ("POST", "/v1/files")] {
            assert_eq!(scene.ask(method, path, token, b"").status, 401, "{method} {path} {token:?}");
        }
    }
    assert_eq!(scene.ask("GET", "/v1/files", Some(&token), b"").status, 200);
    assert_eq!(scene.ask("GET", "/nothing", Some(&token), b"").status, 404);

    // Paired again: the earlier token is dead, the entry replaced.
    let again = scene.pair(PHONE);
    assert_eq!(scene.ask("GET", "/v1/files", Some(&token), b"").status, 401);
    assert_eq!(paired(scene.state()).len(), 1);

    // "Disconnect" on the iPhone.
    assert_eq!(scene.ask("DELETE", "/v1/pair", Some(&again), b"").status, 204);
    assert_eq!(scene.ask("GET", "/v1/files", Some(&again), b"").status, 401);
    assert!(paired(scene.state()).is_empty());

    // Removed in Settings on the Mac.
    let third = scene.pair(PHONE);
    let other = scene.pair(OTHER);
    forget(scene.state(), PHONE).unwrap();
    assert_eq!(scene.ask("GET", "/v1/files", Some(&third), b"").status, 401);
    assert_eq!(scene.ask("GET", "/v1/files", Some(&other), b"").status, 200);
    assert_eq!(paired(scene.state()).iter().map(|p| p.device.as_str()).collect::<Vec<_>>(), [OTHER]);
}

#[test]
fn the_list_has_the_four_kinds_of_file_and_nothing_else() {
    let scene = Scene::new();
    let token = scene.pair(PHONE);
    let root = scene.folder().to_path_buf();
    let put = |rel: &str, data: &[u8]| {
        let full = root.join(rel);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, data).unwrap();
    };
    put(&format!("snapshots/{MAC}/0000000001.db"), b"12345");
    put(&format!("changes/{MAC}/0000000002.json"), b"{}");
    put("files/abc.png", b"picture");
    // Not sync files: never listed, never served.
    put(&format!("changes/{MAC}/notes.txt"), b"x");
    put(&format!("changes/{MAC}/0000000000.json"), b"x");
    put("files/.abc.png.tmp", b"x");
    put("devices/bad.json", b"{}");
    put("workspace.db", b"secret");
    // (Not the upper-case id: on a Mac's disk that is the same folder.)
    put(&format!("snapshots/{}/0000000001.db", "g".repeat(32)), b"x");
    let outside = scene.base.join("outside.png");
    fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("files/link.png")).unwrap();

    let list = scene.ask("GET", "/v1/files", Some(&token), b"").json();
    assert_eq!(list["format"], 1);
    assert_eq!(list["device"], MAC);
    assert_eq!(list["name"], "Test Mac");
    assert_eq!(list["devices"].as_array().unwrap().len(), 1);
    assert_eq!(list["devices"][0]["kind"], "mac"); // the device file whole
    assert_eq!(list["devices"][0]["seq"], 2);
    let files: Vec<(String, u64)> = list["files"].as_array().unwrap().iter().map(|f| (f["path"].as_str().unwrap().to_string(), f["size"].as_u64().unwrap())).collect();
    assert_eq!(files, [(format!("changes/{MAC}/0000000002.json"), 2), ("files/abc.png".to_string(), 7), (format!("snapshots/{MAC}/0000000001.db"), 5)]);

    let a = scene.ask("GET", &format!("/v1/file/snapshots/{MAC}/0000000001.db"), Some(&token), b"");
    assert_eq!((a.status, a.body.as_slice()), (200, b"12345".as_slice()));
    assert!(a.headers.iter().any(|(k, v)| k == "content-type" && v == "application/octet-stream"));
    for bad in [
        "/v1/file/workspace.db".to_string(),
        "/v1/file/../workspace.db".to_string(),
        "/v1/file/%2e%2e/workspace.db".to_string(),
        "/v1/file/files/%2e%2e%2fworkspace.db".to_string(),
        format!("/v1/file/changes/{MAC}/notes.txt"),
        format!("/v1/file/changes/{MAC}/0000000009.json"),
        "/v1/file/files/link.png".to_string(),
        "/v1/file/".to_string(),
    ] {
        assert_eq!(scene.ask("GET", &bad, Some(&token), b"").status, 404, "{bad}");
    }
}

#[test]
fn an_iphone_writes_only_its_own_files_and_each_once() {
    let scene = Scene::new();
    let token = scene.pair(PHONE);
    let t = Some(token.as_str());
    let change = format!("/v1/file/changes/{PHONE}/0000000001.json");

    assert_eq!(scene.ask("PUT", &change, t, b"{\"a\":1}").status, 204);
    assert_eq!(fs::read(scene.folder().join(format!("changes/{PHONE}/0000000001.json"))).unwrap(), b"{\"a\":1}");
    assert_eq!(scene.puts.load(Ordering::SeqCst), 1);
    // Again with the same length: fine, nothing written. Another length: 409.
    assert_eq!(scene.ask("PUT", &change, t, b"{\"b\":2}").status, 204);
    assert_eq!(fs::read(scene.folder().join(format!("changes/{PHONE}/0000000001.json"))).unwrap(), b"{\"a\":1}");
    assert_eq!(scene.ask("PUT", &change, t, b"{}").status, 409);
    assert_eq!(scene.puts.load(Ordering::SeqCst), 1);

    // Snapshots and pictures.
    assert_eq!(scene.ask("PUT", &format!("/v1/file/snapshots/{PHONE}/0000000002.db"), t, b"db").status, 204);
    assert_eq!(scene.ask("PUT", "/v1/file/files/abc123.png", t, b"png").status, 204);
    assert_eq!(fs::read(scene.folder().join("files/abc123.png")).unwrap(), b"png");

    // Files of another device, and names that are not sync files: 403.
    for path in [
        format!("/v1/file/changes/{MAC}/0000000099.json"),
        format!("/v1/file/devices/{MAC}.json"),
        format!("/v1/file/snapshots/{OTHER}/0000000001.db"),
        format!("/v1/file/devices/{OTHER}.json"),
        "/v1/file/../outside.json".into(),
        "/v1/file/workspace.db".into(),
        "/v1/file/files/a.exe".into(),
    ] {
        assert_eq!(scene.ask("PUT", &path, t, b"{}").status, 403, "{path}");
    }
    assert!(!scene.base.join("Sync/outside.json").exists());

    // The iPhone's device file replaces the one before; it must be one.
    let device = format!("/v1/file/devices/{PHONE}.json");
    assert_eq!(scene.ask("PUT", &device, t, format!(r#"{{"device":"{PHONE}","seq":1}}"#).as_bytes()).status, 204);
    assert_eq!(scene.ask("PUT", &device, t, format!(r#"{{"device":"{PHONE}","seq":22}}"#).as_bytes()).status, 204);
    let kept: serde_json::Value = serde_json::from_slice(&fs::read(scene.folder().join(format!("devices/{PHONE}.json"))).unwrap()).unwrap();
    assert_eq!(kept["seq"], 22);
    assert_eq!(scene.ask("PUT", &device, t, format!(r#"{{"device":"{OTHER}"}}"#).as_bytes()).status, 400);
    assert_eq!(scene.ask("PUT", &device, t, b"not json").status, 400);
    let listed = scene.ask("GET", "/v1/files", t, b"").json();
    assert_eq!(listed["devices"].as_array().unwrap().len(), 2);

    // Too large: refused before a byte of it is read.
    let big = format!("PUT /v1/file/files/big.png HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n", PUT_LIMIT + 1).into_bytes();
    let a = scene.send(big);
    assert_eq!(a[0].status, 413);
    assert!(a[0].headers.iter().any(|(k, v)| k == "connection" && v == "close"));
    let big_device = format!("PUT {device} HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n", DEVICE_FILE_LIMIT + 1).into_bytes();
    assert_eq!(scene.send(big_device)[0].status, 413);
    assert!(!scene.folder().join("files/big.png").exists());

    // Nothing is left on the way.
    let left: Vec<String> = fs::read_dir(scene.state().join("wifi")).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
    assert!(left.iter().all(|n| ["cert.der", "key.der", "phones.json"].contains(&n.as_str())), "{left:?}");
}

#[test]
fn one_connection_carries_many_requests() {
    let scene = Scene::new();
    let token = scene.pair(PHONE);
    let mut bytes = request("GET", "/v1/files", Some(&token), b"");
    bytes.extend(request("GET", "/v1/files", None, b""));
    bytes.extend(request("PUT", &format!("/v1/file/changes/{MAC}/0000000001.json"), Some(&token), b"refused, read away"));
    bytes.extend(request("GET", &format!("/v1/file/devices/{MAC}.json"), Some(&token), b""));
    bytes.extend(b"GET /v1/files HTTP/1.1\r\nConnection: close\r\nAuthorization: Bearer x\r\n\r\n");
    bytes.extend(request("GET", "/v1/files", Some(&token), b"")); // after the close: not answered
    let list = scene.send(bytes);
    assert_eq!(list.iter().map(|a| a.status).collect::<Vec<_>>(), [200, 401, 403, 200, 401]);
    // A head that is not HTTP: 400, and the connection ends.
    let list = scene.send(b"HELLO\r\n\r\n".to_vec());
    assert_eq!(list.iter().map(|a| a.status).collect::<Vec<_>>(), [400]);
}

#[test]
fn the_certificate_is_made_once_and_the_qr_code_draws_the_link() {
    let base = std::env::temp_dir().join(format!("rune-wifi-test-{}", random_hex(8)));
    let first = fingerprint(&base).unwrap();
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(fingerprint(&base).unwrap(), first);
    let der = fs::read(base.join("wifi/cert.der")).unwrap();
    assert_eq!(sha256_hex(&der), first);
    fs::remove_dir_all(&base).unwrap();

    let svg = qr_svg("notion2://pair?v=1&host=localhost").unwrap();
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 "));
    assert!(svg.contains("fill=\"currentColor\""));
    assert!(!svg.contains("width="));
    // The finder pattern at the top left starts after the quiet zone.
    assert!(svg.contains("d=\"M4 4h7v1h-7z"), "{svg}");

    assert_eq!(pair_link(&["a.local".into(), "10.0.0.2".into()], 1, "f", "c", "Jack's Mac"), "notion2://pair?v=1&host=a.local&host=10.0.0.2&port=1&cert=f&code=c&name=Jack%27s%20Mac");
    assert!(valid_host("Jacks-MacBook-Pro.local") && valid_host("192.168.1.20"));
    assert!(!valid_host("a/b") && !valid_host("-a") && !valid_host("") && !valid_host("a b"));
}

#[test]
fn constant_time_equality() {
    assert!(same(b"abc", b"abc"));
    assert!(!same(b"abc", b"abd"));
    assert!(!same(b"abc", b"abcd"));
}

// MARK: Through TLS, on 127.0.0.1

/// Accepts exactly the certificate with this fingerprint, as the iPhone does.
#[derive(Debug)]
struct Pinned(String, Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if sha256_hex(end_entity) == self.0 {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("not the Mac".into()))
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.1.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.1.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.1.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_client(port: u16, fingerprint: &str, tls12: bool) -> rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let versions: &[&rustls::SupportedProtocolVersion] = if tls12 { &[&rustls::version::TLS12] } else { &[&rustls::version::TLS13] };
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(versions)
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned(fingerprint.into(), provider)))
        .with_no_client_auth();
    let conn = rustls::ClientConnection::new(Arc::new(config), "localhost".try_into().unwrap()).unwrap();
    let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    rustls::StreamOwned::new(conn, tcp)
}

/// One request, and its answer read by its `Content-Length`.
fn over_tls(stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>, bytes: &[u8]) -> io::Result<Answer> {
    stream.write_all(bytes)?;
    stream.flush()?;
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&got[..end]).to_ascii_lowercase();
            let length: usize = head.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap().trim().parse().unwrap();
            if got.len() >= end + 4 + length {
                return Ok(answers(&got[..end + 4 + length]).remove(0));
            }
        }
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        got.extend_from_slice(&buf[..n]);
    }
}

#[test]
fn an_iphone_pairs_and_lists_through_tls() {
    let scene = Scene::new();
    let cfg = Config { folder: scene.folder().into(), state: scene.state().into(), device: MAC.into(), name: "Test Mac".into() };
    let puts = Arc::new(AtomicUsize::new(0));
    let counter = puts.clone();
    let server = Server::start_local(cfg, 0, Arc::new(move || _ = counter.fetch_add(1, Ordering::SeqCst))).unwrap();
    let port = server.port();
    let offer = server.offer().unwrap();
    assert!(offer.link.contains(&format!("&port={port}&")));
    let fp = fingerprint(scene.state()).unwrap();
    let code = offer.link.split('&').find_map(|p| p.strip_prefix("code=")).unwrap().to_string();

    // TLS 1.3 and 1.2, both pinned.
    let mut phone = tls_client(port, &fp, false);
    let body = serde_json::json!({"code": code, "device": PHONE, "name": "Test iPhone"}).to_string();
    let paired = over_tls(&mut phone, &request("POST", "/v1/pair", None, body.as_bytes())).unwrap();
    assert_eq!(paired.status, 200);
    let token = paired.json()["token"].as_str().unwrap().to_string();
    // The same connection goes on (keep-alive).
    let list = over_tls(&mut phone, &request("GET", "/v1/files", Some(&token), b"")).unwrap();
    assert_eq!((list.status, list.json()["device"].as_str()), (200, Some(MAC)));
    let put = over_tls(&mut phone, &request("PUT", &format!("/v1/file/changes/{PHONE}/0000000001.json"), Some(&token), b"{}")).unwrap();
    assert_eq!(put.status, 204);
    assert_eq!(puts.load(Ordering::SeqCst), 1);

    let mut old = tls_client(port, &fp, true);
    assert_eq!(over_tls(&mut old, &request("GET", "/v1/files", None, b"")).unwrap().status, 401);

    // Another fingerprint: the client refuses the Mac.
    let mut wrong = tls_client(port, &"0".repeat(64), false);
    assert!(over_tls(&mut wrong, &request("GET", "/v1/files", None, b"")).is_err());

    // Stopping closes the open connections and the port.
    let started = std::time::Instant::now();
    server.stop();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(over_tls(&mut phone, &request("GET", "/v1/files", Some(&token), b"")).is_err());
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
}

#[test]
fn the_port_is_read_back() {
    let base = std::env::temp_dir().join(format!("rune-wifi-test-{}", random_hex(8)));
    let wifi = base.join("wifi");
    assert_eq!(kept_port(&wifi), None);
    write_private(&wifi.join("port"), b"47815\n").unwrap();
    assert_eq!(kept_port(&wifi), Some(47815));
    assert_eq!(fs::metadata(wifi.join("port")).unwrap().permissions().mode() & 0o077, 0);
    fs::remove_dir_all(&base).unwrap();
}

/// The stand-in Mac of mobile/Core's `WifiRunTests`, as `wifi-mac.py DIR
/// --port 0` is: it serves `$RUNE_STANDIN_FOLDER/sync-v1` on 127.0.0.1 and
/// ::1 (port `$RUNE_STANDIN_PORT`, 0 for any), keeps its state in
/// `$RUNE_STANDIN_FOLDER.mac`, prints `LINK notion2://pair?…`, and ends when
/// the process that started it does. From the repository's root:
///
///     cd mobile/Core && RUNE_STANDIN_MAC_CMD="cargo test --manifest-path $PWD/../../Cargo.toml --bin notion2 -q -- --ignored --exact devsync::wifi::tests::stand_in_mac --nocapture" swift test --filter WifiRunTests
#[test]
#[ignore = "the stand-in Mac for WifiRunTests (mobile/Core); runs until its parent ends"]
fn stand_in_mac() {
    let folder = PathBuf::from(std::env::var("RUNE_STANDIN_FOLDER").expect("RUNE_STANDIN_FOLDER"));
    let port: u16 = std::env::var("RUNE_STANDIN_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let root = folder.join(format::FOLDER);
    // The Mac's device id and name, from its device file (as wifi-mac.py).
    let (device, name) = regular_files(&root.join("devices"))
        .into_iter()
        .filter_map(|(n, _)| serde_json::from_slice::<format::DeviceFile>(&fs::read(root.join("devices").join(n)).ok()?).ok())
        .find(|d| d.kind == "mac")
        .map(|d| (d.device, if d.name.is_empty() { "Mac".to_string() } else { d.name }))
        .expect("a Mac's device file: run make-sync-folder.py init first");
    let state = PathBuf::from(format!("{}.mac", folder.to_string_lossy().trim_end_matches('/')));
    let server = Server::start_local(Config { folder: root.clone(), state, device, name: name.clone() }, port, Arc::new(|| {})).unwrap();
    let offer = server.offer().unwrap();
    println!("LINK {}", offer.link);
    println!("Serving {} as \"{name}\" on port {} (Rust). The code works once, for 10 minutes.", root.display(), server.port());
    let parent = unsafe { libc::getppid() };
    while unsafe { libc::getppid() } == parent {
        std::thread::sleep(Duration::from_millis(200));
    }
    server.stop();
}
