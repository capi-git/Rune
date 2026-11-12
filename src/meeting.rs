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

/// A program that people have calls in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CallApp {
    /// Its name, as the date line of the notes says it.
    pub name: &'static str,
    /// What the notes of a call in it are named with no calendar event.
    pub title: &'static str,
    /// A browser: it is in a call only while it uses the microphone and
    /// plays sound at the same time (voice typing alone is not a call).
    pub browser: bool,
}

const fn app(name: &'static str, title: &'static str) -> CallApp {
    CallApp { name, title, browser: false }
}

const fn browser(name: &'static str) -> CallApp {
    CallApp { name, title: "Meeting", browser: true }
}

pub const TEAMS: CallApp = app("Microsoft Teams", "Teams meeting");

/// The call programs, each by a part of where it is on disk. (Safari is not
/// here: the part of it that uses the microphone is shared by every program
/// that shows web pages, so it cannot be told apart.)
const CALL_APPS: &[(&str, CallApp)] = &[
    ("/zoom.us.app/", app("Zoom", "Zoom meeting")),
    ("/Webex.app/", app("Webex", "Webex meeting")),
    ("/Cisco Webex Meetings.app/", app("Webex", "Webex meeting")),
    ("/GoTo.app/", app("GoTo", "GoTo meeting")),
    ("/Slack.app/", app("Slack", "Slack huddle")),
    ("/Discord.app/", app("Discord", "Discord call")),
    ("/Discord PTB.app/", app("Discord", "Discord call")),
    ("/Discord Canary.app/", app("Discord", "Discord call")),
    ("/FaceTime.app/", app("FaceTime", "FaceTime call")),
    ("/avconferenced", app("FaceTime", "FaceTime call")),
    ("/Skype.app/", app("Skype", "Skype call")),
    ("/WhatsApp.app/", app("WhatsApp", "WhatsApp call")),
    ("/Signal.app/", app("Signal", "Signal call")),
    ("/Telegram.app/", app("Telegram", "Telegram call")),
    ("/Google Chrome.app/", browser("Google Chrome")),
    ("/Arc.app/", browser("Arc")),
    ("/Microsoft Edge.app/", browser("Microsoft Edge")),
    ("/Brave Browser.app/", browser("Brave")),
    ("/Firefox.app/", browser("Firefox")),
    ("/Vivaldi.app/", browser("Vivaldi")),
    ("/Opera.app/", browser("Opera")),
    ("/Chromium.app/", browser("Chromium")),
    ("/Dia.app/", browser("Dia")),
];

/// The call program a running program is part of, if it is one.
pub fn call_app(path: &str) -> Option<CallApp> {
    if is_teams(path) {
        return Some(TEAMS);
    }
    CALL_APPS.iter().find(|(part, _)| path.contains(part)).map(|(_, app)| *app)
}

/// The call program with this name (as `Change::Started` gives it).
pub fn call_app_named(name: &str) -> Option<CallApp> {
    if name == TEAMS.name {
        return Some(TEAMS);
    }
    CALL_APPS.iter().map(|(_, app)| *app).find(|app| app.name == name)
}

/// What one call program does with audio right now (all its parts together).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    pub app: CallApp,
    pub input: bool,
    pub output: bool,
}

/// The call programs that use audio now.
pub fn calls(users: &[AudioUser]) -> Vec<Heard> {
    let mut out: Vec<Heard> = Vec::new();
    for u in users {
        let Some(app) = call_app(&u.path) else { continue };
        match out.iter_mut().find(|h| h.app.name == app.name) {
            Some(h) => {
                h.input |= u.input;
                h.output |= u.output;
            }
            None => out.push(Heard { app, input: u.input, output: u.output }),
        }
    }
    out
}

/// `calls` now, off the calling thread; safe to await from the UI.
pub async fn poll() -> Vec<Heard> {
    let (tx, rx) = futures_channel::oneshot::channel();
    crate::ai::runtime().spawn_blocking(move || {
        let _ = tx.send(calls(&audio_users()));
    });
    rx.await.unwrap_or_default()
}

/// How often to look.
pub const POLL_MS: u64 = 3000;
/// Looks in a row with the microphone in use before a meeting has started
/// (so a moment's sound check doesn't count).
const START_AFTER: u32 = 2;
/// Looks in a row with no audio at all before it has ended (a muted
/// microphone alone doesn't end it: the meeting's sound is still playing).
/// Half a minute: a call that changes its sound device, joins again after a
/// bad line, or passes from the screen before the call to the call itself
/// is quiet for some seconds, and an end said too early stops the listening.
/// The price is that a true end is seen this much later (its time is still
/// written as the time of the last sound).
const END_AFTER: u32 = 10;
/// After an end: looks in a row with sound in the program the meeting was
/// in, before it is said to go on (one sound of a message is not a call).
const BACK_AFTER: u32 = 3;
/// After an end: for this many looks (an hour) that sound can say so.
const BACK_WITHIN: u32 = (60 * 60 * 1000 / POLL_MS) as u32;

/// How long after the last sound a meeting is said to have ended.
pub const END_LAG_MS: i64 = END_AFTER as i64 * POLL_MS as i64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// A call started, in the program with this name.
    Started(&'static str),
    /// After an end, the program the meeting was in plays sound again with
    /// no microphone (the person is muted): the meeting may be going on. It
    /// is never a reason to make notes of a new meeting; the app says with
    /// `Watch::not_back` when it is not the meeting that ended.
    Back(&'static str),
    /// During a call, another call program took the microphone (a meeting
    /// in Teams joined while a voice channel of Discord is still on). The
    /// call is one for the watcher; the app decides if the notes are.
    Joined(&'static str),
    Ended,
}

/// Turns a stream of looks into "a meeting started" and "it ended".
#[derive(Default)]
pub struct Watch {
    /// The program that may be starting a meeting.
    app: Option<&'static str>,
    in_meeting: bool,
    streak: u32,
    /// The programs the meeting is in, or was in when it ended. A call can
    /// pass from one to another (a browser, then the call program).
    apps: Vec<&'static str>,
    /// Looks since the meeting ended (None: none ended in the last hour).
    since_end: Option<u32>,
    /// Looks in a row, after the end, with sound in one of its programs.
    back: u32,
    /// A program that took the microphone during the call, and for how many
    /// looks in a row (see `Change::Joined`).
    joining: Option<(&'static str, u32)>,
}

impl Watch {
    /// The app opens, and the last run did not see its meeting end (the app
    /// was closed during the call; an update does that). As after an end,
    /// the sound of that meeting's program alone can say that it goes on.
    pub fn after(app: &'static str) -> Watch {
        Watch { apps: vec![app], since_end: Some(0), ..Watch::default() }
    }

    pub fn step(&mut self, heard: &[Heard]) -> Option<Change> {
        // A program that uses the microphone (a browser must also play sound).
        let starter = heard.iter().find(|h| h.input && (!h.app.browser || h.output)).map(|h| h.app.name);
        if self.in_meeting {
            // A program that would start a meeting is part of this one. A
            // browser the call was in before is left behind then (it plays
            // sound for many other reasons).
            let mut joined = None;
            if let Some(name) = starter {
                if !self.apps.contains(&name) {
                    self.apps.retain(|a| !call_app_named(a).is_some_and(|app| app.browser));
                    self.apps.push(name);
                    self.joining = Some((name, 1));
                } else if let Some((who, looks)) = self.joining.filter(|(who, _)| *who == name) {
                    // As for a start: the microphone for some looks in a row.
                    if looks + 1 >= START_AFTER {
                        self.joining = None;
                        joined = Some(who);
                    } else {
                        self.joining = Some((who, looks + 1));
                    }
                }
            } else {
                self.joining = None;
            }
            // Any program of the meeting keeps it going, with its microphone or its sound.
            let alive = heard.iter().any(|h| self.apps.contains(&h.app.name) && (h.input || h.output));
            self.streak = if alive { 0 } else { self.streak + 1 };
            if self.streak >= END_AFTER {
                self.in_meeting = false;
                self.streak = 0;
                self.app = None;
                self.since_end = Some(0);
                self.back = 0;
                self.joining = None;
                return Some(Change::Ended);
            }
            return joined.map(Change::Joined);
        }
        self.streak = match starter {
            Some(name) if self.app == Some(name) => self.streak + 1,
            Some(_) => 1,
            None => 0,
        };
        self.app = starter;
        if self.streak >= START_AFTER {
            self.in_meeting = true;
            self.streak = 0;
            self.apps = starter.into_iter().collect();
            self.since_end = None;
            self.back = 0;
            return starter.map(Change::Started);
        }
        // A meeting ended a while ago, and its program plays sound again. A
        // person who is muted gives no other sign that the call goes on. (Not
        // for a browser: it plays sound for many other reasons.)
        let since = self.since_end?;
        if since >= BACK_WITHIN {
            self.not_back();
            return None;
        }
        self.since_end = Some(since + 1);
        let sound = heard.iter().find(|h| h.output && !h.app.browser && self.apps.contains(&h.app.name)).map(|h| h.app.name);
        self.back = if sound.is_some() { self.back + 1 } else { 0 };
        if self.back >= BACK_AFTER {
            self.in_meeting = true;
            self.streak = 0;
            self.since_end = None;
            self.back = 0;
            return sound.map(Change::Back);
        }
        None
    }

    pub fn in_meeting(&self) -> bool {
        self.in_meeting
    }

    /// The app's answer to `Change::Back`: that sound was not the meeting
    /// going on (it ended too long ago, or another one has started since).
    pub fn not_back(&mut self) {
        self.in_meeting = false;
        self.streak = 0;
        self.apps.clear();
        self.since_end = None;
        self.back = 0;
    }
}

/// What the watcher said, with the time: a small file beside the workspace
/// (`meeting-watch.log`, the newest lines), to see afterwards why notes were
/// made or not. It holds times and the names of call programs, nothing else.
pub fn log(line: &str) {
    use std::io::Write;
    let Some(dir) = crate::store::db_path().and_then(|p| p.parent().map(|d| d.to_path_buf())) else { return };
    let path = dir.join("meeting-watch.log");
    // Kept small: when it has grown, the older half goes.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 64 * 1024) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let from = text.len() / 2;
            let from = text[from..].find('\n').map(|i| from + i + 1).unwrap_or(text.len());
            let _ = std::fs::write(&path, &text[from..]);
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{}  {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
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
        let known = call_app(&u.path).map(|a| format!("   <- {}", a.name)).unwrap_or_default();
        out.push_str(&format!("{:>7}  {:<18}  {}{known}\n", u.pid, what, u.path));
    }
    let heard = calls(&users);
    let teams = heard.iter().find(|h| h.app.name == TEAMS.name);
    let starting: Vec<&str> = heard.iter().filter(|h| h.input && (!h.app.browser || h.output)).map(|h| h.app.name).collect();
    out.push_str(&format!(
        "Teams is using the microphone: {}. Teams is playing sound: {}.\nA call program is in a call now: {}.\nA meeting counts as started after {} s of microphone use.\n",
        if teams.is_some_and(|h| h.input) { "yes" } else { "no" },
        if teams.is_some_and(|h| h.output) { "yes" } else { "no" },
        if starting.is_empty() { "no".to_string() } else { format!("yes ({})", starting.join(", ")) },
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
        let users = [
            AudioUser { pid: 1, path: "/Applications/zoom.us.app/Contents/MacOS/zoom.us".into(), input: false, output: true },
            AudioUser { pid: 2, path: "/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams".into(), input: true, output: false },
            AudioUser { pid: 3, path: "/Applications/Microsoft Teams.app/Contents/Helpers/Audio".into(), input: false, output: true },
            AudioUser { pid: 4, path: "/Applications/Dictate Me.app/Contents/MacOS/Dictate Me".into(), input: true, output: true },
        ];
        // The parts of one program are one program; a program that is not for calls is left out.
        assert_eq!(
            calls(&users),
            vec![Heard { app: call_app("/Applications/zoom.us.app/x").unwrap(), input: false, output: true }, Heard { app: TEAMS, input: true, output: true }]
        );
    }

    #[test]
    fn the_usual_call_programs_are_known() {
        let name = |path: &str| call_app(path).map(|a| (a.name, a.title, a.browser));
        assert_eq!(name("/Applications/zoom.us.app/Contents/MacOS/zoom.us"), Some(("Zoom", "Zoom meeting", false)));
        assert_eq!(name("/Applications/Slack.app/Contents/Frameworks/Slack Helper.app/Contents/MacOS/Slack Helper"), Some(("Slack", "Slack huddle", false)));
        assert_eq!(name("/System/Applications/FaceTime.app/Contents/MacOS/FaceTime"), Some(("FaceTime", "FaceTime call", false)));
        assert_eq!(name("/Applications/Google Chrome.app/Contents/Frameworks/x/Google Chrome Helper"), Some(("Google Chrome", "Meeting", true)));
        assert_eq!(name("/Applications/Microsoft Teams classic.app/Contents/MacOS/Teams"), Some(("Microsoft Teams", "Teams meeting", false)));
        // A tool that always has the microphone, and this app's own web view, are not call programs.
        assert_eq!(name("/Applications/Dictate Me.app/Contents/MacOS/Dictate Me"), None);
        assert_eq!(name("/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.GPU.xpc/Contents/MacOS/com.apple.WebKit.GPU"), None);
        assert_eq!(call_app_named("Zoom").map(|a| a.title), Some("Zoom meeting"));
        assert_eq!(call_app_named("Nothing"), None);
    }

    #[test]
    fn a_meeting_starts_and_ends_once() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let chrome = call_app("/Applications/Google Chrome.app/x").unwrap();
        let at = |app: CallApp, input: bool, output: bool| vec![Heard { app, input, output }];
        let mut w = Watch::default();
        // A moment's microphone use (a sound check) is not a meeting.
        assert_eq!(w.step(&at(TEAMS, true, false)), None);
        assert_eq!(w.step(&[]), None);
        // Steady use is.
        assert_eq!(w.step(&at(TEAMS, true, true)), None);
        assert_eq!(w.step(&at(TEAMS, true, true)), Some(Change::Started("Microsoft Teams")));
        assert_eq!(w.step(&at(TEAMS, true, true)), None, "only once");
        // Muted (no microphone) while the others talk: still in it.
        for _ in 0..20 {
            assert_eq!(w.step(&at(TEAMS, false, true)), None);
        }
        // Another program that plays sound does not keep it going: silence in Teams for a while, and it is over.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&at(chrome, false, true)), None);
        }
        assert_eq!(w.step(&at(chrome, false, true)), Some(Change::Ended));
        assert_eq!(w.step(&[]), None);
        // Sound alone (a video) in a program that was not in it does not start one.
        for _ in 0..10 {
            assert_eq!(w.step(&at(zoom, false, true)), None);
        }
        // Another call program starts one too, and it is named.
        assert_eq!(w.step(&at(zoom, true, false)), None);
        assert_eq!(w.step(&at(zoom, true, false)), Some(Change::Started("Zoom")));
    }

    #[test]
    fn a_call_that_goes_quiet_for_some_seconds_has_not_ended() {
        let at = |input: bool, output: bool| vec![Heard { app: TEAMS, input, output }];
        let mut w = Watch::default();
        w.step(&at(true, true));
        assert_eq!(w.step(&at(true, true)), Some(Change::Started("Microsoft Teams")));
        // 24 seconds with no sign (a change of the sound device, a bad line,
        // nobody speaks while the person is muted): still the meeting.
        for _ in 0..8 {
            assert_eq!(w.step(&[]), None);
        }
        assert_eq!(w.step(&at(false, true)), None);
        // And the count starts again after each sign.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&[]), None);
        }
        assert_eq!(w.step(&at(true, false)), None);
        // A true end: half a minute with nothing.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&[]), None);
        }
        assert_eq!(w.step(&[]), Some(Change::Ended));
        assert_eq!(END_LAG_MS, 30_000);
    }

    #[test]
    fn a_call_that_passes_to_another_program_is_one_meeting() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let chrome = call_app("/Applications/Google Chrome.app/x").unwrap();
        let at = |app: CallApp, input: bool, output: bool| vec![Heard { app, input, output }];
        let mut w = Watch::default();
        // Joined in the browser…
        w.step(&at(chrome, true, true));
        assert_eq!(w.step(&at(chrome, true, true)), Some(Change::Started("Google Chrome")));
        // …then the call opens in Zoom, and the browser is silent from then on.
        assert_eq!(w.step(&[]), None);
        assert_eq!(w.step(&at(zoom, true, true)), None, "no second start");
        // (The app is told that Zoom is in the call now, once.)
        assert_eq!(w.step(&at(zoom, true, true)), Some(Change::Joined("Zoom")));
        assert_eq!(w.step(&at(zoom, true, true)), None);
        // Muted in Zoom, for a long time: Zoom is a program of this meeting now.
        for _ in 0..40 {
            assert_eq!(w.step(&at(zoom, false, true)), None);
        }
        // The call ends, and the browser plays a video: the call left the
        // browser, so the video does not keep the meeting going.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&at(chrome, false, true)), None);
        }
        assert_eq!(w.step(&at(chrome, false, true)), Some(Change::Ended));
        // A program that only plays sound, and was never in the meeting, does not join it.
        let mut w = Watch::default();
        w.step(&at(TEAMS, true, true));
        assert_eq!(w.step(&at(TEAMS, true, true)), Some(Change::Started("Microsoft Teams")));
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&at(zoom, false, true)), None);
        }
        assert_eq!(w.step(&at(zoom, false, true)), Some(Change::Ended));
    }

    #[test]
    fn after_an_end_the_sound_of_the_meetings_program_says_that_it_goes_on() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let chrome = call_app("/Applications/Google Chrome.app/x").unwrap();
        let at = |app: CallApp, input: bool, output: bool| vec![Heard { app, input, output }];
        let ended = |app: CallApp| {
            let mut w = Watch::default();
            w.step(&at(app, true, true));
            assert!(matches!(w.step(&at(app, true, true)), Some(Change::Started(_))));
            for _ in 0..(END_AFTER - 1) {
                assert_eq!(w.step(&[]), None);
            }
            assert_eq!(w.step(&[]), Some(Change::Ended));
            w
        };
        // The person is muted and alone; then the others speak: sound, no microphone.
        let mut w = ended(TEAMS);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), Some(Change::Back("Microsoft Teams")));
        // It is in the meeting again: more sound says nothing new, silence ends it again.
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&[]), None);
        }
        assert_eq!(w.step(&[]), Some(Change::Ended));
        // The sound of one message is not a call.
        let mut w = ended(TEAMS);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&[]), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&[]), None);
        // Another program's sound is not this meeting, and a browser's sound never says so.
        let mut w = ended(TEAMS);
        for _ in 0..10 {
            assert_eq!(w.step(&at(zoom, false, true)), None);
        }
        let mut w = ended(chrome);
        for _ in 0..10 {
            assert_eq!(w.step(&at(chrome, false, true)), None);
        }
        // The microphone starts it as always (the app says if it is the same meeting).
        let mut w = ended(TEAMS);
        assert_eq!(w.step(&at(TEAMS, true, true)), None);
        assert_eq!(w.step(&at(TEAMS, true, true)), Some(Change::Started("Microsoft Teams")));
        // The app said that the sound was not the meeting: the watcher is out of it,
        // and that sound says no more.
        let mut w = ended(TEAMS);
        w.step(&at(TEAMS, false, true));
        w.step(&at(TEAMS, false, true));
        assert_eq!(w.step(&at(TEAMS, false, true)), Some(Change::Back("Microsoft Teams")));
        w.not_back();
        for _ in 0..10 {
            assert_eq!(w.step(&at(TEAMS, false, true)), None);
        }
        assert_eq!(w.step(&at(TEAMS, true, true)), None);
        assert_eq!(w.step(&at(TEAMS, true, true)), Some(Change::Started("Microsoft Teams")));
        // The app was closed and opened again during a call in Teams: the
        // sound of Teams is enough (the person is muted), another program's is not.
        let mut w = Watch::after(TEAMS.name);
        for _ in 0..5 {
            assert_eq!(w.step(&at(zoom, false, true)), None);
        }
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), Some(Change::Back("Microsoft Teams")));
        assert!(w.in_meeting());
        // …and the microphone starts it as always.
        let mut w = Watch::after(TEAMS.name);
        assert_eq!(w.step(&at(TEAMS, true, true)), None);
        assert_eq!(w.step(&at(TEAMS, true, true)), Some(Change::Started("Microsoft Teams")));
        // An hour after the end, sound alone says nothing.
        let mut w = ended(TEAMS);
        for _ in 0..BACK_WITHIN {
            assert_eq!(w.step(&[]), None);
        }
        for _ in 0..10 {
            assert_eq!(w.step(&at(TEAMS, false, true)), None);
        }
    }

    #[test]
    fn a_browser_is_in_a_call_only_with_the_microphone_and_sound_together() {
        let chrome = call_app("/Applications/Google Chrome.app/x").unwrap();
        let at = |input: bool, output: bool| vec![Heard { app: chrome, input, output }];
        let mut w = Watch::default();
        // Voice typing: the microphone alone, for a long time.
        for _ in 0..10 {
            assert_eq!(w.step(&at(true, false)), None);
        }
        // A call in the browser: the microphone and the others' voices.
        assert_eq!(w.step(&at(true, true)), None);
        assert_eq!(w.step(&at(true, true)), Some(Change::Started("Google Chrome")));
    }

    #[test]
    fn two_programs_one_after_the_other_do_not_add_up() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let mut w = Watch::default();
        assert_eq!(w.step(&[Heard { app: TEAMS, input: true, output: false }]), None);
        assert_eq!(w.step(&[Heard { app: zoom, input: true, output: false }]), None, "another program: the count starts again");
        assert_eq!(w.step(&[Heard { app: zoom, input: true, output: false }]), Some(Change::Started("Zoom")));
    }

    #[test]
    fn asking_about_audio_does_not_fail() {
        // Whatever is playing on this Mac, the question itself must work.
        let users = audio_users();
        assert!(users.iter().all(|u| u.input || u.output));
        let _ = probe();
    }
}
