//! Pictures kept with the workspace: the images in pages, and page covers.
//!
//! Each picture is a file in a folder beside the database (never inside it,
//! so the database stays small and quick to back up). A page shows one from
//! the `n2file://` address, which only ever serves files of that folder, by
//! a name the app itself made.

use dioxus::desktop::wry::http::{header, Request, Response, StatusCode};
use dioxus::desktop::wry::WebViewId;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The address pages load pictures from.
pub const SCHEME: &str = "n2file";
/// The largest picture that is taken in.
pub const LARGEST: usize = 30 * 1024 * 1024;

/// The folder of the pictures: "Files" beside the usual database, or
/// "<database>.files" beside another one (a test's throwaway database).
pub fn dir() -> PathBuf {
    let db = crate::store::Store::default_path();
    if std::env::var_os("NOTION2_DB").is_some() {
        let mut name = db.clone().into_os_string();
        name.push(".files");
        return PathBuf::from(name);
    }
    db.parent().map(|p| p.join("Files")).unwrap_or_else(|| PathBuf::from("Files"))
}

/// What kind of picture these bytes are, as the ending of its file name.
/// `None` for anything a page could not show.
pub fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    let brand = bytes.get(4..12).unwrap_or_default();
    if starts(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if starts(b"\xff\xd8\xff") {
        Some("jpg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("gif")
    } else if starts(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else if brand.starts_with(b"ftyp") && [&b"heic"[..], b"heix", b"mif1", b"msf1", b"hevc"].contains(&&brand[4..]) {
        Some("heic")
    } else if starts(b"II*\0") || starts(b"MM\0*") {
        Some("tiff")
    } else if starts(b"BM") && bytes.len() > 26 {
        Some("bmp")
    } else {
        None
    }
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("tiff") => "image/tiff",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// A name this module made: letters and digits, one dot, a known ending.
/// Nothing else is ever read from the folder.
pub fn valid_name(name: &str) -> bool {
    let Some((stem, ending)) = name.split_once('.') else { return false };
    !stem.is_empty() && stem.len() <= 40 && stem.chars().all(|c| c.is_ascii_alphanumeric()) && content_type(name) != "application/octet-stream" && !ending.contains('.')
}

/// Keep a picture. Returns the name it goes by.
pub fn save(bytes: &[u8]) -> Result<String, String> {
    save_in(&dir(), bytes)
}

fn save_in(folder: &Path, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > LARGEST {
        return Err("That picture is too large (the most is 30 MB).".into());
    }
    let ending = kind_of(bytes).ok_or("That is not a picture a page can show.")?;
    std::fs::create_dir_all(folder).map_err(|e| format!("Couldn't keep the picture ({e})."))?;
    let name = format!("{}.{ending}", &uuid::Uuid::new_v4().simple().to_string()[..20]);
    std::fs::write(folder.join(&name), bytes).map_err(|e| format!("Couldn't keep the picture ({e})."))?;
    Ok(name)
}

/// Keep a copy of a picture file the person chose.
pub fn import(file: &Path) -> Result<String, String> {
    let size = std::fs::metadata(file).map(|m| m.len() as usize).unwrap_or(0);
    if size > LARGEST {
        return Err("That picture is too large (the most is 30 MB).".into());
    }
    save(&std::fs::read(file).map_err(|e| format!("Couldn't read the picture ({e})."))?)
}

/// Where a kept picture is, for copying it out (export).
pub fn path(name: &str) -> Option<PathBuf> {
    valid_name(name).then(|| dir().join(name)).filter(|p| p.is_file())
}

/// The address a page loads a kept picture from.
pub fn url(name: &str) -> String {
    format!("{SCHEME}://f/{name}")
}

/// Answer a page that asks for a picture.
pub fn serve(_: WebViewId, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let name = request.uri().path().trim_start_matches('/');
    let found = valid_name(name).then(|| std::fs::read(dir().join(name)).ok()).flatten();
    let (status, kind, body) = match found {
        Some(bytes) => (StatusCode::OK, content_type(name), bytes),
        None => (StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, kind)
        // A name is never used again for another picture.
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn pictures_are_told_by_what_they_hold_not_by_their_name() {
        assert_eq!(kind_of(PNG), Some("png"));
        assert_eq!(kind_of(b"\xff\xd8\xff\xe0\0\x10JFIF"), Some("jpg"));
        assert_eq!(kind_of(b"GIF89a\x01\0"), Some("gif"));
        assert_eq!(kind_of(b"RIFF\x24\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(kind_of(b"\0\0\0\x18ftypheic\0\0\0\0"), Some("heic"));
        assert_eq!(kind_of(b"<svg xmlns='http://www.w3.org/2000/svg'><script>"), None);
        assert_eq!(kind_of(b"#!/bin/sh\nrm -rf"), None);
        assert_eq!(kind_of(b""), None);
    }

    #[test]
    fn only_names_made_here_are_served() {
        assert!(valid_name("0123456789abcdef0123.png") && valid_name("a.jpg"));
        for bad in ["../workspace.db", "a/b.png", "a.png/..", ".png", "a.txt", "a", "a.b.png", "workspace.db", "a b.png", ""] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_picture_is_kept_under_a_new_name_and_other_things_are_refused() {
        let folder = std::env::temp_dir().join(format!("n2files-{}", uuid::Uuid::new_v4().simple()));
        let name = save_in(&folder, PNG).unwrap();
        assert!(valid_name(&name) && name.ends_with(".png"));
        assert_eq!(std::fs::read(folder.join(&name)).unwrap(), PNG);
        assert_ne!(save_in(&folder, PNG).unwrap(), name);
        assert!(save_in(&folder, b"not a picture").is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
