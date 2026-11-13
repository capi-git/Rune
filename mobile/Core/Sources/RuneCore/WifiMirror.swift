// The iPhone's copy of the Mac's sync folder, for sync over the local
// network. The Mac says which files it has; this file decides which of them
// the iPhone must fetch, and `SyncEngine` then reads the copy exactly as it
// reads the folder in iCloud Drive.
//
// Only what the engine can still use is fetched: change files after the last
// one applied, and a snapshot only when the iPhone starts or cannot go on
// without one. An older snapshot is marked "listed, not here" with the name
// iCloud gives such a file, which the engine already understands.

import Foundation

/// One file in a sync folder: its name and its length.
public struct WifiFile: Codable, Hashable, Sendable {
    public var path: String
    public var size: Int64

    public init(path: String, size: Int64) {
        self.path = path
        self.size = size
    }
}

/// What the Mac answers to "which files do you have?".
public struct WifiList: Codable, Sendable {
    public var format: Int
    /// The Mac's device id.
    public var device: String
    public var name: String
    /// Every device file, whole: they are small and change often.
    public var devices: [SyncDeviceFile]
    /// The snapshots, change files and pictures.
    public var files: [WifiFile]

    public init(format: Int = SyncFormat.version, device: String, name: String, devices: [SyncDeviceFile] = [], files: [WifiFile] = []) {
        self.format = format
        self.device = device
        self.name = name
        self.devices = devices
        self.files = files
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        format = try c.decode(Int.self, forKey: .format)
        device = try c.decodeIfPresent(String.self, forKey: .device) ?? ""
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
        // A list of a later format is not read past its format number.
        let known = format <= SyncFormat.version
        devices = known ? try c.decodeIfPresent([SyncDeviceFile].self, forKey: .devices) ?? [] : []
        files = known ? try c.decodeIfPresent([WifiFile].self, forKey: .files) ?? [] : []
    }
}

/// What to do to the iPhone's copy so that the engine can go on.
public struct WifiPlan: Equatable, Sendable {
    /// Files to fetch, in this order: snapshots, change files, pictures.
    public var downloads: [WifiFile] = []
    /// Snapshots that the Mac has and the iPhone does not need: marked as
    /// "listed, not here".
    public var markers: [String] = []
    /// Files and marks of the copy that are of no use any more.
    public var removals: [String] = []
    /// Files of this device that the Mac does not have yet.
    public var uploads: [String] = []

    public init() {}
}

public enum WifiMirror {
    /// - Parameters:
    ///   - remote: what the Mac has.
    ///   - local: what the copy has.
    ///   - marked: the snapshots the copy marks as "listed, not here".
    ///   - deviceID: this device.
    ///   - applied: the last file of each device that this device applied, or
    ///     nil when it has no synced notes yet.
    public static func plan(remote: [WifiFile], local: [WifiFile], marked: Set<String>, this deviceID: String, applied: [String: Int]?) -> WifiPlan {
        var plan = WifiPlan()
        let localSize = Dictionary(local.map { ($0.path, $0.size) }, uniquingKeysWith: { a, _ in a })
        var remoteSize: [String: Int64] = [:]
        var snapshots: [String: [Int]] = [:]
        var changes: [String: [Int]] = [:]
        var pictures: [WifiFile] = []
        for file in remote {
            guard let path = SyncPath(file.path), file.size >= 0 else { continue }
            remoteSize[file.path] = file.size
            switch path {
            case let .snapshot(device, seq) where device != deviceID: snapshots[device, default: []].append(seq)
            case let .change(device, seq) where device != deviceID: changes[device, default: []].append(seq)
            case .picture: pictures.append(file)
            default: break
            }
        }

        var wanted: Set<String> = []
        var marks: Set<String> = []
        var wantedChanges: [WifiFile] = []
        for device in Set(snapshots.keys).union(changes.keys).sorted() {
            let done = applied?[device] ?? 0
            let changeSeqs = Set(changes[device] ?? [])
            let snapshotSeqs = Set(snapshots[device] ?? [])

            // The newest snapshot is needed to start, and when a file between
            // the last one applied and that snapshot is gone.
            if let newest = snapshotSeqs.max() {
                var needed = applied == nil
                if !needed && newest > done + 1 {
                    needed = ((done + 1)..<newest).contains { !changeSeqs.contains($0) && !snapshotSeqs.contains($0) }
                }
                for seq in snapshotSeqs {
                    let path = SyncPath.snapshot(device: device, seq: seq).path
                    if seq == newest && needed {
                        wanted.insert(path)
                        if localSize[path] != remoteSize[path] {
                            plan.downloads.append(WifiFile(path: path, size: remoteSize[path] ?? 0))
                        }
                    } else {
                        // Also the newest one, once the notes are past it:
                        // the copy does not keep a second set of the notes.
                        marks.insert(path)
                    }
                }
            }

            for seq in changeSeqs.sorted() where applied == nil || seq > done {
                let path = SyncPath.change(device: device, seq: seq).path
                wanted.insert(path)
                if localSize[path] != remoteSize[path] {
                    wantedChanges.append(WifiFile(path: path, size: remoteSize[path] ?? 0))
                }
            }
        }
        plan.downloads.sort { $0.path < $1.path }
        plan.downloads += wantedChanges
        plan.downloads += pictures.filter { localSize[$0.path] != $0.size }.sorted { $0.path < $1.path }

        plan.markers = marks.subtracting(marked).sorted()
        var removals = marked.subtracting(marks).map(WifiMirror.markerPath)
        for file in local {
            guard let path = SyncPath(file.path) else { continue }
            switch path {
            case .device, .picture:
                // Device files are written from the Mac's list; a picture stays.
                break
            case .snapshot, .change:
                if path.owner == deviceID {
                    if remoteSize[file.path] != file.size {
                        plan.uploads.append(file.path)
                    }
                } else if !wanted.contains(file.path) {
                    removals.append(file.path)
                }
            }
        }
        plan.removals = removals.sorted()
        plan.uploads.sort()
        return plan
    }

    // MARK: The copy on disk

    /// The name of the mark for a file that is listed but not here: the name
    /// iCloud Drive gives such a file (".name.icloud"), which
    /// `SyncEngine.entries` reads as "not here".
    static func markerPath(_ path: String) -> String {
        guard let slash = path.lastIndex(of: "/") else { return ".\(path).icloud" }
        return "\(path[...slash]).\(path[path.index(after: slash)...]).icloud"
    }

    /// The files of a copy and the snapshots it marks as "not here".
    public static func contents(of root: URL) -> (files: [WifiFile], marked: Set<String>) {
        let manager = FileManager.default
        var files: [WifiFile] = []
        var marked: Set<String> = []
        func look(_ folder: String) {
            for name in (try? manager.contentsOfDirectory(atPath: root.appendingPathComponent(folder).path)) ?? [] {
                let path = "\(folder)/\(name)"
                if name.hasPrefix("."), name.hasSuffix(".icloud") {
                    marked.insert("\(folder)/\(name.dropFirst().dropLast(".icloud".count))")
                } else if SyncPath(path) != nil, let size = try? manager.attributesOfItem(atPath: root.appendingPathComponent(path).path)[.size] as? NSNumber {
                    files.append(WifiFile(path: path, size: size.int64Value))
                }
            }
        }
        look("devices")
        look(SyncFormat.filesFolderName)
        for part in ["snapshots", "changes"] {
            for device in (try? manager.contentsOfDirectory(atPath: root.appendingPathComponent(part).path)) ?? [] where SyncPath.isDeviceID(device) {
                look("\(part)/\(device)")
            }
        }
        return (files.sorted { $0.path < $1.path }, marked)
    }

    /// Do the parts of a plan that need no network: remove, mark, and write
    /// the other devices' device files as the Mac gave them.
    public static func prepare(_ plan: WifiPlan, devices: [SyncDeviceFile], in root: URL, this deviceID: String) throws {
        let manager = FileManager.default
        for path in plan.removals {
            try? manager.removeItem(at: root.appendingPathComponent(path))
        }
        for path in plan.markers {
            let mark = root.appendingPathComponent(markerPath(path))
            try manager.createDirectory(at: mark.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? manager.removeItem(at: root.appendingPathComponent(path))
            if !manager.fileExists(atPath: mark.path) {
                try Data().write(to: mark)
            }
        }

        let folder = root.appendingPathComponent("devices")
        try manager.createDirectory(at: folder, withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        var known: Set<String> = [deviceID]
        for file in devices where SyncPath.isDeviceID(file.device) && file.device != deviceID {
            known.insert(file.device)
            let url = folder.appendingPathComponent("\(file.device).json")
            if let old = try? Data(contentsOf: url), (try? JSONDecoder().decode(SyncDeviceFile.self, from: old)) == file {
                continue
            }
            try encoder.encode(file).write(to: url, options: .atomic)
        }
        // A device the Mac no longer lists is gone from the copy too.
        for name in (try? manager.contentsOfDirectory(atPath: folder.path)) ?? [] where name.hasSuffix(".json") {
            if !known.contains(String(name.dropLast(5))) {
                try? manager.removeItem(at: folder.appendingPathComponent(name))
            }
        }
    }

    /// Put a fetched file in its place in the copy. A mark for it goes.
    public static func place(_ fetched: URL, at path: String, in root: URL) throws {
        let manager = FileManager.default
        let target = root.appendingPathComponent(path)
        try manager.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        if manager.fileExists(atPath: target.path) {
            _ = try manager.replaceItemAt(target, withItemAt: fetched)
        } else {
            try manager.moveItem(at: fetched, to: target)
        }
        try? manager.removeItem(at: root.appendingPathComponent(markerPath(path)))
    }
}
