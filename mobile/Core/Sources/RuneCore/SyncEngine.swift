// Brings this device's database up to date from a sync folder
// (`docs/sync-format.md`): takes a snapshot when the device has no synced
// notes yet, then applies the other devices' change files in order.
//
// The folder is the one in iCloud Drive, or the copy of the Mac's folder that
// came over the local network (`WifiSync`). Both may be read in turn: a file
// that was applied from one is passed over in the other.
//
// The iPhone only reads for now (it cannot edit yet), so it never writes
// change files; it writes its own `devices/<id>.json` so the Mac can show it.
// The rule for a record changed in two places at once is complete, because a
// second Mac can cause that already.

import Foundation

/// A device that takes part in sync, for the Settings screen.
public struct SyncDevice: Hashable, Sendable, Identifiable {
    public var id: String
    public var name: String
    /// "mac" or "iphone".
    public var kind: String
    /// When it last wrote its device file (milliseconds since 1970).
    public var at: Int64
    public var isThisDevice: Bool
}

/// What one run of sync did.
public struct SyncReport: Sendable, Equatable {
    /// The database was replaced by a snapshot.
    public var adoptedSnapshot = false
    /// Changes taken from other devices.
    public var applied = 0
    /// Records that were changed in two places at once.
    public var conflicts = 0
    /// Some files are listed but not here yet (iCloud is still bringing them).
    public var waiting = false
    /// A device writes a newer format: this app must be updated to read it.
    public var needsUpdate = false
    /// The database has synced notes after the run.
    public var isSynced = false
    public var devices: [SyncDevice] = []

    /// Whether the notes on screen are out of date.
    public var changed: Bool { adoptedSnapshot || applied > 0 }

    public init() {}
}

public enum SyncEngine {
    /// Bring the database at `store` up to date from the sync folder `root`
    /// (the `sync-v1` folder). `isHere` says whether a listed file's content
    /// is on this device yet.
    public static func pull(
        root: URL,
        store: URL,
        deviceID: String,
        deviceName: String,
        deviceKind: String = "iphone",
        now: Int64 = When.ms(Date()),
        isHere: (URL) -> Bool = { _ in true }
    ) throws -> SyncReport {
        let files = FileManager.default
        var report = SyncReport()
        try files.createDirectory(at: root.appendingPathComponent("devices"), withIntermediateDirectories: true)

        var deviceFiles: [String: SyncDeviceFile] = [:]
        for entry in entries(in: root.appendingPathComponent("devices"), isHere: isHere) where entry.name.hasSuffix(".json") {
            guard entry.here, let data = try? Data(contentsOf: entry.url), let file = try? JSONDecoder().decode(SyncDeviceFile.self, from: data) else {
                report.waiting = true
                continue
            }
            deviceFiles[file.device] = file
        }

        // No synced notes here yet: start from the newest snapshot.
        if !isSynced(store, deviceID: deviceID) {
            guard let snapshot = newestSnapshot(root: root, after: [:], isHere: isHere, waiting: &report.waiting) else {
                report.devices = devices(deviceFiles, this: deviceID)
                return report
            }
            try adopt(snapshot, store: store, deviceID: deviceID)
            report.adoptedSnapshot = true
        }

        // Apply change files. A gap that a newer snapshot covers means the
        // files before it were removed: start again from that snapshot.
        var peers: [String: Int] = [:]
        for _ in 0..<4 {
            let outcome = try applyChanges(root: root, store: store, deviceID: deviceID, deviceFiles: deviceFiles, isHere: isHere, report: &report)
            peers = outcome.peers
            guard let behind = outcome.behind else { break }
            // The iPhone has no changes of its own to keep, so it can take
            // the snapshot whole. A device that edits must merge instead.
            guard let snapshot = newestSnapshot(root: root, after: [behind: peers[behind] ?? 0], isHere: isHere, waiting: &report.waiting),
                  snapshot.device == behind
            else {
                report.waiting = true
                break
            }
            try adopt(snapshot, store: store, deviceID: deviceID)
            report.adoptedSnapshot = true
        }
        report.isSynced = true

        // Tell the other devices that this one is here and how far it is.
        var mine = SyncDeviceFile(device: deviceID, name: deviceName, kind: deviceKind, at: now, seq: ownSeq(store), applied: peers)
        if let old = deviceFiles[deviceID] {
            var same = old
            same.at = now
            if same == mine {
                mine.at = old.at
            } else {
                try write(mine, to: root, beside: store)
            }
        } else {
            try write(mine, to: root, beside: store)
        }
        deviceFiles[deviceID] = mine
        report.devices = devices(deviceFiles, this: deviceID)
        return report
    }

    // MARK: Files

    struct Entry {
        let name: String
        let url: URL
        /// The file's content is on this device.
        let here: Bool
    }

    /// What is in a folder. A file that iCloud has not brought yet shows as a
    /// hidden ".name.icloud" file on the iPhone: it is listed as not here.
    static func entries(in folder: URL, isHere: (URL) -> Bool) -> [Entry] {
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: folder.path) else { return [] }
        return names.sorted().compactMap { raw in
            if raw.hasPrefix("."), raw.hasSuffix(".icloud") {
                let name = String(raw.dropFirst().dropLast(".icloud".count))
                return Entry(name: name, url: folder.appendingPathComponent(name), here: false)
            }
            if raw.hasPrefix(".") {
                return nil
            }
            let url = folder.appendingPathComponent(raw)
            return Entry(name: raw, url: url, here: isHere(url))
        }
    }

    private static func devices(_ files: [String: SyncDeviceFile], this: String) -> [SyncDevice] {
        files.values
            .map { SyncDevice(id: $0.device, name: $0.name, kind: $0.kind, at: $0.at, isThisDevice: $0.device == this) }
            .sorted { ($0.kind == "mac" ? 0 : 1, $0.name, $0.id) < ($1.kind == "mac" ? 0 : 1, $1.name, $1.id) }
    }

    /// Write a device file: made beside the database first, then moved into
    /// the sync folder, so a half-written file is never there.
    private static func write(_ file: SyncDeviceFile, to root: URL, beside store: URL) throws {
        let files = FileManager.default
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let temp = store.deletingLastPathComponent().appendingPathComponent("device-\(UUID().uuidString).json")
        try files.createDirectory(at: temp.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(file).write(to: temp)
        let target = root.appendingPathComponent("devices").appendingPathComponent("\(file.device).json")
        if files.fileExists(atPath: target.path) {
            _ = try files.replaceItemAt(target, withItemAt: temp)
        } else {
            try files.moveItem(at: temp, to: target)
        }
    }

    // MARK: Snapshots

    struct Snapshot {
        let device: String
        let seq: Int
        let url: URL
        let clock: Int64
    }

    /// Whether the database at `store` is a synced database of this device.
    static func isSynced(_ store: URL, deviceID: String) -> Bool {
        guard let db = try? Connection(url: store, mode: .readWrite) else { return false }
        defer { db.close() }
        guard let tables = try? db.tableNames(), tables.contains("sync_meta"), tables.contains("sync_records"), tables.contains("pages") else {
            return false
        }
        var device = ""
        try? db.each("SELECT value FROM sync_meta WHERE key = 'device'") { device = $0.text(0) }
        return device == deviceID
    }

    /// How far this device has applied each other device's files, or nil
    /// when the database at `store` has no synced notes yet.
    public static func applied(store: URL, deviceID: String) -> [String: Int]? {
        guard isSynced(store, deviceID: deviceID), let db = try? Connection(url: store, mode: .readWrite) else { return nil }
        defer { db.close() }
        var peers: [String: Int] = [:]
        guard (try? db.each("SELECT device, seq FROM sync_peers") { peers[$0.text(0)] = Int($0.int64(1)) }) != nil else { return nil }
        return peers
    }

    /// Make this device start again from a snapshot at the next run, when its
    /// notes did not come from `device` (it never applied a file of that
    /// device). For the moment a phone is paired with a Mac it has not synced
    /// with before: that Mac's notes then take the place of the ones here.
    /// Only for a device with no changes of its own to keep. Returns whether
    /// the notes will start again.
    @discardableResult
    public static func startAgain(unlessSyncedWith device: String, store: URL, deviceID: String) throws -> Bool {
        guard let peers = applied(store: store, deviceID: deviceID), peers[device] == nil else { return false }
        let db = try Connection(url: store, mode: .readWrite)
        defer { db.close() }
        // With no device of its own the database counts as "not synced yet".
        try db.run("DELETE FROM sync_meta WHERE key = 'device'")
        return true
    }

    /// The snapshot with the newest notes that is on this device: for each
    /// device its latest one (after the seq in `after`, when given), then the
    /// one with the highest clock.
    static func newestSnapshot(root: URL, after: [String: Int], isHere: (URL) -> Bool, waiting: inout Bool) -> Snapshot? {
        var best: Snapshot?
        for folder in entries(in: root.appendingPathComponent("snapshots"), isHere: { _ in true }) {
            if !after.isEmpty && after[folder.name] == nil {
                continue
            }
            let found = entries(in: folder.url, isHere: isHere)
                .filter { $0.name.hasSuffix(".db") }
                .compactMap { entry in SyncFormat.seq(inFileName: entry.name).map { (seq: $0, entry: entry) } }
                .filter { $0.seq > (after[folder.name] ?? 0) }
                .sorted { $0.seq > $1.seq }
            if found.contains(where: { !$0.entry.here }) {
                waiting = true
            }
            guard let latest = found.first(where: { $0.entry.here }), let clock = clock(ofSnapshot: latest.entry.url) else { continue }
            let candidate = Snapshot(device: folder.name, seq: latest.seq, url: latest.entry.url, clock: clock)
            if best.map({ (candidate.clock, candidate.device) > ($0.clock, $0.device) }) ?? true {
                best = candidate
            }
        }
        return best
    }

    /// The clock of a snapshot, or nil when the file is not a snapshot.
    private static func clock(ofSnapshot url: URL) -> Int64? {
        guard let db = try? Connection(url: url) else { return nil }
        defer { db.close() }
        guard let tables = try? db.tableNames(), ["pages", "blocks", "sync_records", "sync_meta"].allSatisfy(tables.contains) else {
            return nil
        }
        var clock: Int64 = 0
        try? db.each("SELECT value FROM sync_meta WHERE key = 'clock'") { clock = Int64($0.text(0)) ?? 0 }
        return clock
    }

    /// Make a copy of the snapshot this device's database.
    static func adopt(_ snapshot: Snapshot, store: URL, deviceID: String) throws {
        let files = FileManager.default
        let folder = store.deletingLastPathComponent()
        try files.createDirectory(at: folder, withIntermediateDirectories: true)
        let temp = folder.appendingPathComponent("snapshot-\(UUID().uuidString).db")
        defer { try? files.removeItem(at: temp) }
        try files.copyItem(at: snapshot.url, to: temp)

        let db = try Connection(url: temp, mode: .readWrite)
        do {
            try db.script("PRAGMA journal_mode = DELETE;" + SyncFormat.tables)
            try db.transaction {
                // Go on after the snapshot in its author's files.
                try db.run(
                    "INSERT INTO sync_peers(device, seq) VALUES (?1, ?2) ON CONFLICT(device) DO UPDATE SET seq = ?2",
                    [.text(snapshot.device), .number(snapshot.seq)]
                )
                try db.run("DELETE FROM sync_peers WHERE device = ?1", [.text(deviceID)])
                try db.run(
                    "INSERT INTO sync_meta(key, value) VALUES ('device', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                    [.text(deviceID)]
                )
                try db.run("INSERT INTO sync_meta(key, value) VALUES ('seq', '0') ON CONFLICT(key) DO UPDATE SET value = '0'")
                // What the snapshot's author had not sent yet is not this
                // device's to send.
                try db.run("UPDATE sync_records SET synced_clock = clock, synced_device = device")
            }
            db.close()
        } catch {
            db.close()
            throw error
        }

        try? files.removeItem(at: URL(fileURLWithPath: store.path + "-journal"))
        if files.fileExists(atPath: store.path) {
            _ = try files.replaceItemAt(store, withItemAt: temp)
        } else {
            try files.moveItem(at: temp, to: store)
        }
    }

    // MARK: Changes

    /// Apply every change file that is next in line. Returns how far each
    /// device was applied, and a device whose next file is gone (if any).
    private static func applyChanges(
        root: URL,
        store: URL,
        deviceID: String,
        deviceFiles: [String: SyncDeviceFile],
        isHere: (URL) -> Bool,
        report: inout SyncReport
    ) throws -> (peers: [String: Int], behind: String?) {
        let db = try Connection(url: store, mode: .readWrite)
        defer { db.close() }
        try db.script(SyncFormat.tables)
        try SyncFormat.addMissingColumns(db)

        var peers: [String: Int] = [:]
        try db.each("SELECT device, seq FROM sync_peers") { peers[$0.text(0)] = Int($0.int64(1)) }

        var behind: String?
        for folder in entries(in: root.appendingPathComponent("changes"), isHere: { _ in true }) where folder.name != deviceID {
            let device = folder.name
            var changeFiles: [Int: Entry] = [:]
            for entry in entries(in: folder.url, isHere: isHere) where entry.name.hasSuffix(".json") {
                if let seq = SyncFormat.seq(inFileName: entry.name) {
                    changeFiles[seq] = entry
                }
            }
            // A snapshot uses a number of the same counter: it is not a gap.
            let snapshotSeqs = Set(
                entries(in: root.appendingPathComponent("snapshots").appendingPathComponent(device), isHere: { _ in true })
                    .compactMap { SyncFormat.seq(inFileName: $0.name) }
            )
            let last = max(changeFiles.keys.max() ?? 0, deviceFiles[device]?.seq ?? 0)
            var next = (peers[device] ?? 0) + 1
            while next <= last {
                if let entry = changeFiles[next] {
                    guard entry.here,
                          let data = try? Data(contentsOf: entry.url),
                          let file = try? JSONDecoder().decode(SyncChangeFile.self, from: data)
                    else {
                        report.waiting = true
                        break
                    }
                    if file.format > SyncFormat.version {
                        report.needsUpdate = true
                        break
                    }
                    try db.transaction {
                        for change in file.changes {
                            try apply(change, to: db, report: &report)
                        }
                        try setPeer(db, device, next)
                    }
                } else if snapshotSeqs.contains(next) {
                    try setPeer(db, device, next)
                } else {
                    // Not here, not even on its way.
                    if snapshotSeqs.contains(where: { $0 > next }) {
                        behind = behind ?? device
                    } else {
                        report.waiting = true
                    }
                    break
                }
                peers[device] = next
                next += 1
            }
        }
        return (peers, behind)
    }

    private static func setPeer(_ db: Connection, _ device: String, _ seq: Int) throws {
        try db.run(
            "INSERT INTO sync_peers(device, seq) VALUES (?1, ?2) ON CONFLICT(device) DO UPDATE SET seq = ?2",
            [.text(device), .number(seq)]
        )
    }

    /// Apply one change by the rules of `docs/sync-format.md`.
    static func apply(_ change: SyncChange, to db: Connection, report: inout SyncReport) throws {
        guard ["page", "event", "space"].contains(change.kind), change.isComplete else { return }

        var local: (version: SyncVersion, removed: Bool)?
        try db.each(
            "SELECT clock, device, removed FROM sync_records WHERE kind = ?1 AND id = ?2",
            values: [.text(change.kind), .text(change.id)]
        ) { row in
            local = (SyncVersion(clock: row.int64(0), device: row.text(1)), row.int64(2) != 0)
        }

        // This device's clock is never behind a change it has seen.
        try db.run(
            """
            INSERT INTO sync_meta(key, value) VALUES ('clock', ?1)
            ON CONFLICT(key) DO UPDATE SET value = CAST(max(CAST(value AS INTEGER), ?1) AS TEXT)
            """,
            [.int(change.version.clock)]
        )

        var take = true
        if let local {
            if local.version == change.version {
                return
            }
            if change.base != local.version {
                // Changed in two places at once. The one that is not a
                // removal wins; else the newer one. The loser of two pages
                // goes to the page's history.
                report.conflicts += 1
                let changeWins = (change.removed ? 0 : 1, change.version) > (local.removed ? 0 : 1, local.version)
                if changeWins {
                    if change.kind == "page" && !local.removed {
                        try keepCurrentPageInHistory(db, change.id)
                    }
                } else {
                    if change.kind == "page", let page = change.page {
                        try addToHistory(db, pageID: change.id, title: page.title, blocks: page.blocks, at: page.updatedAt)
                    }
                    take = false
                }
            }
        }
        guard take else { return }
        try writeRows(change, to: db)

        try db.run(
            """
            INSERT INTO sync_records(kind, id, clock, device, synced_clock, synced_device, removed) VALUES (?1, ?2, ?3, ?4, ?3, ?4, ?5)
            ON CONFLICT(kind, id) DO UPDATE SET clock = ?3, device = ?4, synced_clock = ?3, synced_device = ?4, removed = ?5
            """,
            [.text(change.kind), .text(change.id), .int(change.version.clock), .text(change.version.device), .bool(change.removed)]
        )
        report.applied += 1
    }

    /// Put a record's rows in the database as a change has them (or take
    /// them out for a removal). Shared by applying another device's change
    /// and by an edit made here (`NotesWriter`).
    static func writeRows(_ change: SyncChange, to db: Connection) throws {
        switch change.kind {
        case "page":
            try db.run("DELETE FROM blocks WHERE page_id = ?1", [.text(change.id)])
            if let page = change.page, !change.removed {
                try db.run(
                    """
                    INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id, cover, cover_y, font, row_of)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                    ON CONFLICT(id) DO UPDATE SET parent_id = ?2, title = ?3, icon = ?4, position = ?5, favorite = ?6,
                        trashed = ?7, created_at = ?8, updated_at = ?9, space_id = ?10, cover = ?11, cover_y = ?12, font = ?13, row_of = ?14
                    """,
                    [
                        .text(change.id), .maybeText(page.parentID), .text(page.title), .maybeText(page.icon), .double(page.position),
                        .bool(page.favorite), .bool(page.trashed), .int(page.createdAt), .int(page.updatedAt), .text(page.spaceID),
                        .maybeText(page.cover), .number(min(max(page.coverY, 0), 100)), .text(page.font),
                        .maybeText(page.rowOf),
                    ]
                )
                for (position, block) in page.blocks.enumerated() {
                    // "OR REPLACE": a block that moved here from another page
                    // may still be in that page until its change is applied.
                    try db.run(
                        """
                        INSERT OR REPLACE INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind)
                        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                        """,
                        [
                            .text(block.id), .text(change.id), .int(Int64(position)), .text(block.kind), .text(block.text),
                            .bool(block.checked), .int(Int64(block.indent)), .bool(block.collapsed), .maybeText(block.icon),
                            .maybeText(block.language), .maybeText(block.target), .text(block.data), .text(block.place), .text(block.due),
                            .text(block.remind),
                        ]
                    )
                }
            } else {
                try db.run("DELETE FROM page_versions WHERE page_id = ?1", [.text(change.id)])
                try db.run("DELETE FROM pages WHERE id = ?1", [.text(change.id)])
            }
        case "event":
            if let event = change.event, !change.removed {
                try db.run(
                    """
                    INSERT INTO events(id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat, join_url)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                    ON CONFLICT(id) DO UPDATE SET space_id = ?2, title = ?3, start_ms = ?4, end_ms = ?5, all_day = ?6, color = ?7,
                        notes = ?8, page_id = ?9, created_at = ?10, updated_at = ?11, remind_min = ?12, repeat = ?13, join_url = ?14
                    """,
                    [
                        .text(change.id), .text(event.spaceID), .text(event.title), .int(event.startMS), .int(event.endMS),
                        .bool(event.allDay), .text(event.color), .text(event.notes), .maybeText(event.pageID), .int(event.createdAt),
                        .int(event.updatedAt), .maybeInt(event.remindMin), .text(event.repeatRule), .text(event.joinURL),
                    ]
                )
            } else {
                try db.run("DELETE FROM events WHERE id = ?1", [.text(change.id)])
            }
        default:
            if let space = change.space, !change.removed {
                try db.run(
                    """
                    INSERT INTO spaces(id, name, color, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(id) DO UPDATE SET name = ?2, color = ?3, position = ?4, created_at = ?5
                    """,
                    [.text(change.id), .text(space.name), .text(space.color), .double(space.position), .int(space.createdAt)]
                )
            } else {
                try db.run("DELETE FROM spaces WHERE id = ?1", [.text(change.id)])
            }
        }

    }

    // MARK: Page history

    /// How many earlier versions of a page are kept (as on the desktop).
    static let versionsKept = 60

    private static func keepCurrentPageInHistory(_ db: Connection, _ pageID: ID) throws {
        var title: String?
        var updatedAt: Int64 = 0
        try db.each("SELECT title, updated_at FROM pages WHERE id = ?1", bind: [pageID]) { row in
            title = row.text(0)
            updatedAt = row.int64(1)
        }
        guard let title else { return }
        var blocks: [SyncBlock] = []
        try db.each(
            "SELECT id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind FROM blocks WHERE page_id = ?1 ORDER BY position",
            bind: [pageID]
        ) { row in
            blocks.append(SyncBlock(
                id: row.text(0), kind: row.text(1), text: row.text(2), checked: row.int64(3) != 0, indent: Int(row.int64(4)),
                collapsed: row.int64(5) != 0, icon: row.optionalText(6), language: row.optionalText(7), target: row.optionalText(8),
                data: row.text(9), place: row.text(10), due: row.text(11), remind: row.text(12)
            ))
        }
        try addToHistory(db, pageID: pageID, title: title, blocks: blocks, at: updatedAt)
    }

    /// A block as the desktop app writes it in a page's history.
    private struct HistoryBlock: Encodable {
        let id: String
        let kind: String
        let text: String
        let checked: Bool
        let indent: Int
        let collapsed: Bool
        let icon: String?
        let language: String?
        let target: String?
        let data: String
        /// A picture's place as the desktop writes it: the object itself,
        /// and nothing at all for a picture in line at its own size.
        let place: JSONValue?
        /// A to-do's due day, and nothing at all for none.
        let due: String?
        /// A to-do's reminder time, and nothing at all for none.
        let remind: String?

        func encode(to encoder: Encoder) throws {
            var c = encoder.container(keyedBy: CodingKeys.self)
            try c.encode(id, forKey: .id)
            try c.encode(kind, forKey: .kind)
            try c.encode(text, forKey: .text)
            try c.encode(checked, forKey: .checked)
            try c.encode(indent, forKey: .indent)
            try c.encode(collapsed, forKey: .collapsed)
            try c.encode(icon, forKey: .icon)
            try c.encode(language, forKey: .language)
            try c.encode(target, forKey: .target)
            try c.encode(data, forKey: .data)
            try c.encodeIfPresent(place, forKey: .place)
            try c.encodeIfPresent(due, forKey: .due)
            try c.encodeIfPresent(remind, forKey: .remind)
        }

        enum CodingKeys: String, CodingKey {
            case id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind
        }
    }

    /// Keep a copy of a page as it was. The same copy twice in a row is kept
    /// once, and only the newest `versionsKept` copies stay.
    static func addToHistory(_ db: Connection, pageID: ID, title: String, blocks: [SyncBlock], at: Int64) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let history = blocks.map {
            HistoryBlock(
                id: $0.id, kind: SyncFormat.historyKind($0.kind), text: $0.text, checked: $0.checked, indent: $0.indent,
                collapsed: $0.collapsed, icon: $0.icon, language: $0.language, target: $0.target, data: $0.data,
                place: $0.place.isEmpty ? nil : try? JSONDecoder().decode(JSONValue.self, from: Data($0.place.utf8)),
                due: $0.due.isEmpty ? nil : $0.due,
                remind: $0.remind.isEmpty ? nil : $0.remind
            )
        }
        let json = String(decoding: try encoder.encode(history), as: UTF8.self)
        var same = false
        try db.each(
            "SELECT title, blocks FROM page_versions WHERE page_id = ?1 ORDER BY at DESC, id DESC LIMIT 1",
            bind: [pageID]
        ) { row in
            same = row.text(0) == title && row.text(1) == json
        }
        if same {
            return
        }
        try db.run(
            "INSERT INTO page_versions(page_id, at, title, blocks) VALUES (?1, ?2, ?3, ?4)",
            [.text(pageID), .int(at > 0 ? at : When.ms(Date())), .text(title), .text(json)]
        )
        try db.run(
            """
            DELETE FROM page_versions WHERE page_id = ?1 AND id NOT IN
                (SELECT id FROM page_versions WHERE page_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2)
            """,
            [.text(pageID), .int(Int64(versionsKept))]
        )
    }
}

/// Any JSON value, kept as it is: for a field this app passes on and does
/// not read (a picture's place in a page's history).
enum JSONValue: Codable, Hashable, Sendable {
    case null
    case bool(Bool)
    case int(Int64)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self = .null
        } else if let value = try? c.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? c.decode(Int64.self) {
            self = .int(value)
        } else if let value = try? c.decode(Double.self) {
            self = .number(value)
        } else if let value = try? c.decode(String.self) {
            self = .string(value)
        } else if let value = try? c.decode([JSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try c.decode([String: JSONValue].self))
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case let .bool(value): try c.encode(value)
        case let .int(value): try c.encode(value)
        case let .number(value): try c.encode(value)
        case let .string(value): try c.encode(value)
        case let .array(value): try c.encode(value)
        case let .object(value): try c.encode(value)
        }
    }
}
