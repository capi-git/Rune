// The files that devices exchange to keep the same notes (format 1). The
// rules are in `docs/sync-format.md`; this file is their Swift form.

import Foundation

/// The version of one record: a device's clock when it changed the record.
public struct SyncVersion: Codable, Hashable, Sendable, Comparable {
    public var clock: Int64
    public var device: String

    public init(clock: Int64, device: String) {
        self.clock = clock
        self.device = device
    }

    /// Newer: a higher clock, or the same clock and the greater device id.
    public static func < (a: SyncVersion, b: SyncVersion) -> Bool {
        a.clock != b.clock ? a.clock < b.clock : a.device < b.device
    }
}

/// `changes/<device>/<seq>.json`: what a device changed.
public struct SyncChangeFile: Codable, Sendable {
    public var format: Int
    public var device: String
    public var seq: Int
    public var at: Int64
    public var changes: [SyncChange]

    public init(format: Int = SyncFormat.version, device: String, seq: Int, at: Int64, changes: [SyncChange]) {
        self.format = format
        self.device = device
        self.seq = seq
        self.at = at
        self.changes = changes
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        format = try c.decode(Int.self, forKey: .format)
        device = try c.decodeIfPresent(String.self, forKey: .device) ?? ""
        seq = try c.decodeIfPresent(Int.self, forKey: .seq) ?? 0
        at = try c.decodeIfPresent(Int64.self, forKey: .at) ?? 0
        // A file of a later format is not read past its format number.
        changes = format > SyncFormat.version ? [] : try c.decodeIfPresent([SyncChange].self, forKey: .changes) ?? []
    }
}

/// One record as it is now on the device that changed it, or its removal.
public struct SyncChange: Codable, Sendable {
    /// "page", "event" or "space". A kind this app does not know is skipped.
    public var kind: String
    public var id: ID
    public var version: SyncVersion
    /// The version the writer last wrote to, or read from, the folder.
    public var base: SyncVersion?
    public var removed: Bool
    public var page: SyncPage?
    public var event: SyncEvent?
    public var space: SyncSpace?

    public init(kind: String, id: ID, version: SyncVersion, base: SyncVersion?, removed: Bool = false, page: SyncPage? = nil, event: SyncEvent? = nil, space: SyncSpace? = nil) {
        self.kind = kind
        self.id = id
        self.version = version
        self.base = base
        self.removed = removed
        self.page = page
        self.event = event
        self.space = space
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(String.self, forKey: .kind)
        id = try c.decode(String.self, forKey: .id)
        version = try c.decode(SyncVersion.self, forKey: .version)
        base = try c.decodeIfPresent(SyncVersion.self, forKey: .base)
        removed = try c.decodeIfPresent(Bool.self, forKey: .removed) ?? false
        page = try c.decodeIfPresent(SyncPage.self, forKey: .page)
        event = try c.decodeIfPresent(SyncEvent.self, forKey: .event)
        space = try c.decodeIfPresent(SyncSpace.self, forKey: .space)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(kind, forKey: .kind)
        try c.encode(id, forKey: .id)
        try c.encode(version, forKey: .version)
        try c.encode(base, forKey: .base)
        if removed {
            try c.encode(true, forKey: .removed)
        }
        try c.encodeIfPresent(page, forKey: .page)
        try c.encodeIfPresent(event, forKey: .event)
        try c.encodeIfPresent(space, forKey: .space)
    }

    enum CodingKeys: String, CodingKey {
        case kind, id, version, base, removed, page, event, space
    }

    /// Whether the change says what its kind needs.
    var isComplete: Bool {
        if removed {
            return true
        }
        switch kind {
        case "page": return page != nil
        case "event": return event != nil
        case "space": return space != nil
        default: return false
        }
    }
}

// The names below are the column names of the database (`src/store.rs`).

public struct SyncPage: Codable, Hashable, Sendable {
    public var spaceID: String
    public var parentID: String?
    public var title: String
    public var icon: String?
    public var position: Double
    public var favorite: Bool
    public var trashed: Bool
    public var createdAt: Int64
    public var updatedAt: Int64
    /// The page's cover as stored ("preset:<name>" or "file:<name>").
    public var cover: String?
    public var coverY: Int
    /// The face of the page's title and headings as stored: "" (sans),
    /// "serif" or "mono". Any other value passes through unchanged.
    public var font: String
    /// For the page of a row of a table: "<block id>/<row id>" (nil for an
    /// ordinary page). Passes through unchanged.
    public var rowOf: String?
    public var blocks: [SyncBlock]

    enum CodingKeys: String, CodingKey {
        case spaceID = "space_id", parentID = "parent_id", title, icon, position, favorite, trashed
        case createdAt = "created_at", updatedAt = "updated_at", cover, coverY = "cover_y", font, rowOf = "row_of", blocks
    }

    public init(spaceID: String = "", parentID: String? = nil, title: String = "", icon: String? = nil, position: Double = 0, favorite: Bool = false, trashed: Bool = false, createdAt: Int64 = 0, updatedAt: Int64 = 0, cover: String? = nil, coverY: Int = 50, font: String = "", rowOf: String? = nil, blocks: [SyncBlock] = []) {
        self.spaceID = spaceID
        self.parentID = parentID
        self.title = title
        self.icon = icon
        self.position = position
        self.favorite = favorite
        self.trashed = trashed
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.cover = cover
        self.coverY = coverY
        self.font = font
        self.rowOf = rowOf
        self.blocks = blocks
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        cover = try c.decodeIfPresent(String.self, forKey: .cover)
        coverY = try c.decodeIfPresent(Int.self, forKey: .coverY) ?? 50
        font = try c.decodeIfPresent(String.self, forKey: .font) ?? ""
        rowOf = try c.decodeIfPresent(String.self, forKey: .rowOf)
        spaceID = try c.decodeIfPresent(String.self, forKey: .spaceID) ?? ""
        parentID = try c.decodeIfPresent(String.self, forKey: .parentID)
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        icon = try c.decodeIfPresent(String.self, forKey: .icon)
        position = try c.decodeIfPresent(Double.self, forKey: .position) ?? 0
        favorite = try c.decodeIfPresent(Bool.self, forKey: .favorite) ?? false
        trashed = try c.decodeIfPresent(Bool.self, forKey: .trashed) ?? false
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
        updatedAt = try c.decodeIfPresent(Int64.self, forKey: .updatedAt) ?? 0
        blocks = try c.decodeIfPresent([SyncBlock].self, forKey: .blocks) ?? []
    }
}

public struct SyncBlock: Codable, Hashable, Sendable {
    public var id: String
    /// As stored: "text", "heading1", …, "page", "drawing", "image",
    /// "table". A kind this app does not know passes through unchanged,
    /// with its `data`.
    public var kind: String
    public var text: String
    public var checked: Bool
    public var indent: Int
    public var collapsed: Bool
    public var icon: String?
    public var language: String?
    public var target: String?
    public var data: String
    /// Where a picture sits and how wide it is, as stored (JSON, or "" for
    /// in line at its own size). It passes through unchanged.
    public var place: String
    /// The day a to-do is due as stored ("2026-11-12", or "" for none).
    public var due: String
    /// The time of a to-do's reminder as stored ("09:00", or "" for none).
    public var remind: String

    public init(id: String, kind: String, text: String = "", checked: Bool = false, indent: Int = 0, collapsed: Bool = false, icon: String? = nil, language: String? = nil, target: String? = nil, data: String = "", place: String = "", due: String = "", remind: String = "") {
        self.id = id
        self.kind = kind
        self.text = text
        self.checked = checked
        self.indent = indent
        self.collapsed = collapsed
        self.icon = icon
        self.language = language
        self.target = target
        self.data = data
        self.place = place
        self.due = due
        self.remind = remind
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "text"
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
        checked = try c.decodeIfPresent(Bool.self, forKey: .checked) ?? false
        indent = try c.decodeIfPresent(Int.self, forKey: .indent) ?? 0
        collapsed = try c.decodeIfPresent(Bool.self, forKey: .collapsed) ?? false
        icon = try c.decodeIfPresent(String.self, forKey: .icon)
        language = try c.decodeIfPresent(String.self, forKey: .language)
        target = try c.decodeIfPresent(String.self, forKey: .target)
        data = try c.decodeIfPresent(String.self, forKey: .data) ?? ""
        place = try c.decodeIfPresent(String.self, forKey: .place) ?? ""
        due = try c.decodeIfPresent(String.self, forKey: .due) ?? ""
        remind = try c.decodeIfPresent(String.self, forKey: .remind) ?? ""
    }
}

public struct SyncEvent: Codable, Hashable, Sendable {
    public var spaceID: String
    public var title: String
    public var startMS: Int64
    public var endMS: Int64
    public var allDay: Bool
    public var color: String
    public var notes: String
    public var pageID: String?
    public var createdAt: Int64
    public var updatedAt: Int64
    public var remindMin: Int?
    /// The repeat rule as stored ("" for none).
    public var repeatRule: String
    /// The address to join the event's call ("" for none).
    public var joinURL: String

    enum CodingKeys: String, CodingKey {
        case spaceID = "space_id", title, startMS = "start_ms", endMS = "end_ms", allDay = "all_day", color, notes
        case pageID = "page_id", createdAt = "created_at", updatedAt = "updated_at", remindMin = "remind_min"
        case repeatRule = "repeat", joinURL = "join_url"
    }

    public init(spaceID: String = "", title: String = "", startMS: Int64, endMS: Int64, allDay: Bool = false, color: String = "blue", notes: String = "", pageID: String? = nil, createdAt: Int64 = 0, updatedAt: Int64 = 0, remindMin: Int? = nil, repeatRule: String = "", joinURL: String = "") {
        self.spaceID = spaceID
        self.title = title
        self.startMS = startMS
        self.endMS = endMS
        self.allDay = allDay
        self.color = color
        self.notes = notes
        self.pageID = pageID
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.remindMin = remindMin
        self.repeatRule = repeatRule
        self.joinURL = joinURL
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        spaceID = try c.decodeIfPresent(String.self, forKey: .spaceID) ?? ""
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        startMS = try c.decode(Int64.self, forKey: .startMS)
        endMS = try c.decode(Int64.self, forKey: .endMS)
        allDay = try c.decodeIfPresent(Bool.self, forKey: .allDay) ?? false
        color = try c.decodeIfPresent(String.self, forKey: .color) ?? "blue"
        notes = try c.decodeIfPresent(String.self, forKey: .notes) ?? ""
        pageID = try c.decodeIfPresent(String.self, forKey: .pageID)
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
        updatedAt = try c.decodeIfPresent(Int64.self, forKey: .updatedAt) ?? 0
        remindMin = try c.decodeIfPresent(Int.self, forKey: .remindMin)
        repeatRule = try c.decodeIfPresent(String.self, forKey: .repeatRule) ?? ""
        joinURL = try c.decodeIfPresent(String.self, forKey: .joinURL) ?? ""
    }
}

public struct SyncSpace: Codable, Hashable, Sendable {
    public var name: String
    public var color: String
    public var position: Double
    public var createdAt: Int64

    enum CodingKeys: String, CodingKey {
        case name, color, position, createdAt = "created_at"
    }

    public init(name: String = "", color: String = "blue", position: Double = 0, createdAt: Int64 = 0) {
        self.name = name
        self.color = color
        self.position = position
        self.createdAt = createdAt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
        color = try c.decodeIfPresent(String.self, forKey: .color) ?? "blue"
        position = try c.decodeIfPresent(Double.self, forKey: .position) ?? 0
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
    }
}

/// `devices/<device>.json`: who a device is and how far it is.
public struct SyncDeviceFile: Codable, Hashable, Sendable {
    public struct Snapshot: Codable, Hashable, Sendable {
        public var seq: Int
        public var clock: Int64
    }

    public var format: Int
    public var device: String
    public var name: String
    /// "mac" or "iphone".
    public var kind: String
    public var at: Int64
    /// The last file the device wrote.
    public var seq: Int
    public var snapshot: Snapshot?
    /// The last file of each other device that it applied.
    public var applied: [String: Int]

    public init(format: Int = SyncFormat.version, device: String, name: String, kind: String, at: Int64, seq: Int, snapshot: Snapshot? = nil, applied: [String: Int] = [:]) {
        self.format = format
        self.device = device
        self.name = name
        self.kind = kind
        self.at = at
        self.seq = seq
        self.snapshot = snapshot
        self.applied = applied
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        format = try c.decodeIfPresent(Int.self, forKey: .format) ?? 1
        device = try c.decode(String.self, forKey: .device)
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? ""
        at = try c.decodeIfPresent(Int64.self, forKey: .at) ?? 0
        seq = try c.decodeIfPresent(Int.self, forKey: .seq) ?? 0
        snapshot = try c.decodeIfPresent(Snapshot.self, forKey: .snapshot)
        applied = try c.decodeIfPresent([String: Int].self, forKey: .applied) ?? [:]
    }
}

public enum SyncFormat {
    /// The format this app reads and writes.
    public static let version = 1
    /// The folder inside the container's `Documents`.
    public static let folderName = "sync-v1"
    /// The folder of the pictures inside it (`files/<name>`).
    public static let filesFolderName = "files"

    /// Columns that a database from an older version of the Mac app lacks.
    static func addMissingColumns(_ db: Connection) throws {
        let pages = try db.columnNames(of: "pages")
        if !pages.contains("cover") {
            try db.script("ALTER TABLE pages ADD COLUMN cover TEXT; ALTER TABLE pages ADD COLUMN cover_y INTEGER NOT NULL DEFAULT 50;")
        }
        if !pages.contains("font") {
            try db.script("ALTER TABLE pages ADD COLUMN font TEXT NOT NULL DEFAULT '';")
        }
        if !pages.contains("row_of") {
            try db.script("ALTER TABLE pages ADD COLUMN row_of TEXT;")
        }
        let blocks = try db.columnNames(of: "blocks")
        if !blocks.contains("place") {
            try db.script("ALTER TABLE blocks ADD COLUMN place TEXT NOT NULL DEFAULT '';")
        }
        if !blocks.contains("due") {
            try db.script("ALTER TABLE blocks ADD COLUMN due TEXT NOT NULL DEFAULT '';")
        }
        if !blocks.contains("remind") {
            try db.script("ALTER TABLE blocks ADD COLUMN remind TEXT NOT NULL DEFAULT '';")
        }
        let events = try db.columnNames(of: "events")
        if !events.isEmpty && !events.contains("join_url") {
            try db.script("ALTER TABLE events ADD COLUMN join_url TEXT NOT NULL DEFAULT '';")
        }
    }

    /// The tables every synced database has.
    static let tables = """
        CREATE TABLE IF NOT EXISTS sync_records(
            kind TEXT NOT NULL,
            id TEXT NOT NULL,
            clock INTEGER NOT NULL,
            device TEXT NOT NULL,
            synced_clock INTEGER,
            synced_device TEXT,
            removed INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(kind, id)
        );
        CREATE TABLE IF NOT EXISTS sync_peers(device TEXT PRIMARY KEY, seq INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS sync_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS page_versions(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            page_id TEXT NOT NULL,
            at INTEGER NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            blocks TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS versions_by_page ON page_versions(page_id, at);
        """

    /// A new device id: 32 lower-case hex characters.
    /// A new id for a page, a block or an event: the Mac's form (32
    /// lower-case hex characters).
    public static func newRecordID() -> String {
        newDeviceID()
    }

    public static func newDeviceID() -> String {
        UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
    }

    /// "0000000007": a write counter as it is in a file name.
    public static func name(ofSeq seq: Int) -> String {
        String(format: "%010d", seq)
    }

    /// The counter in a file name such as "0000000007.json".
    public static func seq(inFileName name: String) -> Int? {
        let stem = name.split(separator: ".").first.map(String.init) ?? ""
        guard stem.count == 10, stem.allSatisfy({ $0.isASCII && $0.isNumber }) else { return nil }
        return Int(stem)
    }

    /// The name a block kind has in a page's history, which the desktop app
    /// writes with the names of its own code (`BlockKind` in `src/model.rs`).
    static func historyKind(_ stored: String) -> String {
        switch stored {
        case "heading1": "Heading1"
        case "heading2": "Heading2"
        case "heading3": "Heading3"
        case "bulleted": "Bulleted"
        case "numbered": "Numbered"
        case "todo": "Todo"
        case "toggle": "Toggle"
        case "quote": "Quote"
        case "callout": "Callout"
        case "code": "Code"
        case "divider": "Divider"
        case "page": "PageLink"
        case "drawing": "Drawing"
        case "image": "Image"
        case "table": "Table"
        case "chart": "Chart"
        case "locked": "Locked"
        default: "Text"
        }
    }
}
