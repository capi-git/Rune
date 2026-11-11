//! The sync engine of the Mac (docs/sync-format.md): it notes which records
//! changed, gives them versions, writes them to change files, writes
//! snapshots, and applies the other devices' change files.
//!
//! It works on a connection of its own, off the UI thread. The windows see
//! what it applied through `PRAGMA data_version` (src/sync.rs), as they see
//! a change made by the MCP server.
//!
//! Which records changed is noted by triggers in the notes file, so every
//! program that writes the file is seen: the app, `notion2 --mcp`, an older
//! copy of Rune. A trigger only adds (kind, id) to `sync_dirty`; the engine
//! turns those into versions when it runs. While the engine applies another
//! device's change it puts the row `applying` in `sync_meta`, and the
//! triggers pass over its own writes (no other connection sees that row).

use super::format::*;
use crate::model::{Block, BlockKind, Place};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The tables every synced database has (the same as the iPhone's), and the
/// Mac's list of records changed since the engine last ran.
const TABLES: &str = "
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
CREATE TABLE IF NOT EXISTS sync_dirty(kind TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY(kind, id)) WITHOUT ROWID;
";

/// The triggers that fill `sync_dirty`. Kept in the file once sync was on.
fn triggers() -> String {
    let skip = "WHEN NOT EXISTS (SELECT 1 FROM sync_meta WHERE key = 'applying')";
    let mut sql = String::new();
    for (kind, table) in [("page", "pages"), ("event", "events"), ("space", "spaces")] {
        sql += &format!(
            "CREATE TRIGGER IF NOT EXISTS sync_{table}_insert AFTER INSERT ON {table} {skip} BEGIN INSERT OR IGNORE INTO sync_dirty VALUES ('{kind}', NEW.id); END;
             CREATE TRIGGER IF NOT EXISTS sync_{table}_update AFTER UPDATE ON {table} {skip} BEGIN INSERT OR IGNORE INTO sync_dirty VALUES ('{kind}', NEW.id); INSERT OR IGNORE INTO sync_dirty VALUES ('{kind}', OLD.id); END;
             CREATE TRIGGER IF NOT EXISTS sync_{table}_delete AFTER DELETE ON {table} {skip} BEGIN INSERT OR IGNORE INTO sync_dirty VALUES ('{kind}', OLD.id); END;
            "
        );
    }
    sql += &format!(
        "CREATE TRIGGER IF NOT EXISTS sync_blocks_insert AFTER INSERT ON blocks {skip} BEGIN INSERT OR IGNORE INTO sync_dirty VALUES ('page', NEW.page_id); END;
         CREATE TRIGGER IF NOT EXISTS sync_blocks_update AFTER UPDATE ON blocks {skip} BEGIN INSERT OR IGNORE INTO sync_dirty VALUES ('page', NEW.page_id); INSERT OR IGNORE INTO sync_dirty VALUES ('page', OLD.page_id); END;
         CREATE TRIGGER IF NOT EXISTS sync_blocks_delete AFTER DELETE ON blocks {skip} BEGIN INSERT OR IGNORE INTO sync_dirty VALUES ('page', OLD.page_id); END;
        "
    );
    sql
}

/// The names of the triggers, to take them out of a snapshot.
const TRIGGERED: [&str; 4] = ["pages", "events", "spaces", "blocks"];

/// A change file is kept under about this many bytes.
const LARGEST_FILE: usize = 1_000_000;
/// A new snapshot after this many files of this device since the last one.
const SNAPSHOT_EVERY: u64 = 500;
/// Own change files older than this (and before the newest snapshot) go.
const KEEP_FILES_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// What one run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Records this Mac wrote to a change file.
    pub written: usize,
    /// Changes taken from other devices.
    pub applied: usize,
    /// Records changed in two places at once.
    pub conflicts: usize,
    /// Some files are listed but not here yet (iCloud is still bringing them).
    pub waiting: bool,
    /// A device writes a newer format: Rune must be updated to read it.
    pub needs_update: bool,
    /// The other devices, as their device files say.
    pub devices: Vec<DeviceFile>,
}

pub struct Engine {
    conn: Connection,
    /// Where files are made before they are moved into a sync folder.
    state: PathBuf,
    /// The pictures of the notes (src/files.rs).
    pictures: PathBuf,
    device: String,
    name: String,
    /// The device file last written to each folder, without its time.
    told: HashMap<PathBuf, String>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn now_ms() -> i64 {
    crate::model::now_ms()
}

impl Engine {
    /// Open the notes file `db` for sync. The first time, the file gets the
    /// sync tables and the triggers, every record gets a version, and the
    /// Mac's first snapshot goes into `own` (its own sync folder).
    pub fn open(db: &Path, state: &Path, own: &Path, pictures: PathBuf, name: &str) -> Result<Engine, String> {
        // The file in the form this program writes (the columns a change
        // carries), as the app makes it when it opens the file.
        drop(crate::store::Store::open(db.to_path_buf()).map_err(err)?);
        let conn = Connection::open(db).map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        std::fs::create_dir_all(state).map_err(err)?;
        let mut engine = Engine { conn, state: state.to_path_buf(), pictures, device: String::new(), name: name.to_string(), told: HashMap::new() };
        engine.conn.execute_batch(TABLES).map_err(err)?;
        engine.conn.execute_batch(&triggers()).map_err(err)?;
        // Left by a run that stopped in the middle: it must not hide changes.
        engine.conn.execute("DELETE FROM sync_meta WHERE key = 'applying'", []).map_err(err)?;
        match engine.meta("device") {
            Some(device) if valid_device(&device) => engine.device = device,
            _ => engine.start(own)?,
        }
        Ok(engine)
    }

    pub fn device(&self) -> &str {
        &self.device
    }

    /// The first time: a device id, a version for every record, a snapshot.
    fn start(&mut self, own: &Path) -> Result<(), String> {
        self.device = uuid::Uuid::new_v4().simple().to_string();
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        set_meta(&tx, "device", &self.device)?;
        set_meta(&tx, "clock", "0")?;
        set_meta(&tx, "seq", "0")?;
        tx.execute("DELETE FROM sync_dirty", []).map_err(err)?;
        let mut clock = 0i64;
        for (kind, table) in [("space", "spaces"), ("page", "pages"), ("event", "events")] {
            let ids: Vec<String> = {
                let mut stmt = tx.prepare(&format!("SELECT id FROM {table}")).map_err(err)?;
                let rows = stmt.query_map([], |r| r.get(0)).map_err(err)?;
                rows.collect::<Result<_, _>>().map_err(err)?
            };
            for id in ids {
                clock += 1;
                tx.execute(
                    "INSERT INTO sync_records(kind, id, clock, device, synced_clock, synced_device, removed) VALUES (?1, ?2, ?3, ?4, ?3, ?4, 0)
                     ON CONFLICT(kind, id) DO UPDATE SET clock = ?3, device = ?4, synced_clock = ?3, synced_device = ?4, removed = 0",
                    params![kind, id, clock, self.device],
                )
                .map_err(err)?;
            }
        }
        set_meta(&tx, "clock", &clock.to_string())?;
        tx.commit().map_err(err)?;
        self.snapshot(&[own.to_path_buf()])?;
        Ok(())
    }

    fn meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM sync_meta WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    fn meta_number(&self, key: &str) -> i64 {
        self.meta(key).and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    /// One run: note the changes made here, apply the other devices' files
    /// in every folder, write what changed here, and say where this Mac is.
    /// `folders[0]` is the Mac's own folder; the rest (iCloud Drive) get a
    /// copy of each of its own files.
    pub fn run(&mut self, folders: &[PathBuf]) -> Result<Report, String> {
        let mut report = Report::default();
        self.stamp()?;
        let mut devices: BTreeMap<String, DeviceFile> = BTreeMap::new();
        for folder in folders {
            self.apply_folder(folder, &mut report, &mut devices)?;
        }
        self.fetch_pictures(folders);
        report.written = self.write_changes(folders)?;
        let mark = self.meta_number("snapshot_seq") as u64;
        if self.meta_number("seq") as u64 >= mark + SNAPSHOT_EVERY {
            self.snapshot(folders)?;
            self.prune(folders);
        }
        self.mirror(folders);
        self.tell(folders)?;
        devices.remove(&self.device);
        report.devices = devices.into_values().collect();
        Ok(report)
    }

    // ----- what changed here ---------------------------------------------

    /// Give every record a trigger noted a new version (one clock step each).
    fn stamp(&mut self) -> Result<(), String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let dirty: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT kind, id FROM sync_dirty").map_err(err)?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        if dirty.is_empty() {
            return Ok(());
        }
        let mut clock: i64 = get_meta(&tx, "clock")?.and_then(|v| v.parse().ok()).unwrap_or(0);
        for (kind, id) in dirty {
            let table = match kind.as_str() {
                "page" => "pages",
                "event" => "events",
                "space" => "spaces",
                _ => continue,
            };
            let here: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"), [&id], |r| r.get(0)).map_err(err)?;
            let known: Option<bool> = tx
                .query_row("SELECT removed FROM sync_records WHERE kind = ?1 AND id = ?2", params![kind, id], |r| r.get::<_, i64>(0).map(|v| v != 0))
                .optional()
                .map_err(err)?;
            // A record made and removed between two runs never travels.
            if !here && known.is_none_or(|removed| removed) {
                continue;
            }
            clock += 1;
            tx.execute(
                "INSERT INTO sync_records(kind, id, clock, device, removed) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, id) DO UPDATE SET clock = ?3, device = ?4, removed = ?5",
                params![kind, id, clock, self.device, !here as i64],
            )
            .map_err(err)?;
        }
        set_meta(&tx, "clock", &clock.to_string())?;
        tx.execute("DELETE FROM sync_dirty", []).map_err(err)?;
        tx.commit().map_err(err)
    }

    /// Write every record whose version is not the one last written, in
    /// change files of about 1 MB at most. Returns how many.
    fn write_changes(&mut self, folders: &[PathBuf]) -> Result<usize, String> {
        let dirty: Vec<(String, String, i64, String, Option<i64>, Option<String>, bool)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT kind, id, clock, device, synced_clock, synced_device, removed FROM sync_records
                     WHERE synced_clock IS NULL OR clock != synced_clock OR device != synced_device ORDER BY clock",
                )
                .map_err(err)?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, i64>(6)? != 0)))
                .map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        if dirty.is_empty() {
            return Ok(0);
        }
        let mut batch: Vec<(Change, (String, String, i64, String))> = Vec::new();
        let mut size = 0usize;
        let mut written = 0;
        for (kind, id, clock, device, synced_clock, synced_device, removed) in dirty {
            let mut change = Change {
                kind: kind.clone(),
                id: id.clone(),
                version: Version { clock, device: device.clone() },
                base: synced_clock.zip(synced_device).map(|(clock, device)| Version { clock, device }),
                removed,
                page: None,
                event: None,
                space: None,
            };
            if !removed {
                match kind.as_str() {
                    "page" => change.page = read_page(&self.conn, &id)?,
                    "event" => change.event = read_event(&self.conn, &id)?,
                    "space" => change.space = read_space(&self.conn, &id)?,
                    _ => continue,
                }
                change.removed = !change.is_complete();
            }
            if let Some(page) = &change.page {
                self.publish_pictures(&pictures_of(page), folders);
            }
            let len = serde_json::to_string(&change).map(|s| s.len()).unwrap_or(0);
            if !batch.is_empty() && size + len > LARGEST_FILE {
                written += self.write_batch(std::mem::take(&mut batch), folders)?;
                size = 0;
            }
            size += len;
            batch.push((change, (kind, id, clock, device)));
        }
        written += self.write_batch(batch, folders)?;
        Ok(written)
    }

    fn write_batch(&mut self, batch: Vec<(Change, (String, String, i64, String))>, folders: &[PathBuf]) -> Result<usize, String> {
        if batch.is_empty() {
            return Ok(0);
        }
        let seq = self.next_seq()?;
        let file = ChangeFile { format: FORMAT, device: self.device.clone(), seq, at: now_ms(), changes: batch.iter().map(|(c, _)| c.clone()).collect() };
        let json = serde_json::to_vec(&file).map_err(err)?;
        let path = SyncPath::Change(self.device.clone(), seq).path();
        for folder in folders {
            place(folder, &path, &json)?;
        }
        // Only the records that did not change again meanwhile are done.
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        for (_, (kind, id, clock, device)) in &batch {
            tx.execute(
                "UPDATE sync_records SET synced_clock = clock, synced_device = device WHERE kind = ?1 AND id = ?2 AND clock = ?3 AND device = ?4",
                params![kind, id, clock, device],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(batch.len())
    }

    fn next_seq(&mut self) -> Result<u64, String> {
        let seq = self.meta_number("seq") as u64 + 1;
        set_meta(&self.conn, "seq", &seq.to_string())?;
        Ok(seq)
    }

    // ----- snapshots -------------------------------------------------------

    /// A complete, cleaned copy of the notes in each folder: no settings but
    /// `text_format`, no history, no triggers.
    pub fn snapshot(&mut self, folders: &[PathBuf]) -> Result<u64, String> {
        let names: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT id FROM pages").map_err(err)?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        for id in names {
            if let Some(page) = read_page(&self.conn, &id)? {
                self.publish_pictures(&pictures_of(&page), folders);
            }
        }
        let seq = self.next_seq()?;
        set_meta(&self.conn, "snapshot_seq", &seq.to_string())?;
        set_meta(&self.conn, "snapshot_clock", &self.meta_number("clock").to_string())?;
        let temp = self.state.join(format!("snapshot-{}.db", uuid::Uuid::new_v4().simple()));
        self.conn.execute("VACUUM INTO ?1", [temp.to_string_lossy()]).map_err(err)?;
        let clean = (|| -> rusqlite::Result<()> {
            let copy = Connection::open(&temp)?;
            let mut sql = String::from(
                "DELETE FROM settings WHERE key != 'text_format';
                 DELETE FROM page_versions;
                 DELETE FROM sync_meta WHERE key NOT IN ('device', 'clock', 'seq');
                 DROP TABLE IF EXISTS sync_dirty;",
            );
            for table in TRIGGERED {
                for when in ["insert", "update", "delete"] {
                    sql += &format!("DROP TRIGGER IF EXISTS sync_{table}_{when};");
                }
            }
            copy.execute_batch(&sql)?;
            copy.pragma_update(None, "journal_mode", "DELETE")?;
            copy.execute_batch("VACUUM")?;
            Ok(())
        })();
        if let Err(e) = clean {
            let _ = std::fs::remove_file(&temp);
            return Err(e.to_string());
        }
        let bytes = std::fs::read(&temp).map_err(err);
        let _ = std::fs::remove_file(&temp);
        let bytes = bytes?;
        let path = SyncPath::Snapshot(self.device.clone(), seq).path();
        for folder in folders {
            place(folder, &path, &bytes)?;
        }
        self.tell(folders)?;
        Ok(seq)
    }

    /// After a new snapshot: own change files older than 30 days and before
    /// it go, and own snapshots but the two newest.
    fn prune(&self, folders: &[PathBuf]) {
        let mark = self.meta_number("snapshot_seq") as u64;
        let old = std::time::SystemTime::now() - std::time::Duration::from_millis(KEEP_FILES_MS as u64);
        for folder in folders {
            for (seq, path) in own_files(folder, "changes", &self.device, ".json") {
                let aged = std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| t < old);
                if seq < mark && aged {
                    let _ = std::fs::remove_file(path);
                }
            }
            let mut snaps = own_files(folder, "snapshots", &self.device, ".db");
            snaps.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
            for (_, path) in snaps.into_iter().skip(2) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    // ----- the device file -------------------------------------------------

    /// Write this Mac's device file in each folder, when it says something new.
    fn tell(&mut self, folders: &[PathBuf]) -> Result<(), String> {
        let applied: BTreeMap<String, u64> = {
            let mut stmt = self.conn.prepare("SELECT device, seq FROM sync_peers").map_err(err)?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        let snapshot_seq = self.meta_number("snapshot_seq") as u64;
        let mut file = DeviceFile {
            format: FORMAT,
            device: self.device.clone(),
            name: self.name.clone(),
            kind: "mac".into(),
            at: 0,
            seq: self.meta_number("seq") as u64,
            snapshot: (snapshot_seq > 0).then(|| SnapshotMark { seq: snapshot_seq, clock: self.meta_number("snapshot_clock") }),
            applied,
        };
        let key = serde_json::to_string(&file).map_err(err)?;
        file.at = now_ms();
        let json = serde_json::to_vec_pretty(&file).map_err(err)?;
        let path = SyncPath::Device(self.device.clone()).path();
        for folder in folders {
            let target = folder.join(&path);
            if self.told.get(folder) == Some(&key) && target.exists() {
                continue;
            }
            place_replacing(folder, &path, &json)?;
            self.told.insert(folder.clone(), key.clone());
        }
        Ok(())
    }

    /// Copy each own file of the first folder that another folder lacks
    /// (a folder in iCloud Drive that came later gets the snapshot too).
    fn mirror(&self, folders: &[PathBuf]) {
        let Some((own, others)) = folders.split_first() else { return };
        for other in others {
            for (kind, ending) in [("snapshots", ".db"), ("changes", ".json")] {
                for (seq, path) in own_files(own, kind, &self.device, ending) {
                    let name = if kind == "snapshots" { SyncPath::Snapshot(self.device.clone(), seq) } else { SyncPath::Change(self.device.clone(), seq) }.path();
                    if !other.join(&name).exists() {
                        if let Ok(bytes) = std::fs::read(&path) {
                            let _ = place(other, &name, &bytes);
                        }
                    }
                }
            }
        }
    }

    // ----- pictures --------------------------------------------------------

    /// Put each picture a change names in each folder, once.
    fn publish_pictures(&self, names: &[String], folders: &[PathBuf]) {
        for name in names {
            let path = SyncPath::Picture(name.clone());
            if SyncPath::parse(&path.path()).is_none() {
                continue;
            }
            let source = self.pictures.join(name);
            for folder in folders {
                if folder.join(path.path()).exists() || !source.is_file() {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&source) {
                    let _ = place(folder, &path.path(), &bytes);
                }
            }
        }
    }

    /// Bring the pictures that other devices put in a folder into the notes'
    /// own folder of pictures, so pages here show them.
    fn fetch_pictures(&self, folders: &[PathBuf]) {
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(folder.join("files")) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !crate::files::valid_name(&name) || self.pictures.join(&name).exists() {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(entry.path()) {
                    let _ = std::fs::create_dir_all(&self.pictures);
                    let temp = self.pictures.join(format!(".in-{}", uuid::Uuid::new_v4().simple()));
                    if std::fs::write(&temp, &bytes).is_ok() && std::fs::rename(&temp, self.pictures.join(&name)).is_err() {
                        let _ = std::fs::remove_file(&temp);
                    }
                }
            }
        }
    }

    // ----- applying other devices' files -------------------------------------

    fn apply_folder(&mut self, root: &Path, report: &mut Report, devices: &mut BTreeMap<String, DeviceFile>) -> Result<(), String> {
        let mut here: HashMap<String, DeviceFile> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(root.join("devices")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    if name.ends_with(".icloud") {
                        report.waiting = true;
                    }
                    continue;
                }
                let Some(SyncPath::Device(device)) = SyncPath::parse(&format!("devices/{name}")) else { continue };
                match std::fs::read(entry.path()).ok().and_then(|b| serde_json::from_slice::<DeviceFile>(&b).ok()) {
                    Some(file) if file.device == device => {
                        devices.entry(device.clone()).and_modify(|d| if file.at > d.at { *d = file.clone() }).or_insert(file.clone());
                        here.insert(device, file);
                    }
                    _ => report.waiting = true,
                }
            }
        }
        let mut writers: BTreeSet<String> = BTreeSet::new();
        for kind in ["changes", "snapshots"] {
            if let Ok(entries) = std::fs::read_dir(root.join(kind)) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if valid_device(&name) && name != self.device {
                        writers.insert(name);
                    }
                }
            }
        }
        for device in writers {
            self.apply_device(root, &device, here.get(&device), report)?;
        }
        Ok(())
    }

    fn peer(&self, device: &str) -> u64 {
        self.conn.query_row("SELECT seq FROM sync_peers WHERE device = ?1", [device], |r| r.get::<_, i64>(0)).optional().ok().flatten().unwrap_or(0) as u64
    }

    fn apply_device(&mut self, root: &Path, device: &str, file: Option<&DeviceFile>, report: &mut Report) -> Result<(), String> {
        let changes: HashMap<u64, PathBuf> = own_files(root, "changes", device, ".json").into_iter().collect();
        let snapshots: BTreeMap<u64, PathBuf> = own_files(root, "snapshots", device, ".db").into_iter().collect();
        let waiting_files = icloud_placeholders(&root.join("changes").join(device));
        let last = changes.keys().copied().max().unwrap_or(0).max(file.map(|f| f.seq).unwrap_or(0)).max(snapshots.keys().copied().max().unwrap_or(0));
        let mut next = self.peer(device) + 1;
        // The first time a device is seen and it has a snapshot: take its
        // notes record by record (a second Mac joining), then go on after it.
        if next == 1 {
            if let Some((&seq, path)) = snapshots.iter().next_back() {
                self.take_snapshot(device, seq, path, report)?;
                next = seq + 1;
            }
        }
        while next <= last {
            if let Some(path) = changes.get(&next) {
                let Some(file) = std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<ChangeFile>(&b).ok()) else {
                    report.waiting = true;
                    return Ok(());
                };
                if file.format > FORMAT {
                    report.needs_update = true;
                    return Ok(());
                }
                if file.device != device || file.seq != next {
                    report.waiting = true;
                    return Ok(());
                }
                self.apply_file(device, next, &file.changes, report)?;
            } else if snapshots.contains_key(&next) {
                self.set_peer(device, next)?;
            } else if let Some((&seq, path)) = snapshots.range(next..).next_back() {
                // The files before that snapshot are gone: take it record by
                // record (this Mac has changes of its own to keep).
                self.take_snapshot(device, seq, path, report)?;
                next = seq;
            } else {
                if waiting_files.contains(&next) || file.is_none_or(|f| f.seq >= next) {
                    report.waiting = true;
                }
                return Ok(());
            }
            next += 1;
        }
        Ok(())
    }

    fn set_peer(&self, device: &str, seq: u64) -> Result<(), String> {
        set_peer(&self.conn, device, seq)
    }

    /// Apply one change file in one transaction, the triggers quiet.
    fn apply_file(&mut self, device: &str, seq: u64, changes: &[Change], report: &mut Report) -> Result<(), String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        set_meta(&tx, "applying", "1")?;
        for change in changes {
            apply(&tx, change, report)?;
        }
        set_peer(&tx, device, seq)?;
        tx.execute("DELETE FROM sync_meta WHERE key = 'applying'", []).map_err(err)?;
        tx.commit().map_err(err)
    }

    fn take_snapshot(&mut self, device: &str, seq: u64, path: &Path, report: &mut Report) -> Result<(), String> {
        let snap = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
        let records: Vec<(String, String, i64, String, bool)> = {
            let Ok(mut stmt) = snap.prepare("SELECT kind, id, clock, device, removed FROM sync_records") else {
                report.waiting = true;
                return Ok(());
            };
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0))).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        let mut changes = Vec::new();
        for (kind, id, clock, by, removed) in records {
            let mut change = Change { kind: kind.clone(), id: id.clone(), version: Version { clock, device: by }, base: None, removed, page: None, event: None, space: None };
            if !removed {
                match kind.as_str() {
                    "page" => change.page = read_page(&snap, &id)?,
                    "event" => change.event = read_event(&snap, &id)?,
                    "space" => change.space = read_space(&snap, &id)?,
                    _ => continue,
                }
            }
            if change.is_complete() {
                changes.push(change);
            }
        }
        drop(snap);
        // A change with no base that differs from what is here is a conflict:
        // the rules pick the same winner on every device.
        self.apply_file(device, seq, &changes, report)
    }
}

/// Apply one change by the rules of docs/sync-format.md.
fn apply(tx: &Connection, change: &Change, report: &mut Report) -> Result<(), String> {
    if !["page", "event", "space"].contains(&change.kind.as_str()) || !change.is_complete() {
        return Ok(());
    }
    let local: Option<(Version, bool)> = tx
        .query_row("SELECT clock, device, removed FROM sync_records WHERE kind = ?1 AND id = ?2", params![change.kind, change.id], |r| {
            Ok((Version { clock: r.get(0)?, device: r.get(1)? }, r.get::<_, i64>(2)? != 0))
        })
        .optional()
        .map_err(err)?;
    // This device's clock is never behind a change it has seen.
    tx.execute(
        "INSERT INTO sync_meta(key, value) VALUES ('clock', ?1)
         ON CONFLICT(key) DO UPDATE SET value = CAST(max(CAST(value AS INTEGER), ?1) AS TEXT)",
        [change.version.clock],
    )
    .map_err(err)?;
    if let Some((version, removed)) = &local {
        if *version == change.version {
            return Ok(());
        }
        if change.base.as_ref() != Some(version) {
            // Changed in two places at once. The one that is not a removal
            // wins; else the newer one. The loser of two pages goes to the
            // page's history.
            report.conflicts += 1;
            let wins = match (change.removed, *removed) {
                (false, true) => true,
                (true, false) => false,
                _ => change.version.newer_than(version),
            };
            if !wins {
                if let Some(page) = &change.page {
                    add_to_history(tx, &change.id, &page.title, &page.blocks, page.updated_at)?;
                }
                return Ok(());
            }
            if change.kind == "page" && !removed {
                if let Some(page) = read_page(tx, &change.id)? {
                    add_to_history(tx, &change.id, &page.title, &page.blocks, page.updated_at)?;
                }
            }
        }
    }
    match change.kind.as_str() {
        "page" => {
            tx.execute("DELETE FROM blocks WHERE page_id = ?1", [&change.id]).map_err(err)?;
            match &change.page {
                Some(p) if !change.removed => {
                    tx.execute(
                        "INSERT INTO pages(id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, space_id, cover, cover_y, font, row_of)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                         ON CONFLICT(id) DO UPDATE SET parent_id = ?2, title = ?3, icon = ?4, position = ?5, favorite = ?6, trashed = ?7,
                             created_at = ?8, updated_at = ?9, space_id = ?10, cover = ?11, cover_y = ?12, font = ?13, row_of = ?14",
                        params![
                            change.id,
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
                            p.cover_y.clamp(0, 100),
                            p.font,
                            p.row_of.as_deref().filter(|r| !r.is_empty())
                        ],
                    )
                    .map_err(err)?;
                    for (position, b) in p.blocks.iter().enumerate() {
                        // "OR REPLACE": a block that moved here from another
                        // page may still be in that page.
                        tx.execute(
                            "INSERT OR REPLACE INTO blocks(id, page_id, position, kind, text, checked, indent, collapsed, icon, language, target, data, place, due, remind)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                            params![
                                b.id,
                                change.id,
                                position as i64,
                                b.kind,
                                b.text,
                                b.checked as i64,
                                b.indent.clamp(0, crate::model::MAX_INDENT as i64),
                                b.collapsed as i64,
                                b.icon,
                                b.language,
                                b.target,
                                b.data,
                                b.place,
                                b.due,
                                b.remind
                            ],
                        )
                        .map_err(err)?;
                    }
                }
                _ => {
                    tx.execute("DELETE FROM page_versions WHERE page_id = ?1", [&change.id]).map_err(err)?;
                    tx.execute("DELETE FROM pages WHERE id = ?1", [&change.id]).map_err(err)?;
                }
            }
        }
        "event" => match &change.event {
            Some(e) if !change.removed => {
                tx.execute(
                    "INSERT INTO events(id, space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, remind_min, repeat, join_url)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(id) DO UPDATE SET space_id = ?2, title = ?3, start_ms = ?4, end_ms = ?5, all_day = ?6, color = ?7, notes = ?8,
                         page_id = ?9, created_at = ?10, updated_at = ?11, remind_min = ?12, repeat = ?13, join_url = ?14",
                    params![
                        change.id,
                        e.space_id,
                        e.title,
                        e.start_ms,
                        e.end_ms.max(e.start_ms + 1),
                        e.all_day as i64,
                        e.color,
                        e.notes,
                        e.page_id,
                        e.created_at,
                        e.updated_at,
                        e.remind_min,
                        e.repeat,
                        e.join_url
                    ],
                )
                .map_err(err)?;
            }
            _ => {
                tx.execute("DELETE FROM events WHERE id = ?1", [&change.id]).map_err(err)?;
            }
        },
        _ => match &change.space {
            Some(s) if !change.removed => {
                tx.execute(
                    "INSERT INTO spaces(id, name, color, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET name = ?2, color = ?3, position = ?4, created_at = ?5",
                    params![change.id, s.name, s.color, s.position, s.created_at],
                )
                .map_err(err)?;
            }
            _ => {
                tx.execute("DELETE FROM spaces WHERE id = ?1", [&change.id]).map_err(err)?;
            }
        },
    }
    tx.execute(
        "INSERT INTO sync_records(kind, id, clock, device, synced_clock, synced_device, removed) VALUES (?1, ?2, ?3, ?4, ?3, ?4, ?5)
         ON CONFLICT(kind, id) DO UPDATE SET clock = ?3, device = ?4, synced_clock = ?3, synced_device = ?4, removed = ?5",
        params![change.kind, change.id, change.version.clock, change.version.device, change.removed as i64],
    )
    .map_err(err)?;
    // Taken here: not to be sent back as a change of this Mac.
    tx.execute("DELETE FROM sync_dirty WHERE kind = ?1 AND id = ?2", params![change.kind, change.id]).map_err(err)?;
    report.applied += 1;
    Ok(())
}

/// Keep a page as it was in its history, in the desktop's own form (as
/// `Store::save_version` does).
fn add_to_history(tx: &Connection, page: &str, title: &str, blocks: &[SyncBlock], at: i64) -> Result<(), String> {
    let blocks: Vec<Block> = blocks.iter().map(to_block).collect();
    let json = serde_json::to_string(&blocks).unwrap_or_else(|_| "[]".into());
    let same: bool = tx
        .query_row(
            "SELECT title = ?2 AND blocks = ?3 FROM page_versions WHERE page_id = ?1 ORDER BY at DESC, id DESC LIMIT 1",
            params![page, title, json],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?
        .unwrap_or(false);
    if same {
        return Ok(());
    }
    let at = if at > 0 { at } else { now_ms() };
    tx.execute("INSERT INTO page_versions(page_id, at, title, blocks) VALUES (?1, ?2, ?3, ?4)", params![page, at, title, json]).map_err(err)?;
    tx.execute(
        "DELETE FROM page_versions WHERE page_id = ?1 AND id NOT IN
         (SELECT id FROM page_versions WHERE page_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2)",
        params![page, crate::store::VERSIONS_KEPT as i64],
    )
    .map_err(err)?;
    Ok(())
}

fn to_block(b: &SyncBlock) -> Block {
    Block {
        id: b.id.clone(),
        kind: BlockKind::parse(&b.kind),
        text: b.text.clone(),
        checked: b.checked,
        indent: b.indent.clamp(0, crate::model::MAX_INDENT as i64) as u8,
        collapsed: b.collapsed,
        icon: b.icon.clone(),
        language: b.language.clone(),
        target: b.target.clone(),
        data: b.data.clone(),
        place: Place::from_column(&b.place),
        due: Block::due_from_column(&b.due),
        remind: Block::remind_from_column(&b.remind),
    }
}

// ----- records as they travel ------------------------------------------------

fn columns(conn: &Connection, table: &str) -> Result<HashSet<String>, String> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1)").map_err(err)?;
    let rows = stmt.query_map([table], |r| r.get::<_, String>(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// A column, or a stand-in for a file from before it.
fn or(cols: &HashSet<String>, name: &str, otherwise: &str) -> String {
    if cols.contains(name) { name.to_string() } else { format!("{otherwise} AS {name}") }
}

pub fn read_page(conn: &Connection, id: &str) -> Result<Option<SyncPage>, String> {
    let pc = columns(conn, "pages")?;
    let sql = format!(
        "SELECT space_id, parent_id, title, icon, position, favorite, trashed, created_at, updated_at, {}, {}, {}, {} FROM pages WHERE id = ?1",
        or(&pc, "cover", "NULL"),
        or(&pc, "cover_y", "50"),
        or(&pc, "font", "''"),
        or(&pc, "row_of", "NULL")
    );
    let page = conn
        .query_row(&sql, [id], |r| {
            Ok(SyncPage {
                space_id: r.get(0)?,
                parent_id: r.get(1)?,
                title: r.get(2)?,
                icon: r.get(3)?,
                position: r.get(4)?,
                favorite: r.get::<_, i64>(5)? != 0,
                trashed: r.get::<_, i64>(6)? != 0,
                created_at: r.get(7)?,
                updated_at: r.get(8)?,
                cover: r.get(9)?,
                cover_y: r.get(10)?,
                font: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                row_of: r.get::<_, Option<String>>(12)?.filter(|s| !s.is_empty()),
                blocks: Vec::new(),
            })
        })
        .optional()
        .map_err(err)?;
    let Some(mut page) = page else { return Ok(None) };
    let bc = columns(conn, "blocks")?;
    let sql = format!(
        "SELECT id, kind, text, checked, indent, collapsed, icon, language, target, {}, {}, {}, {} FROM blocks WHERE page_id = ?1 ORDER BY position",
        or(&bc, "data", "''"),
        or(&bc, "place", "''"),
        or(&bc, "due", "''"),
        or(&bc, "remind", "''")
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map([id], |r| {
            Ok(SyncBlock {
                id: r.get(0)?,
                kind: r.get(1)?,
                text: r.get(2)?,
                checked: r.get::<_, i64>(3)? != 0,
                indent: r.get(4)?,
                collapsed: r.get::<_, i64>(5)? != 0,
                icon: r.get(6)?,
                language: r.get(7)?,
                target: r.get(8)?,
                data: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
                place: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                due: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                remind: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
            })
        })
        .map_err(err)?;
    page.blocks = rows.collect::<Result<_, _>>().map_err(err)?;
    Ok(Some(page))
}

pub fn read_event(conn: &Connection, id: &str) -> Result<Option<SyncEvent>, String> {
    let ec = columns(conn, "events")?;
    let sql = format!(
        "SELECT space_id, title, start_ms, end_ms, all_day, color, notes, page_id, created_at, updated_at, {}, {}, {} FROM events WHERE id = ?1",
        or(&ec, "remind_min", "NULL"),
        or(&ec, "repeat", "''"),
        or(&ec, "join_url", "''")
    );
    conn.query_row(&sql, [id], |r| {
        Ok(SyncEvent {
            space_id: r.get(0)?,
            title: r.get(1)?,
            start_ms: r.get(2)?,
            end_ms: r.get(3)?,
            all_day: r.get::<_, i64>(4)? != 0,
            color: r.get(5)?,
            notes: r.get(6)?,
            page_id: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
            remind_min: r.get(10)?,
            repeat: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
            join_url: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
        })
    })
    .optional()
    .map_err(err)
}

pub fn read_space(conn: &Connection, id: &str) -> Result<Option<SyncSpace>, String> {
    conn.query_row("SELECT name, color, position, created_at FROM spaces WHERE id = ?1", [id], |r| {
        Ok(SyncSpace { name: r.get(0)?, color: r.get(1)?, position: r.get(2)?, created_at: r.get(3)? })
    })
    .optional()
    .map_err(err)
}

/// The pictures a page names: its image blocks and a cover of its own.
fn pictures_of(page: &SyncPage) -> Vec<String> {
    let mut names: Vec<String> = page.blocks.iter().filter(|b| b.kind == "image" && !b.data.is_empty()).map(|b| b.data.clone()).collect();
    if let Some(name) = page.cover.as_deref().and_then(|c| c.strip_prefix("file:")) {
        names.push(name.to_string());
    }
    names
}

// ----- small helpers ---------------------------------------------------------

fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM sync_meta WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(err)
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute("INSERT INTO sync_meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, value]).map_err(err)?;
    Ok(())
}

fn set_peer(conn: &Connection, device: &str, seq: u64) -> Result<(), String> {
    conn.execute("INSERT INTO sync_peers(device, seq) VALUES (?1, ?2) ON CONFLICT(device) DO UPDATE SET seq = ?2", params![device, seq as i64])
        .map_err(err)?;
    Ok(())
}

/// The files of one device in `<folder>/<kind>/<device>/`, by number.
fn own_files(folder: &Path, kind: &str, device: &str, ending: &str) -> Vec<(u64, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder.join(kind).join(device)) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let path = format!("{kind}/{device}/{name}");
            match SyncPath::parse(&path)? {
                SyncPath::Change(_, seq) | SyncPath::Snapshot(_, seq) if name.ends_with(ending) => Some((seq, e.path())),
                _ => None,
            }
        })
        .collect()
}

/// Files iCloud has listed but not brought yet (".name.icloud"), by number.
fn icloud_placeholders(folder: &Path) -> HashSet<u64> {
    let Ok(entries) = std::fs::read_dir(folder) else { return HashSet::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let inner = name.strip_prefix('.')?.strip_suffix(".icloud")?.strip_suffix(".json")?.to_string();
            (inner.len() == 10).then(|| inner.parse().ok()).flatten()
        })
        .collect()
}

/// Write a file that is written once: made in the folder under a hidden
/// name first, then moved into place, so a half-written file is never seen.
/// One that is already there is left as it is.
fn place(folder: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = folder.join(path);
    if target.exists() {
        return Ok(());
    }
    write_then_move(folder, &target, bytes)
}

/// The same for the device file, the one file that is written again.
fn place_replacing(folder: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    write_then_move(folder, &folder.join(path), bytes)
}

fn write_then_move(folder: &Path, target: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let temp = folder.join(format!(".out-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&temp, bytes).map_err(err)?;
    std::fs::rename(&temp, target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        err(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = "99999999999999999999999999999999";

    /// A throwaway notes file (made as the app makes it), its sync state,
    /// its own folder and a folder that stands in for iCloud Drive.
    struct Mac {
        dir: PathBuf,
        db: PathBuf,
        engine: Engine,
    }

    impl Mac {
        fn new(name: &str) -> Mac {
            let dir = std::env::temp_dir().join(format!("rune-sync-test-{name}-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&dir).unwrap();
            let db = dir.join("workspace.db");
            {
                let store = crate::store::Store::open(db.clone()).unwrap();
                let mut page = crate::model::Page::new(None, 1.0);
                page.id = "plan".into();
                page.title = "Plan".into();
                store.save_page(&page).unwrap();
                store.save_blocks("plan", &[Block::new(BlockKind::Todo, "Call Sam")]).unwrap();
            }
            let engine = Engine::open(&db, &dir.join("state"), &dir.join("own"), dir.join("Files"), name).unwrap();
            Mac { dir, db, engine }
        }

        fn own(&self) -> PathBuf {
            self.dir.join("own")
        }

        fn sql(&self, sql: &str) {
            Connection::open(&self.db).unwrap().execute_batch(sql).unwrap();
        }

        fn one(&self, sql: &str) -> String {
            Connection::open(&self.db).unwrap().query_row(sql, [], |r| r.get::<_, rusqlite::types::Value>(0)).map(|v| format!("{v:?}")).unwrap()
        }

        fn run(&mut self, folders: &[PathBuf]) -> Report {
            self.engine.run(folders).unwrap()
        }

        fn change_files(&self, folder: &Path) -> Vec<ChangeFile> {
            let mut files = own_files(folder, "changes", self.engine.device(), ".json");
            files.sort();
            files.into_iter().map(|(_, p)| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()).collect()
        }
    }

    impl Drop for Mac {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn version_of(mac: &Mac, kind: &str, id: &str) -> Version {
        let conn = Connection::open(&mac.db).unwrap();
        conn.query_row("SELECT clock, device FROM sync_records WHERE kind = ?1 AND id = ?2", params![kind, id], |r| Ok(Version { clock: r.get(0)?, device: r.get(1)? }))
            .unwrap()
    }

    /// The iPhone writes one change file into the Mac's own folder.
    fn phone_writes(folder: &Path, seq: u64, changes: Vec<Change>) {
        let file = ChangeFile { format: FORMAT, device: PHONE.into(), seq, at: 0, changes };
        place(folder, &SyncPath::Change(PHONE.into(), seq).path(), &serde_json::to_vec(&file).unwrap()).unwrap();
    }

    fn phone_page(base: Option<Version>, clock: i64, title: &str, todo: &str) -> Change {
        Change {
            kind: "page".into(),
            id: "plan".into(),
            version: Version { clock, device: PHONE.into() },
            base,
            removed: false,
            page: Some(SyncPage { title: title.into(), cover_y: 50, blocks: vec![SyncBlock { id: "b1".into(), kind: "todo".into(), text: todo.into(), due: "2026-11-12".into(), ..Default::default() }], ..Default::default() }),
            event: None,
            space: None,
        }
    }

    #[test]
    fn turning_sync_on_versions_every_record_and_writes_a_snapshot_only() {
        let mut mac = Mac::new("first");
        let own = mac.own();
        assert_eq!(mac.one("SELECT count(*) FROM sync_records WHERE synced_clock = clock"), mac.one("SELECT count(*) FROM sync_records"));
        let snaps = own_files(&own, "snapshots", mac.engine.device(), ".db");
        assert_eq!(snaps.len(), 1);
        // The snapshot is clean: no triggers, no list of changes, no history.
        let snap = Connection::open(&snaps[0].1).unwrap();
        let triggers: i64 = snap.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'trigger'", [], |r| r.get(0)).unwrap();
        let dirty: i64 = snap.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'sync_dirty'", [], |r| r.get(0)).unwrap();
        let mode: String = snap.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!((triggers, dirty, mode.as_str()), (0, 0, "delete"));
        assert!(read_page(&snap, "plan").unwrap().is_some());
        // Nothing changed since: no change file.
        let report = mac.run(&[own.clone()]);
        assert_eq!(report.written, 0);
        assert!(mac.change_files(&own).is_empty());
        assert!(own.join(SyncPath::Device(mac.engine.device().into()).path()).exists());
    }

    #[test]
    fn a_change_made_here_goes_out_once_with_its_base() {
        let mut mac = Mac::new("writer");
        let own = mac.own();
        let first = version_of(&mac, "page", "plan");
        // As the app or the MCP server writes: straight into the file.
        mac.sql("UPDATE pages SET title = 'Plan B' WHERE id = 'plan'; UPDATE blocks SET text = 'Call Ana' WHERE page_id = 'plan';");
        assert_eq!(mac.run(&[own.clone()]).written, 1);
        let files = mac.change_files(&own);
        let change = &files[0].changes[0];
        assert_eq!(change.base.as_ref(), Some(&first));
        assert!(change.version.newer_than(&first));
        let page = change.page.as_ref().unwrap();
        assert_eq!((page.title.as_str(), page.blocks[0].text.as_str()), ("Plan B", "Call Ana"));
        // Sent: nothing more to send.
        assert_eq!(mac.run(&[own.clone()]).written, 0);
        // A removal travels as one.
        mac.sql("INSERT INTO events(id, title, start_ms, end_ms) VALUES ('e1', 'Gym', 0, 1);");
        mac.run(&[own.clone()]);
        mac.sql("DELETE FROM events WHERE id = 'e1';");
        mac.run(&[own.clone()]);
        let last = mac.change_files(&own).pop().unwrap();
        assert!(last.changes[0].removed && last.changes[0].kind == "event");
        // Made and removed between two runs: it never travels.
        mac.sql("INSERT INTO events(id, title, start_ms, end_ms) VALUES ('e2', 'Gone', 0, 1); DELETE FROM events WHERE id = 'e2';");
        assert_eq!(mac.run(&[own.clone()]).written, 0);
    }

    #[test]
    fn a_change_from_the_iphone_is_taken_and_not_sent_back() {
        let mut mac = Mac::new("reader");
        let own = mac.own();
        let base = version_of(&mac, "page", "plan");
        phone_writes(&own, 1, vec![phone_page(Some(base), 50, "From the phone", "Buy milk")]);
        let report = mac.run(&[own.clone()]);
        assert_eq!((report.applied, report.conflicts, report.written), (1, 0, 0));
        assert_eq!(mac.one("SELECT title FROM pages WHERE id = 'plan'"), "Text(\"From the phone\")");
        assert_eq!(mac.one("SELECT due FROM blocks WHERE id = 'b1'"), "Text(\"2026-11-12\")");
        assert_eq!(mac.one(&format!("SELECT seq FROM sync_peers WHERE device = '{PHONE}'")), "Integer(1)");
        // The Mac's clock is past the phone's now, and the device file says how far it is.
        assert!(mac.engine.meta_number("clock") >= 50);
        let told: DeviceFile = serde_json::from_slice(&std::fs::read(own.join(SyncPath::Device(mac.engine.device().into()).path())).unwrap()).unwrap();
        assert_eq!(told.applied.get(PHONE), Some(&1));
        // The same file again (from a second folder) does nothing.
        assert_eq!(mac.run(&[own.clone()]), Report { devices: vec![], ..Default::default() });
    }

    #[test]
    fn a_page_changed_in_two_places_keeps_the_loser_in_its_history() {
        let mut mac = Mac::new("conflict");
        let own = mac.own();
        let base = version_of(&mac, "page", "plan");
        // The Mac changes the page and sends it; the phone changed it too,
        // from the same base, with a newer clock: the phone's wins.
        mac.sql("UPDATE pages SET title = 'Mac title' WHERE id = 'plan';");
        mac.run(&[own.clone()]);
        phone_writes(&own, 1, vec![phone_page(Some(base), 1000, "Phone title", "x")]);
        let report = mac.run(&[own.clone()]);
        assert_eq!((report.applied, report.conflicts), (1, 1));
        assert_eq!(mac.one("SELECT title FROM pages WHERE id = 'plan'"), "Text(\"Phone title\")");
        assert_eq!(mac.one("SELECT title FROM page_versions WHERE page_id = 'plan' ORDER BY id DESC LIMIT 1"), "Text(\"Mac title\")");
        // An older change from the phone loses: kept in the history only.
        let now = version_of(&mac, "page", "plan");
        phone_writes(&own, 2, vec![phone_page(Some(Version { clock: 3, device: PHONE.into() }), 4, "Old phone title", "y")]);
        let report = mac.run(&[own.clone()]);
        assert_eq!((report.applied, report.conflicts), (0, 1));
        assert_eq!(version_of(&mac, "page", "plan"), now);
        assert_eq!(mac.one("SELECT title FROM page_versions WHERE page_id = 'plan' ORDER BY id DESC LIMIT 1"), "Text(\"Old phone title\")");
    }

    #[test]
    fn files_wait_in_order_and_a_newer_format_stops_the_reading() {
        let mut mac = Mac::new("order");
        let own = mac.own();
        let base = version_of(&mac, "page", "plan");
        // File 2 came before file 1: nothing is applied yet.
        phone_writes(&own, 2, vec![phone_page(Some(Version { clock: 60, device: PHONE.into() }), 61, "Second", "")]);
        let report = mac.run(&[own.clone()]);
        assert!(report.waiting && report.applied == 0);
        phone_writes(&own, 1, vec![phone_page(Some(base), 60, "First", "")]);
        let report = mac.run(&[own.clone()]);
        assert_eq!((report.applied, report.conflicts), (2, 0));
        assert_eq!(mac.one("SELECT title FROM pages WHERE id = 'plan'"), "Text(\"Second\")");
        let file = ChangeFile { format: 2, device: PHONE.into(), seq: 3, at: 0, changes: vec![] };
        place(&own, &SyncPath::Change(PHONE.into(), 3).path(), &serde_json::to_vec(&file).unwrap()).unwrap();
        assert!(mac.run(&[own.clone()]).needs_update);
    }

    #[test]
    fn a_second_mac_gets_the_notes_of_the_first_through_icloud() {
        let mut one = Mac::new("one");
        let mut two = Mac::new("two");
        let cloud = one.dir.join("icloud");
        std::fs::create_dir_all(&cloud).unwrap();
        let (own1, own2) = (one.own(), two.own());
        // Mac one copies its own files, its first snapshot too, to iCloud.
        one.sql("INSERT INTO pages(id, title, space_id) VALUES ('ideas', 'Ideas', '');");
        one.run(&[own1.clone(), cloud.clone()]);
        assert_eq!(own_files(&cloud, "snapshots", one.engine.device(), ".db").len(), 1);
        assert_eq!(own_files(&cloud, "changes", one.engine.device(), ".json").len(), 1);
        // Mac two takes the snapshot record by record and the change after it.
        let report = two.run(&[own2.clone(), cloud.clone()]);
        assert!(report.applied >= 1);
        assert_eq!(two.one("SELECT title FROM pages WHERE id = 'ideas'"), "Text(\"Ideas\")");
        // Both had a page "plan" with no common base: one winner, the same on both.
        one.run(&[own1.clone(), cloud.clone()]);
        two.run(&[own2.clone(), cloud.clone()]);
        one.run(&[own1, cloud]);
        assert_eq!(version_of(&one, "page", "plan"), version_of(&two, "page", "plan"));
    }

    /// One run of the engine on a copy of the notes, into a folder that
    /// stands in for iCloud Drive, to check the iPhone app against it:
    /// RUNE_SYNC_E2E_DB=<a copy> RUNE_SYNC_E2E_CLOUD=<folder> cargo test e2e_one_run -- --ignored
    /// (never the live notes file).
    #[test]
    #[ignore]
    fn e2e_one_run() {
        let db = PathBuf::from(std::env::var("RUNE_SYNC_E2E_DB").expect("RUNE_SYNC_E2E_DB"));
        assert_ne!(db.file_name().and_then(|n| n.to_str()), Some("workspace.db"), "use a copy, not the live file");
        let cloud = PathBuf::from(std::env::var("RUNE_SYNC_E2E_CLOUD").expect("RUNE_SYNC_E2E_CLOUD")).join(FOLDER);
        std::fs::create_dir_all(&cloud).unwrap();
        let state = PathBuf::from(format!("{}.sync", db.display()));
        let own = state.join(FOLDER);
        let pictures = PathBuf::from(format!("{}.files", db.display()));
        let mut engine = Engine::open(&db, &state, &own, pictures, "Test Mac").unwrap();
        let report = engine.run(&[own, cloud]).unwrap();
        println!("device {} report {report:?}", engine.device());
    }

    #[test]
    fn pictures_go_into_the_folder_before_the_page_that_names_them() {
        let mut mac = Mac::new("pictures");
        let own = mac.own();
        std::fs::create_dir_all(mac.dir.join("Files")).unwrap();
        std::fs::write(mac.dir.join("Files/abc123.png"), b"png").unwrap();
        mac.sql("INSERT INTO blocks(id, page_id, position, kind, text, data) VALUES ('i1', 'plan', 5, 'image', '', 'abc123.png');");
        mac.run(&[own.clone()]);
        assert_eq!(std::fs::read(own.join("files/abc123.png")).unwrap(), b"png");
        // A picture the phone put in the folder comes into the Mac's pictures.
        std::fs::write(own.join("files/fromphone1.jpg"), b"jpg").unwrap();
        mac.run(&[own.clone()]);
        assert_eq!(std::fs::read(mac.dir.join("Files/fromphone1.jpg")).unwrap(), b"jpg");
    }
}
