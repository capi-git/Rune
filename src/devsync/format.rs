//! The files of sync between devices (docs/sync-format.md, format 1): their
//! names, and what is in them. The iPhone has the same rules in
//! mobile/Core/Sources/RuneCore/SyncFormat.swift.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// The format this program reads and writes.
pub const FORMAT: u32 = 1;
/// The folder inside a sync place (the Mac's own, or iCloud Drive's).
pub const FOLDER: &str = "sync-v1";

/// A device id: 32 lower-case hex characters.
pub fn valid_device(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A device's write counter as a file name: ten digits.
pub fn seq_name(seq: u64) -> String {
    format!("{seq:010}")
}

fn parse_seq(s: &str) -> Option<u64> {
    (s.len() == 10 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten().filter(|&n| n > 0)
}

/// The only names that are ever taken in a sync folder. A name is checked
/// before it touches the file system, so it can never point outside the
/// folder.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SyncPath {
    Device(String),
    Snapshot(String, u64),
    Change(String, u64),
    Picture(String),
}

impl SyncPath {
    pub fn parse(path: &str) -> Option<SyncPath> {
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["devices", file] => {
                let device = file.strip_suffix(".json")?;
                valid_device(device).then(|| SyncPath::Device(device.to_string()))
            }
            ["snapshots", device, file] if valid_device(device) => Some(SyncPath::Snapshot(device.to_string(), parse_seq(file.strip_suffix(".db")?)?)),
            ["changes", device, file] if valid_device(device) => Some(SyncPath::Change(device.to_string(), parse_seq(file.strip_suffix(".json")?)?)),
            ["files", name] if crate::files::valid_name(name) => Some(SyncPath::Picture(name.to_string())),
            _ => None,
        }
    }

    /// The path inside the folder, with "/" between its parts.
    pub fn path(&self) -> String {
        match self {
            SyncPath::Device(d) => format!("devices/{d}.json"),
            SyncPath::Snapshot(d, s) => format!("snapshots/{d}/{}.db", seq_name(*s)),
            SyncPath::Change(d, s) => format!("changes/{d}/{}.json", seq_name(*s)),
            SyncPath::Picture(n) => format!("files/{n}"),
        }
    }

    /// The device that wrote it (none for a picture).
    pub fn device(&self) -> Option<&str> {
        match self {
            SyncPath::Device(d) | SyncPath::Snapshot(d, _) | SyncPath::Change(d, _) => Some(d),
            SyncPath::Picture(_) => None,
        }
    }
}

/// The version of a record: (clock, device). Wall-clock time is never used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub clock: i64,
    pub device: String,
}

impl Version {
    /// Newer: a higher clock, or the same clock and the greater device id.
    pub fn newer_than(&self, other: &Version) -> bool {
        match self.clock.cmp(&other.clock) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => self.device > other.device,
        }
    }
}

/// `devices/<device>.json`: who a device is and how far it is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceFile {
    #[serde(default = "one")]
    pub format: u32,
    pub device: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotMark>,
    #[serde(default)]
    pub applied: std::collections::BTreeMap<String, u64>,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMark {
    pub seq: u64,
    pub clock: i64,
}

/// `changes/<device>/<seq>.json`: what one device changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeFile {
    #[serde(default = "one")]
    pub format: u32,
    pub device: String,
    pub seq: u64,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub changes: Vec<Change>,
}

/// One record, sent whole (or its removal).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub kind: String,
    pub id: String,
    pub version: Version,
    #[serde(default)]
    pub base: Option<Version>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<SyncPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<SyncEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<SyncSpace>,
}

impl Change {
    /// It says what its kind needs.
    pub fn is_complete(&self) -> bool {
        self.removed
            || match self.kind.as_str() {
                "page" => self.page.is_some(),
                "event" => self.event.is_some(),
                "space" => self.space.is_some(),
                _ => false,
            }
    }
}

// The names below are the column names of the database (src/store.rs). Every
// text column passes through exactly as stored: a device never reads it to
// write it again in its own form.

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncPage {
    #[serde(default)]
    pub space_id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default = "fifty")]
    pub cover_y: i64,
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub row_of: Option<String>,
    #[serde(default)]
    pub blocks: Vec<SyncBlock>,
}

fn fifty() -> i64 {
    50
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncBlock {
    pub id: String,
    #[serde(default = "text_kind")]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub indent: i64,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub place: String,
    #[serde(default)]
    pub due: String,
    #[serde(default)]
    pub remind: String,
    /// The colour of the line (see `Block::tint`). A device that does not
    /// know colours sends none: the colour the line has here then stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
}

fn text_kind() -> String {
    "text".into()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncEvent {
    #[serde(default)]
    pub space_id: String,
    #[serde(default)]
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default = "blue")]
    pub color: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub page_id: Option<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub remind_min: Option<i64>,
    #[serde(default)]
    pub repeat: String,
    #[serde(default)]
    pub join_url: String,
}

fn blue() -> String {
    "blue".into()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncSpace {
    #[serde(default)]
    pub name: String,
    #[serde(default = "blue")]
    pub color: String,
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn only_the_four_kinds_of_name_are_taken() {
        assert_eq!(SyncPath::parse(&format!("devices/{D}.json")), Some(SyncPath::Device(D.into())));
        assert_eq!(SyncPath::parse(&format!("changes/{D}/0000000007.json")), Some(SyncPath::Change(D.into(), 7)));
        assert_eq!(SyncPath::parse(&format!("snapshots/{D}/0000000012.db")), Some(SyncPath::Snapshot(D.into(), 12)));
        assert_eq!(SyncPath::parse("files/abc123.png"), Some(SyncPath::Picture("abc123.png".into())));
        for bad in [
            "../workspace.db",
            &format!("changes/{D}/0000000000.json"),
            &format!("changes/{D}/7.json"),
            &format!("changes/{}/0000000001.json", D.to_uppercase()),
            &format!("devices/{D}.json/x"),
            "files/../x.png",
            "files/a.b.png",
            "files/x.exe",
            &format!("snapshots/{D}/0000000001.json"),
            "",
        ] {
            assert_eq!(SyncPath::parse(bad), None, "{bad}");
        }
        let p = SyncPath::Change(D.into(), 13);
        assert_eq!(SyncPath::parse(&p.path()), Some(p));
    }

    #[test]
    fn the_newer_version_is_the_higher_clock_then_the_greater_device() {
        let v = |c, d: &str| Version { clock: c, device: d.into() };
        assert!(v(5, "a").newer_than(&v(4, "b")));
        assert!(v(5, "b").newer_than(&v(5, "a")));
        assert!(!v(5, "a").newer_than(&v(5, "a")));
    }

    #[test]
    fn a_change_from_the_iphone_reads_with_defaults() {
        let file: ChangeFile = serde_json::from_str(&format!(
            r#"{{"device":"{D}","seq":1,"changes":[{{"kind":"page","id":"p","version":{{"clock":3,"device":"{D}"}},"base":null,"page":{{"title":"T","blocks":[{{"id":"b"}}]}},"later":1}}]}}"#
        ))
        .unwrap();
        let page = file.changes[0].page.as_ref().unwrap();
        assert_eq!((page.cover_y, page.blocks[0].kind.as_str(), page.row_of.as_deref()), (50, "text", None));
        assert!(file.changes[0].is_complete());
    }
}
