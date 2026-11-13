// Changes made on this device (docs/sync-format.md, "Writing"). Each edit
// writes the record's rows and gives the record a new version (this
// device's clock, one step up) in one transaction. `SyncEngine.push` then
// writes every record whose version is not the one last sent into a change
// file in this device's outbox, and `SyncEngine.publish` copies the outbox
// into each sync folder (iCloud Drive, the copy of the Mac's folder that
// Wi-Fi uploads from).
//
// The phone changes only notes that came from sync (a copy loaded by hand
// would have nowhere to send its changes), and never notes from a newer
// Rune: it would write rows without the columns it does not know.

import Foundation

public enum NotesWriterError: Error, LocalizedError, Equatable {
    /// The notes did not come from sync: a change could not be sent.
    case notSynced
    /// The notes come from a newer Rune.
    case newerNotes

    public var errorDescription: String? {
        switch self {
        case .notSynced: "These notes are a copy. Connect to your Mac to change them."
        case .newerNotes: "These notes come from a newer Rune. Update this app to change them."
        }
    }
}

public enum NotesWriter {
    /// The form of the notes file this app writes (the Mac's `store::SCHEMA`
    /// it was built for). A file with a greater mark is only read.
    public static let schema: Int64 = 7

    /// Whether the notes at `store` may be changed here.
    public static func canEdit(store: URL, deviceID: String) -> Bool {
        (try? check(store: store, deviceID: deviceID)) != nil
    }

    private static func check(store: URL, deviceID: String) throws {
        guard SyncEngine.isSynced(store, deviceID: deviceID) else { throw NotesWriterError.notSynced }
        let db = try Connection(url: store, mode: .readWrite)
        defer { db.close() }
        var mark: Int64 = 0
        try db.each("PRAGMA user_version") { mark = $0.int64(0) }
        if mark > schema {
            throw NotesWriterError.newerNotes
        }
    }

    /// Save a page with all of its blocks (`at`: the time of the change).
    public static func save(page: Page, blocks: [Block], store: URL, deviceID: String, at: Int64 = When.ms(Date())) throws {
        var page = page
        page.updatedAt = max(at, page.updatedAt)
        try write(store: store, deviceID: deviceID) { db in
            // The phone does not hold a drawing's scene (it is read when it
            // is shown): keep the one that is stored.
            var scenes: [String: String] = [:]
            try db.each("SELECT id, data FROM blocks WHERE page_id = ?1 AND kind = 'drawing'", bind: [page.id]) { scenes[$0.text(0)] = $0.text(1) }
            let synced = blocks.map { block in
                var b = SyncBlock(block)
                if block.kind == .drawing, block.data.isEmpty, let scene = scenes[block.id] {
                    b.data = scene
                }
                return b
            }
            return SyncChange(kind: "page", id: page.id, version: SyncVersion(clock: 0, device: deviceID), base: nil, page: SyncPage(page, blocks: synced))
        }
    }

    /// Tick a to-do (or take the tick away) where it is.
    public static func setChecked(_ checked: Bool, block blockID: ID, page pageID: ID, store: URL, deviceID: String, at: Int64 = When.ms(Date())) throws {
        let notes = try WorkspaceDatabase.load(from: store)
        guard let page = notes.pages[pageID], var blocks = notes.blocks[pageID], let i = blocks.firstIndex(where: { $0.id == blockID }) else { return }
        blocks[i].checked = checked
        try save(page: page, blocks: blocks, store: store, deviceID: deviceID, at: at)
    }

    /// Save an event (the stored event, not one repeat of it).
    public static func save(event: Event, store: URL, deviceID: String, at: Int64 = When.ms(Date())) throws {
        var event = event
        event.updatedAt = max(at, event.updatedAt)
        if event.createdAt == 0 {
            event.createdAt = event.updatedAt
        }
        try write(store: store, deviceID: deviceID) { _ in
            SyncChange(kind: "event", id: event.id, version: SyncVersion(clock: 0, device: deviceID), base: nil, event: SyncEvent(event))
        }
    }

    /// Delete an event for good.
    public static func remove(event id: ID, store: URL, deviceID: String) throws {
        try write(store: store, deviceID: deviceID) { _ in
            SyncChange(kind: "event", id: id, version: SyncVersion(clock: 0, device: deviceID), base: nil, removed: true)
        }
    }

    /// Write the change's rows and give its record a new version of this
    /// device, in one transaction. The version last sent stays, so the next
    /// push sends the record with it as its base.
    private static func write(store: URL, deviceID: String, _ make: (Connection) throws -> SyncChange) throws {
        try check(store: store, deviceID: deviceID)
        let db = try Connection(url: store, mode: .readWrite)
        defer { db.close() }
        try db.script(SyncFormat.tables)
        try SyncFormat.addMissingColumns(db)
        try db.transaction {
            let change = try make(db)
            try SyncEngine.writeRows(change, to: db)
            var clock: Int64 = 0
            try db.each("SELECT value FROM sync_meta WHERE key = 'clock'") { clock = Int64($0.text(0)) ?? 0 }
            clock += 1
            try db.run(
                "INSERT INTO sync_meta(key, value) VALUES ('clock', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                [.text(String(clock))]
            )
            try db.run(
                """
                INSERT INTO sync_records(kind, id, clock, device, removed) VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(kind, id) DO UPDATE SET clock = ?3, device = ?4, removed = ?5
                """,
                [.text(change.kind), .text(change.id), .int(clock), .text(deviceID), .bool(change.removed)]
            )
        }
    }
}

// MARK: - From the app's model to the form records travel in

extension SyncBlock {
    public init(_ b: Block) {
        self.init(
            id: b.id, kind: b.kind.rawValue, text: b.text, checked: b.checked, indent: b.indent, collapsed: b.collapsed,
            icon: b.icon, language: b.language, target: b.target, data: b.data, place: b.place, due: b.due, remind: b.remind
        )
    }
}

extension SyncPage {
    public init(_ p: Page, blocks: [SyncBlock]) {
        self.init(
            spaceID: p.spaceID, parentID: p.parentID, title: p.title, icon: p.icon, position: p.position, favorite: p.favorite,
            trashed: p.trashed, createdAt: p.createdAt, updatedAt: p.updatedAt, cover: p.cover, coverY: p.coverY, font: p.font,
            rowOf: p.rowOf, blocks: blocks
        )
    }
}

extension SyncEvent {
    public init(_ e: Event) {
        let rule = e.repeatRule.flatMap { try? JSONEncoder().encode($0) }.map { String(decoding: $0, as: UTF8.self) } ?? ""
        self.init(
            spaceID: e.spaceID, title: e.title, startMS: e.start, endMS: e.end, allDay: e.allDay, color: e.color, notes: e.notes,
            pageID: e.pageID, createdAt: e.createdAt, updatedAt: e.updatedAt, remindMin: e.remind, repeatRule: rule, joinURL: e.join
        )
    }
}

// MARK: - Sending

extension SyncEngine {
    /// Write every record this device changed and has not sent into the next
    /// change file in `outbox` (a `sync-v1` folder of this device's own
    /// files). Returns how many records went.
    @discardableResult
    public static func push(outbox: URL, store: URL, deviceID: String, now: Int64 = When.ms(Date())) throws -> Int {
        guard isSynced(store, deviceID: deviceID) else { return 0 }
        let db = try Connection(url: store, mode: .readWrite)
        defer { db.close() }
        try db.script(SyncFormat.tables)
        struct Dirty {
            let kind: String
            let id: String
            let version: SyncVersion
            let base: SyncVersion?
            let removed: Bool
        }
        var dirty: [Dirty] = []
        try db.each(
            """
            SELECT kind, id, clock, device, synced_clock, synced_device, removed FROM sync_records
            WHERE device = ?1 AND (synced_clock IS NULL OR clock != synced_clock OR device != synced_device) ORDER BY clock
            """,
            bind: [deviceID]
        ) { row in
            dirty.append(Dirty(
                kind: row.text(0), id: row.text(1), version: SyncVersion(clock: row.int64(2), device: row.text(3)),
                base: row.isNull(4) ? nil : SyncVersion(clock: row.int64(4), device: row.text(5)), removed: row.int64(6) != 0
            ))
        }
        guard !dirty.isEmpty else { return 0 }
        let notes = try WorkspaceDatabase.load(from: store)
        var changes: [SyncChange] = []
        for d in dirty {
            var change = SyncChange(kind: d.kind, id: d.id, version: d.version, base: d.base, removed: d.removed)
            if !d.removed {
                switch d.kind {
                case "page":
                    if let page = notes.pages[d.id] {
                        var blocks: [SyncBlock] = []
                        try db.each(
                            "SELECT id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind FROM blocks WHERE page_id = ?1 ORDER BY position",
                            bind: [d.id]
                        ) { row in
                            blocks.append(SyncBlock(
                                id: row.text(0), kind: row.text(1), text: row.text(2), checked: row.int64(3) != 0, indent: Int(row.int64(4)),
                                collapsed: row.int64(5) != 0, icon: row.optionalText(6), language: row.optionalText(7), target: row.optionalText(8),
                                data: row.text(9), place: row.text(10), due: row.text(11), remind: row.text(12)
                            ))
                        }
                        change.page = SyncPage(page, blocks: blocks)
                    }
                case "event":
                    change.event = notes.events.first { $0.id == d.id }.map(SyncEvent.init)
                default:
                    change.space = notes.spaces.first { $0.id == d.id }.map { SyncSpace(name: $0.name, color: $0.color, position: $0.position, createdAt: $0.createdAt) }
                }
                change.removed = !change.isComplete
            }
            changes.append(change)
        }

        var seq = 0
        try db.each("SELECT value FROM sync_meta WHERE key = 'seq'") { seq = Int($0.text(0)) ?? 0 }
        seq += 1
        let file = SyncChangeFile(device: deviceID, seq: seq, at: now, changes: changes)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(file)
        try placeOnce(data, at: SyncPath.change(device: deviceID, seq: seq).path, in: outbox)
        try db.transaction {
            try db.run(
                "INSERT INTO sync_meta(key, value) VALUES ('seq', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                [.text(String(seq))]
            )
            for d in dirty {
                try db.run(
                    "UPDATE sync_records SET synced_clock = clock, synced_device = device WHERE kind = ?1 AND id = ?2 AND clock = ?3 AND device = ?4",
                    [.text(d.kind), .text(d.id), .int(d.version.clock), .text(d.version.device)]
                )
            }
        }
        return dirty.count
    }

    /// Copy each file of the outbox that `root` (a sync folder) lacks. A
    /// file is written once: one that is there is left as it is.
    public static func publish(outbox: URL, to root: URL) throws {
        let files = FileManager.default
        for kind in ["changes", "snapshots"] {
            for owner in entries(in: outbox.appendingPathComponent(kind), isHere: { _ in true }) {
                for entry in entries(in: owner.url, isHere: { _ in true }) {
                    let path = "\(kind)/\(owner.name)/\(entry.name)"
                    guard SyncPath(path) != nil, !files.fileExists(atPath: root.appendingPathComponent(path).path) else { continue }
                    try placeOnce(try Data(contentsOf: entry.url), at: path, in: root)
                }
            }
        }
    }

    /// The last file this device wrote (its device file says it).
    static func ownSeq(_ store: URL) -> Int {
        guard let db = try? Connection(url: store, mode: .readWrite) else { return 0 }
        defer { db.close() }
        var seq = 0
        try? db.each("SELECT value FROM sync_meta WHERE key = 'seq'") { seq = Int($0.text(0)) ?? 0 }
        return seq
    }

    /// Made beside the target under a hidden name first, then moved in, so a
    /// half-written file is never seen.
    static func placeOnce(_ data: Data, at path: String, in root: URL) throws {
        let files = FileManager.default
        let target = root.appendingPathComponent(path)
        guard !files.fileExists(atPath: target.path) else { return }
        try files.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        let temp = target.deletingLastPathComponent().appendingPathComponent(".out-\(UUID().uuidString)")
        try data.write(to: temp)
        do {
            try files.moveItem(at: temp, to: target)
        } catch {
            try? files.removeItem(at: temp)
            if !files.fileExists(atPath: target.path) {
                throw error
            }
        }
    }
}
