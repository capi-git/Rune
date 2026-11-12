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
                if d.weekday().num_days_from_monday() < 5 {
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
    ((when::date_of(e.end - 1) - when::date_of(e.start)).num_days() + 1).max(1)
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
        return if e.overlaps(from, to) { vec![e.clone()] } else { Vec::new() };
    };
    if to <= from {
        return Vec::new();
    }
    // A repeat that starts a little before `from` may still run into it.
    let span = (when::date_of(e.end - 1) - when::date_of(e.start)).num_days().max(0) + 1;
    let lo = when::add_days(when::date_of(from), -span);
    let hi = when::date_of(to - 1);
    days(when::date_of(e.start), r, lo, hi).into_iter().map(|d| at(e, d)).filter(|o| o.overlaps(from, to)).collect()
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
        (None, _) => Some(e.clone()),
        (Some(d), Some(r)) => days(when::date_of(e.start), r, d, d).first().map(|&d| at(e, d)),
        (Some(d), None) => (d == when::date_of(e.start)).then(|| e.clone()),
    }
}

/// Carry a change made to one repeat (`before` → `after`) to the stored event
/// `e`: the same fields, and the same move in days and time of day.
pub fn apply(e: &mut Event, before: &Event, after: &Event) {
    let shift = (when::date_of(after.start) - when::date_of(before.start)).num_days();
    let first = when::add_days(when::date_of(e.start), shift);
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
        let same_days = |r: &Repeat| r.every == Every::Weekday || (r.every == Every::Day && r.interval <= 1);
        if let Some(r) = e.repeat.as_mut().filter(|r| !same_days(r)) {
            for s in &mut r.skip {
                if let Some(d) = when::parse_date(s) {
                    *s = when::date_value(when::add_days(d, shift));
                }
            }
        }
    }
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
