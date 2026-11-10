//! Daily backups of the workspace file.
//!
//! Once a day the app writes a complete, consistent copy of the workspace into
//! a "Backups" folder next to it (SQLite's `VACUUM INTO`, which is safe while
//! the app is in use) and keeps the newest few. A backup is an ordinary
//! workspace file: to use one, quit the app and put it in place of
//! `workspace.db`.
//!
//! The copy is made on its own read-only connection off the UI thread, so a
//! large workspace never makes the window pause.

use crate::model::now_ms;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How many backups are kept.
pub const KEPT: usize = 14;
/// How often a new one is made.
const EVERY_MS: i64 = 24 * 60 * 60 * 1000;

const PREFIX: &str = "workspace-";

/// The folder backups of the workspace file `db` go in.
pub fn dir_for(db: &Path) -> PathBuf {
    db.parent().unwrap_or_else(|| Path::new(".")).join("Backups")
}

/// Write a complete copy of the workspace file `db` to a new backup file,
/// then keep only the newest few. Blocks; call it off the UI thread.
pub fn back_up(db: &Path) -> Result<PathBuf, String> {
    let dir = dir_for(db);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = crate::when::local(now_ms()).format("%Y-%m-%d %H.%M.%S");
    let file = dir.join(format!("{PREFIX}{stamp}.db"));
    if file.exists() {
        return Ok(file);
    }
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    conn.execute("VACUUM INTO ?1", [file.to_string_lossy()]).map_err(|e| e.to_string())?;
    prune(&dir, KEPT);
    Ok(file)
}

/// Backup files in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(PathBuf, SystemTime)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with(PREFIX) && name.ends_with(".db")
        })
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((p, modified))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    out
}

/// Remove all but the newest `keep` backups. Only files this app named are touched.
fn prune(dir: &Path, keep: usize) {
    for (old, _) in list(dir).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}

fn ms_of(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// When the newest backup of the open workspace was made (ms), if there is one.
pub fn last() -> Option<i64> {
    let db = crate::store::db_path()?;
    list(&dir_for(&db)).first().map(|(_, t)| ms_of(*t))
}

/// Make a backup of the open workspace now. Returns where it went.
pub async fn now() -> Result<PathBuf, String> {
    let db = crate::store::db_path().ok_or("This workspace isn't saved to a file.")?;
    let (tx, rx) = futures_channel::oneshot::channel();
    crate::ai::runtime().spawn_blocking(move || {
        let _ = tx.send(back_up(&db));
    });
    rx.await.unwrap_or_else(|_| Err("The backup stopped.".into()))
}

/// Make a backup if the newest one is a day old (or there is none).
pub async fn if_due() {
    if last().is_some_and(|t| now_ms() - t < EVERY_MS) {
        return;
    }
    if let Err(e) = now().await {
        eprintln!("[backup] {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn backups_are_complete_copies_and_only_the_newest_are_kept() {
        let dir = std::env::temp_dir().join(format!("notion2-backup-test-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("workspace.db");
        let store = Store::open(db.clone()).unwrap();
        store.set_setting("hello", "world").unwrap();
        let file = back_up(&db).unwrap();
        assert_eq!(file.parent(), Some(dir_for(&db).as_path()));
        let copy = Store::open(file).unwrap();
        assert_eq!(copy.get_setting("hello").as_deref(), Some("world"));

        let backups = dir_for(&db);
        for i in 0..(KEPT + 3) {
            std::fs::write(backups.join(format!("{PREFIX}2000-01-{:02} 00.00.00.db", i + 1)), b"x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::fs::write(backups.join("notes.txt"), b"keep me").unwrap();
        prune(&backups, KEPT);
        assert_eq!(list(&backups).len(), KEPT);
        assert!(backups.join("notes.txt").exists(), "other files are left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
