// One run of sync over the local network: ask the Mac which files it has,
// fetch the ones the notes still need into the iPhone's copy of the folder,
// let `SyncEngine` apply them, and send back this device's own files.

import Foundation

/// Lets one thing at a time touch the app's database file. The network work
/// between two uses does not hold it.
public protocol StoreAccess: Sendable {
    func withStore<T: Sendable>(_ body: @Sendable () throws -> T) async throws -> T
}

public enum WifiSync {
    /// - Parameters:
    ///   - mirror: the iPhone's copy of the Mac's sync folder (a `sync-v1` folder).
    ///   - store: the app's database.
    public static func run(
        client: WifiClient,
        mirror: URL,
        store: URL,
        deviceID: String,
        deviceName: String,
        deviceKind: String = "iphone",
        gate: any StoreAccess
    ) async throws -> SyncReport {
        let list = try await client.list()
        // Files on their way are kept beside the copy, on the same volume.
        let arriving = mirror.deletingLastPathComponent().appendingPathComponent("arriving", isDirectory: true)
        try? FileManager.default.removeItem(at: arriving)
        defer { try? FileManager.default.removeItem(at: arriving) }

        var report = SyncReport()
        var uploads: [String] = []
        for round in 0..<3 {
            let applied = try await gate.withStore { SyncEngine.applied(store: store, deviceID: deviceID) }
            let here = WifiMirror.contents(of: mirror)
            let plan = WifiMirror.plan(remote: list.files, local: here.files, marked: here.marked, this: deviceID, applied: applied)
            uploads = plan.uploads
            // After the first round, go on only when it found more to fetch.
            if round > 0 && plan.downloads.isEmpty {
                try WifiMirror.prepare(plan, devices: list.devices, in: mirror, this: deviceID)
                break
            }
            try WifiMirror.prepare(plan, devices: list.devices, in: mirror, this: deviceID)
            for file in plan.downloads {
                let fetched = try await client.download(file, into: arriving)
                try WifiMirror.place(fetched, at: file.path, in: mirror)
            }

            let pulled = try await gate.withStore {
                try SyncEngine.pull(root: mirror, store: store, deviceID: deviceID, deviceName: deviceName, deviceKind: deviceKind)
            }
            report.adoptedSnapshot = report.adoptedSnapshot || pulled.adoptedSnapshot
            report.applied += pulled.applied
            report.conflicts += pulled.conflicts
            report.needsUpdate = report.needsUpdate || pulled.needsUpdate
            report.waiting = pulled.waiting
            report.isSynced = pulled.isSynced
            report.devices = pulled.devices
            // A snapshot that was just taken says how far the other devices
            // are: there may be more to fetch.
            if !pulled.adoptedSnapshot && !pulled.waiting {
                break
            }
        }

        // Tell the Mac how far this device is, and send what it lacks.
        let mine = mirror.appendingPathComponent(SyncPath.device(deviceID).path)
        if let data = try? Data(contentsOf: mine), let file = try? JSONDecoder().decode(SyncDeviceFile.self, from: data),
           !list.devices.contains(file) {
            try await client.upload(SyncPath.device(deviceID).path, from: mine)
        }
        for path in uploads {
            try await client.upload(path, from: mirror.appendingPathComponent(path))
        }
        return report
    }
}
