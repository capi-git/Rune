//! An event typed in words: "Lunch with Sam Friday 1pm", "Dentist tomorrow
//! 9:30 for 45 min", "Trip to Lisbon Nov 20", "Call Ana 3-4pm".
//!
//! The words that say when are taken out and read as a day and a time; what
//! is left is the name of the event. Nothing is asked of an AI: the same
//! words always give the same event, at once.
//!
//! What it knows:
//! - days: today, tonight, tomorrow, a day of the week ("fri", "next
//!   friday"), "in 3 days", "in 2 weeks", a date ("nov 20", "20 nov",
//!   "11/20", "2026-11-20");
//! - times: "1pm", "1:30 pm", "13:00", "noon", "at 1", a stretch ("1-2pm",
//!   "1pm to 2:30pm", "from 9 to 10");
//! - a length: "for 30 min", "for 2 hours", "for 1.5h".
//!
//! With a day and no time the event is all day. With a time and no day it
//! is today, or tomorrow when that time of today is past. With neither,
//! there is nothing to read and the caller makes a plain new event.

use crate::when::{self, MINUTE};
use chrono::{Datelike, NaiveDate, NaiveTime};

/// The event that some words describe.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
}

/// A time of day as typed: the hour and minute, and whether "am" or "pm" was said.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Clock {
    hour: u32,
    minute: u32,
    pm: Option<bool>,
}

impl Clock {
    /// The hour on a 24-hour clock. With no "am" or "pm": 1 to 7 are in the
    /// afternoon (nobody types "lunch 1" for the night), 8 to 11 the morning.
    fn hour24(self) -> u32 {
        match self.pm {
            Some(true) if self.hour < 12 => self.hour + 12,
            Some(false) if self.hour == 12 => 0,
            Some(_) => self.hour,
            None if (1..=7).contains(&self.hour) => self.hour + 12,
            None => self.hour,
        }
    }

    fn minutes(self) -> i64 {
        i64::from(self.hour24()) * 60 + i64::from(self.minute)
    }
}

fn clock(word: &str) -> Option<Clock> {
    let w = word.trim_matches(|c: char| c == ',' || c == '.');
    match w {
        "noon" | "midday" => return Some(Clock { hour: 12, minute: 0, pm: Some(true) }),
        "midnight" => return Some(Clock { hour: 0, minute: 0, pm: Some(false) }),
        _ => {}
    }
    let (digits, pm) = if let Some(d) = w.strip_suffix("pm").or_else(|| w.strip_suffix("p.m")).or_else(|| w.strip_suffix('p')) {
        (d, Some(true))
    } else if let Some(d) = w.strip_suffix("am").or_else(|| w.strip_suffix("a.m")).or_else(|| w.strip_suffix('a')) {
        (d, Some(false))
    } else {
        (w, None)
    };
    let (h, m) = digits.split_once(':').unwrap_or((digits, "0"));
    if h.is_empty() || h.len() > 2 || m.is_empty() || m.len() > 2 || !h.bytes().all(|b| b.is_ascii_digit()) || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hour, minute) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    let ok = minute < 60 && if pm.is_some() { (1..=12).contains(&hour) } else { hour < 24 };
    // (An hour written with a zero before it, "06:45", is the hour of a
    // 24-hour clock: the morning, not the afternoon.)
    let pm = if pm.is_none() && h.len() == 2 && h.starts_with('0') && hour != 0 { Some(false) } else { pm };
    ok.then_some(Clock { hour, minute, pm })
}

/// A time typed into a box: "4:15 pm", "4:15pm", "16:15", "4p", "noon".
/// With no "am" or "pm", 1 to 7 are in the afternoon, as for an event typed
/// in words.
pub fn typed_time(text: &str) -> Option<NaiveTime> {
    let word: String = text.trim().to_lowercase().split_whitespace().collect();
    let c = clock(&word)?;
    NaiveTime::from_hms_opt(c.hour24(), c.minute, 0)
}

/// Whether a word is a time by itself: it says "am" or "pm", or has a colon.
fn sure_clock(word: &str) -> Option<Clock> {
    clock(word).filter(|c| c.pm.is_some() || word.contains(':'))
}

fn weekday(word: &str) -> Option<chrono::Weekday> {
    use chrono::Weekday::*;
    Some(match word.trim_matches(',') {
        "mon" | "monday" => Mon,
        "tue" | "tues" | "tuesday" => Tue,
        "wed" | "weds" | "wednesday" => Wed,
        "thu" | "thur" | "thurs" | "thursday" => Thu,
        "fri" | "friday" => Fri,
        "sat" | "saturday" => Sat,
        "sun" | "sunday" => Sun,
        _ => return None,
    })
}

fn month(word: &str) -> Option<u32> {
    const NAMES: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    let w = word.trim_matches(|c: char| c == ',' || c == '.');
    if w.len() < 3 {
        return None;
    }
    NAMES.iter().position(|name| *name == w || (w.len() == 3 && name.starts_with(w)) || (w == "sept" && *name == "september")).map(|i| i as u32 + 1)
}

/// "20", "20th", "1st," as a day of the month.
fn day_number(word: &str) -> Option<u32> {
    let w = word.trim_matches(',');
    let digits = ["st", "nd", "rd", "th"].iter().find_map(|s| w.strip_suffix(s)).unwrap_or(w);
    (!digits.is_empty() && digits.len() <= 2 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten().filter(|d| (1..=31).contains(d))
}

/// A month and a day as a date: this year, or the next when it is past.
fn coming(month: u32, day: u32, year: Option<i32>, today: NaiveDate) -> Option<NaiveDate> {
    match year {
        Some(y) => NaiveDate::from_ymd_opt(y, month, day),
        None => {
            let this = NaiveDate::from_ymd_opt(today.year(), month, day)?;
            if this < today { NaiveDate::from_ymd_opt(today.year() + 1, month, day) } else { Some(this) }
        }
    }
}

/// "30min", "2h", "1.5", with the word after it when the number is alone: minutes.
fn length(word: &str, next: Option<&str>) -> Option<(i64, bool)> {
    let w = word.trim_matches(',');
    let split = w.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(w.len());
    let (number, unit) = w.split_at(split);
    let n: f64 = number.parse().ok()?;
    let (unit, took_next) = if unit.is_empty() { (next?, true) } else { (unit, false) };
    let minutes = match unit.trim_matches(|c: char| c == ',' || c == '.') {
        "m" | "min" | "mins" | "minute" | "minutes" => n,
        "h" | "hr" | "hrs" | "hour" | "hours" => n * 60.0,
        _ => return None,
    };
    (minutes >= 5.0 && minutes <= 24.0 * 60.0).then_some((minutes.round() as i64, took_next))
}

/// Read an event from words typed at `now`.
pub fn parse(text: &str, now: i64) -> Option<Parsed> {
    let words: Vec<&str> = text.split_whitespace().collect();
    // The same words in small letters, with the long dashes of a Mac made plain.
    let low: Vec<String> = words.iter().map(|w| w.to_lowercase().replace(['–', '—'], "-")).collect();
    let mut used = vec![false; words.len()];
    let today = when::date_of(now);
    let is = |i: usize, what: &[&str]| low.get(i).is_some_and(|w| what.contains(&w.as_str()));

    // ----- the day -----
    let mut day: Option<NaiveDate> = None;
    let mut evening = false;
    let mut i = 0;
    while i < low.len() {
        let w = low[i].trim_matches(',');
        let before = i.checked_sub(1);
        let mut found: Option<(NaiveDate, Vec<usize>)> = None;
        if w == "today" {
            found = Some((today, vec![i]));
        } else if w == "tonight" {
            evening = true;
            found = Some((today, vec![i]));
        } else if matches!(w, "tomorrow" | "tmrw" | "tmr") {
            found = Some((when::next_day(today), vec![i]));
        } else if let Some(wd) = weekday(w) {
            let ahead = (7 + wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64 - 1) % 7 + 1;
            let mut date = when::add_days(today, ahead);
            let mut took = vec![i];
            if let Some(b) = before.filter(|b| is(*b, &["next"])) {
                // "next friday" is the Friday of the week after this one.
                if when::week_start(date) == when::week_start(today) {
                    date = when::add_days(date, 7);
                }
                took.push(b);
            } else if let Some(b) = before.filter(|b| is(*b, &["on", "this"])) {
                took.push(b);
            }
            found = Some((date, took));
        } else if let Some(m) = month(w) {
            // "nov 20", "20 nov", with a year after it or not.
            let after = low.get(i + 1).and_then(|n| day_number(n)).map(|d| (d, i + 1));
            let ahead = before.and_then(|b| day_number(&low[b]).map(|d| (d, b)));
            if let Some((d, at)) = after.or(ahead) {
                let year_at = if at > i { at + 1 } else { i + 1 };
                let year = low.get(year_at).and_then(|y| (y.len() == 4).then(|| y.parse::<i32>().ok()).flatten()).filter(|y| (2000..2100).contains(y));
                if let Some(date) = coming(m, d, year, today) {
                    let mut took = vec![i, at];
                    if year.is_some() {
                        took.push(year_at);
                    }
                    if let Some(b) = at.min(i).checked_sub(1).filter(|b| is(*b, &["on"])) {
                        took.push(b);
                    }
                    found = Some((date, took));
                }
            }
        } else if w == "in" && low.get(i + 1).and_then(|n| n.parse::<i64>().ok()).is_some() && is(i + 2, &["day", "days", "week", "weeks"]) {
            let n = low[i + 1].parse::<i64>().unwrap_or(0).clamp(0, 3650);
            let days = if low[i + 2].starts_with("week") { n * 7 } else { n };
            found = Some((when::add_days(today, days), vec![i, i + 1, i + 2]));
        } else if let Ok(date) = NaiveDate::parse_from_str(w, "%Y-%m-%d") {
            found = Some((date, vec![i]));
        } else if w.matches('/').count() >= 1 && w.bytes().all(|b| b.is_ascii_digit() || b == b'/') {
            // "11/20" or "11/20/26": the month first.
            let parts: Vec<u32> = w.split('/').filter_map(|p| p.parse().ok()).collect();
            if let [m, d, rest @ ..] = parts.as_slice() {
                let year = rest.first().map(|y| if *y < 100 { 2000 + *y as i32 } else { *y as i32 });
                if rest.len() <= 1 && w.split('/').count() == parts.len() {
                    if let Some(date) = coming(*m, *d, year, today) {
                        found = Some((date, vec![i]));
                    }
                }
            }
        }
        if let (Some((date, took)), None) = (found, day) {
            day = Some(date);
            for t in took {
                used[t] = true;
            }
        }
        i += 1;
    }

    // ----- a length: "for 45 min" -----
    let mut long: Option<i64> = None;
    for i in 0..low.len() {
        if used[i] || low[i] != "for" || used.get(i + 1).copied().unwrap_or(true) {
            continue;
        }
        if let Some((minutes, took_next)) = length(&low[i + 1], low.get(i + 2).map(String::as_str)) {
            long = Some(minutes);
            used[i] = true;
            used[i + 1] = true;
            if took_next {
                used[i + 2] = true;
            }
            break;
        }
    }

    // ----- the time, or a stretch of time -----
    let mut from: Option<Clock> = None;
    let mut to: Option<Clock> = None;
    let mut i = 0;
    while i < low.len() && from.is_none() {
        if used[i] {
            i += 1;
            continue;
        }
        let w = low[i].as_str();
        let led = i.checked_sub(1).filter(|b| !used[*b] && is(*b, &["at", "@", "from"]));
        // "1-2pm", "9:30-10"
        if let Some((a, b)) = w.split_once('-').filter(|(a, b)| !a.is_empty() && !b.is_empty()) {
            if let (Some(a), Some(b)) = (clock(a), clock(b)) {
                if a.pm.is_some() || b.pm.is_some() || w.contains(':') || led.is_some() {
                    from = Some(a);
                    to = Some(b);
                    used[i] = true;
                    if let Some(l) = led {
                        used[l] = true;
                    }
                }
            }
            i += 1;
            continue;
        }
        // "1 pm": the two words are one time.
        let (found, width) = match (clock(w), low.get(i + 1).map(String::as_str)) {
            (Some(c), Some("pm" | "p.m." | "p.m")) if c.pm.is_none() && (1..=12).contains(&c.hour) => (Some(Clock { pm: Some(true), ..c }), 2),
            (Some(c), Some("am" | "a.m." | "a.m")) if c.pm.is_none() && (1..=12).contains(&c.hour) => (Some(Clock { pm: Some(false), ..c }), 2),
            _ => (sure_clock(w).or_else(|| led.and_then(|_| clock(w))), 1),
        };
        // A bare number with nothing that says it is a time is part of the name ("Review 3 plans").
        let Some(c) = found else {
            i += 1;
            continue;
        };
        from = Some(c);
        for k in i..i + width {
            used[k] = true;
        }
        if let Some(l) = led {
            used[l] = true;
        }
        // "... to 2:30pm", "... - 3", "... until 5"
        let next = i + width;
        if is(next, &["to", "-", "until", "till"]) && !used.get(next + 1).copied().unwrap_or(true) {
            let w2 = low[next + 1].as_str();
            let (end, width2) = match (clock(w2), low.get(next + 2).map(String::as_str)) {
                (Some(c), Some("pm" | "p.m." | "p.m")) if c.pm.is_none() => (Some(Clock { pm: Some(true), ..c }), 2),
                (Some(c), Some("am" | "a.m." | "a.m")) if c.pm.is_none() => (Some(Clock { pm: Some(false), ..c }), 2),
                (c, _) => (c, 1),
            };
            if let Some(end) = end {
                to = Some(end);
                for k in next..next + 1 + width2 {
                    used[k] = true;
                }
            }
        }
        i += 1;
    }

    if day.is_none() && from.is_none() {
        return None;
    }

    // ----- the name: what is left -----
    let title = words.iter().zip(&used).filter(|(_, u)| !**u).map(|(w, _)| *w).collect::<Vec<_>>().join(" ");
    let title = title.trim_matches(|c: char| c == ',' || c == '-' || c.is_whitespace()).to_string();
    let title = if title.is_empty() { "New event".to_string() } else { title };

    let from = from.or_else(|| evening.then_some(Clock { hour: 7, minute: 0, pm: Some(true) }));
    let Some(mut from) = from else {
        // A day and no time: all day.
        let day = day?;
        return Some(Parsed { title, start: when::midnight(day), end: when::midnight(when::next_day(day)), all_day: true });
    };
    // ("tonight at 8": the evening, not the morning.)
    if evening && from.pm.is_none() && (1..=11).contains(&from.hour) {
        from.pm = Some(true);
    }
    // "1-2pm": the start is in the afternoon too, unless that puts it after the end ("11-1pm").
    if let (None, Some(end)) = (from.pm, to) {
        if let Some(pm) = end.pm {
            let same = Clock { pm: Some(pm), ..from };
            from = if (1..=12).contains(&from.hour) && same.minutes() < end.minutes() { same } else { Clock { pm: Some(!pm), ..from } };
            if !(1..=12).contains(&from.hour) {
                from.pm = None;
            }
        }
    }
    let at = |day: NaiveDate, minutes: i64| when::at(day, NaiveTime::from_hms_opt((minutes / 60 % 24) as u32, (minutes % 60) as u32, 0).unwrap_or_default());
    let day = day.unwrap_or_else(|| if at(today, from.minutes()) <= now { when::next_day(today) } else { today });
    let start = at(day, from.minutes());
    let end = match (to, long) {
        (Some(mut end), _) => {
            // An end with no "am" or "pm" is after the start ("9 to 10", "11 to 1").
            if end.pm.is_none() && end.minutes() <= from.minutes() && end.hour < 12 {
                end.pm = Some(true);
            }
            let minutes = end.minutes() - from.minutes();
            start + if minutes > 0 { minutes } else { minutes + 24 * 60 } * MINUTE
        }
        (None, Some(minutes)) => start + minutes * MINUTE,
        (None, None) => start + 60 * MINUTE,
    };
    Some(Parsed { title, start, end, all_day: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tuesday, 10 November 2026, 10:20 in the morning.
    fn now() -> i64 {
        when::at(NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(), NaiveTime::from_hms_opt(10, 20, 0).unwrap())
    }

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    #[test]
    fn an_hour_with_a_zero_before_it_is_the_morning() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0);
        assert_eq!(typed_time("06:45"), t(6, 45));
        assert_eq!(typed_time("07"), t(7, 0));
        assert_eq!(typed_time("6:45"), t(18, 45));
        assert_eq!(typed_time("00:30"), t(0, 30));
        assert_eq!(typed_time("09:00"), t(9, 0));
    }

    #[test]
    fn a_time_typed_into_a_box() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0);
        assert_eq!(typed_time("4:15 pm"), t(16, 15));
        assert_eq!(typed_time(" 4:15PM "), t(16, 15));
        assert_eq!(typed_time("16:15"), t(16, 15));
        assert_eq!(typed_time("9"), t(9, 0));
        assert_eq!(typed_time("4"), t(16, 0));
        assert_eq!(typed_time("12 am"), t(0, 0));
        assert_eq!(typed_time("noon"), t(12, 0));
        assert_eq!(typed_time("soon"), None);
        assert_eq!(typed_time("25:00"), None);
    }

    /// (name, day, start "H:MM", minutes long) or all day (minutes 0).
    fn read(text: &str) -> (String, NaiveDate, String, i64) {
        let p = parse(text, now()).unwrap_or_else(|| panic!("nothing read from {text:?}"));
        let start = when::local(p.start);
        let clock = if p.all_day { String::new() } else { start.format("%-H:%M").to_string() };
        (p.title, when::date_of(p.start), clock, if p.all_day { 0 } else { (p.end - p.start) / MINUTE })
    }

    fn got(name: &str, d: NaiveDate, clock: &str, minutes: i64) -> (String, NaiveDate, String, i64) {
        (name.to_string(), d, clock.to_string(), minutes)
    }

    #[test]
    fn a_day_and_a_time_in_words() {
        assert_eq!(read("Lunch with Sam Friday 1pm"), got("Lunch with Sam", day(11, 13), "13:00", 60));
        assert_eq!(read("Dentist tomorrow 9:30 for 45 min"), got("Dentist", day(11, 11), "9:30", 45));
        assert_eq!(read("call Ana 3-4pm"), got("call Ana", day(11, 10), "15:00", 60));
        assert_eq!(read("Stand-up at 9"), got("Stand-up", day(11, 11), "9:00", 60), "9 today is past: tomorrow");
        assert_eq!(read("Review at 4"), got("Review", day(11, 10), "16:00", 60), "4 with no am or pm is the afternoon");
        assert_eq!(read("Workshop 11-1pm on Thu"), got("Workshop", day(11, 12), "11:00", 120));
        assert_eq!(read("Dinner tonight"), got("Dinner", day(11, 10), "19:00", 60));
        assert_eq!(read("Flight 13:05 nov 20"), got("Flight", day(11, 20), "13:05", 60));
        assert_eq!(read("Plan from 9 to 10:30 tomorrow"), got("Plan", day(11, 11), "9:00", 90));
        assert_eq!(read("Run 6 am for 1.5h"), got("Run", day(11, 11), "6:00", 90));
        assert_eq!(read("Sync noon"), got("Sync", day(11, 10), "12:00", 60));
        assert_eq!(read("Budget next friday 2 pm to 3 pm"), got("Budget", day(11, 20), "14:00", 60));
    }

    #[test]
    fn a_day_with_no_time_is_all_day() {
        assert_eq!(read("Trip to Lisbon Nov 20"), got("Trip to Lisbon", day(11, 20), "", 0));
        assert_eq!(read("Sam's birthday 3rd March"), got("Sam's birthday", NaiveDate::from_ymd_opt(2027, 3, 3).unwrap(), "", 0), "a date that is past is next year's");
        assert_eq!(read("Rent due 12/1"), got("Rent due", day(12, 1), "", 0));
        assert_eq!(read("Release 2026-11-30"), got("Release", day(11, 30), "", 0));
        assert_eq!(read("Tidy the desk in 3 days"), got("Tidy the desk", day(11, 13), "", 0));
        assert_eq!(read("Offsite in 2 weeks"), got("Offsite", day(11, 24), "", 0));
        assert_eq!(read("tuesday"), got("New event", day(11, 17), "", 0), "the day of today by its name is the next one");
    }

    #[test]
    fn words_that_only_look_like_a_time_stay_in_the_name() {
        // No day and no time: nothing to read.
        assert_eq!(parse("Review 3 proposals", now()), None);
        assert_eq!(parse("Read chapter 12", now()), None);
        assert_eq!(parse("", now()), None);
        // A number in the name is left there when the time is said in its own words.
        assert_eq!(read("Review 3 proposals 2pm"), got("Review 3 proposals", day(11, 10), "14:00", 60));
        assert_eq!(read("May review on May 4"), got("May review", NaiveDate::from_ymd_opt(2027, 5, 4).unwrap(), "", 0));
        assert_eq!(read("Marathon training sat 7am"), got("Marathon training", day(11, 14), "7:00", 60), "the letters Mar inside a word are not a month");
    }
}
