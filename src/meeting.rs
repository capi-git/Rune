//! Notices when a Microsoft Teams meeting (or call) starts and ends, so the
//! app can make a dated notes page for it.
//!
//! macOS knows which programs are using the microphone and the speakers
//! (CoreAudio's list of audio processes, macOS 14.2 and later). A Teams
//! process that starts using the microphone is a meeting starting; when Teams
//! uses neither for a while, it has ended. Nothing is recorded or listened
//! to, and no permission is needed: the app only asks *whether* audio is in
//! use, and by which program.
//!
//! `notion2 --meeting-probe` prints what this sees, for checking it during a
//! real meeting.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

/// A program that is using audio right now.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioUser {
    pub pid: i32,
    /// Where the program is on disk ("" if it can't be read).
    pub path: String,
    /// Using a microphone.
    pub input: bool,
    /// Playing sound.
    pub output: bool,
}

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

const fn code(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

// CoreAudio (AudioHardware.h).
const SYSTEM_OBJECT: u32 = 1;
const PROCESS_LIST: u32 = code(b"prs#");
const PROCESS_PID: u32 = code(b"ppid");
const PROCESS_RUNS_INPUT: u32 = code(b"piri");
const PROCESS_RUNS_OUTPUT: u32 = code(b"piro");
const SCOPE_GLOBAL: u32 = code(b"glob");

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(object: u32, address: *const Address, qualifier_size: u32, qualifier: *const c_void, size: *mut u32) -> i32;
    fn AudioObjectGetPropertyData(object: u32, address: *const Address, qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
}

extern "C" {
    /// libproc: the path of a running program.
    fn proc_pidpath(pid: i32, buffer: *mut c_void, size: u32) -> i32;
}

fn address(selector: u32) -> Address {
    Address { selector, scope: SCOPE_GLOBAL, element: 0 }
}

/// One 32-bit property of an audio object.
fn number(object: u32, selector: u32) -> Option<u32> {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `value` is a valid 4-byte buffer and `size` says so; these
    // properties are 32-bit numbers.
    let status = unsafe {
        AudioObjectGetPropertyData(object, &address(selector), 0, std::ptr::null(), &mut size, (&mut value as *mut u32).cast())
    };
    (status == 0 && size as usize == std::mem::size_of::<u32>()).then_some(value)
}

fn path_of(pid: i32) -> String {
    let mut buffer = vec![0u8; 4096];
    // SAFETY: the buffer is `buffer.len()` bytes; proc_pidpath writes at most
    // that many and returns how many it wrote (0 or less on failure).
    let n = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buffer[..n as usize]).into_owned()
}

/// Every program using a microphone or playing sound right now. Empty on a
/// macOS too old to say.
pub fn audio_users() -> Vec<AudioUser> {
    let list = address(PROCESS_LIST);
    let mut size: u32 = 0;
    // SAFETY: asks only for the size of the list; `size` is a valid u32.
    if unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &list, 0, std::ptr::null(), &mut size) } != 0 || size == 0 {
        return Vec::new();
    }
    let mut objects = vec![0u32; size as usize / std::mem::size_of::<u32>()];
    let mut size = (objects.len() * std::mem::size_of::<u32>()) as u32;
    // SAFETY: `objects` holds `size` bytes, and the list is 32-bit object ids.
    if unsafe { AudioObjectGetPropertyData(SYSTEM_OBJECT, &list, 0, std::ptr::null(), &mut size, objects.as_mut_ptr().cast()) } != 0 {
        return Vec::new();
    }
    objects.truncate(size as usize / std::mem::size_of::<u32>());
    objects
        .into_iter()
        .filter_map(|object| {
            let input = number(object, PROCESS_RUNS_INPUT)? != 0;
            let output = number(object, PROCESS_RUNS_OUTPUT)? != 0;
            if !input && !output {
                return None;
            }
            let pid = number(object, PROCESS_PID)? as i32;
            Some(AudioUser { pid, path: path_of(pid), input, output })
        })
        .collect()
}

/// Whether a program at `path` is part of Microsoft Teams (the app itself or
/// one of its helpers; the new, classic and "work or school" apps).
pub fn is_teams(path: &str) -> bool {
    path.contains("/Microsoft Teams") || path.ends_with("/MSTeams")
}

/// Whether Teams is using a microphone, and whether it is playing sound.
pub fn teams_audio(users: &[AudioUser]) -> (bool, bool) {
    let teams = || users.iter().filter(|u| is_teams(&u.path));
    (teams().any(|u| u.input), teams().any(|u| u.output))
}

/// `teams_audio` now, off the calling thread; safe to await from the UI.
pub async fn poll() -> (bool, bool) {
    let (tx, rx) = futures_channel::oneshot::channel();
    crate::ai::runtime().spawn_blocking(move || {
        let _ = tx.send(teams_audio(&audio_users()));
    });
    rx.await.unwrap_or((false, false))
}

/// How often to look.
pub const POLL_MS: u64 = 3000;
/// Looks in a row with the microphone in use before a meeting has started
/// (so a moment's sound check doesn't count).
const START_AFTER: u32 = 2;
/// Looks in a row with no audio at all before it has ended (a muted
/// microphone alone doesn't end it: the meeting's sound is still playing).
const END_AFTER: u32 = 6;

/// How long after the last sound a meeting is said to have ended.
pub const END_LAG_MS: i64 = END_AFTER as i64 * POLL_MS as i64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Started,
    Ended,
}

/// Turns a stream of looks into "a meeting started" and "it ended".
#[derive(Default)]
pub struct Watch {
    in_meeting: bool,
    streak: u32,
}

impl Watch {
    pub fn step(&mut self, input: bool, output: bool) -> Option<Change> {
        if !self.in_meeting {
            self.streak = if input { self.streak + 1 } else { 0 };
            if self.streak >= START_AFTER {
                self.in_meeting = true;
                self.streak = 0;
                return Some(Change::Started);
            }
        } else {
            self.streak = if input || output { 0 } else { self.streak + 1 };
            if self.streak >= END_AFTER {
                self.in_meeting = false;
                self.streak = 0;
                return Some(Change::Ended);
            }
        }
        None
    }
}

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Whether a notes page is made when a meeting starts (Settings).
/// Whether a web address is one that a meeting is joined by: a meeting
/// service's own site, at a page of a meeting.
fn is_meeting_address(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let on = |site: &str| host == site || host.ends_with(&format!(".{site}"));
    let path = path.to_ascii_lowercase();
    if on("teams.microsoft.com") || on("teams.live.com") || on("teams.microsoft.us") {
        return path.starts_with("l/meetup-join") || path.starts_with("meet") || path.starts_with("l/meeting");
    }
    if on("zoom.us") || on("zoomgov.com") {
        return ["j/", "my/", "w/", "s/"].iter().any(|p| path.starts_with(p));
    }
    if host == "app.slack.com" {
        return path.starts_with("huddle");
    }
    ["meet.google.com", "webex.com", "whereby.com", "gotomeeting.com", "gotomeet.me", "facetime.apple.com", "meet.jit.si", "chime.aws", "bluejeans.com"]
        .iter()
        .any(|site| on(site))
        && !path.is_empty()
}

/// The address to join a meeting by, when the words of an event (its notes,
/// its place) hold one: the first web address of a meeting service. Mail
/// programs put such an address in pointed brackets or in a sentence, so it
/// is cut at the first sign that cannot be part of it.
pub fn join_link(text: &str) -> Option<String> {
    let mut from = 0;
    while let Some(found) = text[from..].find("https://") {
        let start = from + found;
        let rest = &text[start..];
        let len = rest.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | ')' | ']' | '}')).unwrap_or(rest.len());
        let url = rest[..len].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if is_meeting_address(url) {
            return Some(url.to_string());
        }
        from = start + "https://".len();
    }
    None
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    crate::store::set_setting("meeting_notes", if on { "auto" } else { "off" });
}

/// Read the setting once the workspace is open.
pub fn load_setting() {
    ENABLED.store(crate::store::setting("meeting_notes").as_deref() != Some("off"), Ordering::Relaxed);
}

/// `notion2 --meeting-probe`: what the watcher sees right now.
pub fn probe() -> String {
    let users = audio_users();
    let mut out = String::new();
    if users.is_empty() {
        out.push_str("No program is using audio right now.\n");
    }
    for u in &users {
        let what = match (u.input, u.output) {
            (true, true) => "microphone + sound",
            (true, false) => "microphone",
            _ => "sound",
        };
        out.push_str(&format!("{:>7}  {:<18}  {}{}\n", u.pid, what, u.path, if is_teams(&u.path) { "   <- Teams" } else { "" }));
    }
    let (input, output) = teams_audio(&users);
    out.push_str(&format!(
        "Teams is using the microphone: {}. Teams is playing sound: {}.\nA meeting counts as started after {} s of microphone use.\n",
        if input { "yes" } else { "no" },
        if output { "yes" } else { "no" },
        START_AFTER as u64 * POLL_MS / 1000
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_to_join_a_meeting_is_found_in_the_words_of_an_event() {
        let teams = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%7d";
        assert_eq!(join_link(&format!("Microsoft Teams meeting\nJoin: <{teams}>\nMeeting ID: 123")).as_deref(), Some(teams));
        assert_eq!(join_link("Call on https://us02web.zoom.us/j/8123456789?pwd=abc. See you there").as_deref(), Some("https://us02web.zoom.us/j/8123456789?pwd=abc"));
        assert_eq!(join_link("(https://meet.google.com/abc-defg-hij)").as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        // The first one that is a meeting's, not the first address.
        assert_eq!(join_link("Agenda: https://example.com/doc and https://acme.webex.com/meet/sam").as_deref(), Some("https://acme.webex.com/meet/sam"));
        // Not meetings: another site, a look-alike, a site of a service that is not a meeting, no lock.
        for text in ["https://example.com/j/123", "https://zoom.us.evil.example/j/1", "https://teams.microsoft.com/v2/", "https://zoom.us/pricing", "http://meet.google.com/abc", "Room 2", ""] {
            assert_eq!(join_link(text), None, "{text}");
        }
    }

    #[test]
    fn teams_programs_are_recognised() {
        assert!(is_teams("/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams"));
        assert!(is_teams("/Applications/Microsoft Teams.app/Contents/Helpers/Microsoft Teams WebView.app/Contents/Frameworks/Microsoft Edge Framework.framework/Versions/1/Helpers/Microsoft Teams WebView Helper.app/Contents/MacOS/Microsoft Teams WebView Helper"));
        assert!(is_teams("/Applications/Microsoft Teams classic.app/Contents/MacOS/Teams"));
        assert!(!is_teams("/Applications/zoom.us.app/Contents/MacOS/zoom.us"));
        assert!(!is_teams(""));
        let users = vec![
            AudioUser { pid: 1, path: "/Applications/Music.app/Contents/MacOS/Music".into(), input: false, output: true },
            AudioUser { pid: 2, path: "/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams".into(), input: true, output: false },
        ];
        assert_eq!(teams_audio(&users), (true, false));
        assert_eq!(teams_audio(&users[..1]), (false, false));
    }

    #[test]
    fn a_meeting_starts_and_ends_once() {
        let mut w = Watch::default();
        // A moment's microphone use (a sound check) is not a meeting.
        assert_eq!(w.step(true, false), None);
        assert_eq!(w.step(false, false), None);
        // Steady use is.
        assert_eq!(w.step(true, true), None);
        assert_eq!(w.step(true, true), Some(Change::Started));
        assert_eq!(w.step(true, true), None, "only once");
        // Muted (no microphone) while the others talk: still in it.
        for _ in 0..20 {
            assert_eq!(w.step(false, true), None);
        }
        // Silence for a while: over.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(false, false), None);
        }
        assert_eq!(w.step(false, false), Some(Change::Ended));
        assert_eq!(w.step(false, false), None);
        // Sound alone (a video in chat) does not start one.
        for _ in 0..10 {
            assert_eq!(w.step(false, true), None);
        }
    }

    #[test]
    fn asking_about_audio_does_not_fail() {
        // Whatever is playing on this Mac, the question itself must work.
        let users = audio_users();
        assert!(users.iter().all(|u| u.input || u.output));
        let _ = probe();
    }
}
