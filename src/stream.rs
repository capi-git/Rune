//! The Stream: one long page of thoughts for the whole app, whatever space
//! is in front, written day after day. Each day starts with a date line (a
//! small heading, so that search, the AI and Markdown all know it), the
//! newest day is at the foot, and the page opens there.
//!
//! A day starts at 4 in the morning, not at midnight: what is written at 1
//! in the night stays with the evening it belongs to.

use crate::model::{Block, BlockKind};
use chrono::{DateTime, Local, NaiveDate};

/// The hour at which a new day of the Stream starts.
pub const DAY_STARTS_AT: i64 = 4;

/// The icon of the Stream page, and what marks a page as the Stream. (It
/// is not one of the icons a person or the AI can give a page.)
pub const ICON: &str = "stream:gray";

/// Whether `page` is a Stream page (by its icon; see `ICON`).
pub fn is_stream_page(page: &crate::model::Page) -> bool {
    page.icon.as_deref().is_some_and(|i| i.split(':').next() == Some("stream"))
}

/// The day of the Stream that `now` is in.
pub fn day_of(now: DateTime<Local>) -> NaiveDate {
    (now - chrono::Duration::hours(DAY_STARTS_AT)).date_naive()
}

/// The words of a day's date line, as "Tuesday, November 17, 2026".
pub fn day_line(day: NaiveDate) -> String {
    day.format("%A, %B %-d, %Y").to_string()
}

/// The day a date line names, if `text` is one.
pub fn parse_day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%A, %B %-d, %Y").ok()
}

fn is_day_line(b: &Block) -> Option<NaiveDate> {
    (b.kind == BlockKind::Heading3).then(|| parse_day(&b.plain_text())).flatten()
}

/// The day of the last date line of the page.
pub fn last_day(blocks: &[Block]) -> Option<NaiveDate> {
    blocks.iter().rev().find_map(is_day_line)
}

/// A line with nothing in it: an empty paragraph.
fn is_blank(b: &Block) -> bool {
    b.kind == BlockKind::Text && b.plain_text().trim().is_empty()
}

/// Make the page ready to write on `today`: a date line for it at the foot
/// (or the last date line moved to today when nothing was written under
/// it), and a line to write on. Gives the id of the line for the caret and
/// whether the page changed. Lines that were written are never changed.
pub fn open_day(blocks: &mut Vec<Block>, today: NaiveDate) -> (String, bool) {
    let mut changed = false;
    let last_day = blocks.iter().rposition(|b| is_day_line(b).is_some());
    match last_day {
        Some(i) if is_day_line(&blocks[i]) == Some(today) => {}
        // Nothing written under the last date line: it becomes today's.
        Some(i) if blocks[i + 1..].iter().all(is_blank) => {
            blocks[i].text = day_line(today);
            blocks.truncate(i + 1);
            changed = true;
        }
        // A new page (or one with only empty lines): today's line first.
        None if blocks.iter().all(is_blank) => {
            blocks.clear();
            blocks.push(Block::new(BlockKind::Heading3, day_line(today)));
            changed = true;
        }
        _ => {
            // (An empty line at the foot is taken away: the date line is
            // what parts the days.)
            while blocks.last().is_some_and(is_blank) {
                blocks.pop();
            }
            blocks.push(Block::new(BlockKind::Heading3, day_line(today)));
            changed = true;
        }
    }
    // The caret goes to the end of the last paragraph of today, or to a new
    // line under the date line.
    let last = blocks.last().expect("the page has today's date line");
    if last.kind == BlockKind::Text {
        return (last.id.clone(), changed);
    }
    let line = Block::new(BlockKind::Text, "");
    let id = line.id.clone();
    blocks.push(line);
    (id, true)
}

/// What was written on a day before, shown at the foot of the Stream.
#[derive(Clone, Debug, PartialEq)]
pub struct Echo {
    /// "A week ago" or "A year ago".
    pub label: &'static str,
    /// The id of that day's date line (a click goes there).
    pub line: String,
    /// The first words written that day.
    pub words: String,
}

/// How many letters of a day an echo shows.
const ECHO_LETTERS: usize = 140;

/// What was written a year and a week before `today`, for the days that
/// have words (a day with nothing written has no echo).
pub fn echoes(blocks: &[Block], today: NaiveDate) -> Vec<Echo> {
    let year = today.checked_sub_months(chrono::Months::new(12));
    let week = today.checked_sub_days(chrono::Days::new(7));
    let mut out = Vec::new();
    for (label, day) in [("A year ago", year), ("A week ago", week)] {
        let Some(day) = day else { continue };
        let Some(at) = blocks.iter().position(|b| is_day_line(b) == Some(day)) else { continue };
        let words: Vec<String> = blocks[at + 1..]
            .iter()
            .take_while(|b| is_day_line(b).is_none())
            .map(|b| b.plain_text().trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let words = words.join(" ");
        if words.is_empty() {
            continue;
        }
        let words = if words.chars().count() > ECHO_LETTERS {
            format!("{}…", words.chars().take(ECHO_LETTERS).collect::<String>().trim_end())
        } else {
            words
        };
        out.push(Echo { label, line: blocks[at].id.clone(), words });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn text(t: &str) -> Block {
        Block::new(BlockKind::Text, t)
    }

    fn shape(blocks: &[Block]) -> Vec<String> {
        blocks.iter().map(|b| format!("{}:{}", b.kind.as_str(), b.plain_text())).collect()
    }

    #[test]
    fn date_line_round_trips() {
        let d = day("2026-11-17");
        assert_eq!(day_line(d), "Tuesday, November 17, 2026");
        assert_eq!(parse_day(&day_line(d)), Some(d));
        assert_eq!(parse_day("Tuesday plans"), None);
    }

    #[test]
    fn a_day_starts_at_four() {
        use chrono::TimeZone;
        let late = Local.with_ymd_and_hms(2026, 11, 18, 1, 30, 0).unwrap();
        assert_eq!(day_of(late), day("2026-11-17"));
        let morning = Local.with_ymd_and_hms(2026, 11, 18, 4, 0, 0).unwrap();
        assert_eq!(day_of(morning), day("2026-11-18"));
    }

    #[test]
    fn new_page_gets_todays_line() {
        let mut bs = vec![text("")];
        let (caret, changed) = open_day(&mut bs, day("2026-11-17"));
        assert!(changed);
        assert_eq!(shape(&bs), ["heading3:Tuesday, November 17, 2026", "text:"]);
        assert_eq!(caret, bs[1].id);
    }

    #[test]
    fn same_day_changes_nothing() {
        let mut bs = vec![text("")];
        open_day(&mut bs, day("2026-11-17"));
        bs[1].text = "a thought".into();
        let before = shape(&bs);
        let (caret, changed) = open_day(&mut bs, day("2026-11-17"));
        assert!(!changed);
        assert_eq!(shape(&bs), before);
        assert_eq!(caret, bs[1].id);
    }

    #[test]
    fn next_day_adds_a_line_under_the_last() {
        let mut bs = vec![Block::new(BlockKind::Heading3, day_line(day("2026-11-16"))), text("why? cuz am sure?"), text("")];
        let (caret, changed) = open_day(&mut bs, day("2026-11-17"));
        assert!(changed);
        assert_eq!(
            shape(&bs),
            ["heading3:Monday, November 16, 2026", "text:why? cuz am sure?", "heading3:Tuesday, November 17, 2026", "text:"]
        );
        assert_eq!(caret, bs[3].id);
    }

    #[test]
    fn an_empty_day_becomes_today() {
        let mut bs = vec![
            Block::new(BlockKind::Heading3, day_line(day("2026-11-15"))),
            text("first"),
            Block::new(BlockKind::Heading3, day_line(day("2026-11-16"))),
            text(""),
        ];
        open_day(&mut bs, day("2026-11-17"));
        assert_eq!(shape(&bs), ["heading3:Sunday, November 15, 2026", "text:first", "heading3:Tuesday, November 17, 2026", "text:"]);
    }

    #[test]
    fn written_lines_without_dates_are_kept() {
        let mut bs = vec![text("before the stream")];
        open_day(&mut bs, day("2026-11-17"));
        assert_eq!(shape(&bs), ["text:before the stream", "heading3:Tuesday, November 17, 2026", "text:"]);
    }

    #[test]
    fn the_stream_is_known_by_its_icon() {
        let mut p = crate::model::Page::new(None, 1.0);
        assert!(!is_stream_page(&p));
        p.icon = Some(ICON.into());
        assert!(is_stream_page(&p));
        p.icon = Some("stream:blue".into());
        assert!(is_stream_page(&p));
        p.icon = Some("streamer:gray".into());
        assert!(!is_stream_page(&p));
        // A person or the AI cannot give a page that icon.
        assert!(!crate::ui::icons::PAGE_ICONS.contains(&"stream"));
    }

    #[test]
    fn the_last_day_of_the_page() {
        let bs = vec![Block::new(BlockKind::Heading3, day_line(day("2026-11-16"))), text("a"), Block::new(BlockKind::Heading3, day_line(day("2026-11-17"))), text("")];
        assert_eq!(last_day(&bs), Some(day("2026-11-17")));
        assert_eq!(last_day(&[text("no days")]), None);
    }

    #[test]
    fn echoes_of_a_week_and_a_year_ago() {
        let bs = vec![
            Block::new(BlockKind::Heading3, day_line(day("2025-11-17"))),
            text("a year of steps"),
            Block::new(BlockKind::Heading3, day_line(day("2026-11-09"))),
            text("eight days ago"),
            Block::new(BlockKind::Heading3, day_line(day("2026-11-10"))),
            text("one call."),
            text("one thing shipped."),
            Block::new(BlockKind::Heading3, day_line(day("2026-11-17"))),
            text(""),
        ];
        let e = echoes(&bs, day("2026-11-17"));
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].label, e[0].words.as_str(), e[0].line.as_str()), ("A year ago", "a year of steps", bs[0].id.as_str()));
        assert_eq!((e[1].label, e[1].words.as_str(), e[1].line.as_str()), ("A week ago", "one call. one thing shipped.", bs[4].id.as_str()));
        // A day with nothing written, or no day at all: no echo.
        let empty = vec![Block::new(BlockKind::Heading3, day_line(day("2026-11-10"))), text(" ")];
        assert!(echoes(&empty, day("2026-11-17")).is_empty());
        // Long days are cut.
        let long = vec![Block::new(BlockKind::Heading3, day_line(day("2026-11-10"))), text(&"word ".repeat(60))];
        assert!(echoes(&long, day("2026-11-17"))[0].words.ends_with('…'));
    }
}
