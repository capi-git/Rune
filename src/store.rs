//! Local persistence in a single SQLite file.

use crate::model::{Block, BlockKind, Event, Id, Page, Space};
use rusqlite::{params, Connection, OptionalExtension};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Store {
    conn: Connection,
    pub path: Option<PathBuf>,
}

pub type Loaded = (HashMap<Id, Page>, HashMap<Id, Vec<Block>>);

/// How many earlier versions of each page are kept (see Page history).
pub const VERSIONS_KEPT: usize = 60;

/// A page as it was at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub id: i64,
    pub at: i64,
    pub title: String,
    pub blocks: Vec<Block>,
}

/// How long a read or a write waits for another program that holds the file.
const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// What the store says when plain lines would take the place of a locked
/// page that is not open here.
pub const REFUSED_LOCKED: &str = "The page is locked: it can be changed only in Rune, with its password.";

/// The form of the notes file that this program writes. Make it one larger
/// with each change that an older program would damage: a column it would
/// write back empty, a kind of line it does not know.
/// 7: a page can be locked (a line of the kind "locked", see lock.rs).
pub const SCHEMA: i64 = 7;

/// The folder of the notes, in the person's Application Support folder.
pub const FOLDER: &str = "Rune";
/// The name that folder had before the app was Rune. The app gives the
/// folder its new name as it starts (`move_folder`) and leaves a link of
/// this name in its place, for the programs that still look there: an older
/// Rune, a connector that was started before the move, a script.
pub const OLD_FOLDER: &str = "Notion 2.0";
/// The notes file in that folder. (Its name is part of what two devices and
/// older programs know: it stays.)
const FILE: &str = "workspace.db";

/// The folder of the notes in `base`, for the app and for the connector
/// alike (`Store::default_path` is the only caller): `Rune` when it holds a
/// notes file; else the folder of the first name when that one holds a
/// notes file (the app did not move it yet, or could not); else `Rune`.
pub fn folder_in(base: &Path) -> PathBuf {
    let (new, old) = (base.join(FOLDER), base.join(OLD_FOLDER));
    if !new.join(FILE).is_file() && old.join(FILE).is_file() {
        old
    } else {
        new
    }
}

/// What `move_folder_in` did.
#[derive(Debug, PartialEq)]
pub enum FolderMove {
    /// There was nothing to move.
    Nothing,
    /// The folder has its new name, and a link of the first name is in its place.
    Moved,
    /// The folder of the first name is still there and nothing was changed:
    /// what to tell the person.
    Stayed(String),
}

/// Give the notes folder of the first name in `base` the name `Rune`: the
/// whole folder in one step (a rename; never a copy, never a merge of two
/// folders, never over a file), and then a link of the first name to it.
/// Only when `Rune` is not there or is an empty folder, and the folder of
/// the first name is a real folder (a link says that the move was made).
/// When the rename fails, or `Rune` is in the way, nothing is changed and
/// `folder_in` goes on giving the folder that holds the notes.
///
/// What else can have the notes open in this moment, and why this is safe:
/// - A connector (`--mcp`) or an older Rune that opened the notes file
///   before the move keeps its open files: a rename of the folder does not
///   change them, and the log files beside the notes file (`-wal`, `-shm`)
///   move with the folder, so every program still writes to the same files.
///   What such a program then opens by the path it knows (a backup, a
///   picture, the watcher's record, the notes file again) goes through the
///   link to the same folder. SQLite's locks belong to the file, not to its
///   path, so a program that opened it by the first name and one that
///   opened it by the new name still wait for each other.
/// - Between the rename and the link there is a moment with no folder of
///   the first name. A program that looks for a file there in that moment
///   is told that there is none; one that STARTS in that moment makes an
///   empty folder of the first name with an empty notes file. Then the link
///   cannot be made, the notes are safe in `Rune`, and the next start says
///   that there are notes in two folders.
/// - Two copies of the app that start together: one rename takes the
///   folder, the other fails (the folder is gone, or is a link by then) and
///   that copy sees that the move was made.
pub fn move_folder_in(base: &Path) -> FolderMove {
    let (new, old) = (base.join(FOLDER), base.join(OLD_FOLDER));
    let real_folder = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir());
    if !real_folder(&old) {
        return FolderMove::Nothing;
    }
    let old_has_notes = old.join(FILE).is_file();
    let free = match std::fs::symlink_metadata(&new) {
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        Ok(m) => m.file_type().is_dir() && std::fs::read_dir(&new).is_ok_and(|mut inside| inside.next().is_none()),
    };
    let stayed = |why: &str| format!("Your notes are still in the folder “{OLD_FOLDER}”: {why}. Nothing was changed, and Rune uses the notes where they are.");
    if !free {
        return if old_has_notes && new.join(FILE).is_file() {
            FolderMove::Stayed(format!("There are notes in two folders, “{FOLDER}” and “{OLD_FOLDER}” (in Library ▸ Application Support). Rune uses the folder “{FOLDER}” and changed nothing in the two."))
        } else if old_has_notes {
            FolderMove::Stayed(stayed(&format!("a folder “{FOLDER}” with other files is in the place they would move to")))
        } else {
            FolderMove::Nothing
        };
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => {
            // (A relative link: it holds when the home folder is moved or restored.)
            if let Err(e) = std::os::unix::fs::symlink(FOLDER, &old) {
                eprintln!("The notes folder is now {}, but the link {} could not be made ({e}).", new.display(), old.display());
            }
            FolderMove::Moved
        }
        // Another copy of the app made the move in this moment.
        Err(_) if !real_folder(&old) => FolderMove::Nothing,
        Err(e) if old_has_notes => FolderMove::Stayed(stayed(&format!("the folder could not get the name “{FOLDER}” ({e})"))),
        Err(_) => FolderMove::Nothing,
    }
}

/// As the app starts, before it opens the notes file: the notes folder gets
/// the app's name (see `move_folder_in`). Only the app calls this, one time.
/// The connector, the commands of `main` and a run on a notes file of its
/// own (`RUNE_DB`) never move anything: they use the folder `folder_in` gives.
pub fn move_folder() {
    if std::env::var_os("RUNE_DB").is_some_and(|p| !p.is_empty()) {
        return;
    }
    let Some(base) = dirs::data_dir() else { return };
    match move_folder_in(&base) {
        FolderMove::Nothing => {}
        FolderMove::Moved => eprintln!("The notes folder is now {} (a link named “{OLD_FOLDER}” is in its old place).", base.join(FOLDER).display()),
        FolderMove::Stayed(why) => {
            eprintln!("{why}");
            tell(&why);
        }
    }
}

impl Store {
    /// `~/Library/Application Support/Rune/workspace.db` (or the folder of
    /// the first name while the notes are still there: see `folder_in`), or
    /// the file named by `RUNE_DB` (handy for tests and for trying the MCP
    /// server safely).
    pub fn default_path() -> PathBuf {
        if let Some(path) = std::env::var_os("RUNE_DB").filter(|p| !p.is_empty()) {
            return PathBuf::from(path);
        }
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        folder_in(&base).join(FILE)
    }

    pub fn open(path: PathBuf) -> rusqlite::Result<Store> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(&path)?;
        // The app and the MCP server (`rune --mcp`) can both have the file
        // open; wait briefly for the other's write instead of failing.
        conn.busy_timeout(BUSY_WAIT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // What is deleted is overwritten too: the plain lines of a page that
        // was just locked do not stay in the free parts of the file.
        conn.pragma_update(None, "secure_delete", "ON")?;
        let store = Store { conn, path: Some(path) };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> rusqlite::Result<Store> {
        let store = Store { conn: Connection::open_in_memory()?, path: None };
        store.migrate()?;
        Ok(store)
    }

    // ----- page history ------------------------------------------------------

    /// Keep a copy of a page as it was (title and blocks). Only the newest
    /// `VERSIONS_KEPT` copies of each page are kept.
    pub fn save_version(&self, page_id: &str, at: i64, title: &str, blocks: &[Block]) -> rusqlite::Result<()> {
        // (A locked page keeps no history: it would hold its lines in plain.)
        if crate::lock::is_open(page_id) || blocks.iter().any(|b| b.kind == BlockKind::Locked) || crate::lock::sealed(blocks).is_some() {
            return Ok(());
        }
        let json = serde_json::to_string(blocks).unwrap_or_else(|_| "[]".into());
        if self.latest_version(page_id)?.is_some_and(|(_, t, b)| t == title && b == json) {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.prepare_cached("INSERT INTO page_versions(page_id, at, title, blocks) VALUES (?1, ?2, ?3, ?4)")?
            .execute(params![page_id, at, title, json])?;
        tx.prepare_cached(
            "DELETE FROM page_versions WHERE page_id=?1 AND id NOT IN
             (SELECT id FROM page_versions WHERE page_id=?1 ORDER BY at DESC, id DESC LIMIT ?2)",
        )?
        .execute(params![page_id, VERSIONS_KEPT as i64])?;
        tx.commit()
    }

    /// Lock a page: its lines written sealed (its key is held, see
    /// `write_blocks`) and every kept copy of it removed, in one
    /// transaction. Both or none: as two steps, a history that could not be
    /// removed left the page sealed on disk while the window said that
    /// nothing was changed, and what was typed then was not kept.
    pub fn lock_page(&self, page_id: &str, blocks: &[Block], at: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::write_blocks(&tx, page_id, blocks)?;
        tx.prepare_cached("UPDATE pages SET updated_at=?2 WHERE id=?1")?.execute(params![page_id, at])?;
        tx.execute("DELETE FROM page_versions WHERE page_id=?1", params![page_id])?;
        tx.commit()
    }

    /// Write what the log of changes holds into the file itself and empty
    /// the log (after a page was locked: the log held its plain lines).
    /// False when another program reads the file at this moment (a sync
    /// run, a backup, an AI tool), so that the log could not be emptied:
    /// the caller tries again later. (It does not wait here.)
    pub fn checkpoint(&self) -> bool {
        let _ = self.conn.busy_timeout(std::time::Duration::ZERO);
        let done = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0)).is_ok_and(|busy| busy == 0);
        let _ = self.conn.busy_timeout(BUSY_WAIT);
        done
    }

    /// The newest kept copy of a page: (time, title, blocks as saved).
    pub fn latest_version(&self, page_id: &str) -> rusqlite::Result<Option<(i64, String, String)>> {
        self.conn
            .prepare_cached("SELECT at, title, blocks FROM page_versions WHERE page_id=?1 ORDER BY at DESC, id DESC LIMIT 1")?
            .query_row(params![page_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()
    }

    /// Every kept copy of a page, newest first.
    pub fn load_versions(&self, page_id: &str) -> rusqlite::Result<Vec<Version>> {
        let mut stmt =
            self.conn.prepare("SELECT id, at, title, blocks FROM page_versions WHERE page_id=?1 ORDER BY at DESC, id DESC")?;
        let rows = stmt.query_map(params![page_id], |r| {
            let json: String = r.get(3)?;
            Ok(Version { id: r.get(0)?, at: r.get(1)?, title: r.get(2)?, blocks: serde_json::from_str(&json).unwrap_or_default() })
        })?;
        rows.collect()
    }

    /// Changes whenever another connection (for example the MCP server) commits
    /// to the file, so the app can notice edits made outside it and reload.
    pub fn data_version(&self) -> i64 {
        self.conn.query_row("PRAGMA data_version", [], |r| r.get(0)).unwrap_or(0)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pages(
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
            CREATE TABLE IF NOT EXISTS blocks(
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
                place TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS blocks_by_page ON blocks(page_id, position);
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(
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
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS events_by_time ON events(start_ms);
            CREATE TABLE IF NOT EXISTS page_versions(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                page_id TEXT NOT NULL,
                at INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                blocks TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS versions_by_page ON page_versions(page_id, at);
            CREATE TABLE IF NOT EXISTS spaces(
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                color TEXT NOT NULL DEFAULT 'blue',
                position REAL NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        let has_space_column: bool = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('pages') WHERE name='space_id'")?
            .exists([])?;
        if !has_space_column {
            self.conn.execute_batch("ALTER TABLE pages ADD COLUMN space_id TEXT NOT NULL DEFAULT ''")?;
        }
        let has_remind_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('events') WHERE name='remind_min'")?.exists([])?;
        if !has_remind_column {
            self.conn.execute_batch("ALTER TABLE events ADD COLUMN remind_min INTEGER")?;
        }
        let has_data_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('blocks') WHERE name='data'")?.exists([])?;
        if !has_data_column {
            self.conn.execute_batch("ALTER TABLE blocks ADD COLUMN data TEXT NOT NULL DEFAULT ''")?;
        }
        let has_repeat_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('events') WHERE name='repeat'")?.exists([])?;
        if !has_repeat_column {
            self.conn.execute_batch("ALTER TABLE events ADD COLUMN repeat TEXT NOT NULL DEFAULT ''")?;
        }
        // The link to join the call of an event (form 5 of the file).
        let has_join_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('events') WHERE name='join_url'")?.exists([])?;
        if !has_join_column {
            self.conn.execute_batch("ALTER TABLE events ADD COLUMN join_url TEXT NOT NULL DEFAULT ''")?;
        }
        let has_cover_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('pages') WHERE name='cover'")?.exists([])?;
        if !has_cover_column {
            self.conn.execute_batch("ALTER TABLE pages ADD COLUMN cover TEXT; ALTER TABLE pages ADD COLUMN cover_y INTEGER NOT NULL DEFAULT 50;")?;
        }
        let has_place_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('blocks') WHERE name='place'")?.exists([])?;
        if !has_place_column {
            self.conn.execute_batch("ALTER TABLE blocks ADD COLUMN place TEXT NOT NULL DEFAULT ''")?;
        }
        let has_font_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('pages') WHERE name='font'")?.exists([])?;
        if !has_font_column {
            self.conn.execute_batch("ALTER TABLE pages ADD COLUMN font TEXT NOT NULL DEFAULT ''")?;
        }
        let has_row_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('pages') WHERE name='row_of'")?.exists([])?;
        if !has_row_column {
            self.conn.execute_batch("ALTER TABLE pages ADD COLUMN row_of TEXT")?;
        }
        let has_due_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('blocks') WHERE name='due'")?.exists([])?;
        if !has_due_column {
            self.conn.execute_batch("ALTER TABLE blocks ADD COLUMN due TEXT NOT NULL DEFAULT ''")?;
        }
        // The time of a to-do's reminder (an older program leaves it as it is).
        let has_remind_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('blocks') WHERE name='remind'")?.exists([])?;
        if !has_remind_column {
            self.conn.execute_batch("ALTER TABLE blocks ADD COLUMN remind TEXT NOT NULL DEFAULT ''")?;
        }
        // The colour of a line (an older program leaves it as it is).
        let has_tint_column: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('blocks') WHERE name='tint'")?.exists([])?;
        if !has_tint_column {
            self.conn.execute_batch("ALTER TABLE blocks ADD COLUMN tint TEXT NOT NULL DEFAULT ''")?;
        }
        self.upgrade_text_format()?;
        self.ensure_space()?;
        // Mark the file with its form. The mark is never made smaller: a file
        // from a newer program keeps the mark that program gave it.
        if self.file_schema()? < SCHEMA {
            self.conn.pragma_update(None, "user_version", SCHEMA)?;
        }
        // (A file where sync was on: its triggers of before failed saves.)
        crate::devsync::engine::mend_triggers(&self.conn)?;
        Ok(())
    }

    fn file_schema(&self) -> rusqlite::Result<i64> {
        self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))
    }

    /// A newer program wrote this file (the app took an update while an old
    /// copy of the connector was still open). This program does not know
    /// everything in it: what it wrote back would lose the new parts, so it
    /// must only read. Asked again before each change, not once at the start.
    pub fn from_newer_program(&self) -> bool {
        self.file_schema().is_ok_and(|v| v > SCHEMA)
    }

    #[cfg(test)]
    pub fn run_for_test(&self, sql: &str) {
        self.conn.execute_batch(sql).unwrap();
    }

    #[cfg(test)]
    pub fn set_file_schema(&self, v: i64) {
        self.conn.pragma_update(None, "user_version", v).unwrap();
    }

    /// Every workspace has at least one space, and every page belongs to one.
    /// Pages written without a space (older versions, an agent) join the
    /// first space. So do the pages and events of a space that is gone (a
    /// second window made them just after the space was deleted): a page in
    /// no space shows in no sidebar and no search.
    fn ensure_space(&self) -> rusqlite::Result<()> {
        let first: Option<String> = self
            .conn
            .query_row("SELECT id FROM spaces ORDER BY position, created_at LIMIT 1", [], |r| r.get(0))
            .optional()?;
        let first = match first {
            Some(id) => id,
            None => {
                let name = self.get_setting("workspace_name").unwrap_or_else(|| "My Space".into());
                let space = Space::new(name, "blue", 1.0);
                self.save_space(&space)?;
                space.id
            }
        };
        self.conn.execute("UPDATE pages SET space_id=?1 WHERE space_id NOT IN (SELECT id FROM spaces)", params![first])?;
        self.conn.execute("UPDATE events SET space_id=?1 WHERE space_id NOT IN (SELECT id FROM spaces)", params![first])?;
        Ok(())
    }

    /// The ids of the spaces, the first space first.
    fn space_ids(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM spaces ORDER BY position, created_at")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    pub fn load_events(&self) -> rusqlite::Result<Vec<Event>> {
        let spaces = self.space_ids()?;
        let first = spaces.first().cloned().unwrap_or_default();
        let mut stmt = self.conn.prepare(
            "SELECT id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat, join_url
             FROM events ORDER BY start_ms",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Event {
                id: r.get(0)?,
                space_id: r.get(1)?,
                title: r.get(2)?,
                start: r.get(3)?,
                end: r.get(4)?,
                all_day: r.get::<_, i64>(5)? != 0,
                color: r.get(6)?,
                notes: r.get(7)?,
                join: r.get(13)?,
                page_id: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
                remind: r.get(11)?,
                repeat: crate::repeat::Repeat::from_json(&r.get::<_, String>(12)?),
            })
        })?;
        rows.map(|e| {
            e.map(|mut e| {
                // (No space, or a space that is gone: see `ensure_space`.)
                if !spaces.contains(&e.space_id) {
                    e.space_id = first.clone();
                }
                e
            })
        })
        .collect()
    }

    pub fn save_event(&self, e: &Event) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO events(id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat, join_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET space_id=?2, title=?3, start_ms=?4, end_ms=?5, all_day=?6,
                color=?7, notes=?8, page_id=?9, updated_at=?11, remind_min=?12, repeat=?13, join_url=?14",
            params![
                e.id,
                e.space_id,
                e.title,
                e.start,
                e.end,
                e.all_day as i64,
                e.color,
                e.notes,
                e.page_id,
                e.created_at,
                e.updated_at,
                e.remind,
                crate::repeat::Repeat::to_json(e.repeat.as_ref()),
                e.join
            ],
        )?;
        Ok(())
    }

    pub fn delete_event(&self, id: &str) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM events WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn load_spaces(&self) -> rusqlite::Result<Vec<Space>> {
        let mut stmt = self.conn.prepare("SELECT id, name, color, position, created_at FROM spaces ORDER BY position, created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(Space { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, position: r.get(3)?, created_at: r.get(4)? })
        })?;
        rows.collect()
    }

    pub fn save_space(&self, s: &Space) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO spaces(id, name, color, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name=?2, color=?3, position=?4",
            params![s.id, s.name, s.color, s.position, s.created_at],
        )?;
        Ok(())
    }

    /// Remove a space; its remaining (trashed) pages move to `into`.
    pub fn delete_space(&self, id: &str, into: &str) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE pages SET space_id=?2 WHERE space_id=?1", params![id, into])?;
        tx.execute("UPDATE events SET space_id=?2 WHERE space_id=?1", params![id, into])?;
        tx.execute("DELETE FROM spaces WHERE id=?1", params![id])?;
        tx.commit()
    }

    /// Block text used to be plain; it is now inline Markdown (see `inline.rs`).
    /// Escape old text once so every character still shows exactly as typed.
    fn upgrade_text_format(&self) -> rusqlite::Result<()> {
        if self.get_setting("text_format").is_some() {
            return Ok(());
        }
        let rows: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare("SELECT id, text FROM blocks WHERE kind != 'code' AND text != ''")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let tx = self.conn.unchecked_transaction()?;
        for (id, text) in rows {
            tx.execute("UPDATE blocks SET text=?1 WHERE id=?2", params![crate::inline::escape(&text), id])?;
        }
        tx.execute(
            "INSERT INTO settings(key, value) VALUES ('text_format', 'inline-md-1') ON CONFLICT(key) DO NOTHING",
            [],
        )?;
        tx.commit()
    }

    pub fn load(&self) -> rusqlite::Result<Loaded> {
        let mut pages = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id, cover, cover_y, font, row_of FROM pages",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Page {
                space_id: r.get(9)?,
                id: r.get(0)?,
                parent_id: r.get(1)?,
                title: r.get(2)?,
                icon: r.get(3)?,
                position: r.get(4)?,
                favorite: r.get::<_, i64>(5)? != 0,
                trashed: r.get::<_, i64>(6)? != 0,
                created_at: r.get(7)?,
                updated_at: r.get(8)?,
                cover: r.get(10)?,
                cover_y: r.get::<_, i64>(11)?.clamp(0, 100) as u8,
                font: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
                row_of: r.get::<_, Option<String>>(13)?.filter(|id| !id.is_empty()),
            })
        })?;
        // A page an agent wrote without a space joins the first one, and so
        // does a page of a space that is gone (see `ensure_space`).
        let spaces = self.space_ids()?;
        let first_space = spaces.first().cloned().unwrap_or_default();
        for page in rows {
            let mut page = page?;
            if !spaces.contains(&page.space_id) {
                page.space_id = first_space.clone();
            }
            pages.insert(page.id.clone(), page);
        }

        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT page_id, id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind, tint
             FROM blocks ORDER BY page_id, position",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Block {
                    id: r.get(1)?,
                    kind: BlockKind::parse(&r.get::<_, String>(2)?),
                    text: r.get(3)?,
                    checked: r.get::<_, i64>(4)? != 0,
                    indent: r.get::<_, i64>(5)?.clamp(0, crate::model::MAX_INDENT as i64) as u8,
                    collapsed: r.get::<_, i64>(6)? != 0,
                    icon: r.get(7)?,
                    language: r.get(8)?,
                    target: r.get(9)?,
                    data: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    place: crate::model::Place::from_column(&r.get::<_, Option<String>>(11)?.unwrap_or_default()),
                    due: Block::due_from_column(&r.get::<_, Option<String>>(12)?.unwrap_or_default()),
                    remind: Block::remind_from_column(&r.get::<_, Option<String>>(13)?.unwrap_or_default()),
                    tint: Block::tint_or_none(&r.get::<_, Option<String>>(14)?.unwrap_or_default()),
                },
            ))
        })?;
        for row in rows {
            let (page_id, block) = row?;
            blocks.entry(page_id).or_default().push(block);
        }
        for page_id in crate::lock::open_pages() {
            if let Some(bs) = blocks.remove(&page_id) {
                blocks.insert(page_id.clone(), crate::lock::for_store_read(&page_id, bs));
            }
        }
        Ok((pages, blocks))
    }

    pub fn save_page(&self, p: &Page) -> rusqlite::Result<()> {
        self.conn.prepare_cached(
            "INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id, cover, cover_y, font, row_of)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET parent_id=?2, title=?3, icon=?4, position=?5,
                favorite=?6, trashed=?7, updated_at=MAX(updated_at, ?9), space_id=?10, cover=?11, cover_y=?12, font=?13, row_of=?14",
        )?.execute(
            params![
                p.id,
                p.parent_id,
                p.title,
                p.icon,
                p.position,
                p.favorite as i64,
                p.trashed as i64,
                p.created_at,
                p.updated_at,
                p.space_id,
                p.cover,
                p.cover_y as i64,
                p.font,
                p.row_of
            ],
        )?;
        Ok(())
    }


    /// The blocks of one page as they are on disk now.
    pub fn load_page_blocks(&self, page_id: &str) -> rusqlite::Result<Vec<Block>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind, tint
             FROM blocks WHERE page_id=?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![page_id], |r| {
            Ok(Block {
                id: r.get(0)?,
                kind: BlockKind::parse(&r.get::<_, String>(1)?),
                text: r.get(2)?,
                checked: r.get::<_, i64>(3)? != 0,
                indent: r.get::<_, i64>(4)?.clamp(0, crate::model::MAX_INDENT as i64) as u8,
                collapsed: r.get::<_, i64>(5)? != 0,
                icon: r.get(6)?,
                language: r.get(7)?,
                target: r.get(8)?,
                data: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
                place: crate::model::Place::from_column(&r.get::<_, Option<String>>(10)?.unwrap_or_default()),
                due: Block::due_from_column(&r.get::<_, Option<String>>(11)?.unwrap_or_default()),
                remind: Block::remind_from_column(&r.get::<_, Option<String>>(12)?.unwrap_or_default()),
                tint: Block::tint_or_none(&r.get::<_, Option<String>>(13)?.unwrap_or_default()),
            })
        })?;
        let blocks: Vec<Block> = rows.collect::<rusqlite::Result<_>>()?;
        Ok(crate::lock::for_store_read(page_id, blocks))
    }

    /// The page is locked on disk (its one line is the sealed block: see
    /// `lock::sealed`, also for the line an older Rune saved as text).
    fn sealed_on_disk(tx: &Connection, page_id: &str) -> rusqlite::Result<bool> {
        tx.prepare_cached(
            "SELECT COUNT(*) FROM blocks WHERE page_id=?1 AND (kind='locked'
                 OR (kind='text' AND substr(data, 1, 12)='rune-lock-1:' AND (SELECT COUNT(*) FROM blocks WHERE page_id=?1)=1))",
        )?
        .query_row(params![page_id], |r| r.get::<_, i64>(0))
        .map(|n| n > 0)
    }

    fn refuse_locked() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_AUTH), Some(REFUSED_LOCKED.into()))
    }

    /// Add blocks after the last block of a page, and mark the page as
    /// changed, in one transaction. The blocks already there are not written
    /// again (so nothing another program typed into them meanwhile is lost);
    /// a page that holds only one empty line gets the new blocks in its place.
    pub fn append_blocks(&self, page_id: &str, blocks: &[Block], at: i64) -> rusqlite::Result<usize> {
        // A locked page that is open here takes the lines into its sealed
        // block; one that is not open takes none.
        if crate::lock::is_open(page_id) {
            let mut all = self.load_page_blocks(page_id)?;
            if all.len() == 1 && all[0].kind == BlockKind::Text && all[0].text.trim().is_empty() {
                all.clear();
            }
            all.extend_from_slice(blocks);
            self.save_blocks_at(page_id, &all, at)?;
            return Ok(all.len());
        }
        if Self::sealed_on_disk(&self.conn, page_id)? {
            return Err(Self::refuse_locked());
        }
        let tx = self.conn.unchecked_transaction()?;
        let (count, last, blank): (i64, Option<i64>, i64) = tx.query_row(
            "SELECT COUNT(*), MAX(position), COALESCE(SUM(kind = 'text' AND TRIM(text) = ''), 0) FROM blocks WHERE page_id=?1",
            params![page_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut next = last.map(|p| p + 1).unwrap_or(0);
        let mut kept = count;
        if count == 1 && blank == 1 {
            tx.execute("DELETE FROM blocks WHERE page_id=?1", params![page_id])?;
            next = 0;
            kept = 0;
        }
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind, tint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            )?;
            for (i, b) in blocks.iter().enumerate() {
                stmt.execute(params![
                    b.id,
                    page_id,
                    next + i as i64,
                    b.kind.as_str(),
                    b.text,
                    b.checked as i64,
                    b.indent as i64,
                    b.collapsed as i64,
                    b.icon,
                    b.language,
                    b.target,
                    b.data,
                    b.place.to_column(),
                    b.due_column(),
                    b.remind_column(),
                    b.tint
                ])?;
            }
        }
        tx.prepare_cached("UPDATE pages SET updated_at=?2 WHERE id=?1")?.execute(params![page_id, at])?;
        tx.commit()?;
        Ok(kept as usize + blocks.len())
    }

    /// Replace a page's blocks in one transaction.
    pub fn save_blocks(&self, page_id: &str, blocks: &[Block]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::write_blocks(&tx, page_id, blocks)?;
        tx.commit()
    }

    fn write_blocks(tx: &rusqlite::Transaction, page_id: &str, blocks: &[Block]) -> rusqlite::Result<()> {
        // A locked page that is open here is written sealed; plain lines
        // never take the place of a sealed page that is not open here (an
        // AI tool, a meeting's notes), unless its password is being removed.
        let sealed_now;
        let blocks = match crate::lock::for_store_write(page_id, blocks) {
            Some(block) => {
                sealed_now = [block];
                &sealed_now[..]
            }
            None => blocks,
        };
        if crate::lock::sealed(blocks).is_none() && !crate::lock::may_unseal(page_id) && Self::sealed_on_disk(tx, page_id)? {
            return Err(Self::refuse_locked());
        }
        tx.prepare_cached("DELETE FROM blocks WHERE page_id=?1")?.execute(params![page_id])?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind, tint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            )?;
            for (i, b) in blocks.iter().enumerate() {
                stmt.execute(params![
                    b.id,
                    page_id,
                    i as i64,
                    b.kind.as_str(),
                    b.text,
                    b.checked as i64,
                    b.indent as i64,
                    b.collapsed as i64,
                    b.icon,
                    b.language,
                    b.target,
                    b.data,
                    b.place.to_column(),
                    b.due_column(),
                    b.remind_column(),
                    b.tint
                ])?;
            }
        }
        Ok(())
    }

    /// Save one block's text after typing, and mark the page as changed, in
    /// one transaction. Much cheaper than rewriting the page on every key.
    pub fn save_block_text(&self, page_id: &str, block_id: &str, text: &str, at: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.prepare_cached("UPDATE blocks SET text=?2 WHERE id=?1")?.execute(params![block_id, text])?;
        tx.prepare_cached("UPDATE pages SET updated_at=?2 WHERE id=?1")?.execute(params![page_id, at])?;
        tx.commit()
    }

    /// Save a table's cells, with the words kept beside them for search, and
    /// mark the page as changed, in one transaction.
    pub fn save_block_cells(&self, page_id: &str, block_id: &str, text: &str, data: &str, at: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.prepare_cached("UPDATE blocks SET text=?2, data=?3 WHERE id=?1")?.execute(params![block_id, text, data])?;
        tx.prepare_cached("UPDATE pages SET updated_at=?2 WHERE id=?1")?.execute(params![page_id, at])?;
        tx.commit()
    }

    /// Save a drawing's scene and mark the page as changed, in one transaction.
    pub fn save_block_data(&self, page_id: &str, block_id: &str, data: &str, at: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE blocks SET data=?2 WHERE id=?1", params![block_id, data])?;
        tx.execute("UPDATE pages SET updated_at=?2 WHERE id=?1", params![page_id, at])?;
        tx.commit()
    }

    /// Replace a page's blocks and mark the page as changed, in one transaction.
    pub fn save_blocks_at(&self, page_id: &str, blocks: &[Block], at: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::write_blocks(&tx, page_id, blocks)?;
        tx.prepare_cached("UPDATE pages SET updated_at=?2 WHERE id=?1")?.execute(params![page_id, at])?;
        tx.commit()
    }

    /// Write several settings in one transaction.
    pub fn set_settings(&self, pairs: &[(&str, String)]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2",
            )?;
            for (k, v) in pairs {
                stmt.execute(params![k, v])?;
            }
        }
        tx.commit()
    }

    /// Add many pages and their blocks at once (an import), in one transaction.
    pub fn insert_pages(&self, pages: &[Page], blocks: &[(Id, Vec<Block>)]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for p in pages {
            self.save_page(p)?;
        }
        for (id, bs) in blocks {
            Self::write_blocks(&tx, id, bs)?;
        }
        tx.commit()
    }

    /// Permanently remove pages and their blocks.
    pub fn delete_pages(&self, ids: &[Id]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            tx.execute("DELETE FROM blocks WHERE page_id=?1", params![id])?;
            tx.execute("DELETE FROM page_versions WHERE page_id=?1", params![id])?;
            tx.execute("DELETE FROM pages WHERE id=?1", params![id])?;
        }
        tx.commit()
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let mut stmt = self.conn.prepare_cached("SELECT value FROM settings WHERE key=?1").ok()?;
        stmt.query_row(params![key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2",
            params![key, value],
        )?;
        Ok(())
    }
}

// The UI runs on one thread, so the open store lives in a thread-local.
thread_local! {
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
}

pub fn install(store: Store) {
    STORE.with(|s| *s.borrow_mut() = Some(store));
}

/// Run `f` against the open store, logging (not panicking on) database errors.
/// The fault is also kept for the window to show: see [`trouble`].
pub fn with<R>(f: impl FnOnce(&Store) -> rusqlite::Result<R>) -> Option<R> {
    STORE.with(|s| {
        let guard = s.borrow();
        let store = guard.as_ref()?;
        match f(store) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("[store] {e}");
                TROUBLE.with(|t| t.borrow_mut().waiting = Some(in_words(&e.to_string())));
                None
            }
        }
    })
}

/// A storage fault that the person at the screen has to know about.
#[derive(Default)]
struct Trouble {
    /// The notes file did not open: the app runs on memory, and keeps nothing.
    not_kept: bool,
    /// A read or a write that failed, not shown yet.
    waiting: Option<String>,
    /// What was shown last, and when: the same fault is not told again at once.
    told: Option<(String, std::time::Instant)>,
}

thread_local! {
    static TROUBLE: RefCell<Trouble> = RefCell::new(Trouble::default());
}

/// The same fault is told again only after this long.
const TELL_AGAIN: std::time::Duration = std::time::Duration::from_secs(120);

/// Whether a notes file is open (the app can run without one: see `trouble`).
pub fn is_open() -> bool {
    STORE.with(|s| s.borrow().is_some())
}

/// The notes file did not open, and the app runs without one.
pub fn runs_without_a_file() {
    TROUBLE.with(|t| t.borrow_mut().not_kept = true);
}

/// Something about the notes file that the window has to say.
pub fn tell(message: &str) {
    TROUBLE.with(|t| t.borrow_mut().waiting = Some(message.to_string()));
}

/// A database error in words a person can act on.
fn in_words(error: &str) -> String {
    let lower = error.to_lowercase();
    // (Before "locked": that word is also in what SQLite says of a file
    // another program holds, and the notice blamed another program.)
    if error.contains(REFUSED_LOCKED) {
        "A change was not saved: the page is locked. Open it with its password first.".into()
    } else if lower.contains("full") {
        "A change was not saved: the disk is full.".into()
    } else if lower.contains("locked") || lower.contains("busy") {
        "A change was not saved: another program holds the notes file.".into()
    } else if lower.contains("readonly") || lower.contains("read-only") || lower.contains("permission") {
        "A change was not saved: the notes file cannot be written.".into()
    } else {
        format!("A change was not saved. ({error})")
    }
}

/// A storage fault to show now, if there is one. Each window asks at short
/// intervals; a fault is given out one time, and again after two minutes if
/// it is still there.
pub fn trouble() -> Option<String> {
    TROUBLE.with(|t| {
        let mut t = t.borrow_mut();
        let message = match t.waiting.take() {
            Some(m) => m,
            None if t.not_kept => "Your notes file did not open. Nothing you write now is kept.".to_string(),
            None => return None,
        };
        if t.told.as_ref().is_some_and(|(last, at)| *last == message && at.elapsed() < TELL_AGAIN) {
            return None;
        }
        t.told = Some((message.clone(), std::time::Instant::now()));
        Some(message)
    })
}

pub fn setting(key: &str) -> Option<String> {
    with(|s| Ok(s.get_setting(key))).flatten()
}

pub fn set_setting(key: &str, value: &str) {
    with(|s| s.set_setting(key, value));
}

pub fn set_settings(pairs: &[(&str, String)]) {
    with(|s| s.set_settings(pairs));
}

pub fn db_path() -> Option<PathBuf> {
    STORE.with(|s| s.borrow().as_ref().and_then(|s| s.path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_of_a_row_and_a_table_beside_another_are_kept() {
        let store = Store::open_in_memory().unwrap();
        let page = Page::new(None, 1.0);
        let mut row = Page::new(Some(page.id.clone()), 0.0);
        row.row_of = Some("block-1/row-1".into());
        store.save_page(&page).unwrap();
        store.save_page(&row).unwrap();
        let mut table = Block::new(BlockKind::Table, "");
        table.place.beside = true;
        store.save_blocks(&page.id, &[Block::new(BlockKind::Table, ""), table]).unwrap();
        let (pages, blocks) = store.load().unwrap();
        assert_eq!(pages.get(&row.id).and_then(|p| p.row_of.as_deref()), Some("block-1/row-1"));
        assert_eq!(pages.get(&page.id).and_then(|p| p.row_of.as_deref()), None);
        assert_eq!(blocks.get(&page.id).map(|b| (b[0].place.beside, b[1].place.beside)), Some((false, true)));
        // A page saved again by a program that leaves the row alone keeps it.
        store.save_page(&row).unwrap();
        assert!(store.load().unwrap().0.get(&row.id).is_some_and(|p| p.row_of.is_some()));
    }

    #[test]
    fn pages_and_events_of_a_space_that_is_gone_join_the_first_space() {
        let store = Store::open_in_memory().unwrap();
        let first = store.load_spaces().unwrap()[0].id.clone();
        let mut work = Space::new("Work", "green", 2.0);
        work.id = "work".into();
        store.save_space(&work).unwrap();
        // A second window still showed a space that was deleted, and a page
        // and an event were made there.
        let mut lost = Page::new(None, 1.0);
        lost.space_id = "gone".into();
        let mut kept = Page::new(None, 1.0);
        kept.space_id = "work".into();
        store.save_page(&lost).unwrap();
        store.save_page(&kept).unwrap();
        let event = Event::new("gone", "Call", 0, 1, false);
        store.save_event(&event).unwrap();
        // What is read has them in the first space at once.
        let (pages, _) = store.load().unwrap();
        assert_eq!(pages[&lost.id].space_id, first);
        assert_eq!(pages[&kept.id].space_id, "work");
        assert_eq!(store.load_events().unwrap()[0].space_id, first);
        // And the next start writes that into the file.
        store.migrate().unwrap();
        let in_file = |sql: &str| store.conn.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(in_file(&format!("SELECT space_id FROM pages WHERE id='{}'", lost.id)), first);
        assert_eq!(in_file(&format!("SELECT space_id FROM pages WHERE id='{}'", kept.id)), "work");
        assert_eq!(in_file("SELECT space_id FROM events"), first);
    }

    #[test]
    fn a_page_keeps_its_face_and_a_picture_its_caption() {
        let store = Store::open_in_memory().unwrap();
        let mut page = Page::new(None, 1.0);
        page.title = "Field notes".into();
        page.font = "serif".into();
        store.save_page(&page).unwrap();
        let mut picture = Block::new(BlockKind::Image, "The first screen");
        picture.data = "a.png".into();
        store.save_blocks(&page.id, std::slice::from_ref(&picture)).unwrap();
        let (pages, blocks) = store.load().unwrap();
        assert_eq!(pages.get(&page.id).map(|p| p.font.as_str()), Some("serif"));
        assert_eq!(blocks.get(&page.id).map(|b| b[0].text.as_str()), Some("The first screen"));
        assert_eq!(crate::model::minutes_to_read(0), None);
        assert_eq!(crate::model::minutes_to_read(229), None);
        assert_eq!(crate::model::minutes_to_read(230), Some(1));
        assert_eq!(crate::model::minutes_to_read(231), Some(2));
    }

    #[test]
    fn a_to_do_keeps_its_day_in_the_notes_file() {
        let store = Store::open_in_memory().unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let mut todo = Block::new(BlockKind::Todo, "Call Sam");
        todo.due = Some(day);
        // (And a reminder at a time of that day.)
        todo.remind = chrono::NaiveTime::from_hms_opt(9, 30, 0);
        let plain = Block::new(BlockKind::Todo, "No day");
        store.save_blocks("p", &[todo.clone(), plain.clone()]).unwrap();
        assert_eq!(store.load_page_blocks("p").unwrap(), vec![todo.clone(), plain.clone()]);
        assert_eq!(store.load().unwrap().1.get("p"), Some(&vec![todo, plain]));
    }

    #[test]
    fn a_line_keeps_its_colour_in_the_notes_file() {
        let store = Store::open_in_memory().unwrap();
        let mut words = Block::text("Loud");
        words.tint = "red".into();
        let mut band = Block::new(BlockKind::Heading2, "Quiet");
        band.tint = "blue-bg".into();
        let plain = Block::text("Plain");
        store.save_blocks("p", &[words.clone(), band.clone(), plain.clone()]).unwrap();
        assert_eq!(store.load_page_blocks("p").unwrap(), vec![words.clone(), band.clone(), plain.clone()]);
        assert_eq!(store.load().unwrap().1.get("p"), Some(&vec![words, band, plain]));
        // A word that is no colour is none.
        assert_eq!((Block::tint_or_none("red"), Block::tint_or_none("red-bg"), Block::tint_or_none("loud"), Block::tint_or_none("-bg"), Block::tint_or_none("")), ("red".to_string(), "red-bg".to_string(), String::new(), String::new(), String::new()));
        // Both together: the words first, one of each, and what is no colour left out.
        assert_eq!((Block::tint_or_none("blue-bg red"), Block::tint_or_none("red green loud gray-bg pink-bg"), Block::tint_or_none(" red  ")), ("red blue-bg".to_string(), "red gray-bg".to_string(), "red".to_string()));
        let mut both = Block::text("Both");
        both.tint = "red blue-bg".into();
        assert_eq!(both.tint_parts(), ("red", "blue-bg"));
        store.save_blocks("q", &[both.clone()]).unwrap();
        assert_eq!(store.load_page_blocks("q").unwrap(), vec![both]);
        assert_eq!((Block::text("x").tint_parts(), { let mut b = Block::text("x"); b.tint = "green-bg".into(); b.tint_parts() == ("", "green-bg") }), (("", ""), true));
    }

    #[test]
    fn a_file_from_a_newer_program_is_known_and_keeps_its_mark() {
        let dir = std::env::temp_dir().join(format!("rune-schema-{}", crate::model::new_id()));
        let path = dir.join("workspace.db");
        let store = Store::open(path.clone()).unwrap();
        assert!(!store.from_newer_program());
        assert_eq!(store.file_schema().unwrap(), SCHEMA);
        // A newer program opens the same file and marks it.
        let newer = Store::open(path.clone()).unwrap();
        newer.set_file_schema(SCHEMA + 1);
        // The older program, still open, sees it before its next change...
        assert!(store.from_newer_program());
        // ...and one started later does not put the mark back.
        let later = Store::open(path).unwrap();
        assert!(later.from_newer_program());
        assert_eq!(later.file_schema().unwrap(), SCHEMA + 1);
        drop((store, newer, later));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_log_is_not_emptied_while_another_program_reads_and_that_is_said() {
        let dir = std::env::temp_dir().join(format!("rune-checkpoint-{}", crate::model::new_id()));
        let path = dir.join("workspace.db");
        let store = Store::open(path.clone()).unwrap();
        store.save_blocks("p", &[Block::text("one")]).unwrap();
        // Another program is in the middle of a read (a sync run, a backup).
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN; SELECT count(*) FROM blocks;").unwrap();
        store.save_blocks("p", &[Block::text("two")]).unwrap();
        let before = std::time::Instant::now();
        assert!(!store.checkpoint(), "held up: the caller tries again");
        assert!(before.elapsed() < std::time::Duration::from_secs(1), "and it does not wait");
        reader.execute_batch("COMMIT;").unwrap();
        assert!(store.checkpoint());
        assert_eq!(std::fs::metadata(dir.join("workspace.db-wal")).map(|m| m.len()).unwrap_or(0), 0, "the log is empty");
        drop((store, reader));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_is_told_one_time() {
        install(Store::open_in_memory().unwrap());
        assert_eq!(trouble(), None);
        assert!(with(|s| s.conn.execute("INSERT INTO no_such_table VALUES (1)", []).map(|_| ())).is_none());
        let told = trouble().expect("the fault is given to the window");
        assert!(told.starts_with("A change was not saved"), "{told}");
        // The same fault again at once: not told a second time.
        assert!(with(|s| s.conn.execute("INSERT INTO no_such_table VALUES (1)", []).map(|_| ())).is_none());
        assert_eq!(trouble(), None);
        assert_eq!(in_words("database or disk is full"), "A change was not saved: the disk is full.");
        assert_eq!(in_words("database is locked"), "A change was not saved: another program holds the notes file.");
        // A refused write into a locked page is not the fault of another program.
        assert_eq!(in_words(&Store::refuse_locked().to_string()), "A change was not saved: the page is locked. Open it with its password first.");
    }

    #[test]
    fn page_versions_are_kept_newest_first_and_capped() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..(VERSIONS_KEPT + 5) {
            store.save_version("p", i as i64, &format!("v{i}"), &[Block::text(format!("text {i}"))]).unwrap();
        }
        store.save_version("other", 1, "x", &[]).unwrap();
        store.save_version("other", 2, "x", &[]).unwrap();
        let versions = store.load_versions("p").unwrap();
        assert_eq!(versions.len(), VERSIONS_KEPT);
        assert_eq!(versions[0].title, format!("v{}", VERSIONS_KEPT + 4));
        assert_eq!(versions[0].blocks[0].text, format!("text {}", VERSIONS_KEPT + 4));
        assert_eq!(store.latest_version("p").unwrap().unwrap().1, format!("v{}", VERSIONS_KEPT + 4));
        store.delete_pages(&["p".to_string()]).unwrap();
        assert!(store.load_versions("p").unwrap().is_empty());
        assert_eq!(store.load_versions("other").unwrap().len(), 1);
    }

    #[test]
    fn pages_blocks_and_settings_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let mut page = Page::new(None, 1.0);
        page.title = "Hello".into();
        page.icon = Some("🚀".into());
        store.save_page(&page).unwrap();

        let mut todo = Block::new(BlockKind::Todo, "ship it").with_indent(2);
        todo.checked = true;
        let mut code = Block::new(BlockKind::Code, "fn main() {}");
        code.language = Some("rust".into());
        // A picture keeps where it was put and how wide it was made.
        let mut picture = Block::new(BlockKind::Image, "");
        picture.data = "abcdefghij0123456789.png".into();
        picture.place = crate::model::Place { free: true, right: true, x: -40, y: 0, w: 320, beside: false };
        let blocks = vec![Block::text("one"), todo, code, Block::page_link("abc".into()), picture];
        store.save_blocks(&page.id, &blocks).unwrap();
        // Saving again replaces rather than duplicates.
        store.save_blocks(&page.id, &blocks).unwrap();

        let (pages, loaded) = store.load().unwrap();
        // One page alone reads the same as in the whole.
        assert_eq!(store.load_page_blocks(&page.id).unwrap(), blocks);
        assert!(store.load_page_blocks("no such page").unwrap().is_empty());
        // Saved without a space, the page joins the first (default) space.
        let spaces = store.load_spaces().unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(pages[&page.id], Page { space_id: spaces[0].id.clone(), ..page.clone() });
        assert_eq!(loaded[&page.id], blocks);

        // Spaces save, rename and delete; a deleted space's pages move over.
        let mut second = Space::new("Work", "green", 2.0);
        store.save_space(&second).unwrap();
        second.name = "Work stuff".into();
        store.save_space(&second).unwrap();
        let moved = Page { space_id: second.id.clone(), ..Page::new(None, 2.0) };
        store.save_page(&moved).unwrap();
        assert_eq!(store.load_spaces().unwrap()[1].name, "Work stuff");
        store.delete_space(&second.id, &spaces[0].id).unwrap();
        assert_eq!(store.load_spaces().unwrap().len(), 1);
        assert_eq!(store.load().unwrap().0[&moved.id].space_id, spaces[0].id);

        // Adding to a page writes only the new blocks, after the others.
        let added = vec![Block::text("added one"), Block::new(BlockKind::Todo, "added two")];
        assert_eq!(store.append_blocks(&page.id, &added, 77).unwrap(), blocks.len() + 2);
        let now = store.load_page_blocks(&page.id).unwrap();
        assert_eq!(now[..blocks.len()], blocks[..]);
        assert_eq!(now[blocks.len()..], added[..]);
        assert_eq!(store.load().unwrap().0[&page.id].updated_at, 77);
        // A page with only one empty line gets them in its place.
        let empty = Page::new(None, 9.0);
        store.save_page(&empty).unwrap();
        store.save_blocks(&empty.id, &[Block::text("  ")]).unwrap();
        let more = vec![Block::text("more one"), Block::text("more two")];
        assert_eq!(store.append_blocks(&empty.id, &more, 78).unwrap(), 2);
        assert_eq!(store.load_page_blocks(&empty.id).unwrap(), more);
        // Saving a page with an older time of change does not put that time back.
        let stale = Page { updated_at: 5, ..store.load().unwrap().0[&page.id].clone() };
        store.save_page(&stale).unwrap();
        assert_eq!(store.load().unwrap().0[&page.id].updated_at, 77);

        store.set_setting("theme", "dark").unwrap();
        store.set_setting("theme", "light").unwrap();
        assert_eq!(store.get_setting("theme").as_deref(), Some("light"));
        assert_eq!(store.get_setting("missing"), None);

        store.delete_pages(&[page.id, moved.id, empty.id]).unwrap();
        let (pages, loaded) = store.load().unwrap();
        assert!(pages.is_empty() && loaded.is_empty());
    }

    /// A stand-in for the person's Application Support folder.
    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rune-folder-{name}-{}", crate::model::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A notes folder as a person has it: the notes file with one page, a
    /// picture, a backup and the watcher's record.
    fn notes_folder(dir: &Path, title: &str) {
        let store = Store::open(dir.join(FILE)).unwrap();
        let mut page = Page::new(None, 1.0);
        page.title = title.into();
        store.save_page(&page).unwrap();
        drop(store);
        std::fs::create_dir_all(dir.join("Files")).unwrap();
        std::fs::write(dir.join("Files/picture.png"), b"\x89PNG picture").unwrap();
        std::fs::create_dir_all(dir.join("Backups")).unwrap();
        std::fs::write(dir.join("Backups/workspace-1.db"), b"a backup").unwrap();
        std::fs::write(dir.join("meeting-watch.log"), "2026-11-12 09:00:00 Started Teams\n").unwrap();
    }

    fn titles(db: &Path) -> Vec<String> {
        let (pages, _) = Store::open(db.to_path_buf()).unwrap().load().unwrap();
        let mut titles: Vec<String> = pages.values().map(|p| p.title.clone()).collect();
        titles.sort();
        titles
    }

    /// Every file under `dir` with what it holds (links are not followed).
    fn listing(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut found = Vec::new();
        let mut folders = vec![dir.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    folders.push(entry.path());
                } else if kind.is_symlink() {
                    found.push((entry.path(), std::fs::read_link(entry.path()).unwrap().to_string_lossy().as_bytes().to_vec()));
                } else {
                    found.push((entry.path(), std::fs::read(entry.path()).unwrap()));
                }
            }
        }
        found.sort();
        found
    }

    fn is_link(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }

    #[test]
    fn the_notes_folder_is_the_one_that_holds_the_notes() {
        let b = base("decide");
        let (new, old) = (b.join(FOLDER), b.join(OLD_FOLDER));
        assert_eq!(folder_in(&b), new, "a first start: the new name");
        // Only the folder of the first name (the app did not move it yet).
        notes_folder(&old, "Old");
        assert_eq!(folder_in(&b), old);
        // An empty folder of the new name beside it changes nothing.
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(folder_in(&b), old);
        // Both hold notes: the new name.
        notes_folder(&new, "New");
        assert_eq!(folder_in(&b), new);
        // Only the new one.
        std::fs::remove_dir_all(&old).unwrap();
        assert_eq!(folder_in(&b), new);
        // The first name is a link to the new one (after the move): the new name.
        std::os::unix::fs::symlink(FOLDER, &old).unwrap();
        assert_eq!(folder_in(&b), new);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn the_folder_of_the_first_name_moves_whole_and_one_time() {
        let b = base("move");
        let (new, old) = (b.join(FOLDER), b.join(OLD_FOLDER));
        notes_folder(&old, "Kept");
        // A connector that has the notes open while the app starts.
        let connector = Store::open(old.join(FILE)).unwrap();
        connector.save_blocks("p", &[Block::text("before the move")]).unwrap();
        let before: Vec<(PathBuf, Vec<u8>)> = listing(&old).into_iter().filter(|(p, _)| !p.to_string_lossy().contains("workspace.db")).collect();

        assert_eq!(move_folder_in(&b), FolderMove::Moved);
        assert!(new.is_dir() && !is_link(&new));
        assert!(is_link(&old), "a link is in the old place");
        assert_eq!(std::fs::read_link(&old).unwrap(), PathBuf::from(FOLDER), "and it is relative");
        assert_eq!(folder_in(&b), new);
        let after: Vec<(PathBuf, Vec<u8>)> = listing(&new).into_iter().filter(|(p, _)| !p.to_string_lossy().contains("workspace.db")).collect();
        let moved: Vec<(PathBuf, Vec<u8>)> = before.into_iter().map(|(p, bytes)| (new.join(p.strip_prefix(&old).unwrap()), bytes)).collect();
        assert_eq!(after, moved, "every other file is there as it was: the picture, the backup, the record");
        assert_eq!(std::fs::read(old.join("Files/picture.png")).unwrap(), b"\x89PNG picture", "and is found by the old path too");

        // The connector goes on with the files it has open, and the app
        // (the new path) and an older program (the old path) see what it writes.
        connector.save_blocks("p", &[Block::text("after the move")]).unwrap();
        let app = Store::open(new.join(FILE)).unwrap();
        let older = Store::open(old.join(FILE)).unwrap();
        assert_eq!(app.load_page_blocks("p").unwrap()[0].text, "after the move");
        assert_eq!(older.load_page_blocks("p").unwrap()[0].text, "after the move");
        app.save_blocks("p", &[Block::text("from the app")]).unwrap();
        assert_eq!(connector.load_page_blocks("p").unwrap()[0].text, "from the app");
        assert_eq!(older.load_page_blocks("p").unwrap()[0].text, "from the app");
        drop((connector, app, older));
        assert_eq!(titles(&new.join(FILE)), vec!["Kept"]);

        // A second start changes nothing.
        let once = listing(&b);
        assert_eq!(move_folder_in(&b), FolderMove::Nothing);
        assert_eq!(listing(&b), once);
        assert!(is_link(&old) && new.is_dir());
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn the_folder_moves_into_an_empty_folder_of_the_new_name() {
        let b = base("empty");
        let (new, old) = (b.join(FOLDER), b.join(OLD_FOLDER));
        notes_folder(&old, "Kept");
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(move_folder_in(&b), FolderMove::Moved);
        assert!(is_link(&old) && !is_link(&new));
        assert_eq!(titles(&new.join(FILE)), vec!["Kept"]);
        assert!(new.join("Files/picture.png").is_file() && new.join("Backups/workspace-1.db").is_file() && new.join("meeting-watch.log").is_file());
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn nothing_moves_when_there_is_nothing_to_move() {
        // A first start: no folder at all.
        let b = base("none");
        assert_eq!(move_folder_in(&b), FolderMove::Nothing);
        assert!(listing(&b).is_empty());
        // Only the new name.
        notes_folder(&b.join(FOLDER), "New");
        let was = listing(&b);
        assert_eq!(move_folder_in(&b), FolderMove::Nothing);
        assert_eq!(listing(&b), was);
        assert!(!b.join(OLD_FOLDER).exists(), "no link is made where no folder was");
        // The first name is a link already (the move was made before).
        std::os::unix::fs::symlink(FOLDER, b.join(OLD_FOLDER)).unwrap();
        let was = listing(&b);
        assert_eq!(move_folder_in(&b), FolderMove::Nothing);
        assert_eq!(listing(&b), was);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn two_folders_with_notes_are_left_as_they_are_and_that_is_said() {
        let b = base("both");
        let (new, old) = (b.join(FOLDER), b.join(OLD_FOLDER));
        notes_folder(&old, "Old");
        notes_folder(&new, "New");
        let was = listing(&b);
        let FolderMove::Stayed(said) = move_folder_in(&b) else { panic!("nothing may move") };
        assert!(said.contains("notes in two folders") && said.contains("uses the folder “Rune”"), "{said}");
        assert_eq!(listing(&b), was, "nothing was changed");
        assert!(!is_link(&old) && !is_link(&new));
        assert_eq!(folder_in(&b), new);
        assert_eq!((titles(&new.join(FILE)), titles(&old.join(FILE))), (vec!["New".to_string()], vec!["Old".to_string()]));
        // A folder of the new name that holds other files, and no notes: the
        // notes stay where they are and are used there.
        let b2 = base("other");
        let (new, old) = (b2.join(FOLDER), b2.join(OLD_FOLDER));
        notes_folder(&old, "Old");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("something.txt"), "of another program").unwrap();
        let was = listing(&b2);
        let FolderMove::Stayed(said) = move_folder_in(&b2) else { panic!("nothing may move") };
        assert!(said.starts_with("Your notes are still in the folder “Notion 2.0”") && said.contains("Nothing was changed"), "{said}");
        assert_eq!(listing(&b2), was);
        assert_eq!(folder_in(&b2), old);
        let _ = std::fs::remove_dir_all(b);
        let _ = std::fs::remove_dir_all(b2);
    }

    #[test]
    fn a_rename_that_fails_changes_nothing_and_that_is_said() {
        use std::os::unix::fs::PermissionsExt;
        let b = base("fails");
        let (new, old) = (b.join(FOLDER), b.join(OLD_FOLDER));
        notes_folder(&old, "Old");
        let was = listing(&b);
        // The folder above cannot be written to: no name in it can change.
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o555)).unwrap();
        let done = move_folder_in(&b);
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o755)).unwrap();
        let FolderMove::Stayed(said) = done else { panic!("the rename must fail: {done:?}") };
        assert!(said.starts_with("Your notes are still in the folder “Notion 2.0”") && said.contains("could not get the name “Rune”") && said.contains("uses the notes where they are"), "{said}");
        assert_eq!(listing(&b), was, "nothing was changed");
        assert!(!new.exists() && !is_link(&old));
        assert_eq!(folder_in(&b), old, "and the app goes on with the notes where they are");
        assert_eq!(titles(&old.join(FILE)), vec!["Old"]);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn data_version_changes_when_another_connection_writes() {
        let dir = std::env::temp_dir().join(format!("rune-store-test-{}", crate::model::new_id()));
        let path = dir.join("workspace.db");
        let app = Store::open(path.clone()).unwrap();
        let other = Store::open(path).unwrap();
        let before = app.data_version();
        other.save_page(&Page::new(None, 1.0)).unwrap();
        assert_ne!(app.data_version(), before);
        let seen = app.data_version();
        app.save_page(&Page::new(None, 2.0)).unwrap();
        assert_eq!(app.data_version(), seen, "own writes don't count as external");
        let _ = std::fs::remove_dir_all(dir);
    }
}
