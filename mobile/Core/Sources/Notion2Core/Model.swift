// The data model: spaces, pages, the flat indented block list of a page, and
// calendar events. Mirrors `src/model.rs` of the desktop app; the stored
// names (block kinds, column values) must stay the same on both sides.

import Foundation

public typealias ID = String

/// The deepest a block can be nested.
public let maxIndent = 8

public enum BlockKind: String, Sendable, CaseIterable, Hashable {
    case text
    case heading1
    case heading2
    case heading3
    case bulleted
    case numbered
    case todo
    case toggle
    case quote
    case callout
    case code
    case divider
    /// A link to another page (a sub-page shows in its parent this way).
    case pageLink = "page"
    /// An Excalidraw canvas; its scene is the block's `data`.
    case drawing
    /// A picture; the block's `data` names its file (empty until one is chosen).
    case image
    /// A table; its cells, column kinds and view are in the block's `data`
    /// (see `TableData`).
    case table
    /// A chart of the rows of a table; what it draws is in the block's
    /// `data` (see `Chart`).
    case chart
    /// All the lines of a locked page, scrambled with its password (the
    /// page's only block; see `src/lock.rs`). It opens only on the Mac.
    case locked

    /// The kind a stored name means. Anything unknown is plain text, as on
    /// the desktop.
    public init(stored: String) {
        self = BlockKind(rawValue: stored) ?? .text
    }

    /// Blocks that hold text.
    public var hasText: Bool {
        switch self {
        case .divider, .pageLink, .drawing, .image, .table, .chart, .locked: false
        default: true
        }
    }
}

/// A page whose only block is the sealed one: locked with a password.
public func isLocked(_ blocks: [Block]) -> Bool {
    blocks.count == 1 && blocks[0].kind == .locked
}

public struct Block: Identifiable, Hashable, Sendable {
    public var id: ID
    public var kind: BlockKind
    /// Inline Markdown (see `Inline`), except for code blocks, which are raw.
    public var text: String
    public var checked: Bool
    /// Nesting level, 0...`maxIndent`. A block owns every following block
    /// that is nested deeper.
    public var indent: Int
    public var collapsed: Bool
    /// The icon of a callout ("name:color").
    public var icon: String?
    /// The language label of a code block.
    public var language: String?
    /// The page a page link points to.
    public var target: ID?
    /// What a picture or a table keeps beside its text: a file name, or the
    /// cells as JSON. Empty for a drawing, whose scene can be large: it is
    /// read only when it is shown (`WorkspaceDatabase.blockData`).
    public var data: String
    /// Size in bytes of the block's stored data (also for a drawing).
    public var dataSize: Int
    /// Where a picture sits and how wide it is, or that a table stands
    /// beside the one above it, as stored (JSON; "" for in line). See
    /// `Place` in `src/model.rs`.
    public var place: String
    /// The day a to-do is due ("2026-11-12"; "" for none).
    public var due: String
    /// The time of a to-do's reminder on its due day ("09:00"; "" for none).
    public var remind: String

    public init(
        id: ID = UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased(),
        kind: BlockKind,
        text: String = "",
        checked: Bool = false,
        indent: Int = 0,
        collapsed: Bool = false,
        icon: String? = nil,
        language: String? = nil,
        target: ID? = nil,
        data: String = "",
        dataSize: Int? = nil,
        place: String = "",
        due: String = "",
        remind: String = ""
    ) {
        self.id = id
        self.kind = kind
        self.text = text
        self.checked = checked
        self.indent = min(max(indent, 0), maxIndent)
        self.collapsed = collapsed
        self.icon = icon
        self.language = language
        self.target = target
        self.data = data
        self.dataSize = dataSize ?? data.utf8.count
        self.place = place
        self.due = due
        self.remind = remind
    }

    /// The table stands at the right of the table above it.
    public var standsBeside: Bool {
        guard !place.isEmpty, let json = try? JSONSerialization.jsonObject(with: Data(place.utf8)) as? [String: Any] else { return false }
        return json["beside"] as? Bool ?? false
    }

    /// The text as shown, without formatting marks.
    public var plainText: String {
        kind == .code ? text : Inline.plain(text)
    }

    /// The words of the block that search looks in. A table's words are
    /// its cells; a picture has none.
    public var searchText: String {
        switch kind {
        case .table: TableData.parse(data)?.plainText ?? ""
        case .divider, .pageLink, .drawing, .image, .chart, .locked: ""
        default: plainText
        }
    }
}

public struct Page: Identifiable, Hashable, Sendable {
    public var id: ID
    /// The space the page is in (sub-pages share their parent's).
    public var spaceID: ID
    public var parentID: ID?
    public var title: String
    /// "name" or "name:color" (or an emoji from an old workspace).
    public var icon: String?
    public var position: Double
    public var favorite: Bool
    public var trashed: Bool
    /// Milliseconds since 1970.
    public var createdAt: Int64
    public var updatedAt: Int64
    /// The band across the top of the page, as stored (see `Cover`).
    public var cover: String?
    /// Which part of a cover picture shows: 0 its top … 100 its bottom.
    public var coverY: Int
    /// The face of the title and headings: "" (sans), "serif" or "mono".
    public var font: String
    /// The row of a table this page belongs to ("<block id>/<row id>"), or
    /// nil for an ordinary page. Such a page is opened from its row and is
    /// not listed among the pages.
    public var rowOf: String?

    public init(
        id: ID,
        spaceID: ID = "",
        parentID: ID? = nil,
        title: String = "",
        icon: String? = nil,
        position: Double = 0,
        favorite: Bool = false,
        trashed: Bool = false,
        createdAt: Int64 = 0,
        updatedAt: Int64 = 0,
        cover: String? = nil,
        coverY: Int = 50,
        font: String = "",
        rowOf: String? = nil
    ) {
        self.id = id
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
        self.coverY = min(max(coverY, 0), 100)
        self.font = font
        self.rowOf = rowOf.flatMap { $0.isEmpty ? nil : $0 }
    }

    /// The page of a row of a table.
    public var isRowPage: Bool {
        rowOf != nil
    }

    public var displayTitle: String {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Untitled" : title
    }
}

/// A space: its own set of pages, favourites and events.
public struct Space: Identifiable, Hashable, Sendable {
    public var id: ID
    public var name: String
    /// One of the icon colours.
    public var color: String
    public var position: Double
    public var createdAt: Int64

    public init(id: ID, name: String, color: String = "blue", position: Double = 0, createdAt: Int64 = 0) {
        self.id = id
        self.name = name
        self.color = color
        self.position = position
        self.createdAt = createdAt
    }

    public var displayName: String {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Untitled space" : trimmed
    }

    /// First letter, for the badge.
    public var initial: String {
        displayName.first.map { String($0).uppercased() } ?? ""
    }
}

public struct Event: Identifiable, Hashable, Sendable {
    /// A repeat of a repeating event has the id "<event id>@<YYYY-MM-DD>".
    public var id: ID
    public var spaceID: ID
    public var title: String
    /// Start and end in milliseconds since 1970. An all-day event runs from
    /// local midnight of its first day to local midnight after its last.
    public var start: Int64
    public var end: Int64
    public var allDay: Bool
    /// One of the icon colours.
    public var color: String
    public var notes: String
    /// A page that holds longer notes for the event.
    public var pageID: ID?
    /// Minutes before the start to send a reminder (nil: none).
    public var remind: Int?
    /// How the event repeats (nil: once). `start` and `end` are its first time.
    public var repeatRule: Repeat?
    /// The address to join the event's call (Teams, Zoom, Meet), or "".
    public var join: String
    public var createdAt: Int64
    public var updatedAt: Int64

    public init(
        id: ID,
        spaceID: ID = "",
        title: String = "",
        start: Int64,
        end: Int64,
        allDay: Bool = false,
        color: String = "blue",
        notes: String = "",
        pageID: ID? = nil,
        remind: Int? = nil,
        repeatRule: Repeat? = nil,
        join: String = "",
        createdAt: Int64 = 0,
        updatedAt: Int64 = 0
    ) {
        self.id = id
        self.spaceID = spaceID
        self.title = title
        self.start = start
        self.end = max(end, start + 1)
        self.allDay = allDay
        self.color = color
        self.notes = notes
        self.pageID = pageID
        self.remind = remind
        self.repeatRule = repeatRule
        self.join = join
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public var displayTitle: String {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "New event" : title
    }

    /// Whether the event overlaps [from, to).
    public func overlaps(_ from: Int64, _ to: Int64) -> Bool {
        start < to && end > from
    }

    /// The stored event a repeat belongs to (its own id for a one-off event).
    public var seriesID: ID {
        Repeat.splitID(id).series
    }
}

/// Everything in a workspace file, in memory.
public struct Workspace: Sendable {
    public var spaces: [Space]
    public var pages: [ID: Page]
    public var blocks: [ID: [Block]]
    public var events: [Event]
    public var settings: [String: String]

    public init(
        spaces: [Space] = [],
        pages: [ID: Page] = [:],
        blocks: [ID: [Block]] = [:],
        events: [Event] = [],
        settings: [String: String] = [:]
    ) {
        self.spaces = spaces
        self.pages = pages
        self.blocks = blocks
        self.events = events
        self.settings = settings
    }

    public static let empty = Workspace()
}
