//! The calendars of this Mac: the events of the Calendar app (with the
//! iCloud, Google or Outlook accounts it holds), shown beside the app's own
//! events when the person turns that on in Settings. It is off until then.
//!
//! The reading is done by a small helper beside the app (helper/rune-cal.swift),
//! so macOS asks the person one time, in the app's name. The events are only
//! read: they are not kept in the notes file, not given to an AI tool, and
//! never changed. A notes page can be made for one of them; which page goes
//! with which event is all that is kept.

use crate::model::{Event, Id};
use serde::Deserialize;
use std::path::PathBuf;

/// The start of the id of an event that comes from the Mac's calendars.
pub const PREFIX: &str = "mac:";

/// Whether the events may be read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    /// Not turned on, or not looked at yet.
    #[default]
    Unknown,
    Allowed,
    /// macOS has not asked the person yet.
    NotAsked,
    /// The person, or a rule of the Mac, said no.
    Denied,
    /// No helper beside the app (a build made for tests).
    Unavailable,
}

/// One calendar of the Mac ("Work", "Home").
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MacCalendar {
    pub id: String,
    pub title: String,
    /// Its colour, as "#rrggbb" ("" when it has none we can show).
    #[serde(default)]
    pub color: String,
    /// The account it is from ("iCloud", "Google").
    #[serde(default)]
    pub source: String,
    /// Events can be made and changed in it (a calendar someone shares to
    /// look at, or one of birthdays, cannot).
    #[serde(default)]
    pub writable: bool,
}

/// One event of the Mac's calendars.
#[derive(Clone, Debug, PartialEq)]
pub struct MacEvent {
    /// `PREFIX`, then the id the helper gave.
    pub id: Id,
    pub calendar: String,
    pub title: String,
    pub start: i64,
    /// As the app keeps an end: for an all-day event, midnight after its last day.
    pub end: i64,
    pub all_day: bool,
    pub location: String,
    /// The address to join its meeting by, when it has one ("" if not).
    pub join: String,
}

impl MacEvent {
    /// The event as the calendar views draw one. It is not saved anywhere.
    pub fn as_event(&self, space: &str, color: &str, page: Option<Id>) -> Event {
        let mut e = Event::new(space, self.title.clone(), self.start, self.end, self.all_day);
        e.id = self.id.clone();
        e.color = color.to_string();
        // Its place, and under it the address to join by (the views look for it there).
        e.notes = [self.location.as_str(), self.join.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
        e.join = self.join.clone();
        e.page_id = page;
        e
    }
}

/// What one reading gave.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fetched {
    pub access: Access,
    pub calendars: Vec<MacCalendar>,
    pub events: Vec<MacEvent>,
}

#[derive(Deserialize)]
struct RawEvent {
    id: String,
    cal: String,
    #[serde(default)]
    title: String,
    start: i64,
    end: i64,
    #[serde(default, rename = "allDay")]
    all_day: bool,
    #[serde(default)]
    location: String,
    #[serde(default)]
    join: String,
}

#[derive(Deserialize)]
struct Reply {
    status: String,
    #[serde(default)]
    calendars: Vec<MacCalendar>,
    #[serde(default)]
    events: Vec<RawEvent>,
}

/// A colour of a calendar as the page may use it: "#rrggbb" and nothing else.
pub fn safe_color(color: &str) -> Option<&str> {
    let hex = color.strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(color)
}

/// Read the helper's one line of JSON.
pub fn read(json: &str) -> Fetched {
    let Ok(reply) = serde_json::from_str::<Reply>(json.trim()) else { return Fetched { access: Access::Unavailable, ..Fetched::default() } };
    let access = match reply.status.as_str() {
        "allowed" => Access::Allowed,
        "not-asked" => Access::NotAsked,
        "denied" | "restricted" => Access::Denied,
        _ => Access::Unavailable,
    };
    let calendars = reply.calendars.into_iter().map(|c| MacCalendar { color: safe_color(&c.color).unwrap_or("").to_string(), ..c }).collect();
    let mut events: Vec<MacEvent> = reply
        .events
        .into_iter()
        .filter(|e| e.end >= e.start)
        .map(|e| {
            // The Mac ends an all-day event a second before midnight of its
            // last day; the app ends one at that midnight.
            let end = if e.all_day { crate::when::midnight(crate::when::next_day(crate::when::date_of(e.end - 1))) } else { e.end.max(e.start + 1) };
            MacEvent {
                id: format!("{PREFIX}{}", e.id),
                calendar: e.cal,
                title: if e.title.trim().is_empty() { "Busy".to_string() } else { e.title },
                start: if e.all_day { crate::when::midnight(crate::when::date_of(e.start)) } else { e.start },
                end,
                all_day: e.all_day,
                location: e.location.trim().to_string(),
                // Only an address that is a meeting's is kept.
                join: crate::meeting::join_link(&e.join).unwrap_or_default(),
            }
        })
        .collect();
    events.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
    Fetched { access, calendars, events }
}

/// The helper: beside the app's own program, in the installed app.
fn helper() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("rune-cal");
    beside.is_file().then_some(beside)
}

/// Whether this copy of the app can read the Mac's calendars at all.
pub fn available() -> bool {
    in_a_test() || helper().is_some()
}

fn in_a_test() -> bool {
    std::env::var_os("RUNE_SELFTEST").is_some()
}

/// What a test run reads in place of the Mac's calendars: two calendars and
/// three events (today at 3 PM, all day today, tomorrow at 10 AM). Their ids
/// are made as the helper makes them: the event's own id, then its start.
fn stand_in() -> String {
    let today = crate::when::today();
    let at = |day, h: u32| crate::when::at(day, chrono::NaiveTime::from_hms_opt(h, 0, 0).unwrap_or_default());
    let tomorrow = crate::when::next_day(today);
    let id = |own: &str, start: i64| format!("{own}@{}", start / 1000);
    serde_json::json!({
        "status": "allowed",
        "calendars": [
            { "id": "cal-work", "title": "Work", "color": "#b9a07f", "source": "Google", "writable": true },
            { "id": "cal-home", "title": "Home", "color": "#6f8f8a", "source": "iCloud", "writable": true },
        ],
        "events": [
            { "id": id("ev-budget", at(today, 15)), "cal": "cal-work", "title": "Budget review", "start": at(today, 15), "end": at(today, 16), "allDay": false, "location": "Room 2", "join": "https://teams.microsoft.com/l/meetup-join/stand-in" },
            { "id": id("ev-leave", crate::when::midnight(today)), "cal": "cal-home", "title": "Sam on leave", "start": crate::when::midnight(today), "end": crate::when::midnight(tomorrow) - 1000, "allDay": true },
            { "id": id("ev-sync", at(tomorrow, 10)), "cal": "cal-home", "title": "Garden plan", "start": at(tomorrow, 10), "end": at(tomorrow, 11), "allDay": false },
        ],
    })
    .to_string()
}

/// Read the events between two moments (milliseconds). With `ask`, macOS
/// asks the person first when it has not before (only when they turned the
/// option on just now).
pub async fn fetch(from: i64, to: i64, ask: bool) -> Fetched {
    if in_a_test() {
        return read(&stand_in());
    }
    let Some(helper) = helper() else { return Fetched { access: Access::Unavailable, ..Fetched::default() } };
    let mut command = tokio::process::Command::new(helper);
    command.arg("events").arg((from / 1000).to_string()).arg((to / 1000).to_string());
    if ask {
        command.arg("--ask");
    }
    match command.output().await {
        Ok(out) => read(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Fetched { access: Access::Unavailable, ..Fetched::default() },
    }
}

#[derive(Deserialize)]
struct Done {
    status: String,
    #[serde(default)]
    problem: String,
}

/// Run one of the helper's commands that change a calendar. A test run
/// changes nothing outside the app, and says it went well.
async fn change(args: Vec<String>) -> Result<(), String> {
    if in_a_test() {
        return Ok(());
    }
    let helper = helper().ok_or("Only the installed app can change your calendars.")?;
    let out = tokio::process::Command::new(helper).args(&args).output().await.map_err(|e| format!("The calendar could not be changed. ({e})"))?;
    match serde_json::from_str::<Done>(String::from_utf8_lossy(&out.stdout).trim()) {
        Ok(done) if done.status == "ok" => Ok(()),
        Ok(done) if !done.problem.is_empty() => Err(done.problem),
        _ => Err("The calendar could not be changed.".to_string()),
    }
}

/// Give one event of the Mac a new start and end. `id` is the event's id in
/// the app (with `PREFIX`).
pub async fn move_event(id: &str, start: i64, end: i64) -> Result<(), String> {
    let id = id.strip_prefix(PREFIX).unwrap_or(id).to_string();
    change(vec!["move".into(), id, (start / 1000).to_string(), (end / 1000).to_string()]).await
}

/// Make a new event in a calendar of the Mac.
pub async fn add_event(calendar: &str, title: &str, start: i64, end: i64, all_day: bool) -> Result<(), String> {
    change(vec!["add".into(), calendar.to_string(), (start / 1000).to_string(), (end / 1000).to_string(), u8::from(all_day).to_string(), title.to_string()]).await
}

/// The id an event of the Mac has after it moved to a new start: the helper
/// makes the id from the event's own id and its start in seconds (a repeat
/// has the id of its series, and the start says which one it is).
pub fn moved_id(id: &str, start: i64) -> Id {
    match id.rsplit_once('@') {
        Some((key, _)) => format!("{key}@{}", start / 1000),
        None => id.to_string(),
    }
}

/// Whether this run is a test (it reads stand-in calendars, and writes none).
pub fn stand_in_run() -> bool {
    in_a_test()
}

/// Open the part of System Settings where the person lets an app read the calendars.
pub fn open_settings() {
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars").spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moved_event_has_the_id_of_its_new_start() {
        assert_eq!(moved_id("mac:ABC-1@1794000000", 1_794_003_600_000), "mac:ABC-1@1794003600");
        // (An id of the event with an "@" in it: only the start at its end changes.)
        assert_eq!(moved_id("mac:a@b.example@1794000000", 1_794_003_600_000), "mac:a@b.example@1794003600");
        assert_eq!(moved_id("mac:new-1", 1_794_003_600_000), "mac:new-1");
    }

    #[test]
    fn the_helper_s_line_is_read_and_made_safe() {
        let got = read(
            r##"{"status":"allowed","calendars":[{"id":"c1","title":"Work","color":"#B9A07F","source":"Google"},{"id":"c2","title":"Odd","color":"red;background:url(x)"}],
               "events":[{"id":"b@2","cal":"c1","title":"Later","start":2000000,"end":2600000,"allDay":false,"location":" Room 2 "},
                         {"id":"a@1","cal":"c1","title":"  ","start":1000000,"end":1000000},
                         {"id":"bad","cal":"c1","title":"Ends before it starts","start":5,"end":1}]}"##,
        );
        assert_eq!(got.access, Access::Allowed);
        assert_eq!(got.calendars[0].color, "#B9A07F");
        assert_eq!(got.calendars[1].color, "", "a colour that is not a colour is not passed on");
        // In order of time; ids marked as the Mac's; an event with no name is "Busy"; one with no length gets one.
        assert_eq!(got.events.iter().map(|e| (e.id.as_str(), e.title.as_str())).collect::<Vec<_>>(), [("mac:a@1", "Busy"), ("mac:b@2", "Later")]);
        assert!(got.events[0].end > got.events[0].start);
        assert_eq!(got.events[1].location, "Room 2");
        assert_eq!(read(r#"{"status":"denied"}"#).access, Access::Denied);
        assert_eq!(read(r#"{"status":"not-asked"}"#).access, Access::NotAsked);
        assert_eq!(read("not json").access, Access::Unavailable);
        assert_eq!(safe_color("#12abEF"), Some("#12abEF"));
        assert_eq!(safe_color("#12abE"), None);
        assert_eq!(safe_color("blue"), None);
    }

    #[test]
    fn an_all_day_event_of_the_mac_ends_at_midnight_after_its_last_day() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let start = crate::when::midnight(day);
        let next = crate::when::midnight(crate::when::next_day(day));
        let json = serde_json::json!({ "status": "allowed", "events": [{ "id": "x", "cal": "c", "title": "Leave", "start": start, "end": next - 1000, "allDay": true }] }).to_string();
        let got = read(&json);
        assert_eq!((got.events[0].start, got.events[0].end, got.events[0].all_day), (start, next, true));
        let shown = got.events[0].as_event("home", "#b9a07f", Some("page-1".into()));
        assert_eq!((shown.id.as_str(), shown.space_id.as_str(), shown.color.as_str(), shown.page_id.as_deref()), ("mac:x", "home", "#b9a07f", Some("page-1")));
    }
}
