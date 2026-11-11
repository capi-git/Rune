// Sync straight to the Mac over the local network (`docs/sync-wifi.md`): the
// link in the QR code that the Mac shows, and the names of the files that
// travel. The files are those of `docs/sync-format.md`; only the way they
// get from one device to the other is different from iCloud Drive.

import Foundation

/// What the QR code on the Mac says:
/// `notion2://pair?v=1&host=Jacks-Mac.local&host=192.168.1.20&port=47815&cert=<64 hex>&code=<code>&name=MacBook%20Pro`
public struct PairingLink: Hashable, Sendable {
    public enum Problem: Error, Equatable {
        /// Not a link of this kind at all.
        case notALink
        /// A link of a later version: this app must be updated.
        case newer
        /// A part is missing or is not as it must be.
        case incomplete
    }

    public static let version = 1

    /// Where the Mac listens: its local network name first, then (maybe) an
    /// address to try when the name is not found.
    public var hosts: [String]
    public var port: Int
    /// SHA-256 of the Mac's certificate: 64 lower-case hex characters.
    public var fingerprint: String
    /// The code that lets one iPhone in, once.
    public var code: String
    /// The Mac's name, for the question "Connect to …?".
    public var name: String

    public init(hosts: [String], port: Int, fingerprint: String, code: String, name: String) {
        self.hosts = hosts
        self.port = port
        self.fingerprint = fingerprint
        self.code = code
        self.name = name
    }

    /// Whether a URL is a pairing link (of any version, complete or not).
    public static func isOne(_ url: URL) -> Bool {
        url.scheme?.lowercased() == "notion2" && url.host?.lowercased() == "pair"
    }

    public init(_ url: URL) throws {
        guard PairingLink.isOne(url), let parts = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
            throw Problem.notALink
        }
        let items = parts.queryItems ?? []
        func first(_ name: String) -> String? {
            items.first { $0.name == name }?.value
        }
        if let version = first("v") {
            guard let number = Int(version), number >= 1 else { throw Problem.incomplete }
            if number > PairingLink.version {
                throw Problem.newer
            }
        }
        let hosts = items.filter { $0.name == "host" }.compactMap(\.value)
        guard !hosts.isEmpty, hosts.count <= 4, hosts.allSatisfy(PairingLink.isHost),
              let port = first("port").flatMap(Int.init), (1...65535).contains(port),
              let cert = first("cert")?.lowercased(), cert.count == 64, cert.allSatisfy({ $0.isASCII && $0.isHexDigit }),
              let code = first("code"), (16...128).contains(code.count), code.allSatisfy(PairingLink.isTokenCharacter)
        else {
            throw Problem.incomplete
        }
        self.hosts = hosts
        self.port = port
        self.fingerprint = cert
        self.code = code
        let name = (first("name") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        self.name = name.isEmpty ? "Mac" : String(name.prefix(80))
    }

    public var url: URL {
        var parts = URLComponents()
        parts.scheme = "notion2"
        parts.host = "pair"
        parts.queryItems = [URLQueryItem(name: "v", value: String(PairingLink.version))]
            + hosts.map { URLQueryItem(name: "host", value: $0) }
            + [
                URLQueryItem(name: "port", value: String(port)),
                URLQueryItem(name: "cert", value: fingerprint),
                URLQueryItem(name: "code", value: code),
                URLQueryItem(name: "name", value: name),
            ]
        return parts.url!
    }

    /// A network name or an IPv4 address: letters, digits, dots and hyphens.
    static func isHost(_ text: String) -> Bool {
        (1...253).contains(text.count) && !text.hasPrefix(".") && !text.hasPrefix("-")
            && text.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "." || $0 == "-") }
    }

    /// The characters of a code or a token (base64 for URLs).
    static func isTokenCharacter(_ c: Character) -> Bool {
        c.isASCII && (c.isLetter || c.isNumber || c == "-" || c == "_")
    }
}

/// The Mac this iPhone is paired with. The token is kept apart, in the keychain.
public struct WifiPeer: Codable, Hashable, Sendable {
    /// The Mac's device id in the sync files.
    public var device: String
    public var name: String
    public var hosts: [String]
    public var port: Int
    public var fingerprint: String

    public init(device: String, name: String, hosts: [String], port: Int, fingerprint: String) {
        self.device = device
        self.name = name
        self.hosts = hosts
        self.port = port
        self.fingerprint = fingerprint
    }
}

/// The name of a file in a sync folder, as it travels: one of the four kinds
/// of `docs/sync-format.md`. Anything else is refused, so a name can never
/// point outside the folder.
public enum SyncPath: Hashable, Sendable {
    /// `devices/<device>.json`
    case device(String)
    /// `snapshots/<device>/<seq>.db`
    case snapshot(device: String, seq: Int)
    /// `changes/<device>/<seq>.json`
    case change(device: String, seq: Int)
    /// `files/<name>`
    case picture(String)

    public init?(_ path: String) {
        let parts = path.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        let made: SyncPath
        switch (parts.first, parts.count) {
        case ("devices", 2):
            guard parts[1].hasSuffix(".json") else { return nil }
            made = .device(String(parts[1].dropLast(5)))
        case ("snapshots", 3):
            guard parts[2].hasSuffix(".db"), let seq = SyncFormat.seq(inFileName: parts[2]) else { return nil }
            made = .snapshot(device: parts[1], seq: seq)
        case ("changes", 3):
            guard parts[2].hasSuffix(".json"), let seq = SyncFormat.seq(inFileName: parts[2]) else { return nil }
            made = .change(device: parts[1], seq: seq)
        case (SyncFormat.filesFolderName, 2):
            made = .picture(parts[1])
        default:
            return nil
        }
        // Only the one way to write the name is taken.
        guard made.isValid, made.path == path else { return nil }
        self = made
    }

    public var path: String {
        switch self {
        case let .device(device): "devices/\(device).json"
        case let .snapshot(device, seq): "snapshots/\(device)/\(SyncFormat.name(ofSeq: seq)).db"
        case let .change(device, seq): "changes/\(device)/\(SyncFormat.name(ofSeq: seq)).json"
        case let .picture(name): "\(SyncFormat.filesFolderName)/\(name)"
        }
    }

    /// The device that writes the file. A picture has none.
    public var owner: String? {
        switch self {
        case let .device(device), let .snapshot(device, _), let .change(device, _): device
        case .picture: nil
        }
    }

    private var isValid: Bool {
        switch self {
        case let .device(device): SyncPath.isDeviceID(device)
        case let .snapshot(device, seq), let .change(device, seq): SyncPath.isDeviceID(device) && seq >= 1
        case let .picture(name): Pictures.isValidName(name)
        }
    }

    /// 32 lower-case hex characters.
    public static func isDeviceID(_ text: String) -> Bool {
        text.count == 32 && text.allSatisfy { $0.isASCII && ($0.isNumber || ("a"..."f").contains($0)) }
    }
}
