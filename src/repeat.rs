//! Repeating calendar events.
//!
//! A repeating event is stored once: its first time, plus a rule (`Repeat`).
//! Each repeat is worked out when it is shown. A repeat has an id of its own,
//! "<event id>@<YYYY-MM-DD>", so it can be selected like any event; changes
//! made to it go to the stored event (see `apply` and
//! `AppState::update_event`), except where one repeat is deleted or changed
//! on its own (its day is then in `Repeat::skip`).

use crate::model::{Event, Id};
use crate::when;
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Every {
    Day,
    /// Monday to Friday.
    Weekday,
    Week,
    Month,
    Year,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repeat {
    pub every: Every,
    /// Every `interval` days, weeks, months or years (not used for weekdays).
    #[serde(default = "one")]
    pub interval: u32,
    /// The last day a repeat may fall on ("YYYY-MM-DD"); none: no end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// Days ("YYYY-MM-DD") whose repeat was deleted, or changed on its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<String>,
}

fn one() -> u32 {
    1
}

impl Repeat {
    pub fn new(every: Every, interval: u32) -> Repeat {
        Repeat { every, interval: interval.max(1), until: None, skip: Vec::new() }
    }

    pub fn until_date(&self) -> Option<NaiveDate> {
        self.until.as_deref().and_then(when::parse_date)
    }

    fn skips(&self, d: NaiveDate) -> bool {
        let day = when::date_value(d);
        self.skip.iter().any(|s| *s == day)
    }

    /// The rule as stored in the database ("" for none).
    pub fn to_json(rule: Option<&Repeat>) -> String {
        rule.and_then(|r| serde_json::to_string(r).ok()).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<Repeat> {
        if s.trim().is_empty() { None } else { serde_json::from_str(s).ok() }
    }
}

/// The days from `from` to `to` (both included) on which a rule for an event
/// first held on `first` falls.
pub fn days(first: NaiveDate, r: &Repeat, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let from = from.max(first);
    let to = match r.until_date() {
        Some(u) => to.min(u),
        None => to,
    };
    let mut out = Vec::new();
    if from > to {
        return out;
    }
    let n = r.interval.max(1) as i64;
    match r.every {
        Every::Day | Every::Week => {
            let step = if r.every == Every::Day { n } else { 7 * n };
            let gap = (from - first).num_days();
            let mut k = (gap + step - 1) / step;
            loop {
                let d = when::add_days(first, k * step);
                if d > to {
                    break;
                }
                out.push(d);
                k += 1;
            }
        }
        Every::Weekday => {
            let mut d = from;
            while d <= to {
                // (The day the event was made for is one of its days, also
                // a Saturday or a Sunday: left out, an event of a Saturday
                // went from the calendar when "Every weekday" was chosen
                // for it, and its panel shut.)
                if d.weekday().num_days_from_monday() < 5 || d == first {
                    out.push(d);
                }
                d = when::next_day(d);
            }
        }
        Every::Month | Every::Year => {
            let step = if r.every == Every::Month { n } else { 12 * n };
            let month_no = |d: NaiveDate| d.year() as i64 * 12 + d.month0() as i64;
            let mut k = (month_no(from) - month_no(first)).max(0) / step;
            loop {
                let m = month_no(first) + k * step;
                let (y, mo) = (m.div_euclid(12) as i32, m.rem_euclid(12) as u32 + 1);
                if NaiveDate::from_ymd_opt(y, mo, 1).is_none_or(|start| start > to) {
                    break;
                }
                // A month without that day (the 31st, 29 February) is skipped.
                if let Some(d) = NaiveDate::from_ymd_opt(y, mo, first.day()) {
                    if d >= from && d <= to {
                        out.push(d);
                    }
                }
                k += 1;
            }
        }
    }
    out.retain(|d| !r.skips(*d));
    out
}

fn time_of(ms: i64) -> NaiveTime {
    let t = when::local(ms);
    NaiveTime::from_hms_opt(t.hour(), t.minute(), 0).unwrap_or(NaiveTime::MIN)
}

/// How many days an all-day event covers.
fn day_count(e: &Event) -> i64 {
    (when::whole_day(e.end) - when::whole_day(e.start)).num_days().max(1)
}

/// An all-day event as many days long as `e`, from `day`: whole days by the
/// calendar. (By the length of `e` in hours, a copy of an event of the day
/// the clocks go back, 25 hours long, ran an hour into a second day.)
pub fn whole_days_from(e: &Event, day: NaiveDate) -> (i64, i64) {
    (when::midnight(day), when::midnight(when::add_days(day, day_count(e))))
}

/// The day an event is first held on. (For an all-day event the day it was
/// made for, also when the Mac is in another time zone now: `when::whole_day`.)
pub fn first_day(e: &Event) -> NaiveDate {
    if e.all_day { when::whole_day(e.start) } else { when::date_of(e.start) }
}

/// An event as it is read: an all-day one from the midnight of its first
/// day to the midnight after its last, in the time zone of now (as it is
/// kept when the Mac did not travel; see `when::whole_day`).
pub fn as_read(e: &Event) -> Event {
    let mut o = e.clone();
    if e.all_day {
        let first = when::whole_day(e.start);
        o.start = when::midnight(first);
        o.end = when::midnight(when::add_days(first, day_count(e)));
    }
    o
}

pub fn instance_id(series: &str, d: NaiveDate) -> String {
    format!("{series}@{}", when::date_value(d))
}

/// The stored event an id belongs to, and the day of the repeat it names.
pub fn split_id(id: &str) -> (&str, Option<NaiveDate>) {
    match id.split_once('@') {
        Some((series, day)) => (series, when::parse_date(day)),
        None => (id, None),
    }
}

/// The repeat of `e` on day `d` (whether or not the rule falls on it).
pub fn at(e: &Event, d: NaiveDate) -> Event {
    let mut o = e.clone();
    o.id = instance_id(&e.id, d);
    if e.all_day {
        o.start = when::midnight(d);
        o.end = when::midnight(when::add_days(d, day_count(e)));
    } else {
        o.start = when::at(d, time_of(e.start));
        o.end = o.start + (e.end - e.start);
    }
    o
}

/// Each time `e` happens that overlaps `from..to` (ms). A one-off event comes
/// back as it is; repeats get ids of their own.
pub fn occurrences(e: &Event, from: i64, to: i64) -> Vec<Event> {
    let Some(r) = &e.repeat else {
        let e = as_read(e);
        return if e.overlaps(from, to) { vec![e] } else { Vec::new() };
    };
    if to <= from {
        return Vec::new();
    }
    // A repeat that starts a little before `from` may still run into it.
    let span = (when::date_of(e.end - 1) - when::date_of(e.start)).num_days().max(0) + 1;
    let lo = when::add_days(when::date_of(from), -span);
    let hi = when::date_of(to - 1);
    days(first_day(e), r, lo, hi).into_iter().map(|d| at(e, d)).filter(|o| o.overlaps(from, to)).collect()
}

/// Every event and repeat that overlaps `from..to`.
pub fn expand<'a>(events: impl IntoIterator<Item = &'a Event>, from: i64, to: i64) -> Vec<Event> {
    events.into_iter().flat_map(|e| occurrences(e, from, to)).collect()
}

/// The event or repeat an id names, if it (still) exists.
pub fn find(events: &HashMap<Id, Event>, id: &str) -> Option<Event> {
    let (series, day) = split_id(id);
    let e = events.get(series)?;
    match (day, &e.repeat) {
        (None, _) => Some(as_read(e)),
        (Some(d), Some(r)) => days(first_day(e), r, d, d).first().map(|&d| at(e, d)),
        (Some(d), None) => (d == first_day(e)).then(|| as_read(e)),
    }
}

/// What the notes page of the repeat of `series` on `day` is kept by: the
/// event's id, then a day that is the same in every time zone. For an
/// all-day event that is the day itself. For an event with a time it is the
/// repeat's day, moved by as many days as the event's first start is ahead
/// in UTC (its day in UTC less its day here): the first repeat is kept by
/// the UTC day of its start, and each later one by so many days after it
/// as it is after the first.
/// (Kept by the repeat's own id, which has the day of this Mac's zone: read
/// eight hours to the east, a Tuesday evening is a Wednesday, no repeat
/// found its page, and "Join" made a second one. Kept by the UTC day of
/// each repeat's own start: at 19:30 in New York, the Saturday before the
/// clocks go forward and the Sunday after it start on one UTC day, and the
/// two repeats had one page.)
pub fn page_key(series: &Event, day: NaiveDate) -> String {
    if series.all_day {
        return instance_id(&series.id, day);
    }
    let ahead = chrono::DateTime::from_timestamp_millis(series.start).map_or(0, |t| (t.date_naive() - when::date_of(series.start)).num_days());
    instance_id(&series.id, when::add_days(day, ahead))
}

/// The notes page of the repeat of `series` on `day`. Each repeat has a page
/// of its own: `own` holds them by `page_key` (the app keeps that list
/// as a setting, see `AppState::repeat_pages`; the stored event has room for
/// one page, and with only that one, this week's "Join" opened the notes of
/// last week). `made` gives the day a page was made on, or nothing for a
/// page that is gone. A page kept on the stored event itself (from a Rune
/// that gave all repeats that one page) is the page of the repeat of the day
/// it was made on, and when that was no day of the event, of the first repeat.
pub fn page_of(series: &Event, day: NaiveDate, own: &HashMap<String, Id>, made: impl Fn(&str) -> Option<NaiveDate>) -> Option<Id> {
    if let Some(page) = own.get(&page_key(series, day)).filter(|p| made(p).is_some()) {
        return Some(page.clone());
    }
    let page = series.page_id.as_ref()?;
    let made_on = made(page)?;
    // (By the rule alone: a repeat that was deleted since keeps its page, and no other repeat gets it.)
    let rule = Repeat { skip: Vec::new(), ..series.repeat.clone()? };
    let first = first_day(series);
    let its_day = days(first, &rule, made_on, made_on).first().copied().or_else(|| days(first, &rule, first, when::add_days(first, 400)).first().copied());
    (its_day == Some(day)).then(|| page.clone())
}

/// A repeating event changed from `old` to `new`: the pages of its repeats
/// (see `page_of`) go with their repeats.
/// - Moved, with the rule it had (a drag, a new time): each page goes with
///   its repeat. For an event of every day or every weekday, which is on
///   the same days after the move, a page stays on its day; the page of the
///   first repeat goes with it when it was moved to a later day that has no
///   page of its own.
/// - Another rule: a page stays on its day, where that is still a day of
///   the event.
/// - It repeats no more: its pages leave the list, and the page of its
///   first repeat is given back, to be the page of the event.
/// A page whose day is no day of the event any more stays in the list by
/// its old day (it is still a page of the notes, and no repeat shows it).
pub fn carry_pages(own: &mut HashMap<String, Id>, old: &Event, new: &Event) -> Option<Id> {
    let mine = |key: &str| split_id(key).0 == old.id;
    if !own.keys().any(|key| mine(key)) {
        return None;
    }
    // (By the rule alone: a repeat that was deleted, or is past the last day, keeps its page.)
    let bare = |r: &Repeat| Repeat { skip: Vec::new(), until: None, ..r.clone() };
    let old_rule = bare(old.repeat.as_ref()?);
    let old_first = first_day(old);
    let last = own.keys().filter(|key| mine(key)).filter_map(|key| split_id(key).1).max().map_or(old_first, |d| when::add_days(d, 2));
    let old_days = days(old_first, &old_rule, old_first, last);
    let Some(new_rule) = new.repeat.as_ref().map(bare) else {
        let first = old_days.first().and_then(|d| own.get(&page_key(old, *d)).cloned());
        own.retain(|key, _| !mine(key));
        return first;
    };
    let new_first = first_day(new);
    let moved = (old_rule.every, old_rule.interval) == (new_rule.every, new_rule.interval) && !same_days(&new_rule);
    let shift = if moved { (new_first - old_first).num_days() } else { 0 };
    // (A month is not always as long: the 15th of each month moved 20 days is the 5th, 20 or 21 days on.)
    let reach = if moved && matches!(new_rule.every, Every::Month | Every::Year) { 16 } else { 0 };
    let mut out: HashMap<String, Id> = HashMap::new();
    let mut of_the_first = None;
    for (n, d) in old_days.iter().enumerate() {
        let Some(page) = own.remove(&page_key(old, *d)) else { continue };
        let target = when::add_days(*d, shift);
        let to = days(new_first, &new_rule, when::add_days(target, -reach), when::add_days(target, reach)).into_iter().min_by_key(|x| (*x - target).num_days().abs());
        match to {
            Some(to) => {
                out.entry(page_key(new, to)).or_insert(page);
            }
            None if n == 0 && new_first > *d => of_the_first = Some((page_key(old, *d), page)),
            None => {
                out.entry(page_key(old, *d)).or_insert(page);
            }
        }
    }
    if let Some((old_key, page)) = of_the_first {
        let key = page_key(new, new_first);
        out.insert(if out.contains_key(&key) { old_key } else { key }, page);
    }
    // (What is left in the list was kept by a day that was no day of the event: it stays as it is.)
    own.extend(out);
    None
}

/// Carry a change made to one repeat (`before` → `after`) to the stored event
/// `e`: the same fields, and the same move in days and time of day.
pub fn apply(e: &mut Event, before: &Event, after: &Event) {
    let shift = (when::date_of(after.start) - when::date_of(before.start)).num_days();
    // (An event of every day is on the same days after one of its repeats
    // moved to another day, so its first day stays: moved with the repeat,
    // the first day of the event went with no word. Only a move of the
    // first repeat itself is a new first day.)
    let keeps_first = after.repeat.as_ref().is_some_and(same_days) && when::date_of(before.start) != first_day(e);
    let first = if keeps_first { first_day(e) } else { when::add_days(first_day(e), shift) };
    e.title = after.title.clone();
    e.space_id = after.space_id.clone();
    e.color = after.color.clone();
    e.notes = after.notes.clone();
    e.join = after.join.clone();
    e.page_id = after.page_id.clone();
    e.remind = after.remind;
    e.all_day = after.all_day;
    e.repeat = after.repeat.clone();
    if after.all_day {
        e.start = when::midnight(first);
        e.end = when::midnight(when::add_days(first, day_count(after)));
    } else {
        e.start = when::at(first, time_of(after.start));
        e.end = e.start + (after.end - after.start);
    }
    if shift != 0 {
        // (An event of every day, or of every weekday, is on the same days
        // after the move: the days that were taken out stay those days.
        // Moved with it, a deleted day came back and another one went.)
        if let Some(r) = e.repeat.as_mut().filter(|r| !same_days(r)) {
            for s in &mut r.skip {
                if let Some(d) = when::parse_date(s) {
                    *s = when::date_value(when::add_days(d, shift));
                }
            }
            // The last day moves with the repeats: left where it was, a
            // move to a later day put the last repeat past it, and it went.
            if let Some(until) = r.until_date() {
                r.until = Some(when::date_value(when::add_days(until, shift)));
            }
        }
    }
}

/// A rule whose repeats are on the same days after the event moves by whole
/// days: every day, and every weekday.
pub fn same_days(r: &Repeat) -> bool {
    r.every == Every::Weekday || (r.every == Every::Day && r.interval <= 1)
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The choices the event editor offers, for an event first held on `first`.
pub fn choices(first: NaiveDate) -> Vec<(&'static str, String)> {
    let weekday = first.format("%A").to_string();
    vec![
        ("none", "Does not repeat".into()),
        ("day", "Every day".into()),
        ("weekday", "Every weekday (Mon–Fri)".into()),
        ("week", format!("Every week on {weekday}")),
        ("2week", format!("Every 2 weeks on {weekday}")),
        ("month", format!("Every month on the {}", ordinal(first.day()))),
        ("year", format!("Every year on {}", first.format("%B %-d"))),
    ]
}

/// The choice (see `choices`) a rule is, or "custom" for one not offered.
pub fn choice_of(r: Option<&Repeat>) -> &'static str {
    match r.map(|r| (r.every, r.interval)) {
        None => "none",
        Some((Every::Day, 1)) => "day",
        Some((Every::Weekday, _)) => "weekday",
        Some((Every::Week, 1)) => "week",
        Some((Every::Week, 2)) => "2week",
        Some((Every::Month, 1)) => "month",
        Some((Every::Year, 1)) => "year",
        Some(_) => "custom",
    }
}

/// The rule a choice (or a word an agent uses: "weekly", "weekdays", …) means.
/// `None` for "does not repeat"; `Err` for a word not understood.
pub fn parse(word: &str) -> Result<Option<Repeat>, ()> {
    let w = word.trim().to_lowercase().replace(['-', '_'], " ");
    Ok(Some(match w.as_str() {
        "" | "none" | "never" | "no" | "does not repeat" | "once" => return Ok(None),
        "day" | "daily" | "every day" => Repeat::new(Every::Day, 1),
        "weekday" | "weekdays" | "every weekday" | "workdays" => Repeat::new(Every::Weekday, 1),
        "week" | "weekly" | "every week" => Repeat::new(Every::Week, 1),
        "2week" | "biweekly" | "fortnightly" | "every 2 weeks" | "every other week" => Repeat::new(Every::Week, 2),
        "month" | "monthly" | "every month" => Repeat::new(Every::Month, 1),
        "year" | "yearly" | "annually" | "every year" => Repeat::new(Every::Year, 1),
        _ => return Err(()),
    }))
}

/// Words for a rule: "Every week on Tuesday, until Dec 20".
pub fn describe(r: &Repeat, first: NaiveDate) -> String {
    let base = match (r.every, r.interval.max(1)) {
        (Every::Weekday, _) => "Every weekday".to_string(),
        (Every::Day, 1) => "Every day".to_string(),
        (Every::Day, n) => format!("Every {n} days"),
        (Every::Week, 1) => format!("Every week on {}", first.format("%A")),
        (Every::Week, n) => format!("Every {n} weeks on {}", first.format("%A")),
        (Every::Month, 1) => format!("Every month on the {}", ordinal(first.day())),
        (Every::Month, n) => format!("Every {n} months on the {}", ordinal(first.day())),
        (Every::Year, 1) => format!("Every year on {}", first.format("%B %-d")),
        (Every::Year, n) => format!("Every {n} years on {}", first.format("%B %-d")),
    };
    match r.until_date() {
        Some(u) => format!("{base}, until {}", u.format("%b %-d, %Y")),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn rules_fall_on_the_right_days() {
        let weekly = Repeat::new(Every::Week, 1);
        assert_eq!(days(d(2026, 11, 3), &weekly, d(2026, 11, 1), d(2026, 11, 30)), vec![d(2026, 11, 3), d(2026, 11, 10), d(2026, 11, 17), d(2026, 11, 24)]);
        let fortnight = Repeat::new(Every::Week, 2);
        assert_eq!(days(d(2026, 1, 6), &fortnight, d(2026, 11, 1), d(2026, 11, 30)), vec![d(2026, 11, 10), d(2026, 11, 24)]);
        let weekdays = Repeat::new(Every::Weekday, 1);
        assert_eq!(days(d(2026, 11, 1), &weekdays, d(2026, 11, 6), d(2026, 11, 9)), vec![d(2026, 11, 6), d(2026, 11, 9)]);
        // An event of a Saturday that is made "every weekday": the Saturday stays, then Monday to Friday.
        assert_eq!(days(d(2026, 11, 7), &weekdays, d(2026, 11, 1), d(2026, 11, 15)), vec![d(2026, 11, 7), d(2026, 11, 9), d(2026, 11, 10), d(2026, 11, 11), d(2026, 11, 12), d(2026, 11, 13)]);
        // The 31st is skipped in months without one.
        let monthly = Repeat::new(Every::Month, 1);
        assert_eq!(days(d(2026, 1, 31), &monthly, d(2026, 1, 1), d(2026, 5, 31)), vec![d(2026, 1, 31), d(2026, 3, 31), d(2026, 5, 31)]);
        let yearly = Repeat::new(Every::Year, 1);
        assert_eq!(days(d(2024, 2, 29), &yearly, d(2024, 1, 1), d(2029, 1, 1)), vec![d(2024, 2, 29), d(2028, 2, 29)]);
        // An end date and deleted repeats.
        let mut daily = Repeat::new(Every::Day, 1);
        daily.until = Some("2026-11-05".into());
        daily.skip = vec!["2026-11-03".into()];
        assert_eq!(days(d(2026, 11, 1), &daily, d(2026, 10, 1), d(2026, 12, 1)), vec![d(2026, 11, 1), d(2026, 11, 2), d(2026, 11, 4), d(2026, 11, 5)]);
        // Nothing before the first day.
        assert!(days(d(2026, 11, 20), &weekly, d(2026, 11, 1), d(2026, 11, 19)).is_empty());
    }

    #[test]
    fn repeats_keep_their_time_and_find_their_event() {
        let start = when::at(d(2026, 11, 3), NaiveTime::from_hms_opt(13, 0, 0).unwrap());
        let mut e = Event::new("s", "Standup", start, start + when::HOUR, false);
        e.repeat = Some(Repeat::new(Every::Week, 1));
        let from = when::midnight(d(2026, 11, 9));
        let to = when::midnight(d(2026, 11, 16));
        let shown = occurrences(&e, from, to);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, format!("{}@2026-11-10", e.id));
        assert_eq!(when::time_label(shown[0].start), "1 PM");
        assert_eq!(shown[0].end - shown[0].start, when::HOUR);

        let events: HashMap<Id, Event> = [(e.id.clone(), e.clone())].into_iter().collect();
        assert_eq!(find(&events, &shown[0].id).unwrap().start, shown[0].start);
        assert!(find(&events, &format!("{}@2026-11-11", e.id)).is_none(), "not a day the rule falls on");
        assert_eq!(find(&events, &e.id).unwrap().start, start);
    }

    #[test]
    fn a_change_to_one_repeat_moves_them_all() {
        let start = when::at(d(2026, 11, 3), NaiveTime::from_hms_opt(13, 0, 0).unwrap());
        let mut e = Event::new("s", "Standup", start, start + when::HOUR, false);
        let mut rule = Repeat::new(Every::Week, 1);
        rule.skip = vec!["2026-11-17".into()];
        e.repeat = Some(rule);
        let before = at(&e, d(2026, 11, 10));
        let mut after = before.clone();
        after.title = "Team standup".into();
        after.join = "https://zoom.us/j/8123456789".into();
        after.start = when::at(d(2026, 11, 11), NaiveTime::from_hms_opt(14, 30, 0).unwrap());
        after.end = after.start + 30 * when::MINUTE;
        apply(&mut e, &before, &after);
        assert_eq!(e.title, "Team standup");
        // The link to join the call is the series', as its name is.
        assert_eq!(e.join, "https://zoom.us/j/8123456789");
        assert_eq!(at(&e, d(2026, 11, 25)).join_url().as_deref(), Some("https://zoom.us/j/8123456789"));
        assert_eq!(when::date_of(e.start), d(2026, 11, 4));
        assert_eq!(when::time_label(e.start), "2:30 PM");
        assert_eq!(e.end - e.start, 30 * when::MINUTE);
        assert_eq!(e.repeat.unwrap().skip, vec!["2026-11-18".to_string()]);
    }

    #[test]
    fn a_repeat_moved_to_a_later_day_loses_no_repeat() {
        let one = NaiveTime::from_hms_opt(13, 0, 0).unwrap();
        let later = |e: &Event, from: NaiveDate, to: NaiveDate| {
            let before = at(e, from);
            let mut after = before.clone();
            after.start = when::at(to, one);
            after.end = after.start + when::HOUR;
            let mut moved = e.clone();
            apply(&mut moved, &before, &after);
            moved
        };
        // Every week on Tuesday until Tuesday 15 December: seven times.
        let mut weekly = Event::new("s", "Review", when::at(d(2026, 11, 3), one), when::at(d(2026, 11, 3), one) + when::HOUR, false);
        let mut rule = Repeat::new(Every::Week, 1);
        rule.until = Some("2026-12-15".into());
        weekly.repeat = Some(rule);
        let all = |e: &Event| days(first_day(e), e.repeat.as_ref().unwrap(), d(2026, 10, 1), d(2027, 1, 31));
        assert_eq!(all(&weekly).len(), 7);
        // One repeat dragged to the Wednesday: all seven are on Wednesdays, the last on the 16th.
        let moved = later(&weekly, d(2026, 11, 10), d(2026, 11, 11));
        assert_eq!(all(&moved).len(), 7, "the last repeat is still there");
        assert_eq!((all(&moved)[0], all(&moved)[6]), (d(2026, 11, 4), d(2026, 12, 16)));
        assert_eq!(moved.repeat.as_ref().unwrap().until.as_deref(), Some("2026-12-16"));
        // Every day, to 15 December: a repeat in the middle dragged a day later keeps the first day and the last.
        let mut daily = weekly.clone();
        daily.repeat = Some(Repeat { every: Every::Day, ..weekly.repeat.clone().unwrap() });
        let moved = later(&daily, d(2026, 11, 10), d(2026, 11, 11));
        assert_eq!((when::date_of(moved.start), moved.repeat.as_ref().unwrap().until.as_deref()), (d(2026, 11, 3), Some("2026-12-15")));
        assert_eq!(all(&moved), all(&daily));
        // Its first repeat moved a day later: that is its new first day.
        let moved = later(&daily, d(2026, 11, 3), d(2026, 11, 4));
        assert_eq!(when::date_of(moved.start), d(2026, 11, 4));
    }

    #[test]
    fn each_repeat_has_its_own_notes_page() {
        let start = when::at(d(2026, 11, 3), NaiveTime::from_hms_opt(13, 0, 0).unwrap());
        let mut e = Event::new("s", "One to one", start, start + when::HOUR, false);
        e.repeat = Some(Repeat::new(Every::Week, 1));
        // The pages that are there, and the day each was made on.
        let made = |p: &str| match p {
            "week-1" => Some(d(2026, 11, 3)),
            "week-2" => Some(d(2026, 11, 10)),
            "old" => Some(d(2026, 11, 17)),
            "early" => Some(d(2026, 11, 20)),
            _ => None,
        };
        let mut own: HashMap<String, Id> = HashMap::new();
        own.insert(page_key(&e, d(2026, 11, 3)), "week-1".into());
        own.insert(page_key(&e, d(2026, 11, 10)), "week-2".into());
        own.insert(page_key(&e, d(2026, 11, 24)), "gone".into());
        assert_eq!(page_of(&e, d(2026, 11, 3), &own, made).as_deref(), Some("week-1"));
        assert_eq!(page_of(&e, d(2026, 11, 10), &own, made).as_deref(), Some("week-2"), "the second week does not open the page of the first");
        assert_eq!(page_of(&e, d(2026, 11, 17), &own, made), None, "a week with no page yet");
        assert_eq!(page_of(&e, d(2026, 11, 24), &own, made), None, "a page that is gone is no page");
        // A page kept on the event itself, by a Rune that had one page for
        // all repeats: it is the page of the repeat of the day it was made on.
        e.page_id = Some("old".into());
        assert_eq!(page_of(&e, d(2026, 11, 17), &own, made).as_deref(), Some("old"));
        assert_eq!(page_of(&e, d(2026, 12, 1), &own, made), None);
        assert_eq!(page_of(&e, d(2026, 11, 10), &own, made).as_deref(), Some("week-2"));
        // That repeat deleted since: no other repeat gets its page.
        e.repeat.as_mut().unwrap().skip = vec!["2026-11-17".into()];
        assert_eq!(page_of(&e, d(2026, 11, 3), &HashMap::new(), made), None);
        // Made on a day that is no day of the event: the page of the first repeat.
        e.page_id = Some("early".into());
        assert_eq!(page_of(&e, d(2026, 11, 3), &HashMap::new(), made).as_deref(), Some("early"));
        assert_eq!(page_of(&e, d(2026, 11, 10), &HashMap::new(), made), None);
    }

    #[test]
    fn the_page_of_a_repeat_is_kept_by_a_day_that_no_time_zone_changes() {
        // Every week from 04:00 UTC on Wednesday 11 November 2026: a Tuesday
        // evening in Los Angeles, a Wednesday morning in Berlin.
        let start = d(2026, 11, 11).and_hms_opt(4, 0, 0).unwrap().and_utc().timestamp_millis();
        let mut e = Event::new("s", "Call", start, start + when::HOUR, false);
        e.repeat = Some(Repeat::new(Every::Week, 1));
        // Whatever day the repeat has in the zone of this Mac, its page is kept by the same day.
        let here = when::date_of(start);
        assert_eq!(page_key(&e, here), format!("{}@2026-11-11", e.id));
        assert_eq!(page_key(&e, when::add_days(here, 7)), format!("{}@2026-11-18", e.id));
        // An all-day event is read on the same day in every zone (`when::whole_day`): by that day.
        let trip = Event::new("s", "Trip", when::midnight(d(2026, 11, 20)), when::midnight(d(2026, 11, 21)), true);
        assert_eq!(page_key(&trip, d(2026, 11, 20)), format!("{}@2026-11-20", trip.id));
    }

    #[test]
    fn each_repeat_of_an_evening_event_has_a_key_of_its_own_across_a_clock_change() {
        // Every day at 19:30. (Run with TZ=America/New_York this is the
        // reported case: Saturday 7 March 2026 starts at 00:30 UTC of the
        // 8th and Sunday the 8th, after the clocks went forward, at 23:30
        // UTC of the 8th. In every zone: no two repeats of a year and more
        // share a key, and the keys are a day apart.)
        let first = d(2026, 1, 1);
        let start = when::at(first, NaiveTime::from_hms_opt(19, 30, 0).unwrap());
        let mut e = Event::new("s", "Evening call", start, start + when::HOUR, false);
        e.repeat = Some(Repeat::new(Every::Day, 1));
        assert_ne!(page_key(&e, d(2026, 3, 7)), page_key(&e, d(2026, 3, 8)));
        let keys: Vec<String> = (0..500).map(|n| page_key(&e, when::add_days(first, n))).collect();
        let different: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(different.len(), keys.len());
        for pair in keys.windows(2) {
            let day = |key: &String| split_id(key).1.unwrap();
            assert_eq!((day(&pair[1]) - day(&pair[0])).num_days(), 1);
        }
        // The first repeat is kept by the UTC day of its start, as a reader in any zone works it out.
        assert_eq!(keys[0], instance_id(&e.id, chrono::DateTime::from_timestamp_millis(start).unwrap().date_naive()));
    }

    #[test]
    fn the_pages_of_repeats_go_with_their_repeats() {
        let one = NaiveTime::from_hms_opt(13, 0, 0).unwrap();
        let event = |first: NaiveDate, every: Every, interval: u32| {
            let mut e = Event::new("s", "One to one", when::at(first, one), when::at(first, one) + when::HOUR, false);
            e.id = "ev".into();
            e.repeat = Some(Repeat::new(every, interval));
            e
        };
        // The event as it is after its first day moved to `first` (see `apply`).
        let from = |e: &Event, first: NaiveDate| Event { start: when::at(first, one), end: when::at(first, one) + when::HOUR, ..e.clone() };
        let pages = |e: &Event, list: &[(NaiveDate, &str)]| -> HashMap<String, Id> { list.iter().map(|(day, page)| (page_key(e, *day), page.to_string())).collect() };
        let page_on = |own: &HashMap<String, Id>, e: &Event, day: NaiveDate| own.get(&page_key(e, day)).cloned();

        // Every week, moved a day later: each page goes with its repeat; the pages of another event stay.
        let weekly = event(d(2026, 11, 3), Every::Week, 1);
        let mut own = pages(&weekly, &[(d(2026, 11, 3), "week-1"), (d(2026, 11, 10), "week-2")]);
        own.insert("other@2026-11-03".into(), "theirs".into());
        let moved = from(&weekly, d(2026, 11, 4));
        assert_eq!(carry_pages(&mut own, &weekly, &moved), None);
        assert_eq!((page_on(&own, &moved, d(2026, 11, 4)).as_deref(), page_on(&own, &moved, d(2026, 11, 11)).as_deref()), (Some("week-1"), Some("week-2")));
        assert_eq!(own.len(), 3);
        assert_eq!(own.get("other@2026-11-03").map(String::as_str), Some("theirs"));

        // Every month on the 15th, moved 20 days later: the repeats are on
        // the 5th, 20 or 21 days on. Each page is still with its repeat.
        let monthly = event(d(2026, 10, 15), Every::Month, 1);
        let mut own = pages(&monthly, &[(d(2026, 10, 15), "oct"), (d(2026, 11, 15), "nov"), (d(2027, 1, 15), "jan")]);
        let moved = from(&monthly, d(2026, 11, 4));
        carry_pages(&mut own, &monthly, &moved);
        assert_eq!(
            (page_on(&own, &moved, d(2026, 11, 4)).as_deref(), page_on(&own, &moved, d(2026, 12, 4)).as_deref(), page_on(&own, &moved, d(2027, 2, 4)).as_deref()),
            (Some("oct"), Some("nov"), Some("jan")),
            "{own:?}"
        );
        assert_eq!(own.len(), 3);

        // Every day, its first repeat moved a day later: the event is on the
        // same days, so a page stays on its day, and the page of the first
        // repeat goes with it to its new day when that day has none.
        let daily = event(d(2026, 11, 2), Every::Day, 1);
        let moved = from(&daily, d(2026, 11, 3));
        let mut own = pages(&daily, &[(d(2026, 11, 2), "monday"), (d(2026, 11, 4), "wednesday")]);
        carry_pages(&mut own, &daily, &moved);
        assert_eq!((page_on(&own, &moved, d(2026, 11, 3)).as_deref(), page_on(&own, &moved, d(2026, 11, 4)).as_deref()), (Some("monday"), Some("wednesday")), "{own:?}");
        // …and when that day has its own page, both pages are kept (the first one by its old day).
        let mut own = pages(&daily, &[(d(2026, 11, 2), "monday"), (d(2026, 11, 3), "tuesday")]);
        carry_pages(&mut own, &daily, &moved);
        assert_eq!(page_on(&own, &moved, d(2026, 11, 3)).as_deref(), Some("tuesday"));
        assert_eq!(own.len(), 2);

        // Another rule (every two weeks): a page stays on its day.
        let mut own = pages(&weekly, &[(d(2026, 11, 3), "week-1"), (d(2026, 11, 10), "week-2"), (d(2026, 11, 17), "week-3")]);
        let fortnight = event(d(2026, 11, 3), Every::Week, 2);
        carry_pages(&mut own, &weekly, &fortnight);
        assert_eq!((page_on(&own, &fortnight, d(2026, 11, 3)).as_deref(), page_on(&own, &fortnight, d(2026, 11, 17)).as_deref()), (Some("week-1"), Some("week-3")));
        assert_eq!(own.len(), 3, "the page of a week that is no repeat now is still in the list");

        // A new time of day: the same repeats, each with its page.
        let mut own = pages(&weekly, &[(d(2026, 11, 3), "week-1"), (d(2026, 11, 10), "week-2")]);
        let late = Event { start: when::at(d(2026, 11, 3), NaiveTime::from_hms_opt(23, 30, 0).unwrap()), ..weekly.clone() };
        carry_pages(&mut own, &weekly, &late);
        assert_eq!((page_on(&own, &late, d(2026, 11, 3)).as_deref(), page_on(&own, &late, d(2026, 11, 10)).as_deref()), (Some("week-1"), Some("week-2")));

        // It repeats no more: the page of the first repeat is given back for the event, and the list has none of its pages.
        let mut own = pages(&weekly, &[(d(2026, 11, 3), "week-1"), (d(2026, 11, 10), "week-2")]);
        own.insert("other@2026-11-03".into(), "theirs".into());
        let once = Event { repeat: None, ..weekly.clone() };
        assert_eq!(carry_pages(&mut own, &weekly, &once).as_deref(), Some("week-1"));
        assert_eq!(own.keys().map(String::as_str).collect::<Vec<_>>(), vec!["other@2026-11-03"]);
    }

    #[test]
    fn an_all_day_event_made_in_another_time_zone_stays_on_its_day() {
        let midnight = |day: u32| when::midnight(d(2026, 11, day));
        // Kept as the midnights of a zone eight hours to the west, of one
        // eight hours to the east, and of this one.
        for off in [8 * when::HOUR, -8 * when::HOUR, 0] {
            let mut e = Event::new("s", "Trip", midnight(20) + off, midnight(21) + off, true);
            let shown = occurrences(&e, midnight(15), midnight(25));
            assert_eq!(shown.iter().map(|o| (o.start, o.end)).collect::<Vec<_>>(), vec![(midnight(20), midnight(21))], "one day, the 20th ({off})");
            assert!(occurrences(&e, midnight(19), midnight(20)).is_empty(), "not on the day before ({off})");
            assert!(occurrences(&e, midnight(21), midnight(22)).is_empty(), "not on the day after ({off})");
            let events: HashMap<Id, Event> = [(e.id.clone(), e.clone())].into_iter().collect();
            assert_eq!(find(&events, &e.id).map(|o| (o.start, o.end)), Some((midnight(20), midnight(21))));
            // Every week: it stays on its day of the week, and a deleted day stays deleted.
            let mut rule = Repeat::new(Every::Week, 1);
            rule.skip = vec!["2026-11-27".into()];
            e.repeat = Some(rule);
            let shown = occurrences(&e, midnight(15), when::midnight(d(2026, 12, 6)));
            assert_eq!(shown.iter().map(|o| when::date_of(o.start)).collect::<Vec<_>>(), vec![d(2026, 11, 20), d(2026, 12, 4)], "{off}");
            assert!(shown.iter().all(|o| o.end - o.start <= 25 * when::HOUR), "each repeat is one day ({off})");
        }
        // An event with a time is the moment it is: not rounded.
        let timed = Event::new("s", "Call", midnight(20) + 8 * when::HOUR, midnight(20) + 9 * when::HOUR, false);
        assert_eq!(as_read(&timed), timed);
    }

    #[test]
    fn a_copy_of_an_all_day_event_is_whole_days_long() {
        // The day the clocks go back in the United States, and the day they go forward: 25 and 23 hours there.
        for day in [d(2026, 11, 1), d(2026, 3, 8), d(2026, 11, 20)] {
            let e = Event::new("s", "Trip", when::midnight(day), when::midnight(when::next_day(day)), true);
            // Pasted on a plain day, and on each of those days: one day, to the next midnight.
            for onto in [d(2026, 11, 10), d(2026, 11, 1), d(2026, 3, 8)] {
                assert_eq!(whole_days_from(&e, onto), (when::midnight(onto), when::midnight(when::next_day(onto))), "{day} onto {onto}");
            }
        }
        let three = Event::new("s", "Trip", when::midnight(d(2026, 10, 31)), when::midnight(d(2026, 11, 3)), true);
        assert_eq!(whole_days_from(&three, d(2026, 3, 7)), (when::midnight(d(2026, 3, 7)), when::midnight(d(2026, 3, 10))));
    }

    #[test]
    fn all_day_repeats_cover_whole_days() {
        let first = d(2026, 11, 2);
        let mut e = Event::new("s", "Trip", when::midnight(first), when::midnight(d(2026, 11, 4)), true);
        e.repeat = Some(Repeat::new(Every::Month, 1));
        let shown = occurrences(&e, when::midnight(d(2026, 12, 3)), when::midnight(d(2026, 12, 4)));
        assert_eq!(shown.len(), 1, "a two-day repeat that began the day before still shows");
        assert_eq!(when::date_of(shown[0].start), d(2026, 12, 2));
        assert_eq!(shown[0].end, when::midnight(d(2026, 12, 4)));
    }

    #[test]
    fn words_and_choices() {
        assert_eq!(parse("Weekly").unwrap(), Some(Repeat::new(Every::Week, 1)));
        assert_eq!(parse("every-other-week").unwrap(), Some(Repeat::new(Every::Week, 2)));
        assert_eq!(parse("none").unwrap(), None);
        assert!(parse("sometimes").is_err());
        assert_eq!(choice_of(Some(&Repeat::new(Every::Week, 2))), "2week");
        assert_eq!(choice_of(Some(&Repeat::new(Every::Day, 3))), "custom");
        let mut r = Repeat::new(Every::Month, 1);
        r.until = Some("2026-12-20".into());
        assert_eq!(describe(&r, d(2026, 11, 1)), "Every month on the 1st, until Dec 20, 2026");
        assert_eq!(Repeat::from_json(&Repeat::to_json(Some(&r))), Some(r));
        assert_eq!(Repeat::from_json(""), None);
    }
}
