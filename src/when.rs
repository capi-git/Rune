//! Dates and times for the calendar, in the Mac's local time zone.

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};

pub const MINUTE: i64 = 60_000;
pub const HOUR: i64 = 60 * MINUTE;

pub fn local(ms: i64) -> DateTime<Local> {
    Local.timestamp_millis_opt(ms).single().unwrap_or_else(Local::now)
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn date_of(ms: i64) -> NaiveDate {
    local(ms).date_naive()
}

/// Milliseconds at the given local date and time (the earlier one if the
/// clock repeats that hour; the next valid time if it's skipped).
pub fn at(date: NaiveDate, time: NaiveTime) -> i64 {
    let naive = NaiveDateTime::new(date, time);
    match Local.from_local_datetime(&naive).earliest() {
        Some(t) => t.timestamp_millis(),
        None => Local.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest().map(|t| t.timestamp_millis()).unwrap_or(0),
    }
}

pub fn midnight(date: NaiveDate) -> i64 {
    at(date, NaiveTime::MIN)
}

/// The day that the start of an all-day event means (and, for its end, the
/// day after its last). Such an event is kept as the moments of midnight in
/// the time zone it was made in. After travel those moments are some hours
/// off the midnight of this zone: taken as they are, the event showed on the
/// day before or on two days, and its reminder came a day early. The nearest
/// midnight here is the day that was meant (while the two zones are less
/// than twelve hours apart).
pub fn whole_day(ms: i64) -> NaiveDate {
    whole_day_in(&Local, ms)
}

fn whole_day_in<Z: TimeZone>(zone: &Z, ms: i64) -> NaiveDate {
    zone.timestamp_millis_opt(ms + 12 * HOUR).single().map(|t| t.date_naive()).unwrap_or_else(|| date_of(ms))
}

/// The next whole hour by the local clock after a moment (10:10 gives
/// 11:00). Worked out in hours since 1970, it was a half hour (10:30) where
/// the zone is a half hour off, as in India.
pub fn next_hour(ms: i64) -> i64 {
    next_hour_in(&Local, ms)
}

fn next_hour_in<Z: TimeZone>(zone: &Z, ms: i64) -> i64 {
    let Some(t) = zone.timestamp_millis_opt(ms).single() else { return (ms / HOUR + 1) * HOUR };
    let into_the_hour = i64::from(t.minute()) * MINUTE + i64::from(t.second()) * 1000 + i64::from(t.timestamp_subsec_millis());
    ms - into_the_hour + HOUR
}

pub fn next_day(date: NaiveDate) -> NaiveDate {
    date.checked_add_days(Days::new(1)).unwrap_or(date)
}

pub fn add_days(date: NaiveDate, n: i64) -> NaiveDate {
    if n >= 0 {
        date.checked_add_days(Days::new(n as u64)).unwrap_or(date)
    } else {
        date.checked_sub_days(Days::new((-n) as u64)).unwrap_or(date)
    }
}

pub fn add_months(date: NaiveDate, n: i32) -> NaiveDate {
    let months = date.year() * 12 + date.month0() as i32 + n;
    let (y, m) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(date)
}

/// Sunday of the week containing `date`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    add_days(date, -(date.weekday().num_days_from_sunday() as i64))
}

/// The 6×7 days shown for the month containing `date`, Sunday first.
pub fn month_grid(date: NaiveDate) -> Vec<NaiveDate> {
    let first = date.with_day(1).unwrap_or(date);
    let start = week_start(first);
    (0..42).map(|i| add_days(start, i)).collect()
}

pub fn week_days(date: NaiveDate) -> Vec<NaiveDate> {
    let start = week_start(date);
    (0..7).map(|i| add_days(start, i)).collect()
}

/// "2:30 PM", or "2 PM" on the hour.
pub fn time_label(ms: i64) -> String {
    let t = local(ms);
    let (pm, h) = t.hour12();
    let suffix = if pm { "PM" } else { "AM" };
    if t.minute() == 0 {
        format!("{h} {suffix}")
    } else {
        format!("{h}:{:02} {suffix}", t.minute())
    }
}

pub fn hour_label(hour: u32) -> String {
    match hour {
        0 => "12 AM".into(),
        12 => "Noon".into(),
        h if h < 12 => format!("{h} AM"),
        h => format!("{} PM", h - 12),
    }
}

/// "Today", "Tomorrow", or "Tue, Nov 10".
pub fn day_label(date: NaiveDate) -> String {
    let t = today();
    if date == t {
        "Today".into()
    } else if date == next_day(t) {
        "Tomorrow".into()
    } else {
        date.format("%a, %b %-d").to_string()
    }
}

/// A due day in a few letters, as seen from `today`: "Today", "Tomorrow",
/// "Yesterday", the day's name in the week ahead ("Thu"), else "Nov 20"
/// (with its year when that is not this year).
pub fn due_label(date: NaiveDate, today: NaiveDate) -> String {
    let days = (date - today).num_days();
    match days {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        2..=6 => date.format("%a").to_string(),
        _ if date.year() == today.year() => date.format("%b %-d").to_string(),
        _ => date.format("%b %-d, %Y").to_string(),
    }
}

/// How far an event is from now, in a few words: "In 40 minutes", "Now",
/// "Ended 2 hours ago". More than a day away, it is the day ("Tomorrow").
pub fn from_now(start: i64, end: i64, now: i64) -> String {
    let count = |n: i64, unit: &str| if n == 1 { format!("1 {unit}") } else { format!("{n} {unit}s") };
    let span = |ms: i64| {
        let mins = ms / MINUTE;
        if mins < 60 { count(mins.max(1), "minute") } else { count(mins / 60, "hour") }
    };
    if now >= start && now < end {
        "Now".into()
    } else if now < start {
        if start - now >= 24 * HOUR { day_label(date_of(start)) } else { format!("In {}", span(start - now)) }
    } else if now - end >= 24 * HOUR {
        day_label(date_of(start))
    } else {
        format!("Ended {} ago", span(now - end))
    }
}

/// What day it is, for an AI that has to turn "on Friday" into a date.
pub fn today_for_ai() -> String {
    today().format("Today is %A, %Y-%m-%d.").to_string()
}

/// Minutes since local midnight.
pub fn minutes_into_day(ms: i64) -> i64 {
    let t = local(ms);
    (t.hour() * 60 + t.minute()) as i64
}

/// "2026-11-10" for a date input.
pub fn date_value(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// "14:30" for a time input.
pub fn time_value(ms: i64) -> String {
    local(ms).format("%H:%M").to_string()
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// Read a time written by an agent: "2026-11-10" (a day), "2026-11-10T14:00"
/// (local time) or a full RFC 3339 time with an offset. Returns (ms, all_day).
pub fn parse_iso(s: &str) -> Option<(i64, bool)> {
    let s = s.trim();
    if let Some(d) = parse_date(s) {
        return Some((midnight(d), true));
    }
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some((t.timestamp_millis(), false));
    }
    for f in ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return Some((at(n.date(), n.time()), false));
        }
    }
    None
}

/// "2026-11-10T14:00" in local time, or "2026-11-10" for a day.
pub fn iso(ms: i64, all_day: bool) -> String {
    if all_day {
        date_value(date_of(ms))
    } else {
        local(ms).format("%Y-%m-%dT%H:%M").to_string()
    }
}

/// "just now", "5 min ago", "3 hr ago", "Yesterday", "Nov 3".
pub fn ago(ms: i64) -> String {
    let diff = crate::model::now_ms() - ms;
    if diff < MINUTE {
        "just now".into()
    } else if diff < HOUR {
        format!("{} min ago", diff / MINUTE)
    } else if date_of(ms) == today() {
        format!("{} hr ago", diff / HOUR)
    } else if next_day(date_of(ms)) == today() {
        "Yesterday".into()
    } else {
        date_of(ms).format("%b %-d").to_string()
    }
}

/// "Today, 3:42 PM", "Yesterday, 9 AM", "Tue, Nov 3, 3:42 PM".
pub fn moment(ms: i64) -> String {
    let d = date_of(ms);
    let t = today();
    let day = if d == t {
        "Today".to_string()
    } else if next_day(d) == t {
        "Yesterday".to_string()
    } else if d.year() == t.year() {
        d.format("%a, %b %-d").to_string()
    } else {
        d.format("%b %-d, %Y").to_string()
    };
    format!("{day}, {}", time_label(ms))
}

/// `moment` with seconds ("Today, 3:42:07 PM"), to tell apart two moments in
/// the same minute.
pub fn moment_exact(ms: i64) -> String {
    let t = local(ms);
    let (pm, h) = t.hour12();
    let short = moment(ms);
    let day = short.rsplit_once(", ").map(|(d, _)| d).unwrap_or(&short);
    format!("{day}, {h}:{:02}:{:02} {}", t.minute(), t.second(), if pm { "PM" } else { "AM" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_due_day_in_a_few_letters() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(); // a Tuesday
        let day = |n| add_days(today, n);
        assert_eq!(due_label(day(0), today), "Today");
        assert_eq!(due_label(day(1), today), "Tomorrow");
        assert_eq!(due_label(day(-1), today), "Yesterday");
        assert_eq!(due_label(day(3), today), "Fri");
        assert_eq!(due_label(day(6), today), "Mon");
        assert_eq!(due_label(day(7), today), "Nov 17");
        assert_eq!(due_label(day(-5), today), "Nov 5");
        assert_eq!(due_label(day(60), today), "Jan 9, 2027");
    }

    #[test]
    fn how_far_an_event_is_from_now() {
        let start = 1_000 * HOUR;
        let end = start + HOUR;
        assert_eq!(from_now(start, end, start - 40 * MINUTE), "In 40 minutes");
        assert_eq!(from_now(start, end, start - MINUTE), "In 1 minute");
        assert_eq!(from_now(start, end, start - 20_000), "In 1 minute");
        assert_eq!(from_now(start, end, start - 3 * HOUR - 5 * MINUTE), "In 3 hours");
        assert_eq!(from_now(start, end, start), "Now");
        assert_eq!(from_now(start, end, end - 1), "Now");
        assert_eq!(from_now(start, end, end + 2 * HOUR), "Ended 2 hours ago");
        assert_eq!(from_now(start, end, end + 5 * MINUTE), "Ended 5 minutes ago");
    }

    #[test]
    fn an_all_day_event_keeps_its_day_in_another_time_zone() {
        let los_angeles = chrono::FixedOffset::west_opt(8 * 3600).unwrap();
        let london = chrono::FixedOffset::east_opt(0).unwrap();
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
        let midnight_in = |zone: &chrono::FixedOffset, d: u32| zone.from_local_datetime(&NaiveDateTime::new(day(d), NaiveTime::MIN)).unwrap().timestamp_millis();
        // "Trip" on 20 November, made in Los Angeles: from that midnight to the next.
        let (start, end) = (midnight_in(&los_angeles, 20), midnight_in(&los_angeles, 21));
        assert_eq!((whole_day_in(&los_angeles, start), whole_day_in(&los_angeles, end)), (day(20), day(21)));
        // Read in London, those moments are 8 in the morning: still the 20th, one day.
        // (Taken as they are, the end fell in the 21st, and the trip showed on two days.)
        assert_eq!(london.timestamp_millis_opt(end - 1).unwrap().date_naive(), day(21));
        assert_eq!((whole_day_in(&london, start), whole_day_in(&london, end)), (day(20), day(21)));
        // Made in London and read in Los Angeles, they are 4 in the afternoon of the day before: still the 20th.
        // (Taken as they are, the trip began on the 19th, and its reminder came then.)
        let (start, end) = (midnight_in(&london, 20), midnight_in(&london, 21));
        assert_eq!(los_angeles.timestamp_millis_opt(start).unwrap().date_naive(), day(19));
        assert_eq!((whole_day_in(&los_angeles, start), whole_day_in(&los_angeles, end)), (day(20), day(21)));
        // In this Mac's own zone a midnight is its own day, also on the days the clocks change.
        for d in [NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(), day(20)] {
            assert_eq!((whole_day(midnight(d)), whole_day(midnight(next_day(d)))), (d, next_day(d)));
        }
    }

    #[test]
    fn the_next_whole_hour_is_by_the_local_clock() {
        let india = chrono::FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let clock = |h: u32, m: u32| india.from_local_datetime(&NaiveDateTime::new(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(), NaiveTime::from_hms_opt(h, m, 0).unwrap())).unwrap().timestamp_millis();
        // 10:10 in India: 11:00 there. (In hours since 1970 it was 10:30.)
        assert_eq!(next_hour_in(&india, clock(10, 10)), clock(11, 0));
        assert_eq!((clock(10, 10) / HOUR + 1) * HOUR, clock(10, 30));
        assert_eq!(next_hour_in(&india, clock(10, 0)), clock(11, 0));
        assert_eq!(next_hour_in(&india, clock(10, 59) + 59_999), clock(11, 0));
        // In this Mac's zone: a whole hour of its clock, less than an hour away.
        let now = at(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(), NaiveTime::from_hms_opt(10, 10, 0).unwrap());
        assert_eq!(time_label(next_hour(now)), "11 AM");
    }

    #[test]
    fn grid_and_weeks_start_on_sunday() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(); // a Monday
        assert_eq!(week_start(d), NaiveDate::from_ymd_opt(2026, 11, 8).unwrap());
        let grid = month_grid(d);
        assert_eq!(grid.len(), 42);
        assert_eq!(grid[0], NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()); // Nov 1 2026 is a Sunday
        assert_eq!(add_months(d, 2), NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert_eq!(add_months(d, -11), NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
    }

    #[test]
    fn times_round_trip() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let t = at(d, NaiveTime::from_hms_opt(14, 30, 0).unwrap());
        assert_eq!(iso(t, false), "2026-11-10T14:30");
        assert_eq!(parse_iso("2026-11-10T14:30"), Some((t, false)));
        assert_eq!(parse_iso("2026-11-10"), Some((midnight(d), true)));
        assert_eq!(time_label(t), "2:30 PM");
        assert_eq!(minutes_into_day(t), 14 * 60 + 30);
        assert_eq!(hour_label(0), "12 AM");
        assert_eq!(hour_label(15), "3 PM");
        assert!(parse_iso("not a date").is_none());
    }
}
