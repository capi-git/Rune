//! Local persistence in a single SQLite file.

use crate::model::{Block, BlockKind, Event, Id, Page, Space};
use rusqlite::{params, Connection, OptionalExtension};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

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

/// The form of the notes file that this program writes. Make it one larger
/// with each change that an older program would damage: a column it would
/// write back empty, a kind of line it does not know.
pub const SCHEMA: i64 = 4;

impl Store {
    /// `~/Library/Application Support/Notion 2.0/workspace.db`, or the file named
    /// by `NOTION2_DB` (handy for tests and for trying the MCP server safely).
    pub fn default_path() -> PathBuf {
        if let Some(path) = std::env::var_os("NOTION2_DB").filter(|p| !p.is_empty()) {
            return PathBuf::from(path);
        }
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        // The folder keeps the app's first name: the notes stay where they are.
        base.join("Notion 2.0").join("workspace.db")
    }

    pub fn open(path: PathBuf) -> rusqlite::Result<Store> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(&path)?;
        // The app and the MCP server (`notion2 --mcp`) can both have the file
        // open; wait briefly for the other's write instead of failing.
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
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
        let has_due_column: bool =
            self.conn.prepare("SELECT 1 FROM pragma_table_info('blocks') WHERE name='due'")?.exists([])?;
        if !has_due_column {
            self.conn.execute_batch("ALTER TABLE blocks ADD COLUMN due TEXT NOT NULL DEFAULT ''")?;
        }
        self.upgrade_text_format()?;
        self.ensure_space()?;
        // Mark the file with its form. The mark is never made smaller: a file
        // from a newer program keeps the mark that program gave it.
        if self.file_schema()? < SCHEMA {
            self.conn.pragma_update(None, "user_version", SCHEMA)?;
        }
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
    pub fn set_file_schema(&self, v: i64) {
        self.conn.pragma_update(None, "user_version", v).unwrap();
    }

    /// Every workspace has at least one space, and every page belongs to one.
    /// Pages written without a space (older versions, an agent) join the
    /// first space.
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
        self.conn.execute("UPDATE pages SET space_id=?1 WHERE space_id=''", params![first])?;
        self.conn.execute("UPDATE events SET space_id=?1 WHERE space_id=''", params![first])?;
        Ok(())
    }

    pub fn load_events(&self) -> rusqlite::Result<Vec<Event>> {
        let first: String = self
            .conn
            .query_row("SELECT id FROM spaces ORDER BY position, created_at LIMIT 1", [], |r| r.get(0))
            .optional()?
            .unwrap_or_default();
        let mut stmt = self.conn.prepare(
            "SELECT id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat
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
                page_id: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
                remind: r.get(11)?,
                repeat: crate::repeat::Repeat::from_json(&r.get::<_, String>(12)?),
            })
        })?;
        rows.map(|e| {
            e.map(|mut e| {
                if e.space_id.is_empty() {
                    e.space_id = first.clone();
                }
                e
            })
        })
        .collect()
    }

    pub fn save_event(&self, e: &Event) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO events(id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET space_id=?2, title=?3, start_ms=?4, end_ms=?5, all_day=?6,
                color=?7, notes=?8, page_id=?9, updated_at=?11, remind_min=?12, repeat=?13",
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
                crate::repeat::Repeat::to_json(e.repeat.as_ref())
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
            "SELECT id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id, cover, cover_y, font FROM pages",
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
            })
        })?;
        // A page an agent wrote without a space joins the first one.
        let first_space: String = self
            .conn
            .query_row("SELECT id FROM spaces ORDER BY position, created_at LIMIT 1", [], |r| r.get(0))
            .optional()?
            .unwrap_or_default();
        for page in rows {
            let mut page = page?;
            if page.space_id.is_empty() {
                page.space_id = first_space.clone();
            }
            pages.insert(page.id.clone(), page);
        }

        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT page_id, id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due
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
                },
            ))
        })?;
        for row in rows {
            let (page_id, block) = row?;
            blocks.entry(page_id).or_default().push(block);
        }
        Ok((pages, blocks))
    }

    pub fn save_page(&self, p: &Page) -> rusqlite::Result<()> {
        self.conn.prepare_cached(
            "INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id, cover, cover_y, font)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET parent_id=?2, title=?3, icon=?4, position=?5,
                favorite=?6, trashed=?7, updated_at=MAX(updated_at, ?9), space_id=?10, cover=?11, cover_y=?12, font=?13",
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
                p.font
            ],
        )?;
        Ok(())
    }


    /// The blocks of one page as they are on disk now.
    pub fn load_page_blocks(&self, page_id: &str) -> rusqlite::Result<Vec<Block>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, text, checked, indent, collapsed, icon, language, target, data, place, due
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
            })
        })?;
        rows.collect()
    }

    /// Add blocks after the last block of a page, and mark the page as
    /// changed, in one transaction. The blocks already there are not written
    /// again (so nothing another program typed into them meanwhile is lost);
    /// a page that holds only one empty line gets the new blocks in its place.
    pub fn append_blocks(&self, page_id: &str, blocks: &[Block], at: i64) -> rusqlite::Result<usize> {
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
                "INSERT INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data, place, due)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                    b.due_column()
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
        tx.prepare_cached("DELETE FROM blocks WHERE page_id=?1")?.execute(params![page_id])?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data, place, due)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                    b.due_column()
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
    if lower.contains("full") {
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
        let plain = Block::new(BlockKind::Todo, "No day");
        store.save_blocks("p", &[todo.clone(), plain.clone()]).unwrap();
        assert_eq!(store.load_page_blocks("p").unwrap(), vec![todo.clone(), plain.clone()]);
        assert_eq!(store.load().unwrap().1.get("p"), Some(&vec![todo, plain]));
    }

    #[test]
    fn a_file_from_a_newer_program_is_known_and_keeps_its_mark() {
        let dir = std::env::temp_dir().join(format!("notion2-schema-{}", crate::model::new_id()));
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
        picture.place = crate::model::Place { free: true, right: true, x: -40, y: 0, w: 320 };
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

    #[test]
    fn data_version_changes_when_another_connection_writes() {
        let dir = std::env::temp_dir().join(format!("notion2-store-test-{}", crate::model::new_id()));
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
