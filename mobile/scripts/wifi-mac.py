#!/usr/bin/env python3
"""Play the part of the Mac in sync over Wi-Fi: serve a sync folder as docs/sync-wifi.md says.

The Mac app does not do this yet. Until it does, this tool lets the iPhone app
be tried, and it shows what the Mac app must do: keep one certificate, show a
link with a code that works once, give each iPhone a token, and serve the files.

    wifi-mac.py DIR                the folder that make-sync-folder.py made, for the Simulator
    wifi-mac.py DIR --lan          for a real iPhone on the same Wi-Fi
    wifi-mac.py DIR --lan --qr     and show the QR code for the iPhone's camera

    --port N      listen on this port (default 47815; 0 picks a free one)
    --name NAME   the Mac's name in the link (default: the name in its device file)
    --code CODE   use this pairing code in place of a new one

It prints "LINK rune://pair?…": that link is what the QR code holds. In the
Simulator, open it with:  xcrun simctl openurl booted "<link>"

What it keeps (the certificate, its key, the iPhones it knows) is in
DIR.mac/wifi, beside the stand-in Mac's database and outside the sync folder.
"""

import hashlib
import http.server
import json
import os
import re
import secrets
import socket
import ssl
import subprocess
import sys
import threading
import time
import urllib.parse

FORMAT = 1
CODE_LIFE = 600  # seconds a pairing code can be used
DEVICE = r"[0-9a-f]{32}"
SEQ = r"(?!0{10})[0-9]{10}"
PICTURE = r"[A-Za-z0-9]{1,40}\.(png|jpg|gif|webp|heic|tiff|bmp)"
PATHS = [
    ("device", re.compile(rf"devices/({DEVICE})\.json")),
    ("snapshot", re.compile(rf"snapshots/({DEVICE})/{SEQ}\.db")),
    ("change", re.compile(rf"changes/({DEVICE})/{SEQ}\.json")),
    ("picture", re.compile(rf"files/{PICTURE}")),
]


def kind_and_owner(path: str):
    """The kind of a file name and the device that writes it, or (None, None)."""
    for kind, pattern in PATHS:
        match = pattern.fullmatch(path)
        if match:
            return kind, (match.group(1) if kind != "picture" else None)
    return None, None


class Mac:
    def __init__(self, folder: str, name: str, code: str):
        self.root = os.path.join(folder, "sync-v1")
        self.home = os.path.join(folder.rstrip("/") + ".mac", "wifi")
        os.makedirs(self.home, exist_ok=True)
        self.lock = threading.Lock()
        self.cert = os.path.join(self.home, "cert.pem")
        self.key = os.path.join(self.home, "key.pem")
        if not (os.path.exists(self.cert) and os.path.exists(self.key)):
            self.make_certificate()
        with open(self.cert) as f:
            der = ssl.PEM_cert_to_DER_cert(f.read())
        self.fingerprint = hashlib.sha256(der).hexdigest()
        self.device, self.name = self.who(name)
        # One code, good for one iPhone and for a few minutes.
        self.code = code or secrets.token_urlsafe(16)
        self.code_until = time.time() + CODE_LIFE

    def make_certificate(self) -> None:
        """A certificate that only this Mac has. No authority signs it: the
        iPhone learns its fingerprint from the QR code."""
        subprocess.run(["openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", self.key], check=True)
        os.chmod(self.key, 0o600)
        subprocess.run(
            ["openssl", "req", "-new", "-x509", "-key", self.key, "-out", self.cert, "-days", "3650", "-subj", "/CN=Rune"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def who(self, name: str):
        """The Mac's device id and name, from its device file."""
        folder = os.path.join(self.root, "devices")
        for entry in sorted(os.listdir(folder)) if os.path.isdir(folder) else []:
            try:
                with open(os.path.join(folder, entry)) as f:
                    file = json.load(f)
            except (OSError, ValueError):
                continue
            if file.get("kind") == "mac":
                return file["device"], name or file.get("name") or "Mac"
        sys.exit(f"No Mac's device file in {folder}. Run make-sync-folder.py init first.")

    # The iPhones that are paired: only a hash of each token is kept.

    def phones(self) -> dict:
        try:
            with open(os.path.join(self.home, "phones.json")) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}

    def save_phones(self, phones: dict) -> None:
        path = os.path.join(self.home, "phones.json")
        with open(path + ".new", "w") as f:
            json.dump(phones, f, indent=1, sort_keys=True)
        os.replace(path + ".new", path)

    def pair(self, body: dict):
        device, code = str(body.get("device", "")), str(body.get("code", ""))
        with self.lock:
            good = self.code and secrets.compare_digest(code, self.code) and time.time() < self.code_until
            if not good or not re.fullmatch(DEVICE, device):
                return None
            self.code = ""  # used
            token = secrets.token_urlsafe(32)
            phones = {k: v for k, v in self.phones().items() if v.get("device") != device}
            phones[hashlib.sha256(token.encode()).hexdigest()] = {
                "device": device,
                "name": str(body.get("name", ""))[:80],
                "kind": str(body.get("kind", "iphone"))[:20],
                "at": int(time.time() * 1000),
            }
            self.save_phones(phones)
        return {"token": token, "device": self.device, "name": self.name}

    def phone(self, token: str):
        return self.phones().get(hashlib.sha256(token.encode()).hexdigest())

    def forget(self, token: str) -> None:
        with self.lock:
            phones = self.phones()
            phones.pop(hashlib.sha256(token.encode()).hexdigest(), None)
            self.save_phones(phones)

    def listing(self) -> dict:
        devices, files = [], []
        for base, _, names in os.walk(self.root):
            for name in sorted(names):
                full = os.path.join(base, name)
                path = os.path.relpath(full, self.root).replace(os.sep, "/")
                kind, _ = kind_and_owner(path)
                if kind == "device":
                    try:
                        with open(full) as f:
                            devices.append(json.load(f))
                    except (OSError, ValueError):
                        pass
                elif kind:
                    files.append({"path": path, "size": os.path.getsize(full)})
        files.sort(key=lambda f: f["path"])
        return {"format": FORMAT, "device": self.device, "name": self.name, "devices": devices, "files": files}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    mac: Mac = None  # set before the server starts
    quiet = False

    def log_message(self, format, *args):  # noqa: A002
        if not self.quiet:
            sys.stderr.write("%s %s\n" % (time.strftime("%H:%M:%S"), format % args))

    def answer(self, status: int, body: bytes = b"", kind: str = "application/json") -> None:
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def body(self, limit: int) -> bytes:
        length = int(self.headers.get("Content-Length") or 0)
        if length > limit:
            self.close_connection = True
            return None
        return self.rfile.read(length)

    def token(self) -> str:
        header = self.headers.get("Authorization") or ""
        return header[7:] if header.startswith("Bearer ") else ""

    def known_phone(self):
        """The paired iPhone that asks, or None after a 401."""
        phone = self.mac.phone(self.token()) if self.token() else None
        if phone is None:
            self.body(1 << 30)
            self.answer(401)
        return phone

    def file_path(self):
        path = urllib.parse.unquote(urllib.parse.urlsplit(self.path).path)
        return path[len("/v1/file/"):] if path.startswith("/v1/file/") else None

    def do_POST(self):
        if urllib.parse.urlsplit(self.path).path != "/v1/pair":
            return self.answer(404)
        raw = self.body(4096)
        try:
            body = json.loads(raw or b"")
        except ValueError:
            body = None
        answer = self.mac.pair(body) if isinstance(body, dict) else None
        if answer is None:
            return self.answer(403)
        self.answer(200, json.dumps(answer).encode())

    def do_DELETE(self):
        if urllib.parse.urlsplit(self.path).path != "/v1/pair":
            return self.answer(404)
        if self.known_phone() is None:
            return
        self.mac.forget(self.token())
        self.answer(204)

    def do_GET(self):
        if self.known_phone() is None:
            return
        if urllib.parse.urlsplit(self.path).path == "/v1/files":
            return self.answer(200, json.dumps(self.mac.listing()).encode())
        path = self.file_path()
        kind, _ = kind_and_owner(path or "")
        full = os.path.join(self.mac.root, path) if kind else ""
        if not kind or not os.path.isfile(full):
            return self.answer(404)
        size = os.path.getsize(full)
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(size))
        self.end_headers()
        with open(full, "rb") as f:
            while True:
                piece = f.read(1 << 16)
                if not piece:
                    break
                self.wfile.write(piece)

    def do_PUT(self):
        phone = self.known_phone()
        if phone is None:
            return
        path = self.file_path()
        kind, owner = kind_and_owner(path or "")
        data = self.body(256 << 20)
        if data is None:
            return self.answer(413)
        # An iPhone writes only in its own places (and pictures, which have no owner).
        if not kind or (owner is not None and owner != phone["device"]):
            return self.answer(403)
        full = os.path.join(self.mac.root, path)
        if kind != "device" and os.path.exists(full):
            # Written once: the same file again is fine, another one is not.
            return self.answer(204 if os.path.getsize(full) == len(data) else 409)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        # Made outside the sync folder first, then moved in.
        temp = os.path.join(self.mac.home, f"put-{secrets.token_hex(8)}")
        with open(temp, "wb") as f:
            f.write(data)
        os.replace(temp, full)
        self.answer(204)


def lan_address() -> str:
    """This Mac's address on the local network, or ""."""
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("192.0.2.1", 9))  # nothing is sent
        return s.getsockname()[0]
    except OSError:
        return ""
    finally:
        s.close()


def main() -> int:
    args = sys.argv[1:]
    if not args or args[0].startswith("-"):
        print(__doc__)
        return 2
    folder, port, name, code, lan, qr = args[0], 47815, "", "", False, False
    rest = args[1:]
    while rest:
        flag = rest.pop(0)
        if flag == "--lan":
            lan = True
        elif flag == "--qr":
            qr = True
        elif flag in ("--port", "--name", "--code") and rest:
            value = rest.pop(0)
            if flag == "--port":
                port = int(value)
            elif flag == "--name":
                name = value
            else:
                code = value
        else:
            print(__doc__)
            return 2

    mac = Mac(folder, name, code)
    Handler.mac = mac
    Handler.quiet = bool(os.environ.get("QUIET"))
    server = http.server.ThreadingHTTPServer(("0.0.0.0" if lan else "127.0.0.1", port), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(mac.cert, mac.key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    port = server.server_address[1]

    if lan:
        local = subprocess.run(["scutil", "--get", "LocalHostName"], capture_output=True, text=True).stdout.strip()
        hosts = [h for h in ((local + ".local") if local else "", lan_address()) if h]
    else:
        hosts = ["localhost"]
    query = [("v", "1")] + [("host", h) for h in hosts] + [("port", str(port)), ("cert", mac.fingerprint), ("code", mac.code), ("name", mac.name)]
    link = "rune://pair?" + urllib.parse.urlencode(query, quote_via=urllib.parse.quote)
    print(f"LINK {link}", flush=True)
    print(f"Serving {mac.root} as \"{mac.name}\" on port {port}. The code works once, for {CODE_LIFE // 60} minutes. Ctrl-C stops.", flush=True)
    if qr:
        picture = os.path.join(mac.home, "pair.png")
        subprocess.run(["swift", os.path.join(os.path.dirname(os.path.abspath(__file__)), "qr.swift"), link, picture], check=True)
        subprocess.run(["open", picture])
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
