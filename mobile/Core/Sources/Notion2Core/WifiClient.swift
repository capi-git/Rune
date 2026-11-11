// Talks to the Mac over the local network (`docs/sync-wifi.md`): HTTPS to a
// certificate that only the Mac has, checked against the fingerprint from the
// QR code. No certificate authority and no internet are involved.

import CryptoKit
import Foundation
import Security

public enum WifiError: Error, Equatable {
    /// The Mac does not answer: another network, asleep, or the app is closed.
    case unreachable
    /// Something answers there, but not with the certificate of the paired Mac.
    case notTheMac
    /// The Mac no longer knows this iPhone: pair again.
    case refused
    /// The code from the QR code was used before or is too old.
    case badCode
    /// The Mac does not let this iPhone do that (write a file of another device).
    case forbidden
    /// The Mac speaks a later version: this app must be updated.
    case newer
    case failed(String)
}

/// What the Mac answers when an iPhone pairs.
struct WifiPairAnswer: Codable {
    var token: String
    var device: String
    var name: String?
}

public actor WifiClient {
    private let hosts: [String]
    private let port: Int
    private let token: String?
    private let session: URLSession
    /// The host that answered last: tried first the next time.
    private var preferred = 0

    /// - Parameter patient: wait a while for the network to be usable before
    ///   giving up. The first time the app reaches the local network, iOS
    ///   asks the user to allow it, and the request must wait for the answer.
    public init(hosts: [String], port: Int, fingerprint: String, token: String?, patient: Bool = false) {
        self.hosts = hosts
        self.port = port
        self.token = token
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 6
        configuration.timeoutIntervalForResource = patient ? 15 : 600
        configuration.waitsForConnectivity = patient
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.allowsCellularAccess = false
        session = URLSession(configuration: configuration, delegate: PinnedCertificate(fingerprint: fingerprint), delegateQueue: nil)
    }

    public init(peer: WifiPeer, token: String) {
        self.init(hosts: peer.hosts, port: peer.port, fingerprint: peer.fingerprint, token: token)
    }

    deinit {
        session.invalidateAndCancel()
    }

    /// Pair with the Mac of a QR code. Gives the Mac and the token to keep.
    public static func pair(_ link: PairingLink, deviceID: String, deviceName: String, kind: String = "iphone") async throws -> (peer: WifiPeer, token: String) {
        // Find the address at which the Mac answers first: the code works
        // once, so it is sent to one address only.
        let finder = WifiClient(hosts: link.hosts, port: link.port, fingerprint: link.fingerprint, token: nil, patient: true)
        let host = try await finder.findHost()
        let client = WifiClient(hosts: [host], port: link.port, fingerprint: link.fingerprint, token: nil)
        let body = try JSONEncoder().encode(["code": link.code, "device": deviceID, "name": deviceName, "kind": kind])
        let data: Data
        do {
            (data, _) = try await client.send("POST", "/v1/pair", body: body)
        } catch WifiError.forbidden {
            throw WifiError.badCode
        }
        guard let answer = try? JSONDecoder().decode(WifiPairAnswer.self, from: data),
              SyncPath.isDeviceID(answer.device), answer.token.count >= 32, answer.token.allSatisfy(PairingLink.isTokenCharacter)
        else {
            throw WifiError.failed("The Mac gave an answer that cannot be read.")
        }
        let name = (answer.name ?? "").isEmpty ? link.name : answer.name!
        // The address that answered is tried first from now on.
        let hosts = [host] + link.hosts.filter { $0 != host }
        return (WifiPeer(device: answer.device, name: name, hosts: hosts, port: link.port, fingerprint: link.fingerprint), answer.token)
    }

    /// The address at which the Mac answers, all of them tried at once. Any
    /// answer counts, also "I do not know you".
    func findHost() async throws -> String {
        let session = session
        var requests: [(index: Int, request: URLRequest)] = []
        for index in hosts.indices {
            requests.append((index, try request("GET", "/v1/files", host: hosts[index])))
        }
        let found: (index: Int?, wrong: Bool) = await withTaskGroup(of: (Int?, Bool).self) { group in
            for (index, request) in requests {
                group.addTask {
                    do {
                        _ = try await session.data(for: request)
                        return (index, false)
                    } catch {
                        return (nil, WifiClient.map(error) as? WifiError == .notTheMac)
                    }
                }
            }
            var wrong = false
            for await (index, refusedCertificate) in group {
                if let index {
                    group.cancelAll()
                    return (index, false)
                }
                wrong = wrong || refusedCertificate
            }
            return (nil, wrong)
        }
        guard let index = found.index else {
            throw found.wrong ? WifiError.notTheMac : WifiError.unreachable
        }
        preferred = index
        return hosts[index]
    }

    /// The files the Mac has.
    public func list() async throws -> WifiList {
        let (data, _) = try await send("GET", "/v1/files")
        guard let list = try? JSONDecoder().decode(WifiList.self, from: data) else {
            throw WifiError.failed("The Mac gave a list that cannot be read.")
        }
        if list.format > SyncFormat.version {
            throw WifiError.newer
        }
        return list
    }

    /// Fetch one file into `folder` (a folder for files on their way). The
    /// caller moves it into place, so a half-fetched file is never read.
    public func download(_ file: WifiFile, into folder: URL) async throws -> URL {
        guard SyncPath(file.path) != nil else { throw WifiError.failed("Not a sync file: \(file.path)") }
        var lastError: Error = WifiError.unreachable
        for index in order() {
            do {
                let (temp, response) = try await session.download(for: request("GET", "/v1/file/\(file.path)", host: hosts[index]))
                try WifiClient.check(response)
                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                let kept = folder.appendingPathComponent("fetch-\(UUID().uuidString)")
                try FileManager.default.moveItem(at: temp, to: kept)
                let size = (try? FileManager.default.attributesOfItem(atPath: kept.path)[.size] as? NSNumber)?.int64Value
                guard size == file.size else {
                    try? FileManager.default.removeItem(at: kept)
                    throw WifiError.failed("\(file.path) came with the wrong length.")
                }
                preferred = index
                return kept
            } catch {
                lastError = WifiClient.map(error)
                if lastError as? WifiError != .unreachable {
                    break
                }
            }
        }
        throw lastError
    }

    /// Send one of this device's files to the Mac.
    public func upload(_ path: String, from url: URL) async throws {
        guard SyncPath(path) != nil else { throw WifiError.failed("Not a sync file: \(path)") }
        _ = try await send("PUT", "/v1/file/\(path)", body: try Data(contentsOf: url))
    }

    /// Ask the Mac to forget this iPhone.
    public func unpair() async throws {
        _ = try await send("DELETE", "/v1/pair")
    }

    // MARK: Requests

    /// The hosts to try: the one that answered last, then the others.
    private func order() -> [Int] {
        [preferred] + hosts.indices.filter { $0 != preferred }
    }

    private func request(_ method: String, _ path: String, host: String) throws -> URLRequest {
        guard let url = URL(string: "https://\(host):\(port)\(path)") else {
            throw WifiError.failed("The Mac's address cannot be used.")
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        if let token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        return request
    }

    private func send(_ method: String, _ path: String, body: Data? = nil) async throws -> (Data, URLResponse) {
        var lastError: Error = WifiError.unreachable
        for index in order() {
            do {
                var request = try request(method, path, host: hosts[index])
                if let body {
                    request.httpBody = body
                    request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
                }
                let (data, response) = try await session.data(for: request)
                try WifiClient.check(response)
                preferred = index
                return (data, response)
            } catch {
                lastError = WifiClient.map(error)
                // Only "does not answer" is worth another address.
                if lastError as? WifiError != .unreachable {
                    break
                }
            }
        }
        throw lastError
    }

    private static func check(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else { throw WifiError.failed("No answer.") }
        switch http.statusCode {
        case 200..<300: return
        case 401: throw WifiError.refused
        case 403: throw WifiError.forbidden
        case 426: throw WifiError.newer
        default: throw WifiError.failed("The Mac answered \(http.statusCode).")
        }
    }

    private static func map(_ error: Error) -> Error {
        if error is WifiError {
            return error
        }
        guard let failure = error as? URLError else { return WifiError.failed(error.localizedDescription) }
        switch failure.code {
        case .cancelled, .serverCertificateUntrusted, .serverCertificateHasBadDate, .serverCertificateNotYetValid,
             .serverCertificateHasUnknownRoot, .clientCertificateRejected, .userCancelledAuthentication:
            // The delegate refused the certificate.
            return WifiError.notTheMac
        case .timedOut, .cannotFindHost, .cannotConnectToHost, .networkConnectionLost, .dnsLookupFailed, .notConnectedToInternet,
             .secureConnectionFailed, .internationalRoamingOff, .dataNotAllowed, .resourceUnavailable:
            return WifiError.unreachable
        default:
            return WifiError.failed(failure.localizedDescription)
        }
    }
}

/// Accepts exactly one certificate: the one whose SHA-256 the QR code gave.
private final class PinnedCertificate: NSObject, URLSessionDelegate, Sendable {
    let fingerprint: String

    init(fingerprint: String) {
        self.fingerprint = fingerprint.lowercased()
    }

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping @Sendable (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust,
              let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let leaf = chain.first
        else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        let seen = SHA256.hash(data: SecCertificateCopyData(leaf) as Data).map { String(format: "%02x", $0) }.joined()
        if seen == fingerprint {
            completionHandler(.useCredential, URLCredential(trust: trust))
        } else {
            completionHandler(.cancelAuthenticationChallenge, nil)
        }
    }
}
