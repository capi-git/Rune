// Reads a workspace file (the SQLite file the desktop app writes; see
// `src/store.rs`). Read-only: the file is opened as immutable, so nothing is
// ever written to it or beside it.
//
// The reader accepts files from older versions of the desktop app: a column
// or table that a version did not have yet is read as its default.

import Foundation
import SQLite3

public enum WorkspaceDatabaseError: Error, LocalizedError, Equatable {
    case cannotOpen(String)
    case notAWorkspace
    case query(String)

    public var errorDescription: String? {
        switch self {
        case .cannotOpen: "The file could not be opened."
        case .notAWorkspace: "This file is not a Rune notes file."
        case .query: "The workspace file could not be read."
        }
    }
}

public enum WorkspaceDatabase {
    /// Read everything in the workspace file at `url`.
    public static func load(from url: URL) throws -> Workspace {
        // The app's own database is written by sync. If the app stopped in
        // the middle of a write, SQLite finishes or undoes it when the file
        // is next opened for writing; a read-only open would not.
        if FileManager.default.fileExists(atPath: url.path + "-journal") {
            _ = try? Connection(url: url, mode: .readWrite).tableNames()
        }
        let db = try Connection(url: url)
        defer { db.close() }

        let tables = try db.tableNames()
        guard tables.contains("pages"), tables.contains("blocks") else {
            throw WorkspaceDatabaseError.notAWorkspace
        }

        var settings: [String: String] = [:]
        if tables.contains("settings") {
            try db.each("SELECT key, value FROM settings") { row in
                settings[row.text(0)] = row.text(1)
            }
        }

        // Spaces. A file from before spaces has none: it is one space.
        var spaces: [Space] = []
        if tables.contains("spaces") {
            try db.each("SELECT id, name, color, position, created_at FROM spaces ORDER BY position, created_at") { row in
                spaces.append(Space(id: row.text(0), name: row.text(1), color: row.text(2), position: row.double(3), createdAt: row.int64(4)))
            }
        }
        if spaces.isEmpty {
            spaces = [Space(id: "space", name: settings["workspace_name"] ?? "My Space", color: "blue", position: 1)]
        }
        let firstSpace = spaces[0].id

        // Pages. One written without a space is in the first space.
        var pages: [ID: Page] = [:]
        let pageColumns = try db.columnNames(of: "pages")
        let pageSpace = pageColumns.contains("space_id") ? "space_id" : "''"
        let pageCover = pageColumns.contains("cover") ? "cover, cover_y" : "NULL, 50"
        let pageFont = pageColumns.contains("font") ? "font" : "''"
        let pageRow = pageColumns.contains("row_of") ? "row_of" : "NULL"
        try db.each(
            "SELECT id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, \(pageSpace), \(pageCover), \(pageFont), \(pageRow) FROM pages"
        ) { row in
            let space = row.text(9)
            let page = Page(
                id: row.text(0),
                spaceID: space.isEmpty ? firstSpace : space,
                parentID: row.optionalText(1),
                title: row.text(2),
                icon: row.optionalText(3),
                position: row.double(4),
                favorite: row.int64(5) != 0,
                trashed: row.int64(6) != 0,
                createdAt: row.int64(7),
                updatedAt: row.int64(8),
                cover: row.optionalText(10),
                coverY: Int(row.int64(11)),
                font: row.text(12),
                rowOf: row.optionalText(13)
            )
            pages[page.id] = page
        }

        // Blocks. Text from before inline formatting is plain: escape it, as
        // the desktop app does the first time it opens such a file.
        let plainText = settings["text_format"] == nil
        var blocks: [ID: [Block]] = [:]
        let blockColumns = try db.columnNames(of: "blocks")
        // A drawing's scene can be large: only its size is read here.
        let data = blockColumns.contains("data")
            ? "length(CAST(data AS BLOB)), CASE WHEN kind = 'drawing' THEN '' ELSE data END"
            : "0, ''"
        let place = blockColumns.contains("place") ? "place" : "''"
        let due = blockColumns.contains("due") ? "due" : "''"
        let remindAt = blockColumns.contains("remind") ? "remind" : "''"
        try db.each(
            "SELECT page_id, id, kind, text, checked, indent, collapsed, icon, language, target, \(data), \(place), \(due), \(remindAt) FROM blocks ORDER BY page_id, position"
        ) { row in
            let kind = BlockKind(stored: row.text(2))
            var text = row.text(3)
            if plainText && kind != .code && kind != .table {
                text = Inline.escape(text)
            }
            let block = Block(
                id: row.text(1),
                kind: kind,
                text: text,
                checked: row.int64(4) != 0,
                indent: Int(min(max(row.int64(5), 0), Int64(maxIndent))),
                collapsed: row.int64(6) != 0,
                icon: row.optionalText(7),
                language: row.optionalText(8),
                target: row.optionalText(9),
                data: row.text(11),
                dataSize: Int(row.int64(10)),
                place: row.text(12),
                due: row.text(13),
                remind: row.text(14)
            )
            blocks[row.text(0), default: []].append(block)
        }

        var events: [Event] = []
        if tables.contains("events") {
            let eventColumns = try db.columnNames(of: "events")
            let remind = eventColumns.contains("remind_min") ? "remind_min" : "NULL"
            let rule = eventColumns.contains("repeat") ? "repeat" : "''"
            let join = eventColumns.contains("join_url") ? "join_url" : "''"
            try db.each(
                "SELECT id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, \(remind), \(rule), \(join) FROM events ORDER BY start_ms"
            ) { row in
                let space = row.text(1)
                events.append(Event(
                    id: row.text(0),
                    spaceID: space.isEmpty ? firstSpace : space,
                    title: row.text(2),
                    start: row.int64(3),
                    end: row.int64(4),
                    allDay: row.int64(5) != 0,
                    color: row.text(6),
                    notes: row.text(7),
                    pageID: row.optionalText(8),
                    remind: row.isNull(11) ? nil : Int(row.int64(11)),
                    repeatRule: Repeat.fromJSON(row.text(12)),
                    join: row.text(13),
                    createdAt: row.int64(9),
                    updatedAt: row.int64(10)
                ))
            }
        }

        return Workspace(spaces: spaces, pages: pages, blocks: blocks, events: events, settings: settings)
    }

    /// The scene of a drawing block (Excalidraw JSON). Read when it is shown,
    /// because a scene can be large.
    public static func blockData(in url: URL, blockID: ID) throws -> String {
        let db = try Connection(url: url)
        defer { db.close() }
        guard try db.columnNames(of: "blocks").contains("data") else { return "" }
        var data = ""
        try db.each("SELECT data FROM blocks WHERE id = ?1", bind: [blockID]) { row in
            data = row.text(0)
        }
        return data
    }
}

// MARK: - A small SQLite wrapper

private let sqliteTransient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

/// A value bound to a statement.
enum SQLValue {
    case text(String)
    case int(Int64)
    case double(Double)
    case null

    static func maybeText(_ value: String?) -> SQLValue {
        value.map { .text($0) } ?? .null
    }

    static func maybeInt(_ value: Int?) -> SQLValue {
        value.map { .int(Int64($0)) } ?? .null
    }

    static func number(_ value: Int) -> SQLValue {
        .int(Int64(value))
    }

    static func bool(_ value: Bool) -> SQLValue {
        .int(value ? 1 : 0)
    }
}

struct Row {
    let statement: OpaquePointer

    func isNull(_ column: Int32) -> Bool {
        sqlite3_column_type(statement, column) == SQLITE_NULL
    }

    func text(_ column: Int32) -> String {
        guard let c = sqlite3_column_text(statement, column) else { return "" }
        return String(cString: c)
    }

    func optionalText(_ column: Int32) -> String? {
        isNull(column) ? nil : text(column)
    }

    func int64(_ column: Int32) -> Int64 {
        sqlite3_column_int64(statement, column)
    }

    func double(_ column: Int32) -> Double {
        sqlite3_column_double(statement, column)
    }
}

final class Connection {
    enum Mode {
        /// Read-only and immutable: SQLite takes no locks and makes no
        /// journal or shared-memory files. For reading the notes.
        case immutable
        /// Read and write, one file (rollback journal). Only sync uses it,
        /// on the app's own database.
        case readWrite
    }

    private var handle: OpaquePointer?

    init(url: URL, mode: Mode = .immutable) throws {
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw WorkspaceDatabaseError.cannotOpen("no file")
        }
        let code: Int32
        switch mode {
        case .immutable:
            // In a `file:` name SQLite reads "%", "?" and "#" specially.
            var allowed = CharacterSet.urlPathAllowed
            allowed.remove(charactersIn: "%?#")
            let path = url.path.addingPercentEncoding(withAllowedCharacters: allowed) ?? url.path
            let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_URI | SQLITE_OPEN_NOMUTEX
            code = sqlite3_open_v2("file:\(path)?immutable=1", &handle, flags, nil)
        case .readWrite:
            code = sqlite3_open_v2(url.path, &handle, SQLITE_OPEN_READWRITE | SQLITE_OPEN_NOMUTEX, nil)
        }
        guard code == SQLITE_OK else {
            let message = handle.map { String(cString: sqlite3_errmsg($0)) } ?? "code \(code)"
            close()
            throw WorkspaceDatabaseError.cannotOpen(message)
        }
    }

    func close() {
        if let handle {
            sqlite3_close_v2(handle)
        }
        handle = nil
    }

    deinit {
        close()
    }

    private func failure(_ code: Int32) -> WorkspaceDatabaseError {
        if code == SQLITE_NOTADB || code == SQLITE_CORRUPT {
            return .notAWorkspace
        }
        return .query(handle.map { String(cString: sqlite3_errmsg($0)) } ?? "code \(code)")
    }

    func each(_ sql: String, bind: [String], _ body: (Row) -> Void) throws {
        try each(sql, values: bind.map { .text($0) }, body)
    }

    func each(_ sql: String, values: [SQLValue] = [], _ body: (Row) -> Void) throws {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(handle, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
            // A file that is not a database fails at the first query.
            throw failure(sqlite3_errcode(handle))
        }
        defer { sqlite3_finalize(statement) }
        for (i, value) in values.enumerated() {
            let index = Int32(i + 1)
            switch value {
            case let .text(text): sqlite3_bind_text(statement, index, text, -1, sqliteTransient)
            case let .int(number): sqlite3_bind_int64(statement, index, number)
            case let .double(number): sqlite3_bind_double(statement, index, number)
            case .null: sqlite3_bind_null(statement, index)
            }
        }
        while true {
            let code = sqlite3_step(statement)
            if code == SQLITE_ROW {
                body(Row(statement: statement))
            } else if code == SQLITE_DONE {
                return
            } else {
                throw failure(code)
            }
        }
    }

    /// Run a statement that gives no rows.
    func run(_ sql: String, _ values: [SQLValue] = []) throws {
        try each(sql, values: values) { _ in }
    }

    /// Run several statements with no values (table definitions).
    func script(_ sql: String) throws {
        let code = sqlite3_exec(handle, sql, nil, nil, nil)
        guard code == SQLITE_OK else { throw failure(code) }
    }

    /// Run `body` in one transaction: all of it is kept, or none of it.
    func transaction<T>(_ body: () throws -> T) throws -> T {
        try script("BEGIN IMMEDIATE")
        do {
            let result = try body()
            try script("COMMIT")
            return result
        } catch {
            try? script("ROLLBACK")
            throw error
        }
    }

    func tableNames() throws -> Set<String> {
        var names: Set<String> = []
        try each("SELECT name FROM sqlite_master WHERE type = 'table'") { names.insert($0.text(0)) }
        return names
    }

    func columnNames(of table: String) throws -> Set<String> {
        var names: Set<String> = []
        try each("SELECT name FROM pragma_table_info(?1)", bind: [table]) { names.insert($0.text(0)) }
        return names
    }
}
