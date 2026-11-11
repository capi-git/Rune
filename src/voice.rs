//! What the voice features do with words once the AI has read them (the
//! parts with no window in them, so they can be tested):
//!
//! - **Requests the AI passes on** instead of writing text: "put lunch with
//!   Sam on Friday at 1" comes back as an `@event` line, a bare task as an
//!   `@todo` line (`directives`).
//! - **A meeting recap** merged into a meeting notes page, under its "Notes"
//!   and "Action items" headings (`fill_recap`).
//! - **Speak to edit**: the AI's rewrite put in place of the selected text
//!   (`replace_selection`).
//! - **Ask your notes**: which pages to show the AI for a question
//!   (`relevant_pages`).

use crate::inline;
use crate::model::{Block, BlockKind, Id, Page};
use crate::repeat::{self, Repeat};
use crate::when;
use serde_json::Value;

// ---------------------------------------------------------------------------
// What the AI is told
// ---------------------------------------------------------------------------

/// "Today is Monday, 9 November 2026. The time is 14:10." so that "Friday at
/// 1" can be turned into a date.
pub fn now_context(now: i64) -> String {
    let t = when::local(now);
    format!("Today is {}. The time is {}.", t.format("%A, %-d %B %Y"), t.format("%H:%M"))
}

/// How the AI passes on a request for the calendar (added to the prompts of
/// the voice box).
pub fn calendar_rule(now: i64) -> String {
    format!(
        "Calendar: if the note only asks to put something on the calendar (a meeting, appointment, call, or a reminder at a day or time), do not write text. Reply with one line for each event and nothing else, like this: @event {{\"title\": \"Lunch with Sam\", \"start\": \"2026-11-13T13:00\", \"end\": \"2026-11-13T14:00\", \"repeat\": \"none\", \"notes\": \"\"}}. Leave out \"end\" when no end or length was said. For a whole day give dates without a time (\"start\": \"2026-11-13\"). \"repeat\" is one of none, daily, weekdays, weekly, biweekly, monthly, yearly. Work out the dates from this: {}",
        now_context(now)
    )
}

/// How the AI passes on bare tasks (only when a new page would be made).
pub const TODO_RULE: &str = "To-dos: if the note is only one or a few things to do, with nothing else to write down, do not write a page. Reply with one line for each task and nothing else, like this: @todo Call the bank about the card.";

/// What the AI is told for a meeting recap.
pub const RECAP: &str = "You turn a spoken recap of a meeting into notes for the meeting's page. The recap was dictated, so it may have misheard words, repeats and little punctuation. Reply with Markdown only, with no preamble and no closing remark, in exactly this shape: a line `## Notes`, then the points as `-` bullets; then a line `## Action items`, then each thing someone must do as a `- [ ]` to-do, with the person's name first when one was said. When a day was said for it, end its line with \u{1F4C5} and that day as YYYY-MM-DD (the first line you are given says what day today is); when no day was said, add nothing. If no action was said, leave that part empty. Keep every fact, name, number and decision the speaker gave. Fix obvious dictation errors. Do not add facts. Write in the language the speaker used.";

/// What the AI is told for speak-to-edit.
pub const EDIT: &str = "You change a piece of text the way the person asks. The request was dictated, so it may have misheard words. Reply with only the changed text, in Markdown, with no preamble, no quotes around it and no remark. Keep the meaning and every fact unless the request says otherwise. If the request asks for a list or checklist, use `-` bullets or `- [ ]` to-dos, one item per line. Keep the language of the text unless asked to translate.";

/// What the AI is told for Ask your notes.
pub const ASK_NOTES: &str = "You answer a question using only the person's notes, which are given below as pages. You cannot see anything else. Answer briefly and plainly in Markdown (short paragraphs, `-` bullets when listing). If the notes do not hold the answer, say so in one line and say what is missing. Never invent facts. End with a line `Sources:` and then the titles of the pages you used, one per line, each as `- ` and the title exactly as given; leave the list empty if you used none.";

// ---------------------------------------------------------------------------
// Requests passed on by the AI
// ---------------------------------------------------------------------------

/// An event the speaker asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct SpokenEvent {
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub repeat: Option<Repeat>,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Directive {
    Event(SpokenEvent),
    Todo(String),
}

fn event_from(json: &str) -> Option<SpokenEvent> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    let title = v.get("title")?.as_str()?.trim().to_string();
    let (start, day) = when::parse_iso(v.get("start")?.as_str()?)?;
    let all_day = v.get("all_day").and_then(Value::as_bool).unwrap_or(day);
    let end = match v.get("end").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).and_then(when::parse_iso) {
        Some((t, end_day)) if all_day || end_day => when::midnight(when::next_day(when::date_of(t))),
        Some((t, _)) => t,
        None if all_day => when::midnight(when::next_day(when::date_of(start))),
        None => start + when::HOUR,
    };
    let start = if all_day { when::midnight(when::date_of(start)) } else { start };
    let end = if end > start { end } else { start + if all_day { 24 * when::HOUR } else { when::HOUR } };
    let repeat = v.get("repeat").and_then(Value::as_str).and_then(|w| repeat::parse(w).ok()).flatten();
    let notes = v.get("notes").and_then(Value::as_str).unwrap_or("").trim().to_string();
    (!title.is_empty()).then_some(SpokenEvent { title, start, end, all_day, repeat, notes })
}

/// The requests in a reply, when the reply is nothing but requests
/// (`@event {…}` and `@todo …` lines). None when it is text to write.
pub fn directives(reply: &str) -> Option<Vec<Directive>> {
    let mut out = Vec::new();
    for line in reply.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(json) = line.strip_prefix("@event") {
            out.push(Directive::Event(event_from(json)?));
        } else if let Some(task) = line.strip_prefix("@todo") {
            let task = task.trim().trim_start_matches(':').trim();
            if task.is_empty() {
                return None;
            }
            out.push(Directive::Todo(task.to_string()));
        } else {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Whether a reply that is still arriving looks like requests rather than
/// text (so nothing is written into a page yet).
pub fn may_be_directives(reply_so_far: &str) -> bool {
    reply_so_far.trim_start().starts_with('@')
}

/// "Lunch with Sam, Fri, Nov 13, 1 PM" for the message that says it was added.
pub fn event_summary(e: &SpokenEvent) -> String {
    let day = when::day_label(when::date_of(e.start));
    if e.all_day {
        format!("{}, {day}", e.title)
    } else {
        format!("{}, {day}, {}", e.title, when::time_label(e.start))
    }
}

// ---------------------------------------------------------------------------
// A meeting recap
// ---------------------------------------------------------------------------

fn is_heading(b: &Block) -> bool {
    matches!(b.kind, BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3)
}

fn heading_named(b: &Block, name: &str) -> bool {
    is_heading(b) && b.plain_text().trim().eq_ignore_ascii_case(name)
}

/// A heading's part of a page ends at the next heading, or at the transcript
/// (a closed toggle the listener keeps at the end of the page).
fn ends_part(b: &Block) -> bool {
    is_heading(b) || (b.kind == BlockKind::Toggle && b.indent == 0 && b.plain_text().trim().eq_ignore_ascii_case("Transcript"))
}

/// Take out what is under the "Notes" and "Action items" headings of a
/// meeting page, and headings with no words, so the notes can be written
/// anew. The two headings stay.
pub fn clear_recap(page: &mut Vec<Block>) {
    for name in ["Notes", "Action items"] {
        let Some(h) = page.iter().position(|b| heading_named(b, name)) else { continue };
        let end = page[h + 1..].iter().position(ends_part).map(|i| h + 1 + i).unwrap_or(page.len());
        page.drain(h + 1..end);
    }
    page.retain(|b| !(is_heading(b) && b.plain_text().trim().is_empty()));
}

/// Put `items` under the heading `name` of a page: after what is there (an
/// empty line left by the template is dropped). A page with no such heading
/// gets one at the end.
pub fn add_under(page: &mut Vec<Block>, name: &str, items: Vec<Block>) {
    if items.is_empty() {
        return;
    }
    let Some(h) = page.iter().position(|b| heading_named(b, name)) else {
        page.push(Block::new(BlockKind::Heading2, name));
        page.extend(items);
        return;
    };
    let mut end = page[h + 1..].iter().position(ends_part).map(|i| h + 1 + i).unwrap_or(page.len());
    // The empty bullet or to-do the template left under the heading. Only
    // empty lines of text: a picture, a drawing or a link to a page has no
    // text either, and must stay.
    while end > h + 1 && page[end - 1].kind.is_editable() && page[end - 1].text.trim().is_empty() {
        page.remove(end - 1);
        end -= 1;
    }
    page.splice(end..end, items);
}

/// Merge a recap (the AI's blocks: "Notes" and "Action items" parts) into a
/// meeting notes page. Returns how many blocks went in.
pub fn fill_recap(page: &mut Vec<Block>, recap: Vec<Block>) -> usize {
    let mut notes = Vec::new();
    let mut actions = Vec::new();
    let mut into_actions = false;
    for b in recap {
        if is_heading(&b) {
            into_actions = b.plain_text().to_lowercase().contains("action");
            continue;
        }
        if b.text.trim().is_empty() && b.kind != BlockKind::Divider {
            continue;
        }
        if into_actions || b.kind == BlockKind::Todo { &mut actions } else { &mut notes }.push(b);
    }
    let count = notes.len() + actions.len();
    add_under(page, "Notes", notes);
    add_under(page, "Action items", actions);
    count
}

// ---------------------------------------------------------------------------
// Speak to edit
// ---------------------------------------------------------------------------

/// The text of a block between two places (counted as the page script
/// counts them), as stored.
pub fn selected_text(block: &Block, start: usize, end: usize) -> String {
    let (_, rest) = inline::split_for(block.kind, &block.text, start);
    inline::split_for(block.kind, &rest, end.saturating_sub(start)).0
}

/// Put `new` in place of the text from `start` to `end` of a block. One
/// plain paragraph goes into the line itself; anything else (a list, several
/// paragraphs) takes the place of the selection as blocks of its own, with
/// what was before and after it kept around them. Returns the last block
/// that was written, for the caret.
pub fn replace_selection(page: &mut Vec<Block>, block_id: &str, start: usize, end: usize, mut new: Vec<Block>) -> Option<Id> {
    let i = page.iter().position(|b| b.id == block_id)?;
    new.retain(|b| !b.text.trim().is_empty() || b.kind == BlockKind::Divider);
    if new.is_empty() || end < start {
        return None;
    }
    let block = page[i].clone();
    let (before, rest) = inline::split_for(block.kind, &block.text, start);
    let (_, after) = inline::split_for(block.kind, &rest, end - start);
    if new.len() == 1 && new[0].kind == BlockKind::Text {
        page[i].text = inline::concat(&inline::concat(&before, &new[0].text), &after);
        return Some(block.id);
    }
    for b in new.iter_mut() {
        b.indent = b.indent.saturating_add(block.indent);
    }
    let mut out: Vec<Block> = Vec::new();
    if !before.trim().is_empty() {
        out.push(Block { text: before.trim_end().to_string(), ..block.clone() });
    } else {
        // Nothing before it: the first new block takes the old one's place.
        new[0].id = block.id.clone();
    }
    let last = new.last().map(|b| b.id.clone());
    out.extend(new);
    if !after.trim().is_empty() {
        let mut tail = Block::new(block.kind, after.trim_start());
        tail.indent = block.indent;
        tail.checked = block.checked;
        out.push(tail);
    }
    page.splice(i..=i, out);
    last
}

// ---------------------------------------------------------------------------
// Ask your notes
// ---------------------------------------------------------------------------

const STOP_WORDS: [&str; 44] = [
    "the", "and", "for", "with", "that", "this", "what", "when", "where", "which", "who", "whom", "how", "why", "are", "was", "were", "did", "does", "have", "has",
    "had", "about", "from", "into", "our", "your", "you", "they", "them", "their", "there", "can", "could", "should", "would", "will", "any", "all", "not", "but",
    "say", "said", "tell",
];

/// The words of a question worth looking for.
pub fn search_words(question: &str) -> Vec<String> {
    let mut out: Vec<String> = question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !STOP_WORDS.contains(w))
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A page offered to the AI: its id, title and text.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: Id,
    pub title: String,
    pub text: String,
}

/// The pages most likely to hold the answer: those that say the question's
/// words most (the title counts for more), newest first among equals. With
/// no word found anywhere, the pages edited last.
pub fn relevant_pages(question: &str, pages: &[(&Page, String)], limit: usize) -> Vec<Note> {
    let words = search_words(question);
    let mut scored: Vec<(usize, i64, &Page, &String)> = pages
        .iter()
        .filter(|(p, text)| !p.trashed && (!p.title.trim().is_empty() || !text.trim().is_empty()))
        .map(|(p, text)| {
            let title = p.title.to_lowercase();
            let body = text.to_lowercase();
            let score: usize = words.iter().map(|w| if title.contains(w) { 5 } else { 0 } + body.matches(w.as_str()).count().min(5)).sum();
            (score, p.updated_at, *p, text)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    if scored.first().is_some_and(|s| s.0 > 0) {
        scored.retain(|s| s.0 > 0);
    }
    scored.into_iter().take(limit).map(|(_, _, p, text)| Note { id: p.id.clone(), title: p.display_title().to_string(), text: text.clone() }).collect()
}

/// The request sent for a question: the pages (each clipped so that all fit)
/// and the question.
pub fn ask_message(question: &str, notes: &[Note], budget_chars: usize) -> String {
    let each = budget_chars / notes.len().max(1);
    let mut out = String::new();
    for n in notes {
        let text: String = n.text.chars().take(each).collect();
        out.push_str(&format!("<page title=\"{}\">\n{}\n</page>\n\n", n.title.replace('"', "'"), text.trim()));
    }
    out.push_str(&format!("Question: {}", question.trim()));
    out
}

/// An answer split into its text and the pages it names as sources (matched
/// to the notes that were offered, by title).
pub fn split_answer(reply: &str, notes: &[Note]) -> (String, Vec<Note>) {
    // "Sources:" in any case, found in the reply itself. A lower-case copy can
    // be longer or shorter in bytes ("İ"), so a place in it is not a place here.
    let mark = "sources:";
    let at = reply
        .char_indices()
        .map(|(i, _)| i)
        .filter(|i| reply.as_bytes()[*i..].get(..mark.len()).is_some_and(|b| b.eq_ignore_ascii_case(mark.as_bytes())))
        .last();
    let Some(at) = at else { return (reply.trim().to_string(), Vec::new()) };
    let answer = reply[..at].trim().to_string();
    let listed = &reply[at + mark.len()..];
    let mut used: Vec<Note> = Vec::new();
    for line in listed.lines() {
        let name = line.trim().trim_start_matches(['-', '*', '•']).trim().trim_matches('"').trim();
        if name.is_empty() {
            continue;
        }
        if let Some(n) = notes.iter().find(|n| n.title.eq_ignore_ascii_case(name)) {
            if !used.contains(n) {
                used.push(n.clone());
            }
        }
    }
    (answer, used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown;
    use chrono::NaiveDate;

    #[test]
    fn calendar_requests_are_read_from_the_reply() {
        let reply = "@event {\"title\": \"Lunch with Sam\", \"start\": \"2026-11-13T13:00\", \"repeat\": \"weekly\"}\n@event {\"title\": \"Trip\", \"start\": \"2026-12-01\", \"end\": \"2026-12-03\"}\n@todo Call the bank";
        let got = directives(reply).unwrap();
        assert_eq!(got.len(), 3);
        let Directive::Event(lunch) = &got[0] else { panic!() };
        assert_eq!(lunch.title, "Lunch with Sam");
        assert_eq!(when::date_of(lunch.start), NaiveDate::from_ymd_opt(2026, 11, 13).unwrap());
        assert_eq!(when::time_label(lunch.start), "1 PM");
        assert_eq!(lunch.end - lunch.start, when::HOUR, "an hour when no end was said");
        assert_eq!(lunch.repeat.as_ref().map(|r| r.every), Some(repeat::Every::Week));
        assert!(!lunch.all_day);
        let Directive::Event(trip) = &got[1] else { panic!() };
        assert!(trip.all_day);
        assert_eq!(trip.end, when::midnight(NaiveDate::from_ymd_opt(2026, 12, 4).unwrap()), "the last day is included");
        assert_eq!(got[2], Directive::Todo("Call the bank".into()));

        // Text is not a request, and a broken request is treated as text.
        assert_eq!(directives("# Plan\n\nBuy milk"), None);
        assert_eq!(directives("@event {\"title\": \"x\"}"), None);
        assert_eq!(directives("@event {\"title\": \"x\", \"start\": \"next Friday\"}"), None);
        assert_eq!(directives("@todo Call Sam\nand some text"), None);
        assert_eq!(directives(""), None);
        assert!(may_be_directives("  @ev"));
        assert!(!may_be_directives("# Title"));
        assert!(now_context(when::at(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), chrono::NaiveTime::from_hms_opt(14, 10, 0).unwrap())).contains("Monday, 9 November 2026. The time is 14:10"));
    }

    fn kinds(blocks: &[Block]) -> Vec<(BlockKind, String)> {
        blocks.iter().map(|b| (b.kind, b.text.clone())).collect()
    }

    #[test]
    fn a_recap_fills_the_meeting_page() {
        let mut page = vec![
            Block::text("Monday, November 9, 2026 · 2 PM"),
            Block::new(BlockKind::Heading2, "Notes"),
            Block::new(BlockKind::Bulleted, ""),
            Block::new(BlockKind::Heading2, "Action items"),
            Block::new(BlockKind::Todo, ""),
        ];
        let recap = markdown::to_blocks("## Notes\n- Budget agreed\n- Launch moves to March\n\n## Action items\n- [ ] Sam: send the numbers");
        assert_eq!(fill_recap(&mut page, recap), 3);
        assert_eq!(
            kinds(&page[1..]),
            vec![
                (BlockKind::Heading2, "Notes".to_string()),
                (BlockKind::Bulleted, "Budget agreed".to_string()),
                (BlockKind::Bulleted, "Launch moves to March".to_string()),
                (BlockKind::Heading2, "Action items".to_string()),
                (BlockKind::Todo, "Sam: send the numbers".to_string()),
            ]
        );
        // A second recap is added after the first; notes already typed stay.
        let more = markdown::to_blocks("## Notes\n- Next meeting Tuesday\n\n## Action items\n");
        fill_recap(&mut page, more);
        assert_eq!(page[4].text, "Next meeting Tuesday");
        assert_eq!(page[5].text, "Action items");
        // A meeting that was listened to: the notes go above its transcript.
        let mut toggle = Block::new(BlockKind::Toggle, "Transcript");
        toggle.collapsed = true;
        let mut heard = vec![
            Block::new(BlockKind::Heading2, "Notes"),
            Block::new(BlockKind::Bulleted, ""),
            Block::new(BlockKind::Heading2, "Action items"),
            Block::new(BlockKind::Todo, ""),
            toggle,
            Block::text("**Me** · 0:01 — Hello.").with_indent(1),
        ];
        fill_recap(&mut heard, markdown::to_blocks("## Notes\n- Agreed\n\n## Action items\n- [ ] I: send it"));
        assert_eq!(
            kinds(&heard).iter().map(|k| k.1.as_str()).collect::<Vec<_>>(),
            vec!["Notes", "Agreed", "Action items", "I: send it", "Transcript", "**Me** · 0:01 — Hello."]
        );
        // A page without the headings gets them.
        let mut bare = vec![Block::text("Call with Ana")];
        fill_recap(&mut bare, markdown::to_blocks("- One point\n- [ ] One task"));
        assert_eq!(kinds(&bare).iter().map(|k| k.1.as_str()).collect::<Vec<_>>(), vec!["Call with Ana", "Notes", "One point", "Action items", "One task"]);
    }

    #[test]
    fn spoken_edits_replace_the_selection() {
        // A plain rewrite stays in the line.
        let mut page = vec![Block::text("Please send the very long report today")];
        let id = page[0].id.clone();
        assert_eq!(selected_text(&page[0], 7, 32), "send the very long report");
        let last = replace_selection(&mut page, &id, 7, 32, markdown::to_blocks("send the report"));
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].text, "Please send the report today");
        assert_eq!(last, Some(id.clone()));

        // A checklist takes the selection's place as blocks, keeping what is around it.
        let mut page = vec![Block::text("Buy: milk, eggs and bread. Thanks")];
        let id = page[0].id.clone();
        replace_selection(&mut page, &id, 5, 25, markdown::to_blocks("- [ ] milk\n- [ ] eggs\n- [ ] bread"));
        assert_eq!(
            kinds(&page),
            vec![
                (BlockKind::Text, "Buy:".to_string()),
                (BlockKind::Todo, "milk".to_string()),
                (BlockKind::Todo, "eggs".to_string()),
                (BlockKind::Todo, "bread".to_string()),
                (BlockKind::Text, ". Thanks".to_string()),
            ]
        );
        assert_eq!(page[0].id, id);

        // The whole line selected: the first new block takes its place.
        let mut page = vec![Block::text("one two")];
        let id = page[0].id.clone();
        replace_selection(&mut page, &id, 0, 7, markdown::to_blocks("- one\n- two"));
        assert_eq!(kinds(&page), vec![(BlockKind::Bulleted, "one".to_string()), (BlockKind::Bulleted, "two".to_string())]);
        assert_eq!(page[0].id, id);
        // An empty reply changes nothing.
        assert_eq!(replace_selection(&mut page, &id, 0, 3, Vec::new()), None);
        assert_eq!(page.len(), 2);
    }

    #[test]
    fn the_right_notes_are_found_for_a_question() {
        let mut a = Page::new(None, 1.0);
        a.title = "Trip to Rome".into();
        a.updated_at = 10;
        let mut b = Page::new(None, 2.0);
        b.title = "Budget".into();
        b.updated_at = 30;
        let mut c = Page::new(None, 3.0);
        c.title = "Old".into();
        c.trashed = true;
        let pages = vec![
            (&a, "Flights on 3 May. Hotel near the station.".to_string()),
            (&b, "Rome hotel costs 400. Flights 250.".to_string()),
            (&c, "Rome Rome Rome hotel".to_string()),
        ];
        assert_eq!(search_words("What does the hotel in Rome cost?"), vec!["cost", "hotel", "rome"]);
        let found = relevant_pages("What does the hotel in Rome cost?", &pages, 5);
        assert_eq!(found.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), vec!["Trip to Rome", "Budget"], "the title counts for more; the Trash is left out");
        let none = relevant_pages("zebra", &pages, 5);
        assert_eq!(none.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), vec!["Budget", "Trip to Rome"], "nothing found: the newest pages");

        let message = ask_message("How much?", &found, 40);
        assert!(message.contains("<page title=\"Trip to Rome\">\nFlights on 3 May. Ho\n</page>"), "{message}");
        assert!(message.ends_with("Question: How much?"));
        let (answer, sources) = split_answer("The hotel costs 400.\n\nSources:\n- Budget\n- Not a page\n- budget", &found);
        assert_eq!(answer, "The hotel costs 400.");
        assert_eq!(sources.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), vec!["Budget"]);
        assert_eq!(split_answer("No sources line", &found), ("No sources line".to_string(), Vec::new()));
    }
}
