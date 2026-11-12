//! What a cell of a date column says, in full: its day, a time of that day,
//! an end (a day, a time, or both) and a reminder. All of it is words in the
//! cell, so that a table read as Markdown (by a person or by the AI) says it:
//!
//! - `Tue 3 Nov`
//! - `Tue 3 Nov 2:30 PM`
//! - `Tue 3 Nov → Fri 6 Nov`
//! - `Tue 3 Nov 2:30 PM → 4 PM` (an end on the same day: its time only)
//! - `Tue 3 Nov 2:30 PM ⏰ 15 min before`, `Fri 6 Nov ⏰ 1 day before`
//!
//! A reminder of a day with no time comes at 9 AM (on the day, or days
//! before it), as Notion's does.

use super::{day, format_day, full_day, whole_day};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

/// Between the start and the end.
pub const TO: &str = "→";
/// Before the reminder.
pub const REMIND: &str = "⏰";

/// The hour of the reminder of a day with no time.
const REMIND_HOUR: u32 = 9;

/// The reminders of a day with a time: minutes before it, and their words.
pub const TIMED_REMINDERS: [(i64, &str); 9] = [
    (0, "at the time"),
    (5, "5 min before"),
    (10, "10 min before"),
    (15, "15 min before"),
    (30, "30 min before"),
    (60, "1 hour before"),
    (120, "2 hours before"),
    (1440, "1 day before"),
    (2880, "2 days before"),
];

/// The reminders of a day with no time: days before it (in minutes), at 9 AM.
pub const DAY_REMINDERS: [(i64, &str); 4] = [(0, "on the day"), (1440, "1 day before"), (2880, "2 days before"), (10080, "1 week before")];

/// A date cell, read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct When {
    pub start: NaiveDate,
    pub time: Option<NaiveTime>,
    pub end: Option<NaiveDate>,
    pub end_time: Option<NaiveTime>,
    /// Minutes before the start (a day with no time: before 9 AM of it).
    pub remind: Option<i64>,
}

/// "2:30 PM", "2 PM" on the hour.
pub fn time_words(t: NaiveTime) -> String {
    let (pm, h) = t.hour12();
    let half = if pm { "PM" } else { "AM" };
    if t.minute() == 0 {
        format!("{h} {half}")
    } else {
        format!("{h}:{:02} {half}", t.minute())
    }
}

/// A time of day typed: "2:30 PM", "2pm", "14:30", "9 am", "noon".
pub fn read_time(text: &str) -> Option<NaiveTime> {
    let low = text.trim().to_lowercase().replace('.', "");
    match low.as_str() {
        "noon" | "midday" => return NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        _ => {}
    }
    let (digits, half) = match low.strip_suffix("am").or_else(|| low.strip_suffix('a')) {
        Some(rest) => (rest.trim(), Some(false)),
        None => match low.strip_suffix("pm").or_else(|| low.strip_suffix('p')) {
            Some(rest) => (rest.trim(), Some(true)),
            None => (low.as_str(), None),
        },
    };
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        Some(_) => return None,
        // A bare number is an hour only with AM or PM ("3" alone is no time).
        None if half.is_some() => (digits.parse::<u32>().ok()?, 0),
        None => return None,
    };
    let h = match half {
        Some(pm) if (1..=12).contains(&h) => h % 12 + if pm { 12 } else { 0 },
        Some(_) => return None,
        None => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

/// The words of a part of a cell ("Tue 3 Nov 2:30 PM"), with its time taken
/// out of them: (the words of the day, the time).
fn split_time(part: &str) -> (String, Option<NaiveTime>) {
    let words: Vec<&str> = part.split_whitespace().collect();
    let mut rest = Vec::new();
    let mut time = None;
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next = words.get(i + 1).map(|n| n.to_lowercase());
        let half = next.as_deref().is_some_and(|n| matches!(n, "am" | "pm" | "a.m." | "p.m."));
        if time.is_none() && half {
            if let Some(t) = read_time(&format!("{w} {}", next.unwrap_or_default())) {
                time = Some(t);
                i += 2;
                continue;
            }
        }
        if time.is_none() && (w.contains(':') || w.to_lowercase().ends_with("am") || w.to_lowercase().ends_with("pm") || matches!(w.to_lowercase().as_str(), "noon" | "midnight")) {
            if let Some(t) = read_time(w) {
                time = Some(t);
                i += 1;
                continue;
            }
        }
        rest.push(w);
        i += 1;
    }
    (rest.join(" "), time)
}

/// The minutes of a reminder from its words ("15 min before", "on the day").
fn read_remind(words: &str, timed: bool) -> Option<i64> {
    let low = words.trim().to_lowercase();
    let list: &[(i64, &str)] = if timed { &TIMED_REMINDERS } else { &DAY_REMINDERS };
    if let Some((m, _)) = list.iter().chain(TIMED_REMINDERS.iter()).chain(DAY_REMINDERS.iter()).find(|(_, w)| *w == low) {
        return Some(*m);
    }
    // ("3 days before", "45 min before": what `remind_words` writes for others.)
    let (n, unit) = low.strip_suffix(" before")?.split_once(' ')?;
    let n: i64 = n.parse().ok()?;
    match unit {
        "min" | "mins" | "minutes" => Some(n),
        "hour" | "hours" => Some(n * 60),
        "day" | "days" => Some(n * 1440),
        "week" | "weeks" => Some(n * 10080),
        _ => None,
    }
    .filter(|m| *m >= 0)
}

/// The words of a reminder, as the list of its kind of day says it.
pub fn remind_words(minutes: i64, timed: bool) -> String {
    let list: &[(i64, &str)] = if timed { &TIMED_REMINDERS } else { &DAY_REMINDERS };
    match list.iter().find(|(m, _)| *m == minutes) {
        Some((_, w)) => w.to_string(),
        None if minutes % 1440 == 0 => format!("{} days before", minutes / 1440),
        None => format!("{minutes} min before"),
    }
}

impl When {
    pub fn on(start: NaiveDate) -> When {
        When { start, time: None, end: None, end_time: None, remind: None }
    }

    /// The cell read. `strict`: every word of each day must be part of a
    /// day ("Mark 3 done" is no day; see `whole_day`); else the first day
    /// that its words name.
    pub fn read(text: &str, today: NaiveDate, strict: bool) -> Option<When> {
        let text = text.trim();
        let (main, remind) = match text.split_once(REMIND) {
            Some((main, remind)) => (main, Some(remind)),
            None => (text, None),
        };
        let (a, b) = match main.split_once(TO).or_else(|| main.split_once("->")) {
            Some((a, b)) => (a, Some(b)),
            None => (main, None),
        };
        let read_day = |words: &str| if strict { whole_day(words, today) } else { day(words, today) };
        let (a_words, time) = split_time(a);
        let start = read_day(&a_words)?;
        let (end, end_time) = match b {
            Some(b) => {
                let (b_words, end_time) = split_time(b);
                let end = if b_words.trim().is_empty() { end_time.map(|_| start) } else { Some(read_day(&b_words)?) };
                (end, end_time)
            }
            None => (None, None),
        };
        let remind = remind.and_then(|r| read_remind(r, time.is_some()));
        Some(When { start, time, end, end_time, remind })
    }

    /// The words the cell holds: as the module notes say, each day with its
    /// year (a day with no year was read as the nearest such day, and so
    /// became a day of another year as time went by).
    pub fn write(&self, _today: NaiveDate) -> String {
        let mut out = self.words(&full_day);
        if let Some(m) = self.remind {
            out.push_str(&format!(" {REMIND} {}", remind_words(m, self.time.is_some())));
        }
        out
    }

    /// The words for the box of the date picker: as the cell holds them, but
    /// a day of this year with no year (the box is small, and the year of
    /// today says nothing).
    pub fn said(&self, today: NaiveDate) -> String {
        let mut out = self.shown(today);
        if let Some(m) = self.remind {
            out.push_str(&format!(" {REMIND} {}", remind_words(m, self.time.is_some())));
        }
        out
    }

    /// What the cell shows: its day, time and end (not its reminder, which a
    /// small bell says), a day of this year with no year.
    pub fn shown(&self, today: NaiveDate) -> String {
        self.words(&|d| format_day(d, today))
    }

    fn words(&self, say: &dyn Fn(NaiveDate) -> String) -> String {
        let mut out = say(self.start);
        if let Some(t) = self.time {
            out.push(' ');
            out.push_str(&time_words(t));
        }
        if let Some(end) = self.end {
            out.push_str(&format!(" {TO} "));
            let same_day = end == self.start && self.end_time.is_some();
            if !same_day {
                out.push_str(&say(end));
            }
            if let Some(t) = self.end_time {
                if !same_day {
                    out.push(' ');
                }
                out.push_str(&time_words(t));
            }
        }
        out
    }

    /// When its reminder comes, if it has one.
    pub fn remind_at(&self) -> Option<NaiveDateTime> {
        let minutes = self.remind?;
        let base = match self.time {
            Some(t) => self.start.and_time(t),
            None => self.start.and_hms_opt(REMIND_HOUR, 0, 0)?,
        };
        Some(base - Duration::minutes(minutes))
    }

    /// The days it covers, first to last (one for a single day).
    pub fn covers(&self, d: NaiveDate) -> bool {
        let end = self.end.unwrap_or(self.start);
        let (a, b) = if end < self.start { (end, self.start) } else { (self.start, end) };
        d >= a && d <= b
    }

    /// The same, with its start and end in order (an end before the start
    /// is the start, and the other way round).
    pub fn in_order(mut self) -> When {
        if let Some(end) = self.end {
            if (end, self.end_time.unwrap_or(NaiveTime::MIN)) < (self.start, self.time.unwrap_or(NaiveTime::MIN)) {
                self.end = Some(self.start);
                self.start = end;
                std::mem::swap(&mut self.time, &mut self.end_time);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    const TODAY: (i32, u32, u32) = (2026, 11, 11);

    fn today() -> NaiveDate {
        d(TODAY.0, TODAY.1, TODAY.2)
    }

    #[test]
    fn times_of_day_are_read_as_people_type_them() {
        assert_eq!(read_time("2:30 PM"), Some(t(14, 30)));
        assert_eq!(read_time("2pm"), Some(t(14, 0)));
        assert_eq!(read_time("12 am"), Some(t(0, 0)));
        assert_eq!(read_time("12pm"), Some(t(12, 0)));
        assert_eq!(read_time("14:05"), Some(t(14, 5)));
        assert_eq!(read_time("9:00 a.m."), Some(t(9, 0)));
        assert_eq!(read_time("noon"), Some(t(12, 0)));
        // A bare number, a wrong hour or minute: no time.
        assert_eq!(read_time("3"), None);
        assert_eq!(read_time("13pm"), None);
        assert_eq!(read_time("25:00"), None);
        assert_eq!(read_time("9:5"), None);
        assert_eq!(time_words(t(14, 30)), "2:30 PM");
        assert_eq!(time_words(t(0, 0)), "12 AM");
        assert_eq!(time_words(t(12, 0)), "12 PM");
    }

    #[test]
    fn a_day_with_a_time_an_end_and_a_reminder_is_written_and_read_again() {
        let cases = [
            When::on(d(2026, 11, 3)),
            When { time: Some(t(14, 30)), ..When::on(d(2026, 11, 3)) },
            When { end: Some(d(2026, 11, 6)), ..When::on(d(2026, 11, 3)) },
            When { time: Some(t(14, 30)), end: Some(d(2026, 11, 3)), end_time: Some(t(16, 0)), ..When::on(d(2026, 11, 3)) },
            When { time: Some(t(9, 0)), end: Some(d(2027, 1, 2)), end_time: Some(t(17, 15)), remind: Some(15), ..When::on(d(2026, 12, 30)) },
            When { remind: Some(1440), ..When::on(d(2026, 11, 6)) },
        ];
        let words = [
            "Tue 3 Nov 2026",
            "Tue 3 Nov 2026 2:30 PM",
            "Tue 3 Nov 2026 → Fri 6 Nov 2026",
            "Tue 3 Nov 2026 2:30 PM → 4 PM",
            "Wed 30 Dec 2026 9 AM → Sat 2 Jan 2027 5:15 PM ⏰ 15 min before",
            "Fri 6 Nov 2026 ⏰ 1 day before",
        ];
        for (w, text) in cases.iter().zip(words) {
            assert_eq!(w.write(today()), text);
            assert_eq!(When::read(text, today(), true), Some(*w), "{text}");
            // (The form of before, with no year, is read as the same day.)
            assert_eq!(When::read(&text.replace(" 2026", ""), today(), true), Some(*w), "{text}");
        }
        // What shows in the cell leaves the reminder to its bell.
        assert_eq!(cases[5].shown(today()), "Fri 6 Nov");
    }

    #[test]
    fn typed_forms_are_read_too() {
        assert_eq!(When::read("3 nov 2pm -> 6 nov", today(), true), Some(When { time: Some(t(14, 0)), end: Some(d(2026, 11, 6)), ..When::on(d(2026, 11, 3)) }));
        assert_eq!(When::read("2026-11-20 14:00", today(), true), Some(When { time: Some(t(14, 0)), ..When::on(d(2026, 11, 20)) }));
        // Words that are no day: none when strict; the day they name when not.
        assert_eq!(When::read("Mark 3 done", today(), true), None);
        // ("Mark" is read as March, as `day` reads it.)
        assert_eq!(When::read("Mark 3 done", today(), false).map(|w| w.start), Some(d(2027, 3, 3)));
        // A range over two months is read from its start ("3 Nov → 6 Dec"
        // is not the 3rd of December).
        assert_eq!(When::read("3 Nov → 6 Dec", today(), false).map(|w| (w.start, w.end)), Some((d(2026, 11, 3), Some(d(2026, 12, 6)))));
        assert_eq!(When::read("", today(), false), None);
    }

    #[test]
    fn a_reminder_comes_before_its_time_or_at_9_am() {
        let timed = When { time: Some(t(14, 30)), remind: Some(15), ..When::on(d(2026, 11, 3)) };
        assert_eq!(timed.remind_at(), Some(d(2026, 11, 3).and_time(t(14, 15))));
        let day = When { remind: Some(1440), ..When::on(d(2026, 11, 3)) };
        assert_eq!(day.remind_at(), Some(d(2026, 11, 2).and_time(t(9, 0))));
        assert_eq!(When { remind: Some(0), ..day }.remind_at(), Some(d(2026, 11, 3).and_time(t(9, 0))));
        assert_eq!(When::on(d(2026, 11, 3)).remind_at(), None);
        assert_eq!(remind_words(10080, false), "1 week before");
        assert_eq!(remind_words(4320, true), "3 days before");
    }

    #[test]
    fn a_range_covers_its_days_and_is_put_in_order() {
        let w = When { end: Some(d(2026, 11, 6)), ..When::on(d(2026, 11, 3)) };
        assert!(w.covers(d(2026, 11, 3)) && w.covers(d(2026, 11, 5)) && w.covers(d(2026, 11, 6)) && !w.covers(d(2026, 11, 7)));
        let back = When { end: Some(d(2026, 11, 1)), ..When::on(d(2026, 11, 3)) }.in_order();
        assert_eq!((back.start, back.end), (d(2026, 11, 1), Some(d(2026, 11, 3))));
        let hours = When { time: Some(t(16, 0)), end: Some(d(2026, 11, 3)), end_time: Some(t(9, 0)), ..When::on(d(2026, 11, 3)) }.in_order();
        assert_eq!((hours.time, hours.end_time), (Some(t(9, 0)), Some(t(16, 0))));
    }
}
