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
    /// After an end (of the call, or of one program's part in a call that
    /// goes on), that program plays sound again with no microphone (the
    /// person is muted): the meeting may be going on. It is never a reason
    /// to make notes of a new meeting; the app says with `Watch::not_back`
    /// when it is not the meeting that ended.
    Back(&'static str),
    /// During a call, another call program took the microphone (a meeting
    /// in Teams joined while a voice channel of Discord is still on). The
    /// app decides if it is a call of its own, with notes of its own.
    Joined(&'static str),
    /// During a call in several programs, one of them has been silent for
    /// as long as an end takes while another goes on: its part is over.
    /// (A meeting joined during a voice channel that is on all day had no
    /// end of its own: its notes went on, and the app listened, until the
    /// voice channel ended.) The app decides if notes end with it.
    Left(&'static str),
    Ended,
}

/// A program whose call is over (see `Watch::gone`).
struct Gone {
    app: &'static str,
    /// Looks since then.
    since: u32,
    /// Looks in a row, now, with sound in it.
    back: u32,
}

/// Turns a stream of looks into "a meeting started" and "it ended".
#[derive(Default)]
pub struct Watch {
    /// The program that may be starting a meeting.
    app: Option<&'static str>,
    in_meeting: bool,
    /// Looks in a row with the microphone in that program.
    streak: u32,
    /// The programs the call is in now, each with the looks in a row in
    /// which it used no audio. A call can pass from one to another (a
    /// browser, then the call program), and a second call can be joined
    /// while the first goes on.
    apps: Vec<(&'static str, u32)>,
    /// The programs whose call ended in the last hour: the whole call, or
    /// their part of one that goes on (see `Change::Back`).
    gone: Vec<Gone>,
    /// A program that took the microphone during the call, and for how many
    /// looks in a row (see `Change::Joined`).
    joining: Option<(&'static str, u32)>,
}

impl Watch {
    /// The app opens, and the last run did not see the end of the calls in
    /// these programs (the app was closed during them; an update does
    /// that). As after an end, the sound of such a program alone can say
    /// that its call goes on.
    pub fn after(apps: &[&'static str]) -> Watch {
        let mut watch = Watch::default();
        for app in apps {
            watch.leave(app);
        }
        watch
    }

    /// The call in this program is over: for an hour, its sound alone can
    /// say that it goes on. (Not for a browser: it plays sound for many
    /// other reasons.)
    fn leave(&mut self, app: &'static str) {
        self.gone.retain(|g| g.app != app);
        if !call_app_named(app).is_some_and(|a| a.browser) {
            self.gone.push(Gone { app, since: 0, back: 0 });
        }
    }

    pub fn step(&mut self, heard: &[Heard]) -> Option<Change> {
        // The programs that use the microphone (a browser must also play sound).
        let mut talking = heard.iter().filter(|h| h.input && (!h.app.browser || h.output)).map(|h| h.app.name);
        // A call ended a while ago, and its program plays sound again. A
        // person who is muted gives no other sign that the call goes on.
        for g in &mut self.gone {
            g.since += 1;
            g.back = if heard.iter().any(|h| h.output && h.app.name == g.app) { g.back + 1 } else { 0 };
        }
        self.gone.retain(|g| g.since <= BACK_WITHIN);
        if self.in_meeting {
            // Another program that would start a meeting: the first with
            // the microphone that is not in the call yet. (Only the first
            // program with the microphone was looked at: with the call's
            // own program before it in the list, the other was never seen.)
            let newcomer = talking.find(|name| !self.apps.iter().any(|(a, _)| a == name));
            self.joining = match (newcomer, self.joining) {
                (Some(name), Some((who, looks))) if who == name => Some((who, looks + 1)),
                (Some(name), _) => Some((name, 1)),
                (None, _) => None,
            };
            for (name, quiet) in &mut self.apps {
                let used = heard.iter().any(|h| h.app.name == *name && (h.input || h.output));
                *quiet = if used { 0 } else { *quiet + 1 };
            }
            // As for a start: the microphone for some looks in a row. A
            // browser the call was in before is left behind then (it plays
            // sound for many other reasons).
            if let Some((who, _)) = self.joining.filter(|(_, looks)| *looks >= START_AFTER) {
                self.joining = None;
                self.apps.retain(|(a, _)| !call_app_named(a).is_some_and(|app| app.browser));
                self.apps.push((who, 0));
                self.gone.retain(|g| g.app != who);
                return Some(Change::Joined(who));
            }
            // Any program of the call keeps it going, with its microphone
            // or its sound (and so does one that is taking the microphone).
            let over = |quiet: &u32| *quiet >= END_AFTER;
            if self.apps.iter().all(|(_, quiet)| over(quiet)) {
                if self.joining.is_some() {
                    return None;
                }
                for (name, _) in std::mem::take(&mut self.apps) {
                    self.leave(name);
                }
                self.in_meeting = false;
                self.streak = 0;
                self.app = None;
                return Some(Change::Ended);
            }
            // A program that left this call, or ended a while ago, plays
            // sound again: the app says if that is its meeting going on.
            if let Some(i) = self.gone.iter().position(|g| g.back >= BACK_AFTER) {
                let name = self.gone.remove(i).app;
                self.apps.push((name, 0));
                return Some(Change::Back(name));
            }
            // One program of the call is silent while another goes on: its
            // part is over, and it can join again later.
            let i = self.apps.iter().position(|(_, quiet)| over(quiet))?;
            let name = self.apps.remove(i).0;
            self.leave(name);
            return Some(Change::Left(name));
        }
        let starter = talking.next();
        self.streak = match starter {
            Some(name) if self.app == Some(name) => self.streak + 1,
            Some(_) => 1,
            None => 0,
        };
        self.app = starter;
        if self.streak >= START_AFTER {
            self.in_meeting = true;
            self.streak = 0;
            self.apps = starter.map(|name| (name, 0)).into_iter().collect();
            self.gone.retain(|g| Some(g.app) != starter);
            return starter.map(Change::Started);
        }
        let i = self.gone.iter().position(|g| g.back >= BACK_AFTER)?;
        let name = self.gone.remove(i).app;
        self.in_meeting = true;
        self.streak = 0;
        self.apps = vec![(name, 0)];
        Some(Change::Back(name))
    }

    pub fn in_meeting(&self) -> bool {
        self.in_meeting
    }

    /// The programs the call is in now.
    pub fn apps(&self) -> Vec<&'static str> {
        self.apps.iter().map(|(name, _)| *name).collect()
    }

    /// The app's answer to `Change::Back`: that sound was not a meeting
    /// going on (it ended too long ago, or another one has started since).
    /// The program is out of the call, and its sound says no more; a call
    /// in other programs goes on.
    pub fn not_back(&mut self, app: &str) {
        self.apps.retain(|(a, _)| *a != app);
        if self.apps.is_empty() {
            self.in_meeting = false;
            self.streak = 0;
        }
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
        w.not_back(TEAMS.name);
        for _ in 0..10 {
            assert_eq!(w.step(&at(TEAMS, false, true)), None);
        }
        assert_eq!(w.step(&at(TEAMS, true, true)), None);
        assert_eq!(w.step(&at(TEAMS, true, true)), Some(Change::Started("Microsoft Teams")));
        // The app was closed and opened again during a call in Teams: the
        // sound of Teams is enough (the person is muted), another program's is not.
        let mut w = Watch::after(&[TEAMS.name]);
        for _ in 0..5 {
            assert_eq!(w.step(&at(zoom, false, true)), None);
        }
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), None);
        assert_eq!(w.step(&at(TEAMS, false, true)), Some(Change::Back("Microsoft Teams")));
        assert!(w.in_meeting());
        // …and the microphone starts it as always.
        let mut w = Watch::after(&[TEAMS.name]);
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

    /// A long call (the person is muted in it: sound, no microphone), and a
    /// meeting in Teams joined during it.
    fn joined_during_a_long_call() -> (Watch, Vec<Heard>, Vec<Heard>) {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let long_call = vec![Heard { app: zoom, input: false, output: true }];
        let both = vec![Heard { app: zoom, input: false, output: true }, Heard { app: TEAMS, input: true, output: true }];
        let mut w = Watch::default();
        w.step(&[Heard { app: zoom, input: true, output: true }]);
        assert_eq!(w.step(&[Heard { app: zoom, input: true, output: true }]), Some(Change::Started("Zoom")));
        assert_eq!(w.step(&both), None);
        assert_eq!(w.step(&both), Some(Change::Joined("Microsoft Teams")));
        for _ in 0..20 {
            assert_eq!(w.step(&both), None);
        }
        (w, long_call, both)
    }

    #[test]
    fn a_meeting_joined_during_another_call_has_an_end_of_its_own() {
        let (mut w, long_call, both) = joined_during_a_long_call();
        // The meeting is left; the long call plays on. Half a minute later
        // the meeting is over, once, and the long call is still a call.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&long_call), None);
        }
        assert_eq!(w.step(&long_call), Some(Change::Left("Microsoft Teams")));
        assert_eq!(w.apps(), vec!["Zoom"]);
        for _ in 0..100 {
            assert_eq!(w.step(&long_call), None);
        }
        assert!(w.in_meeting());
        // The next meeting in the same program, later in the same long call, is seen again.
        assert_eq!(w.step(&both), None);
        assert_eq!(w.step(&both), Some(Change::Joined("Microsoft Teams")));
        assert_eq!(w.apps(), vec!["Zoom", "Microsoft Teams"]);
        // Both end at the same moment: one end, of the call.
        let mut said = Vec::new();
        for _ in 0..(END_AFTER + 5) {
            said.extend(w.step(&[]));
        }
        assert_eq!(said, vec![Change::Ended]);
        assert!(!w.in_meeting());
    }

    #[test]
    fn the_first_call_can_end_while_the_joined_meeting_goes_on() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let (mut w, _, _) = joined_during_a_long_call();
        let meeting = vec![Heard { app: TEAMS, input: false, output: true }];
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&meeting), None);
        }
        assert_eq!(w.step(&meeting), Some(Change::Left("Zoom")));
        for _ in 0..40 {
            assert_eq!(w.step(&meeting), None);
        }
        // The meeting is the call now: its end is the end.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&[]), None);
        }
        assert_eq!(w.step(&[]), Some(Change::Ended));
        // A long call that ends five looks before the meeting: its end, then the end.
        let (mut w, _, _) = joined_during_a_long_call();
        let mut said = Vec::new();
        for _ in 0..5 {
            said.extend(w.step(&meeting));
        }
        for _ in 0..END_AFTER {
            said.extend(w.step(&[]));
        }
        assert_eq!(said, vec![Change::Left("Zoom"), Change::Ended]);
        // A program that only plays sound does not join a call it was never in.
        let mut w = Watch::default();
        w.step(&meeting_with_microphone());
        assert_eq!(w.step(&meeting_with_microphone()), Some(Change::Started("Microsoft Teams")));
        for _ in 0..20 {
            assert_eq!(w.step(&[Heard { app: TEAMS, input: false, output: true }, Heard { app: zoom, input: false, output: true }]), None);
        }
        assert_eq!(w.apps(), vec!["Microsoft Teams"]);
    }

    fn meeting_with_microphone() -> Vec<Heard> {
        vec![Heard { app: TEAMS, input: true, output: true }]
    }

    #[test]
    fn a_second_program_is_seen_whatever_the_order_of_the_list() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        // The first call has its microphone open too.
        let first = Heard { app: zoom, input: true, output: true };
        let second = Heard { app: TEAMS, input: true, output: true };
        for both in [[first, second], [second, first]] {
            let mut w = Watch::default();
            w.step(&[first]);
            assert_eq!(w.step(&[first]), Some(Change::Started("Zoom")));
            assert_eq!(w.step(&both), None);
            assert_eq!(w.step(&both), Some(Change::Joined("Microsoft Teams")));
            assert_eq!(w.step(&both), None, "only once");
        }
    }

    #[test]
    fn the_sound_of_a_meeting_that_left_a_call_says_that_it_goes_on() {
        let (mut w, long_call, _) = joined_during_a_long_call();
        let muted = vec![long_call[0], Heard { app: TEAMS, input: false, output: true }];
        // Nobody speaks in the meeting for half a minute (the person is muted): it left the call.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&long_call), None);
        }
        assert_eq!(w.step(&long_call), Some(Change::Left("Microsoft Teams")));
        // The others speak again: as after an end, the app is asked.
        assert_eq!(w.step(&muted), None);
        assert_eq!(w.step(&muted), None);
        assert_eq!(w.step(&muted), Some(Change::Back("Microsoft Teams")));
        assert_eq!(w.apps(), vec!["Zoom", "Microsoft Teams"]);
        // It is in the call again, and has its end again.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&long_call), None);
        }
        assert_eq!(w.step(&long_call), Some(Change::Left("Microsoft Teams")));
        // The app says that the sound was not that meeting: the program is
        // out, its sound says no more, and the long call goes on.
        for _ in 0..2 {
            assert_eq!(w.step(&muted), None);
        }
        assert_eq!(w.step(&muted), Some(Change::Back("Microsoft Teams")));
        w.not_back(TEAMS.name);
        assert!(w.in_meeting());
        assert_eq!(w.apps(), vec!["Zoom"]);
        for _ in 0..20 {
            assert_eq!(w.step(&muted), None);
        }
        // Its microphone joins it as always.
        let both = vec![long_call[0], Heard { app: TEAMS, input: true, output: true }];
        assert_eq!(w.step(&both), None);
        assert_eq!(w.step(&both), Some(Change::Joined("Microsoft Teams")));
    }

    #[test]
    fn after_the_app_was_closed_each_program_that_was_in_a_call_can_say_that_it_goes_on() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        // A meeting with notes in Teams, during a long call that was refused.
        let mut w = Watch::after(&[TEAMS.name, zoom.name]);
        let long_call = Heard { app: zoom, input: false, output: true };
        let muted = Heard { app: TEAMS, input: false, output: true };
        assert_eq!(w.step(&[long_call]), None);
        assert_eq!(w.step(&[long_call]), None);
        assert_eq!(w.step(&[long_call]), Some(Change::Back("Zoom")));
        assert_eq!(w.step(&[long_call, muted]), None);
        assert_eq!(w.step(&[long_call, muted]), None);
        assert_eq!(w.step(&[long_call, muted]), Some(Change::Back("Microsoft Teams")));
        assert_eq!(w.apps(), vec!["Zoom", "Microsoft Teams"]);
        // The meeting's microphone says so too.
        let mut w = Watch::after(&[TEAMS.name, zoom.name]);
        for _ in 0..3 {
            w.step(&[long_call]);
        }
        let speaking = Heard { app: TEAMS, input: true, output: true };
        assert_eq!(w.step(&[long_call, speaking]), None);
        assert_eq!(w.step(&[long_call, speaking]), Some(Change::Joined("Microsoft Teams")));
    }

    #[test]
    fn a_meeting_in_the_browser_during_another_call_joins_and_leaves_it() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let chrome = call_app("/Applications/Google Chrome.app/x").unwrap();
        let long_call = Heard { app: zoom, input: false, output: true };
        let in_browser = Heard { app: chrome, input: true, output: true };
        let mut w = Watch::default();
        w.step(&[Heard { app: zoom, input: true, output: true }]);
        assert_eq!(w.step(&[Heard { app: zoom, input: true, output: true }]), Some(Change::Started("Zoom")));
        // A video in the browser is not a call; the microphone with sound is.
        for _ in 0..5 {
            assert_eq!(w.step(&[long_call, Heard { app: chrome, input: false, output: true }]), None);
        }
        assert_eq!(w.step(&[long_call, in_browser]), None);
        assert_eq!(w.step(&[long_call, in_browser]), Some(Change::Joined("Google Chrome")));
        assert_eq!(w.apps(), vec!["Zoom", "Google Chrome"]);
        // The browser is silent: its meeting is over, and the long call goes on.
        for _ in 0..(END_AFTER - 1) {
            assert_eq!(w.step(&[long_call]), None);
        }
        assert_eq!(w.step(&[long_call]), Some(Change::Left("Google Chrome")));
        // A browser's sound alone never says that a meeting goes on…
        for _ in 0..10 {
            assert_eq!(w.step(&[long_call, Heard { app: chrome, input: false, output: true }]), None);
        }
        // …its microphone does.
        assert_eq!(w.step(&[long_call, in_browser]), None);
        assert_eq!(w.step(&[long_call, in_browser]), Some(Change::Joined("Google Chrome")));
    }

    #[test]
    fn a_moment_of_microphone_in_another_program_does_not_take_the_call_from_the_browser() {
        let zoom = call_app("/Applications/zoom.us.app/x").unwrap();
        let chrome = call_app("/Applications/Google Chrome.app/x").unwrap();
        let call = Heard { app: chrome, input: false, output: true };
        let mut w = Watch::default();
        w.step(&[Heard { app: chrome, input: true, output: true }]);
        assert_eq!(w.step(&[Heard { app: chrome, input: true, output: true }]), Some(Change::Started("Google Chrome")));
        // One look with the microphone in a call program (a sound check):
        // it has not joined, and the call is still the browser's.
        assert_eq!(w.step(&[call, Heard { app: zoom, input: true, output: false }]), None);
        for _ in 0..(2 * END_AFTER) {
            assert_eq!(w.step(&[call]), None);
        }
        assert_eq!(w.apps(), vec!["Google Chrome"]);
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
