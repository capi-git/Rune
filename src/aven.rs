//! Giving a page to a chat in Aven (the person's AI chat app).
//!
//! Chats in Aven already reach this workspace through the Rune
//! connector (`notion2 --mcp`), so what a chat needs is to be told which
//! page: the note as text, led by a line that names the live page, so the AI
//! can also read or change the page itself.
//!
//! How it gets there depends on the Aven that is installed. An Aven that
//! opens Markdown files (it says so in its Info.plist) is handed the note as
//! a file, and puts it in the chat's message box. An older Aven has no way in
//! from outside, so the note goes to the clipboard and Aven comes to the
//! front, ready for a paste.

use std::path::{Path, PathBuf};

/// A test never opens another app.
fn testing() -> bool {
    cfg!(test) || std::env::var_os("NOTION2_SELFTEST").is_some()
}

/// The installed Aven, if there is one. (A test run acts as if there were,
/// and never opens it.)
pub fn app() -> Option<PathBuf> {
    if testing() {
        return Some(PathBuf::from("/Applications/Aven.app"));
    }
    let home = dirs::home_dir().map(|h| h.join("Applications/Aven.app"));
    [Some(PathBuf::from("/Applications/Aven.app")), home].into_iter().flatten().find(|p| p.join("Contents/Info.plist").is_file())
}

/// Whether an app's Info.plist says the app opens Markdown or plain-text
/// files. (Works on both forms of the file: the names are plain letters in
/// each.)
fn declares_notes(info_plist: &[u8]) -> bool {
    let has = |word: &str| info_plist.windows(word.len()).any(|w| w == word.as_bytes());
    has("CFBundleDocumentTypes") && (has("net.daringfireball.markdown") || has("public.plain-text") || has("public.text"))
}

/// Whether this Aven takes a note handed to it as a file.
pub fn opens_notes(app: &Path) -> bool {
    !testing() && std::fs::read(app.join("Contents/Info.plist")).is_ok_and(|bytes| declares_notes(&bytes))
}

/// What the page menu calls the action, for the Aven that is installed.
pub fn label(app: &Path) -> &'static str {
    if opens_notes(app) {
        "Send to Aven"
    } else {
        "Copy for Aven"
    }
}

/// The note as a chat gets it: a line that names the live page, then the
/// page as Markdown.
pub fn note_for_chat(title: &str, page_id: &str, markdown: &str) -> String {
    let title = if title.trim().is_empty() { "Untitled" } else { title.trim() };
    format!(
        "From my Rune note “{title}” (page id {page_id}; the rune tools can read and change the live page):\n\n{}\n",
        markdown.trim_end()
    )
}

/// Where notes handed to Aven are kept until its chat has read them.
fn outbox() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("Notion 2.0").join("Sent to Aven"))
}

/// Write the note where Aven can open it. Notes sent more than a week ago
/// are cleared away.
fn write_note(title: &str, text: &str) -> Result<PathBuf, String> {
    let folder = outbox().ok_or("Couldn't find a folder for the note.")?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let week = std::time::Duration::from_secs(7 * 24 * 3600);
    for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age > week);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let file = folder.join(format!("{}.md", crate::transfer::file_name(title)));
    std::fs::write(&file, text).map_err(|e| e.to_string())?;
    Ok(file)
}

/// How a note reached Aven.
#[derive(Debug, PartialEq)]
pub enum Sent {
    /// Aven was handed the note as a file.
    Handed,
    /// The note is on the clipboard and Aven is in front.
    Copied,
}

/// Give a note to Aven. `copy` puts text on the clipboard.
pub fn send(app: &Path, title: &str, text: &str, copy: impl FnOnce(String) -> Result<(), String>) -> Result<Sent, String> {
    if opens_notes(app) {
        let file = write_note(title, text)?;
        let status = std::process::Command::new("open").arg("-a").arg(app).arg(&file).status().map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(Sent::Handed);
        }
    }
    copy(text.to_string())?;
    if !testing() {
        let _ = std::process::Command::new("open").arg("-a").arg(app).spawn();
    }
    Ok(Sent::Copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_names_the_live_page_and_then_holds_the_page() {
        let text = note_for_chat("Acme", "abc123", "# Acme\n\n- [ ] Call Sam\n\n");
        assert_eq!(text, "From my Rune note “Acme” (page id abc123; the rune tools can read and change the live page):\n\n# Acme\n\n- [ ] Call Sam\n");
        assert!(note_for_chat("  ", "x", "").starts_with("From my Rune note “Untitled”"));
    }

    #[test]
    fn an_aven_that_opens_text_files_is_told_apart() {
        // Today's Aven: no document types at all.
        assert!(!declares_notes(b"<key>CFBundleExecutable</key><string>aven</string>"));
        // Document types, but not for text.
        assert!(!declares_notes(b"<key>CFBundleDocumentTypes</key><array><dict><string>public.image</string></dict></array>"));
        assert!(declares_notes(b"<key>CFBundleDocumentTypes</key><array><dict><key>LSItemContentTypes</key><array><string>net.daringfireball.markdown</string></array></dict></array>"));
        // The compact form of the file keeps the names as plain letters too.
        assert!(declares_notes(b"bplist00\xd1\x01\x02_\x10\x15CFBundleDocumentTypes\xa1\x03_\x10\x11public.plain-text"));
    }

    #[test]
    fn an_older_aven_gets_the_note_on_the_clipboard() {
        let mut copied = String::new();
        let sent = send(Path::new("/nonexistent/Aven.app"), "T", "the note", |text| {
            copied = text;
            Ok(())
        });
        assert_eq!(sent, Ok(Sent::Copied));
        assert_eq!(copied, "the note");
    }
}
