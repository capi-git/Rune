// Keeps the phone's notes the same as the Mac's. The files are those of
// `docs/sync-format.md`, and they come in two ways, each on its own or both:
//
// - through the app's folder in the user's own iCloud Drive;
// - straight from the Mac over the local network (`docs/sync-wifi.md`), after
//   the phone was paired once with the QR code that the Mac shows.
//
// The phone only reads for now: `SyncEngine` takes a snapshot and applies the
// Mac's change files. The same file from both ways is applied once.
//
// While the app is in front, both are looked at every few seconds, and the
// iCloud folder at once when iCloud says something in it changed. Nothing
// runs while the app is in the background.

import Foundation
import RuneCore
import Observation
import Security
import UIKit

extension StoreGate: StoreAccess {
    func withStore<T: Sendable>(_ body: @Sendable () throws -> T) async throws -> T {
        try body()
    }
}

@Observable
final class SyncCoordinator {
    /// Sync through iCloud Drive.
    enum Status: Equatable {
        /// iCloud Drive cannot be used on this iPhone (off, or not signed in).
        case unavailable
        /// The folder is there, but no Mac has put notes in it yet.
        case waiting
        /// Some files are still on their way from iCloud.
        case fetching
        case upToDate
        /// A Mac writes a newer format than this app reads.
        case needsUpdate
        case failed(String)
    }

    /// Sync straight from the Mac.
    enum WifiStatus: Equatable {
        /// No Mac is paired.
        case off
        /// The Mac does not answer: another network, asleep, or its app is closed.
        case away
        case fetching
        case upToDate
        case needsUpdate
        /// The Mac no longer knows this iPhone.
        case refused
        /// Something else answers at the Mac's address.
        case notTheMac
        case failed(String)
    }

    /// The folder in iCloud Drive keeps the app's first name ("notion2"): Apple
    /// registered it and it cannot be renamed, and the Mac reads the same folder.
    nonisolated static let containerID = "iCloud.com.jackhagan.notion2"

    private(set) var status: Status = .unavailable
    private(set) var wifi: WifiStatus = .off
    /// The Mac this iPhone is paired with over the local network.
    private(set) var mac: WifiPeer?
    private(set) var devices: [SyncDevice] = []

    /// A QR code was read: the user is asked whether to connect.
    var pendingLink: PairingLink?
    /// Why a QR code could not be used; shown once.
    var pairError: String?
    /// The name of the Mac that is being paired with, while that takes.
    private(set) var connectingTo: String?

    /// Whether the notes on the phone come from sync. A copy loaded by hand
    /// would then be replaced at the next run.
    var hasNotes: Bool {
        if cloudSynced || wifiSynced {
            return true
        }
        switch status {
        case .fetching, .upToDate, .needsUpdate: return true
        case .unavailable, .waiting, .failed: break
        }
        switch wifi {
        case .fetching, .upToDate, .needsUpdate: return true
        case .off, .away, .refused, .notTheMac, .failed: return false
        }
    }

    /// Where the pictures of the notes may be: the copy of the Mac's folder
    /// first (its files are whole), then the folder in iCloud Drive.
    var filesFolders: [URL] {
        [mac == nil ? nil : SyncCoordinator.mirror, root]
            .compactMap { $0?.appendingPathComponent(SyncFormat.filesFolderName, isDirectory: true) }
    }

    /// Changes when new pictures may have come.
    var picturesStamp: String {
        "\(filesFolders.count) \(status == .upToDate) \(wifi == .upToDate)"
    }

    private var cloudSynced = false
    private var wifiSynced = false
    private var cloudDevices: [SyncDevice] = []
    private var wifiDevices: [SyncDevice] = []
    private var loop: Task<Void, Never>?
    private var kicked = false
    private var root: URL?
    private var query: NSMetadataQuery?
    private var observers: [NSObjectProtocol] = []
    private var client: WifiClient?
    /// Runs to leave out after the Mac did not answer, so that an iPhone away
    /// from home asks twice a minute, not every few seconds.
    private var wifiPause = 0

    init() {
        if let data = UserDefaults.standard.data(forKey: "wifiMac"), let peer = try? JSONDecoder().decode(WifiPeer.self, from: data),
           let token = WifiKeychain.token(for: peer.device) {
            mac = peer
            client = WifiClient(peer: peer, token: token)
            wifi = .fetching
        }
    }

    /// This device's id in the sync files, made once.
    nonisolated static var deviceID: String {
        if let known = UserDefaults.standard.string(forKey: "syncDevice"), known.count == 32 {
            return known
        }
        let made = SyncFormat.newDeviceID()
        UserDefaults.standard.set(made, forKey: "syncDevice")
        return made
    }

    /// Start to follow the Mac (the app came to the front).
    func activate(model: AppModel) {
        guard loop == nil else {
            kick()
            return
        }
        loop = Task { [weak self, weak model] in
            while !Task.isCancelled {
                guard let self, let model else { return }
                // The paired Mac first: after "start again" its snapshot is
                // the one to take.
                await self.runWifi(model: model)
                await self.runCloud(model: model)
                // Look again in ten seconds, or at once when asked.
                for _ in 0..<20 where !self.kicked && !Task.isCancelled {
                    try? await Task.sleep(for: .milliseconds(500))
                }
                self.kicked = false
            }
        }
    }

    /// Stop (the app left the front).
    func pause() {
        loop?.cancel()
        loop = nil
    }

    private func kick() {
        kicked = true
        wifiPause = 0
    }

    /// A change was made here: send it soon.
    func changed() {
        kick()
    }

    /// This device's own sync files (its change files), written once and
    /// copied into each sync folder: iCloud Drive, and the copy of the Mac's
    /// folder that Wi-Fi uploads from.
    nonisolated static var outbox: URL {
        AppModel.storeURL.deletingLastPathComponent()
            .appendingPathComponent("outbox", isDirectory: true)
            .appendingPathComponent(SyncFormat.folderName, isDirectory: true)
    }

    /// Write what changed here into the outbox, then into `root`. Run before
    /// the files of the other devices are read, so a snapshot taken after
    /// cannot lose a change of this phone.
    nonisolated static func send(to root: URL, device: String) throws {
        let store = AppModel.storeURL
        try SyncEngine.push(outbox: outbox, store: store, deviceID: device)
        try SyncEngine.publish(outbox: outbox, to: root)
    }

    // MARK: iCloud Drive

    private func runCloud(model: AppModel) async {
        if root == nil {
            root = await SyncCoordinator.findRoot()
            if root != nil {
                watch()
            }
        }
        guard let root else {
            set(.unavailable)
            return
        }
        let device = SyncCoordinator.deviceID
        let name = UIDevice.current.name
        let store = AppModel.storeURL
        do {
            let report = try await StoreGate.shared.run {
                SyncCoordinator.askForDownloads(in: root)
                try SyncCoordinator.send(to: root, device: device)
                return try SyncEngine.pull(root: root, store: store, deviceID: device, deviceName: name, isHere: SyncCoordinator.isHere)
            }
            if report.changed {
                await model.reload()
            }
            cloudSynced = report.isSynced
            cloudDevices = report.devices
            showDevices()
            set(report.needsUpdate ? .needsUpdate : !report.isSynced ? .waiting : report.waiting ? .fetching : .upToDate)
        } catch {
            set(.failed(error.localizedDescription))
        }
    }

    private func set(_ new: Status) {
        if status != new {
            status = new
        }
    }

    /// Ask iCloud to say when the folder changes, so a change shows at once.
    private func watch() {
        guard query == nil, UserDefaults.standard.string(forKey: "syncFolder") == nil else { return }
        let query = NSMetadataQuery()
        query.searchScopes = [NSMetadataQueryUbiquitousDocumentsScope]
        query.predicate = NSPredicate(format: "%K LIKE '*'", NSMetadataItemFSNameKey)
        for name in [Notification.Name.NSMetadataQueryDidFinishGathering, .NSMetadataQueryDidUpdate] {
            observers.append(NotificationCenter.default.addObserver(forName: name, object: query, queue: .main) { [weak self] _ in
                Task { @MainActor in
                    self?.kicked = true
                }
            })
        }
        query.start()
        self.query = query
    }

    // MARK: The Mac over the local network

    /// A link was opened (the camera read the Mac's QR code).
    func opened(_ url: URL) {
        do {
            pendingLink = try PairingLink(url)
        } catch PairingLink.Problem.newer {
            pairError = "Update Rune on this iPhone to use this code."
        } catch {
            pairError = "This code cannot be read. Show a new code on the Mac."
        }
    }

    /// Pair with the Mac of a QR code (the user said "Connect").
    func pair(_ link: PairingLink) async {
        connectingTo = link.name
        defer { connectingTo = nil }
        do {
            let (peer, token) = try await WifiClient.pair(link, deviceID: SyncCoordinator.deviceID, deviceName: UIDevice.current.name)
            if let old = mac, old.device != peer.device {
                WifiKeychain.remove(for: old.device)
                await SyncCoordinator.removeMirror()
            }
            // A Mac this iPhone has no notes from: its notes take the place
            // of the ones here (the iPhone has no changes of its own to keep).
            let store = AppModel.storeURL
            let device = SyncCoordinator.deviceID
            _ = try? await StoreGate.shared.run {
                try SyncEngine.startAgain(unlessSyncedWith: peer.device, store: store, deviceID: device)
            }
            try WifiKeychain.save(token, for: peer.device)
            UserDefaults.standard.set(try JSONEncoder().encode(peer), forKey: "wifiMac")
            mac = peer
            client = WifiClient(peer: peer, token: token)
            wifiSynced = false
            setWifi(.fetching)
            kick()
        } catch WifiError.badCode {
            pairError = "This code was used before, or it is too old. Show a new code on the Mac."
        } catch WifiError.unreachable {
            pairError = "This iPhone cannot reach \(link.name). Put the two on the same Wi-Fi, and keep Rune open on the Mac."
        } catch WifiError.notTheMac {
            pairError = "Another computer answers in place of \(link.name). Show a new code on the Mac."
        } catch WifiError.newer {
            pairError = "Update Rune on this iPhone to connect to \(link.name)."
        } catch let WifiError.failed(message) {
            pairError = message
        } catch {
            pairError = error.localizedDescription
        }
    }

    /// Forget the Mac. The notes stay on the phone.
    func disconnect() async {
        guard let old = mac else { return }
        let asked = client
        mac = nil
        client = nil
        wifiSynced = false
        wifiDevices = []
        showDevices()
        setWifi(.off)
        UserDefaults.standard.removeObject(forKey: "wifiMac")
        WifiKeychain.remove(for: old.device)
        // Tell the Mac, if it is there; it can also remove the phone itself.
        try? await asked?.unpair()
        await SyncCoordinator.removeMirror()
    }

    private func runWifi(model: AppModel) async {
        guard let client, mac != nil else {
            setWifi(.off)
            return
        }
        if wifiPause > 0 {
            wifiPause -= 1
            return
        }
        let device = SyncCoordinator.deviceID
        let name = UIDevice.current.name
        do {
            let mirror = SyncCoordinator.mirror
            try await StoreGate.shared.run { try SyncCoordinator.send(to: mirror, device: device) }
            let report = try await WifiSync.run(
                client: client, mirror: SyncCoordinator.mirror, store: AppModel.storeURL,
                deviceID: device, deviceName: name, gate: StoreGate.shared
            )
            // The user may have disconnected while the run was on its way.
            guard self.client === client else { return }
            if report.changed {
                await model.reload()
            }
            wifiSynced = report.isSynced
            wifiDevices = report.devices
            showDevices()
            setWifi(report.needsUpdate ? .needsUpdate : report.waiting || !report.isSynced ? .fetching : .upToDate)
        } catch {
            guard self.client === client else { return }
            switch error {
            case WifiError.unreachable:
                setWifi(.away)
                wifiPause = 2
            case WifiError.refused: setWifi(.refused)
            case WifiError.notTheMac: setWifi(.notTheMac)
            case WifiError.newer: setWifi(.needsUpdate)
            case let WifiError.failed(message): setWifi(.failed(message))
            default: setWifi(.failed(error.localizedDescription))
            }
        }
    }

    private func setWifi(_ new: WifiStatus) {
        if wifi != new {
            wifi = new
        }
    }

    /// The devices that take part, as both ways report them.
    private func showDevices() {
        var seen: Set<String> = []
        let all = (wifiDevices + cloudDevices).filter { seen.insert($0.id).inserted }
            .sorted { ($0.kind == "mac" ? 0 : 1, $0.name, $0.id) < ($1.kind == "mac" ? 0 : 1, $1.name, $1.id) }
        if devices != all {
            devices = all
        }
    }

    // MARK: Off the main thread

    /// The phone's copy of the Mac's sync folder, beside the app's database.
    nonisolated static var mirror: URL {
        AppModel.storeURL.deletingLastPathComponent()
            .appendingPathComponent("Wifi", isDirectory: true)
            .appendingPathComponent(SyncFormat.folderName, isDirectory: true)
    }

    /// Remove the copy of the Mac's folder. Its pictures are kept, beside
    /// the app's database: the notes on the phone still name them.
    @concurrent
    private nonisolated static func removeMirror() async {
        let files = FileManager.default
        let pictures = mirror.appendingPathComponent(SyncFormat.filesFolderName, isDirectory: true)
        let kept = PictureStore.ownFolder
        for name in (try? files.contentsOfDirectory(atPath: pictures.path)) ?? [] where Pictures.isValidName(name) {
            try? files.createDirectory(at: kept, withIntermediateDirectories: true)
            if !files.fileExists(atPath: kept.appendingPathComponent(name).path) {
                try? files.moveItem(at: pictures.appendingPathComponent(name), to: kept.appendingPathComponent(name))
            }
        }
        try? files.removeItem(at: mirror.deletingLastPathComponent())
    }

    /// The sync folder in iCloud Drive, or nil when iCloud Drive cannot be used.
    ///
    /// For a test in the Simulator, which has no iCloud account, the launch
    /// argument `-syncFolder <path>` names a plain folder to use instead.
    @concurrent
    private nonisolated static func findRoot() async -> URL? {
        if let stand = UserDefaults.standard.string(forKey: "syncFolder"), !stand.isEmpty {
            return URL(fileURLWithPath: stand).appendingPathComponent(SyncFormat.folderName)
        }
        guard let container = FileManager.default.url(forUbiquityContainerIdentifier: containerID) else { return nil }
        let root = container.appendingPathComponent("Documents").appendingPathComponent(SyncFormat.folderName)
        // The iPhone app makes the folder; the Mac app waits for it.
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }

    /// Whether a file's content is on this device (not only its name).
    private nonisolated static func isHere(_ url: URL) -> Bool {
        guard let status = (try? url.resourceValues(forKeys: [.ubiquitousItemDownloadingStatusKey]))?.ubiquitousItemDownloadingStatus else {
            return true
        }
        return status == .current
    }

    /// Ask iCloud for every file in the folder that is not on this device yet.
    private nonisolated static func askForDownloads(in root: URL) {
        let files = FileManager.default
        let keys: [URLResourceKey] = [.ubiquitousItemDownloadingStatusKey, .isDirectoryKey]
        for part in ["devices", "changes", "snapshots"] {
            guard let walker = files.enumerator(at: root.appendingPathComponent(part), includingPropertiesForKeys: keys) else { continue }
            for case let url as URL in walker {
                let name = url.lastPathComponent
                if name.hasPrefix("."), name.hasSuffix(".icloud") {
                    // A file that is not here yet shows as ".name.icloud".
                    let real = url.deletingLastPathComponent().appendingPathComponent(String(name.dropFirst().dropLast(".icloud".count)))
                    try? files.startDownloadingUbiquitousItem(at: real)
                } else if let values = try? url.resourceValues(forKeys: Set(keys)), values.isDirectory != true,
                          let status = values.ubiquitousItemDownloadingStatus, status != .current {
                    try? files.startDownloadingUbiquitousItem(at: url)
                }
            }
        }
    }
}

/// The token that lets this iPhone in at the Mac. It is kept in the keychain
/// of this device only, never in a file or in the settings.
enum WifiKeychain {
    // The app's first name ("notion2") stays here: the token of a Mac that is
    // already paired is found by this name.
    private nonisolated static let service = "com.jackhagan.notion2.wifi"

    private nonisolated static func query(_ device: String) -> [CFString: Any] {
        [kSecClass: kSecClassGenericPassword, kSecAttrService: service, kSecAttrAccount: device]
    }

    nonisolated static func save(_ token: String, for device: String) throws {
        SecItemDelete(query(device) as CFDictionary)
        var item = query(device)
        item[kSecValueData] = Data(token.utf8)
        item[kSecAttrAccessible] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw WifiError.failed("The iPhone could not keep the key for the Mac (\(status)).")
        }
    }

    nonisolated static func token(for device: String) -> String? {
        var item = query(device)
        item[kSecReturnData] = true
        item[kSecMatchLimit] = kSecMatchLimitOne
        var found: CFTypeRef?
        guard SecItemCopyMatching(item as CFDictionary, &found) == errSecSuccess, let data = found as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    nonisolated static func remove(for device: String) {
        SecItemDelete(query(device) as CFDictionary)
    }
}
