// Makes small workspace files for the tests, with the schema the desktop app
// creates (`migrate` in `src/store.rs`).

import Foundation
import SQLite3
@testable import RuneCore

enum Fixture {
    /// The tables as the current desktop app makes them.
    static let schema = """
        CREATE TABLE pages(
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            title TEXT NOT NULL DEFAULT '',
            icon TEXT,
            position REAL NOT NULL DEFAULT 0,
            favorite INTEGER NOT NULL DEFAULT 0,
            trashed INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            space_id TEXT NOT NULL DEFAULT '',
            cover TEXT,
            cover_y INTEGER NOT NULL DEFAULT 50,
            font TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE blocks(
            id TEXT PRIMARY KEY,
            page_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            kind TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            checked INTEGER NOT NULL DEFAULT 0,
            indent INTEGER NOT NULL DEFAULT 0,
            collapsed INTEGER NOT NULL DEFAULT 0,
            icon TEXT,
            language TEXT,
            target TEXT,
            data TEXT NOT NULL DEFAULT '',
            place TEXT NOT NULL DEFAULT '',
            due TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX blocks_by_page ON blocks(page_id, position);
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE events(
            id TEXT PRIMARY KEY,
            space_id TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            start_ms INTEGER NOT NULL,
            end_ms INTEGER NOT NULL,
            all_day INTEGER NOT NULL DEFAULT 0,
            color TEXT NOT NULL DEFAULT 'blue',
            notes TEXT NOT NULL DEFAULT '',
            page_id TEXT,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            remind_min INTEGER,
            repeat TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX events_by_time ON events(start_ms);
        CREATE TABLE page_versions(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            page_id TEXT NOT NULL,
            at INTEGER NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            blocks TEXT NOT NULL
        );
        CREATE TABLE spaces(
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            color TEXT NOT NULL DEFAULT 'blue',
            position REAL NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO settings(key, value) VALUES ('text_format', 'inline-md-1');
        """

    /// The tables as the desktop app made them before pages had covers and
    /// a font, and before a block had a place and a due day.
    static let schemaBeforeCovers = schema
        .replacingOccurrences(
            of: "space_id TEXT NOT NULL DEFAULT '',\n            cover TEXT,\n            cover_y INTEGER NOT NULL DEFAULT 50,\n            font TEXT NOT NULL DEFAULT ''",
            with: "space_id TEXT NOT NULL DEFAULT ''"
        )
        .replacingOccurrences(
            of: "data TEXT NOT NULL DEFAULT '',\n            place TEXT NOT NULL DEFAULT '',\n            due TEXT NOT NULL DEFAULT ''",
            with: "data TEXT NOT NULL DEFAULT ''"
        )

    /// The tables as Rune 0.1.3 and later make them (form 6 of the file):
    /// the page of a table's row, a to-do's reminder, an event's call link.
    static let schemaForm6 = schema
        .replacingOccurrences(of: "font TEXT NOT NULL DEFAULT ''", with: "font TEXT NOT NULL DEFAULT '',\n            row_of TEXT")
        .replacingOccurrences(of: "due TEXT NOT NULL DEFAULT ''", with: "due TEXT NOT NULL DEFAULT '',\n            remind TEXT NOT NULL DEFAULT ''")
        .replacingOccurrences(of: "repeat TEXT NOT NULL DEFAULT ''", with: "repeat TEXT NOT NULL DEFAULT '',\n            join_url TEXT NOT NULL DEFAULT ''")
        + "PRAGMA user_version = 6;"

    /// The tables as the first versions of the desktop app made them: no
    /// spaces, no reminders, no repeats, no drawings, plain text.
    static let oldSchema = """
        CREATE TABLE pages(
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            title TEXT NOT NULL DEFAULT '',
            icon TEXT,
            position REAL NOT NULL DEFAULT 0,
            favorite INTEGER NOT NULL DEFAULT 0,
            trashed INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE blocks(
            id TEXT PRIMARY KEY,
            page_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            kind TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            checked INTEGER NOT NULL DEFAULT 0,
            indent INTEGER NOT NULL DEFAULT 0,
            collapsed INTEGER NOT NULL DEFAULT 0,
            icon TEXT,
            language TEXT,
            target TEXT
        );
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        """

    /// A new workspace file in a folder of its own. `sql` runs after the schema.
    static func make(schema: String = Fixture.schema, _ sql: String) throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("rune-core-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        // A name with a space and a "#" in it, as a path on a Mac may have.
        let url = folder.appendingPathComponent("work space #1.db")
        var db: OpaquePointer?
        guard sqlite3_open(url.path, &db) == SQLITE_OK else { throw WorkspaceDatabaseError.cannotOpen("fixture") }
        defer { sqlite3_close(db) }
        var message: UnsafeMutablePointer<CChar>?
        guard sqlite3_exec(db, schema + sql, nil, nil, &message) == SQLITE_OK else {
            let text = message.map { String(cString: $0) } ?? "?"
            sqlite3_free(message)
            throw WorkspaceDatabaseError.query(text)
        }
        return url
    }

    static func remove(_ url: URL) {
        try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
    }

    /// What is in the folder of a workspace file.
    static func files(beside url: URL) -> [String] {
        ((try? FileManager.default.contentsOfDirectory(atPath: url.deletingLastPathComponent().path)) ?? []).sorted()
    }
}
