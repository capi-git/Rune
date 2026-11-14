//! MCP server: `rune --mcp` lets AI agents (Aven, Claude Code, Claude Desktop,
//! Codex) read and edit the workspace.
//!
//! Transport is stdio: one JSON-RPC 2.0 message per line in, one per line out.
//! Nothing but protocol messages is ever written to stdout; logs go to stderr.
//! The server opens the same SQLite file as the app (WAL mode, so both can be
//! open at once) and reads it fresh on every call.

use crate::markdown;
use crate::model::*;
use crate::state::{ancestors, children_of, descendants, stale_row_titles};
use crate::store::{Loaded, Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Protocol versions this server speaks, newest first.
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const LINK_PREFIX: &str = crate::inline::PAGE_URL;

pub(crate) const INSTRUCTIONS: &str = "Rune is the user's local notes workspace: pages and a calendar. \
Pages nest inside other pages and each has an id. Find ids with search_pages or list_pages before reading or editing. \
The workspace has spaces (as Work, Personal): each top-level page is in one space, and a sub-page is in the space of its parent. list_pages names the spaces and says which one is open in Rune. \
When the person names a space for a new page (\"in my Personal space\"), pass that name as `space` to create_page; without `space`, a new top-level page goes to the space that is open in Rune, which may not be the one they mean. To put a page in another space, use update_page with `space`. You cannot make a space: if the one they name does not exist, ask them. \
Page content is Markdown: # / ## / ### headings, paragraphs, - bullets, 1. numbered items, - [ ] / - [x] to-dos, > quotes, \
> [!lightbulb:yellow] callouts (a box with an icon, NAME:COLOUR as for a page's icon), - ▸ toggles (the lines indented under one fold away), \
``` code fences and --- dividers; indent list items by two spaces to nest them. \
Lines can stand side by side in two or three columns (a week planner, two lists): a line <!-- column --> before each column and <!-- end columns --> after the last, each on its own line; more than three columns go in a next set. \
A line that is exactly [Title](rune://page/<id>) is a link to another page; sub-pages appear in their parent this way. \
A to-do can end with the day it is due, as 📅 2026-11-12 (the sign, then the date): it then shows in that day of the calendar. Keep that ending when you rewrite a to-do; leave it out to take the day away. \
After the day it can have a reminder, as 📅 2026-11-12 ⏰ 09:00 (a time of that day, 24 hours): Rune sends a notification then. \
When the person asks to be reminded of something, make it a to-do with its day and that time (or set them with set_todo). \
A picture or a drawing shows in read_page as a line [Image](rune://block/<id>) or [Drawing](rune://block/<id>): keep that line as it is to keep the picture or drawing in its place when you replace a page (one that you leave out is kept at the end of the page, never lost). \
Tables are Markdown pipe tables. A table's columns can have kinds, given on a comment line right under it, one part for each column: \
<!-- columns: text count | date | choice | number km sum | number USD sum | time sum | pace average | checkbox checked | formula USD sum = Price * Qty --> \
(kinds: text, number, date, time, pace, checkbox, choice (one tag from a list), multi (several tags, with commas between them), status (one of Not started, In progress, Done, or its own list; a choice, multi or status column can list its options and their colours in braces, as choice {Long:green; Short:red}, colours: gray, brown, orange, yellow, green, blue, purple, pink, red), link, email, phone, file (pictures), formula, relation and the name of a table (rows of that table, by their names, as relation Companies; make that table first), \
rollup (worked out from the page of each row: the sum, count, average, highest or lowest of a column of the first table on that page, by its heading, as rollup Amount sum; a heading of more than one word in square brackets, as rollup [Amount paid] sum); \
then for a number, a formula or a rollup an optional format (a kind of money by its code, as USD or EUR, or percent, or grouped), an optional unit, \
and an optional total shown at the foot: count, sum, average, median, highest, lowest, range, unique, empty, checked; \
a number column can also show a bar or a ring that is full at a goal: number percent ring goal 100, number km bar goal 20; \
a number or formula column marked signed shows amounts above zero green and below it red (a gain and a loss): number USD sum signed; \
a formula column ends with = and what it works out from the other columns of its row, by their headings, with + - * / %, = != < > <= >=, and, or, not, \
if, round, floor, ceil, abs, empty, length, min, max, sum, average and concat, as = if(Status = \"Done\", 1, 0); it knows no dates). \
A cell of a money column is written with its sign ($2,000.00); a plain number there is fine too. \
A pace column is worked out from the first number and the first time column of its row, a formula column from its formula, and a rollup from the pages of the rows: what is in their cells is not kept. \
A table can have a name, on a second comment line under it, and after the name, each between bars, what else it has: \
<!-- table: Trades | icon target:green | sort Date down | frozen 1 | color P&L below 0 red | color Result is Win green --> \
(icon NAME:COLOUR, NAME one of plus, minus, check, star, heart, home, calendar, briefcase, book, flame, gift, coffee, target, trophy, rocket, leaf, music, plane, wrench, globe; \
beside: it stands at the right of the table above it; board by COLUMN, list, gallery or calendar: how the rows show (a board has a column of cards for each option of a choice column); \
group COLUMN; sort COLUMN, or sort COLUMN down for high to low and newest first; frozen N: the first N columns stay in place when a wide table scrolls sideways; hide COLUMN; \
filter COLUMN TEST: only rows that pass show; color COLUMN TEST COLOUR: rows that pass get that colour; locked; full: a row opens as a full page, not at the side; \
and last on the line, page: and the columns of the table that the page of each row starts with, each its heading and then its kind in the words of the columns line, between bars: page: Item | Amount number USD sum | Category choice {Food; Fun}. \
page: makes that table the template of a table that has none (a new table); it never changes a template that is there. \
To change what the pages of new rows start with, change the template's page itself: read_page names it (row_page_template:), and you read and change it as any page. \
A TEST is is, is not, has, has not, above, below, at least or at most, then words or a number, or is empty, is filled, is ticked, is not ticked; \
for a date, above and below are after and before, and today is a day: color Due below today red. Columns go by their headings.) \
Keep both comment lines when you rewrite a table. What Markdown cannot say of a table (the pages of its rows, their nesting, its template), and its view when the table line says none, \
stay when you rewrite it: they are found again by its headings and the names of its rows (their first cells), so keep those the same where you can. \
When the person asks for a tracker, a log, a journal, a list of things to keep or any table of their own, make a real database: \
a name for the table and an icon; the right kind for each column (date for days, number with the money code for amounts, so that totals add up, \
choice or multi for words from a short list, checkbox for yes or no, link for web addresses); the options of each choice column with colours that mean something \
(green for a win, done or good, red for a loss or a problem, yellow or orange for waiting, gray for nothing yet); a total at the foot of the columns that add up; \
the newest first when rows are dated; frozen 1 when the table has more than six columns; and a colour rule when one result matters at a glance. \
The first column names each row (a row opens as a page of that name): make it a text column, such as the ticker, the task or the item, never a number, and a date only when each row is one day. Leave the rows empty unless the person gives you rows or asks for examples; a chart, a short line on how to use the page, \
or a section of rules or a review under the table make it better when they fit what was asked. \
A diagram is a code block of the language mermaid (Mermaid's own syntax), which Rune draws: \
```mermaid flowchart LR; A[Plan] --> B[Enter]; B --> C[Review]; C -. lessons .-> A ``` (each on its own line, without the semicolons). \
Use one when a picture helps more than words (a process, a flow with choices, who talks to whom, a timeline, a mind map, the parts of a plan); \
keep labels short, put the details in the text under it, and give it no colours (no style or classDef lines: Rune colours it for the light and the dark look). Never draw a picture with characters (boxes, lines and arrows made of -, |, +, ^, v, > or of box-drawing signs) \
in a code block or out of one: it breaks on a narrow page and looks broken. Levels or amounts go in a small table, and a rule or a formula in one line in bold. \
A table holds at most 12 columns and 199 rows under its header. A tool refuses Markdown with a larger table and writes nothing: \
write a long log as more than one table (one for each month or year, say), each with its header and its comment lines. \
When each row of a table needs a list of its own (what was spent each day, the costs of each trip), make one table with a rollup column and a page: part, \
and Rune does the rest: the person opens a row and writes in the table its page starts with, and the row and the total at the foot follow. \
For example a budget for a month, a row for each day of the month: \
| Date | Budget | Spent | Left | \
| --- | --- | --- | --- | \
| 2026-11-01 | $60.00 |  |  | \
<!-- columns: date | number USD sum | rollup Amount sum USD sum | formula USD sum = Budget - Spent --> \
<!-- table: November | icon calendar:green | color Left below 0 red | page: Item | Amount number USD sum | Category choice {Food:orange; Fun:purple; Bills:gray} --> \
(each line on its own line). Rune makes the page of a row when it is opened: do not make those pages with create_page, and do not ask the person how to do it. \
To add a row to a table (a log), use add_table_row; to change one, update_table_row; give either of them page (Markdown) to write the row's own page (a recipe's steps, a day's notes; the answer gives its row_page_id); \
to tick a to-do or set its day, set_todo: none of them write the page again. \
A chart is a code block of the language chart holding JSON, for example \
```chart {\"shape\": \"bars\", \"from\": \"Run log\", \"x\": \"Date\", \"by\": \"week\", \"y\": \"km\", \"add\": \"sum\", \"goal\": 20, \"title\": \"Distance per week\"} ``` \
(each on its own line). shape: bars, line, pie, ring or days; from: the title or id of the page whose first table it draws, or left out for the table above it on the same page; \
x: the column along the bottom, usually a date; by: row, day, week or month; y: the column of amounts; add: sum, average, highest, lowest or count; goal and title are optional. \
Add \"running\": true to bars or a line for a running total, each point adding to those before it (money made over time, a balance, a count so far). \
Amounts below zero draw under a line at zero, green and red when the column is signed. \
A pie draws how much of each word of a choice, multi or status column, in the colours of its options: x is that column, and add is count for how many rows, or sum with y for an amount, \
for example {\"shape\": \"pie\", \"x\": \"Status\", \"add\": \"count\"}. \
A ring draws one amount (add over all rows) against the goal; days draws which days of the last four weeks have a row. The chart follows its table by itself. \
Build what the person asks for (a table, its views and colours, the pages it needs, a chart) in one go with the tools; do not stop to ask about details you can choose well. \
Then read each page you made or changed (read_page) and put right, in the same run, what is not as the person asked; a tool's answer with check names what Rune could not take as written. \
When the person says that what you made in this conversation is wrong, read the page again (read_page), fix what most likely does not match what they asked for, \
and say in one sentence what you changed. When you did not make it in this conversation and they do not say what they want instead, ask one short question \
and change nothing. Never reply with a list of guesses. \
You cannot lock a page with a password (the person does it in the ··· menu at the top of the page: Lock with password…) or add a picture (they paste or drop one): say so in one sentence. \
Changes are saved immediately. trash_page is reversible with restore_page; no page is ever deleted permanently (delete_event does remove an event for good).";

/// What a change gets as its answer when this connector is older than the
/// app that keeps the notes.
const OLDER_THAN_THE_NOTES: &str = "This connector is older than the Rune app that keeps these notes, so it must not change them: the change would lose what the newer app added. Nothing was changed. Start the connector again (start the program that runs it again), then repeat the request. Reading still works.";

/// Serve MCP over stdin/stdout until stdin closes. Returns the process exit code.
pub fn run_stdio() -> i32 {
    let path = Store::default_path();
    let store = match Store::open(path.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[rune mcp] couldn't open the workspace at {}: {e}", path.display());
            return 1;
        }
    };
    eprintln!("[rune mcp] serving {}", path.display());
    let mut server = Server::new(store);
    // Started by the app for its own AI: every change is written down, so
    // that the app can say what was done and take it back.
    if let Some(journal) = std::env::var_os(JOURNAL_ENV).filter(|p| !p.is_empty()) {
        server = server.with_journal(PathBuf::from(journal));
    }
    match server.serve(io::stdin().lock(), io::stdout().lock()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[rune mcp] stopped: {e}");
            1
        }
    }
}

pub struct Server {
    store: Store,
    /// Where each change is written down (see `Change`), when the app's own
    /// AI uses the tools.
    journal: Option<PathBuf>,
    /// The lines of each page as `read_page` gave them, and as the tools
    /// left them since: a page whose lines are not these any more was
    /// changed by someone else (a person typing in Rune, a meeting's
    /// transcript), and `replace_page_content` refuses to write it again
    /// from what was read before (see `changed_since_read`).
    seen: std::cell::RefCell<HashMap<Id, Vec<Block>>>,
}

type ToolResult = Result<String, String>;

/// The environment variable that names the file for the record of changes.
pub const JOURNAL_ENV: &str = "RUNE_AI_JOURNAL";

/// One thing a tool changed, with what was there before it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A page that was not there before.
    PageMade { id: Id, title: String },
    /// A page as it was (its name, place, Trash state…) before the change,
    /// and (`left`) as the change left it: to take the change back, only
    /// what it changed is put back.
    PageWas {
        page: Page,
        title: String,
        what: String,
        #[serde(default)]
        left: Option<Page>,
    },
    /// What was on a page before the change, and (`left`) what the change
    /// left on it: to take the change back, only what it changed is put
    /// back, and what the person wrote on the page since stays.
    BlocksWere {
        id: Id,
        title: String,
        blocks: Vec<Block>,
        #[serde(default)]
        left: Vec<Block>,
    },
    /// An event that was not there before.
    EventMade { id: Id, title: String },
    /// An event as it was before it was changed or removed.
    EventWas { event: Event, gone: bool },
}

/// The pages, what is on them, and the events, at one moment.
struct Snap {
    pages: HashMap<Id, Page>,
    blocks: HashMap<Id, Vec<Block>>,
    events: HashMap<Id, Event>,
}

fn changes_between(before: &Snap, after: &Snap) -> Vec<Change> {
    let mut out = Vec::new();
    let mut ids: Vec<&Id> = after.pages.keys().collect();
    ids.sort_by(|a, b| after.pages[*a].created_at.cmp(&after.pages[*b].created_at).then(a.cmp(b)));
    // The new pages first, then what changed on the pages that were there.
    for id in ids.iter().filter(|id| !before.pages.contains_key(**id)) {
        out.push(Change::PageMade { id: (*id).clone(), title: after.pages[*id].title.clone() });
    }
    for id in ids {
        let now = &after.pages[id];
        let Some(was) = before.pages.get(id) else { continue };
        // A page's time of change moves with every save: that alone is no change of the page.
        let same = Page { updated_at: was.updated_at, ..now.clone() } == *was;
        if !same {
            let what = if !was.trashed && now.trashed {
                "trashed"
            } else if was.trashed && !now.trashed {
                "restored"
            } else if was.parent_id != now.parent_id {
                "moved"
            } else if was.title != now.title {
                "renamed"
            } else {
                "changed"
            };
            out.push(Change::PageWas { page: was.clone(), title: now.title.clone(), what: what.into(), left: Some(now.clone()) });
        }
        let empty = Vec::new();
        let (old, new) = (before.blocks.get(id).unwrap_or(&empty), after.blocks.get(id).unwrap_or(&empty));
        if old != new {
            out.push(Change::BlocksWere { id: id.clone(), title: now.title.clone(), blocks: old.clone(), left: new.clone() });
        }
    }
    let mut ids: Vec<&Id> = after.events.keys().chain(before.events.keys()).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        match (before.events.get(id), after.events.get(id)) {
            (None, Some(now)) => out.push(Change::EventMade { id: id.clone(), title: now.title.clone() }),
            (Some(was), None) => out.push(Change::EventWas { event: was.clone(), gone: true }),
            (Some(was), Some(now)) if was != now => out.push(Change::EventWas { event: was.clone(), gone: false }),
            _ => {}
        }
    }
    out
}

/// Take away the records of AI runs of earlier days: each holds the words of
/// the pages that a run changed, and nothing reads it after its run.
pub fn clear_old_journals() {
    let Ok(files) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for file in files.flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        if !((name.starts_with("rune-ai-") || name.starts_with("rune-ask-")) && name.ends_with(".jsonl")) {
            continue;
        }
        let old = file.metadata().and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age.as_secs() > 24 * 60 * 60);
        if old {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

/// The record of an AI run, as the tools wrote it (nothing when there is none).
pub fn read_journal(path: &Path) -> Vec<Change> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Take back what a run changed, the last change first. A page that the run
/// made goes to the Trash (never away for good).
pub fn undo_changes(store: &Store, changes: &[Change]) -> rusqlite::Result<()> {
    for change in changes.iter().rev() {
        match change {
            Change::PageMade { id, .. } => {
                let (pages, _) = store.load()?;
                if let Some(page) = pages.get(id) {
                    store.save_page(&Page { trashed: true, ..page.clone() })?;
                }
            }
            Change::PageWas { page, left, .. } => {
                // (Only what the run changed: its name, its place. The whole
                // page as it was, put back, also took away a favourite or
                // an icon that the person gave the page after the run.)
                let (pages, _) = store.load()?;
                match (left, pages.get(&page.id)) {
                    (Some(left), Some(now)) => store.save_page(&page_less_change(now, left, page))?,
                    _ => store.save_page(page)?,
                }
            }
            Change::BlocksWere { id, blocks, left, .. } => {
                // (The page as it is now, not as the run left it: lines
                // written since are no part of the run, and stay. The whole
                // page as it was, put back over them, lost those lines.)
                let (_, all) = store.load()?;
                let current = all.get(id).cloned().unwrap_or_default();
                let back = if left.is_empty() { blocks.clone() } else { crate::history::rebase(&current, left, blocks) };
                store.save_blocks_at(id, &back, now_ms())?
            }
            Change::EventMade { id, .. } => store.delete_event(id)?,
            Change::EventWas { event, .. } => store.save_event(event)?,
        }
    }
    Ok(())
}

/// A page as it is now, with one change taken back: each part of it that
/// the change made other than it was (`was` to `left`) is as it was again,
/// and every other part stays as it is now.
fn page_less_change(now: &Page, left: &Page, was: &Page) -> Page {
    let parts = |p: &Page| match serde_json::to_value(p) {
        Ok(Value::Object(parts)) => parts,
        _ => serde_json::Map::new(),
    };
    let (mut out, left, was) = (parts(now), parts(left), parts(was));
    for (name, old) in was {
        // (The time of the last change moves with every save.)
        if name != "updated_at" && left.get(&name) != Some(&old) {
            out.insert(name, old);
        }
    }
    serde_json::from_value(Value::Object(out)).unwrap_or_else(|_| now.clone())
}

/// One page's lines as they were before a run's tools changed anything,
/// worked out from the page as it is now: each change of the run is taken
/// out again, the last first, in `pages` too (their names show in the links
/// of a page). `None` when the page is not as a change of the tools left it:
/// another hand wrote on it in between. (With this, "Replace" can tell the
/// run's own changes of the page from lines the person typed meanwhile.)
pub fn before_run(changes: &[Change], page_id: &str, pages: &mut HashMap<Id, Page>, mut blocks: Vec<Block>) -> Option<Vec<Block>> {
    for change in changes.iter().rev() {
        match change {
            Change::PageMade { id, .. } => {
                pages.remove(id);
            }
            Change::PageWas { page, .. } => {
                pages.insert(page.id.clone(), page.clone());
            }
            Change::BlocksWere { id, blocks: was, left, .. } if id == page_id => {
                // (Also when the tool left the page with no line: an empty
                // page and one empty line are the same page here, and a
                // line typed after that is a change like any other.)
                if render_body(pages, &blocks) != render_body(pages, left) {
                    return None;
                }
                blocks = was.clone();
            }
            _ => {}
        }
    }
    Some(blocks)
}

/// What a run did, in plain words, short: a line for each thing, and one
/// line for many things of one kind ("Made 8 pages, “Week 1” to “Week 8”").
pub fn describe(changes: &[Change]) -> Vec<String> {
    let each = describe_each(changes);
    let name_of = |line: &str| line.split('“').nth(1).and_then(|r| r.split('”').next()).unwrap_or("").to_string();
    let mut out: Vec<String> = Vec::new();
    // (the start of the lines of one kind, how many make a group, the one line for them)
    let kinds: [(&str, usize, &dyn Fn(&[&String]) -> String); 3] = [
        ("Made the page", 4, &|l| format!("Made {} pages, “{}” to “{}”", l.len(), name_of(l[0]), name_of(l[l.len() - 1]))),
        ("Changed the page", 4, &|l| format!("Changed {} pages", l.len())),
        ("Added “", 3, &|l| format!("Added {} events to the calendar", l.len())),
    ];
    let mut grouped: Vec<&str> = Vec::new();
    for line in &each {
        match kinds.iter().find(|(start, _, _)| line.starts_with(start)) {
            Some((start, least, one)) => {
                let all: Vec<&String> = each.iter().filter(|l| l.starts_with(start)).collect();
                if all.len() <= *least {
                    out.push(line.clone());
                } else if !grouped.contains(start) {
                    grouped.push(start);
                    out.push(one(&all));
                }
            }
            None => out.push(line.clone()),
        }
    }
    out
}

/// What a run did, a line for each thing.
pub fn describe_each(changes: &[Change]) -> Vec<String> {
    let name = |t: &str| if t.trim().is_empty() { "Untitled".to_string() } else { t.trim().to_string() };
    let made: Vec<&Id> = changes.iter().filter_map(|c| if let Change::PageMade { id, .. } = c { Some(id) } else { None }).collect();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for change in changes {
        let line = match change {
            Change::PageMade { title, .. } => format!("Made the page “{}”", name(title)),
            // What is on a page that the run made itself is part of "made".
            Change::BlocksWere { id, .. } if made.contains(&id) => continue,
            Change::PageWas { page, .. } if made.contains(&&page.id) => continue,
            Change::BlocksWere { title, .. } => format!("Changed the page “{}”", name(title)),
            Change::PageWas { page, title, what, .. } => match what.as_str() {
                "trashed" => format!("Moved “{}” to the Trash", name(title)),
                "restored" => format!("Took “{}” out of the Trash", name(title)),
                "moved" => format!("Moved the page “{}”", name(title)),
                "renamed" => format!("Named “{}” “{}”", name(&page.title), name(title)),
                _ => format!("Changed the page “{}”", name(title)),
            },
            Change::EventMade { title, .. } => format!("Added “{}” to the calendar", name(title)),
            Change::EventWas { event, gone: true } => format!("Removed “{}” from the calendar", name(&event.title)),
            Change::EventWas { event, gone: false } => format!("Changed “{}” in the calendar", name(&event.title)),
        };
        if !seen.contains(&line) {
            seen.push(line.clone());
            out.push(line);
        }
    }
    out
}

/// What a run did, in one short line ("2 pages made · 1 event added").
pub fn count_line(changes: &[Change]) -> String {
    let lines = describe_each(changes);
    let count = |start: &str| lines.iter().filter(|l| l.starts_with(start)).count();
    let pages_made = count("Made the page");
    let events_made = count("Added");
    let events_other = lines.iter().filter(|l| l.ends_with("the calendar") && !l.starts_with("Added")).count();
    let pages_other = lines.len() - pages_made - events_made - events_other;
    let part = |n: usize, one: &str, many: &str, did: &str| (n > 0).then(|| format!("{n} {} {did}", if n == 1 { one } else { many }));
    [
        part(pages_made, "page", "pages", "made"),
        part(pages_other, "page", "pages", "changed"),
        part(events_made, "event", "events", "added"),
        part(events_other, "event", "events", "changed"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

impl Server {
    pub fn new(store: Store) -> Server {
        Server { store, journal: None, seen: Default::default() }
    }

    /// Write every change down in this file (see `Change`).
    pub fn with_journal(mut self, path: PathBuf) -> Server {
        self.journal = Some(path);
        self
    }

    /// The notes it keeps (for the round trips of `ai_keep_tests`).
    #[cfg(test)]
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    fn snap(&self) -> Option<Snap> {
        let (pages, blocks) = self.store.load().ok()?;
        let events = self.store.load_events().ok()?.into_iter().map(|e| (e.id.clone(), e)).collect();
        Some(Snap { pages, blocks, events })
    }

    /// Read newline-delimited messages until EOF, answering each request.
    pub fn serve<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle_line(&line) {
                writer.write_all(reply.as_bytes())?;
                writer.write_all(b"\n")?;
                writer.flush()?;
            }
        }
        Ok(())
    }

    /// Handle one line of input; `None` when nothing should be sent back.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error_response(Value::Null, -32700, &format!("Parse error: {e}")).to_string()),
        };
        match value {
            Value::Array(items) if items.is_empty() => {
                Some(error_response(Value::Null, -32600, "Invalid Request: empty batch").to_string())
            }
            Value::Array(items) => {
                let replies: Vec<Value> = items.into_iter().filter_map(|m| self.handle_message(m)).collect();
                (!replies.is_empty()).then(|| Value::Array(replies).to_string())
            }
            other => self.handle_message(other).map(|v| v.to_string()),
        }
    }

    fn handle_message(&mut self, msg: Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(error_response(Value::Null, -32600, "Invalid Request"));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            // A response to a request we never sent, or not a message at all.
            if obj.contains_key("result") || obj.contains_key("error") {
                return None;
            }
            return Some(error_response(id.unwrap_or(Value::Null), -32600, "Invalid Request"));
        };
        // Notifications (initialized, cancelled, ...) are never answered.
        let id = id?;
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.call_tool(&params),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "Invalid params: missing tool name".to_string()))?;
        let empty = json!({});
        let args = params.get("arguments").filter(|a| !a.is_null()).unwrap_or(&empty);
        if !args.is_object() {
            return Err((-32602, "Invalid params: arguments must be an object".into()));
        }
        // An old copy of this connector, still open after the app took an
        // update, would write the notes back without what the update added.
        let reads = matches!(name, "search_pages" | "list_pages" | "read_page" | "list_events");
        if !reads && self.store.from_newer_program() {
            return Ok(json!({ "content": [{ "type": "text", "text": OLDER_THAN_THE_NOTES }], "isError": true }));
        }
        // For the app's own AI: what is there before a change, to write down what the change was.
        let before = if reads || self.journal.is_none() { None } else { self.snap() };
        // (The pages read whose lines no one else changed since: what this
        // call writes in them is the AI's own, and no change of another.)
        let in_step: Vec<Id> = if reads { Vec::new() } else { self.seen.borrow().iter().filter(|(id, lines)| self.store.load_page_blocks(id).is_ok_and(|now| now == **lines)).map(|(id, _)| id.clone()).collect() };
        let outcome = self.run_tool(name, args)?;
        for id in in_step {
            if let Ok(now) = self.store.load_page_blocks(&id) {
                self.seen.borrow_mut().insert(id, now);
            }
        }
        if let (Some(before), Some(path)) = (before, self.journal.as_ref()) {
            if let Some(after) = self.snap() {
                let lines: String = changes_between(&before, &after).iter().filter_map(|c| serde_json::to_string(c).ok()).map(|l| l + "\n").collect();
                if !lines.is_empty() {
                    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                        let _ = file.write_all(lines.as_bytes());
                    }
                }
            }
        }
        let (text, is_error) = match outcome {
            Ok(text) => (text, false),
            Err(message) => (message, true),
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
    }

    /// One tool, by its name, for the app's own stand-in AI (a test run).
    pub fn tool(&mut self, name: &str, args: &Value) -> Value {
        self.call_tool(&json!({ "name": name, "arguments": args })).unwrap_or_else(|(_, message)| json!({ "content": [{ "type": "text", "text": message }], "isError": true }))
    }

    fn run_tool(&mut self, name: &str, args: &Value) -> Result<ToolResult, (i64, String)> {
        let outcome = match name {
            "search_pages" => self.search_pages(args),
            "list_pages" => self.list_pages(args),
            "read_page" => self.read_page(args),
            "create_page" => self.create_page(args),
            "append_to_page" => self.append_to_page(args),
            "replace_page_content" => self.replace_page_content(args),
            "update_page" => self.update_page(args),
            "trash_page" => self.trash_page(args),
            "restore_page" => self.restore_page(args),
            "list_events" => self.list_events(args),
            "create_event" => self.create_event(args),
            "update_event" => self.update_event(args),
            "delete_event" => self.delete_event(args),
            "add_table_row" => self.add_table_row(args),
            "update_table_row" => self.update_table_row(args),
            "set_todo" => self.set_todo(args),
            _ => return Err((-32602, format!("Unknown tool: {name}"))),
        };
        Ok(outcome)
    }

    // ----- data access ----------------------------------------------------

    fn load(&self) -> Result<Loaded, String> {
        self.store.load().map_err(db_error)
    }

    fn save_page(&self, page: &Page) -> Result<(), String> {
        self.store.save_page(page).map_err(db_error)
    }

    /// Save a page's blocks and mark the page as edited now.
    fn save_blocks(&self, page_id: &str, blocks: &[Block]) -> Result<(), String> {
        self.store.save_blocks_at(page_id, blocks, now_ms()).map_err(db_error)
    }

    // ----- spaces -----------------------------------------------------------

    /// The spaces, in their order, and the one that is open in Rune.
    fn spaces(&self) -> Result<(Vec<Space>, Id), String> {
        let spaces = self.store.load_spaces().map_err(db_error)?;
        let open = self.store.get_setting("space").filter(|id| spaces.iter().any(|s| &s.id == id)).or_else(|| spaces.first().map(|s| s.id.clone())).unwrap_or_default();
        Ok((spaces, open))
    }

    /// The space the tool call names (`space`: its name, in any letters, or
    /// its id), if it names one. A name that no space has is an error that
    /// says which spaces there are: a page must never go to another space
    /// than the one that was asked for.
    fn space_arg(&self, args: &Value) -> Result<Option<Space>, String> {
        let Some(asked) = arg_str(args, "space").map(str::trim).filter(|s| !s.is_empty()) else { return Ok(None) };
        let (spaces, _) = self.spaces()?;
        let low = asked.to_lowercase();
        let exact = spaces.iter().find(|s| s.id == asked || s.display_name().to_lowercase() == low);
        // (A part of the name is enough when only one space has it: "personal"
        // for "Personal projects".)
        let found = exact.or_else(|| {
            let starts = |name: &str| name.starts_with(&low) || name.split(|ch: char| !ch.is_alphanumeric()).any(|word| !word.is_empty() && word.starts_with(&low));
            let mut with: Vec<&Space> = spaces.iter().filter(|s| starts(&s.display_name().to_lowercase())).collect();
            if with.len() == 1 { with.pop() } else { None }
        });
        match found {
            Some(space) => Ok(Some(space.clone())),
            None => {
                let names: Vec<String> = spaces.iter().map(|s| format!("\"{}\"", s.display_name())).collect();
                Err(format!(
                    "No space is named \"{asked}\". The spaces are: {}. Use one of these names, or ask the person to make the space in Rune first (you cannot make a space). Nothing was changed.",
                    names.join(", ")
                ))
            }
        }
    }

    /// The name of the space a page is in (a page with no space is in the first).
    fn space_name(spaces: &[Space], id: &str) -> String {
        spaces.iter().find(|s| s.id == id).or(spaces.first()).map(|s| s.display_name().to_string()).unwrap_or_default()
    }

    // ----- tools ------------------------------------------------------------

    fn search_pages(&self, args: &Value) -> ToolResult {
        let query = arg_str(args, "query").unwrap_or("").trim().to_lowercase();
        let limit = arg_u64(args, "limit")?.unwrap_or(10).clamp(1, 50) as usize;
        let (pages, blocks) = self.load()?;
        let (spaces, _) = self.spaces()?;
        let terms: Vec<&str> = query.split_whitespace().collect();

        let mut hits: Vec<(i64, i64, Value)> = Vec::new();
        for page in pages.values().filter(|p| !p.trashed) {
            let title = page.display_title().to_lowercase();
            let page_blocks = blocks.get(&page.id).map(Vec::as_slice).unwrap_or(&[]);
            let mut score = 0i64;
            let mut snippet = String::new();
            if !terms.is_empty() {
                let found_all = terms.iter().all(|t| {
                    title.contains(t) || page_blocks.iter().any(|b| b.text.to_lowercase().contains(t))
                });
                if !found_all {
                    continue;
                }
                if title == query {
                    score += 100;
                } else if title.contains(&query) {
                    score += 50;
                }
                score += 10 * terms.iter().filter(|t| title.contains(*t)).count() as i64;
                let mut content_hits = 0;
                for b in page_blocks {
                    let lower = b.text.to_lowercase();
                    let at = lower.find(&query).or_else(|| terms.iter().find_map(|t| lower.find(t)));
                    if let Some(at) = at {
                        content_hits += 1;
                        if snippet.is_empty() {
                            snippet = snippet_around(&b.text, &lower, at);
                        }
                    }
                }
                score += 2 * content_hits.min(10);
            }
            if snippet.is_empty() {
                snippet = page_blocks
                    .iter()
                    .find(|b| b.kind.is_editable() && !b.text.trim().is_empty())
                    .map(|b| truncate_chars(b.text.trim(), 140))
                    .unwrap_or_default();
            }
            let entry = json!({
                "id": page.id,
                "title": page.display_title(),
                "icon": page.icon,
                "path": path_of(&pages, &page.id),
                "space": Self::space_name(&spaces, &page.space_id),
                "snippet": snippet,
                "link": format!("{LINK_PREFIX}{}", page.id),
            });
            hits.push((score, page.updated_at, entry));
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let results: Vec<Value> = hits.into_iter().take(limit).map(|h| h.2).collect();
        Ok(pretty(&json!({ "count": results.len(), "results": results })))
    }

    fn list_pages(&self, args: &Value) -> ToolResult {
        let (pages, _) = self.load()?;
        let parent = optional_page_id(args, "parent_id")?;
        if let Some(pid) = &parent {
            live_page(&pages, pid)?;
        }
        let depth = arg_u64(args, "depth")?.unwrap_or(3).clamp(1, 10) as usize;
        let (spaces, open) = self.spaces()?;
        let only = self.space_arg(args)?;

        fn node(pages: &HashMap<Id, Page>, page: &Page, depth: usize) -> Value {
            let kids = children_of(pages, Some(&page.id));
            let children: Vec<Value> =
                if depth > 1 { kids.iter().map(|k| node(pages, k, depth - 1)).collect() } else { Vec::new() };
            json!({
                "id": page.id,
                "title": page.display_title(),
                "icon": page.icon,
                "favorite": page.favorite,
                "child_count": kids.len(),
                "children": children,
            })
        }

        // (A page with no space is in the first one, as the app shows it.)
        let first = spaces.first().map(|s| s.id.clone()).unwrap_or_default();
        let space_of = |p: &Page| if spaces.iter().any(|s| s.id == p.space_id) { p.space_id.clone() } else { first.clone() };
        let roots: Vec<Value> = children_of(&pages, parent.as_deref())
            .iter()
            .filter(|p| parent.is_some() || only.as_ref().map_or(true, |s| space_of(p) == s.id))
            .map(|p| {
                let mut entry = node(&pages, p, depth);
                // The top-level pages of every space come in one list: each says its space.
                if parent.is_none() {
                    entry["space"] = json!(Self::space_name(&spaces, &p.space_id));
                }
                entry
            })
            .collect();
        let listed: Vec<Value> = spaces.iter().map(|s| json!({ "name": s.display_name(), "open_in_rune": s.id == open })).collect();
        Ok(pretty(&json!({ "parent_id": parent, "depth": depth, "spaces": listed, "pages": roots })))
    }

    fn read_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, blocks) = self.load()?;
        let page = live_page(&pages, &id)?;
        self.seen.borrow_mut().insert(id.clone(), blocks.get(&id).cloned().unwrap_or_default());
        let body = render_body_with(&pages, Some(&blocks), blocks.get(&id).map(Vec::as_slice).unwrap_or(&[]));
        let (spaces, _) = self.spaces()?;
        // The page each row's page of a table starts with (its template), to
        // read and change as a page: `page:` makes one only for a table that has none.
        let templates: String = blocks
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|b| b.kind == BlockKind::Table)
            .enumerate()
            .filter_map(|(n, b)| {
                let of = crate::state::template_page_id(&pages, &b.id)?;
                let name = crate::table::Table::parse(&b.data).name;
                let name = if name.trim().is_empty() { format!("table {}", n + 1) } else { format!("table {}", quoted(name.trim())) };
                Some(format!("row_page_template: {of} (what each row's page of the {name} starts with)\n"))
            })
            .collect();
        // (The look of the page, when it has one.)
        let look = page.cover.as_deref().map(|c| format!("cover: {}\n", c.strip_prefix("preset:").unwrap_or("a picture"))).unwrap_or_default()
            + &(!page.font.is_empty()).then(|| format!("font: {}\n", page.font)).unwrap_or_default();
        let header = format!(
            "---\nid: {}\ntitle: {}\nicon: {}\n{look}parent_id: {}\npath: {}\nspace: {}\nfavorite: {}\nupdated: {}\nlink: {LINK_PREFIX}{}\n{templates}---\n\n",
            page.id,
            quoted(page.display_title()),
            page.icon.as_deref().unwrap_or("none"),
            page.parent_id.as_deref().unwrap_or("none (top level)"),
            quoted(&path_of(&pages, &page.id)),
            quoted(&Self::space_name(&spaces, &page.space_id)),
            page.favorite,
            iso8601(page.updated_at),
            page.id,
        );
        Ok(header + &body)
    }

    fn create_page(&self, args: &Value) -> ToolResult {
        let title = required_str(args, "title")?.trim().to_string();
        let parent = optional_page_id(args, "parent_id")?;
        // (An icon Rune does not have does not stop the page: it is made
        // with none, and the answer says so.)
        let (icon, icon_note) = match page_icon(arg_str(args, "icon").unwrap_or("")) {
            Ok(icon) => (icon, None),
            Err(why) => (None, Some(format!("{why} The page was made with no icon: set one with update_page."))),
        };
        let (pages, blocks) = self.load()?;
        if let Some(pid) = &parent {
            live_page(&pages, pid)?;
            // (Before the page is made: its line in a locked parent is
            // refused, and the new page stayed with no line to it.)
            not_locked(&blocks, pid)?;
        }
        let markdown = strip_title_heading(arg_str(args, "markdown").unwrap_or(""), &title);
        refuse_big_tables(&markdown, "No page was made.")?;

        let (spaces, open) = self.spaces()?;
        let asked = self.space_arg(args)?;
        let mut page = Page::new(parent.clone(), next_position(&pages, parent.as_deref()));
        // A sub-page joins its parent's space. A top-level page goes to the
        // space that the call names; with none named, to the space that is
        // open in Rune (empty falls back to the first space on load).
        page.space_id = match (&parent, &asked) {
            (Some(pid), asked) => {
                let of_parent = pages.get(pid).map(|p| p.space_id.clone()).unwrap_or_default();
                if let Some(space) = asked.as_ref().filter(|s| Self::space_name(&spaces, &s.id) != Self::space_name(&spaces, &of_parent)) {
                    return Err(format!(
                        "The parent page is in the space \"{}\", not in \"{}\": a sub-page is in the space of its parent. Leave out `space`, or choose a parent page in that space. Nothing was made.",
                        Self::space_name(&spaces, &of_parent),
                        space.display_name()
                    ));
                }
                of_parent
            }
            (None, Some(space)) => space.id.clone(),
            (None, None) => open,
        };
        page.title = title;
        page.icon = icon;
        page_look(args, &mut page)?;
        let mut body = non_empty(parse_body(&pages, &markdown));
        // (The new page among the pages, for the tables a relation can name.)
        let relation_notes = match markdown.contains("relation") {
            true => link_relations(&HashMap::from_iter(pages.clone().into_iter().chain([(page.id.clone(), page.clone())])), &blocks, &page.id, &mut body),
            false => Vec::new(),
        };
        self.save_page(&page)?;
        self.save_blocks(&page.id, &body)?;

        // Like a sub-page made in the app, the new page appears in its parent's body.
        if let Some(pid) = &parent {
            let mut parent_blocks = blocks.get(pid).cloned().unwrap_or_default();
            if is_blank(&parent_blocks) {
                parent_blocks.clear();
            }
            parent_blocks.push(Block::page_link(page.id.clone()));
            self.save_blocks(pid, &parent_blocks)?;
        }
        self.keep_row_templates(&page.id, &markdown)?;
        let mut answer = json!({
            "id": page.id,
            "title": page.display_title(),
            "parent_id": parent,
            "space": Self::space_name(&spaces, &page.space_id),
            "blocks": body.len(),
            "link": format!("{LINK_PREFIX}{}", page.id),
        });
        Self::with_check(&mut answer, icon_note.into_iter().chain(relation_notes).chain(self.check(&page.id, &markdown, 0)).collect());
        Ok(pretty(&answer))
    }

    /// What an AI tool should put right in what it just wrote on the page
    /// `id` (`markdown`): what the Markdown could not say (see
    /// `markdown::notes`), and charts of the page that draw no table or
    /// name columns their table does not have. (Each was left out or drawn
    /// another way in silence, and the tool said that all was made.)
    /// Only the charts from line `first` on are this call's (an append
    /// adds after the lines there: a chart the person made is not its to
    /// put right).
    fn check(&self, id: &str, markdown: &str, first: usize) -> Vec<String> {
        let mut notes = markdown::notes(markdown);
        if let Ok((pages, blocks)) = self.load() {
            notes.extend(chart_notes(&pages, &blocks, id, first));
        }
        notes
    }

    /// An answer with the notes of `check`, when there are any, and what
    /// to do with them.
    fn with_check(answer: &mut Value, notes: Vec<String>) {
        if !notes.is_empty() {
            answer["check"] = json!(notes);
            answer["to_do"] = json!("Put each point of check right now: read_page, then replace_page_content (or update_page for an icon).");
        }
    }

    fn append_to_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let markdown = required_str(args, "markdown")?;
        let (pages, blocks) = self.load()?;
        live_page(&pages, &id)?;
        refuse_big_tables(markdown, "Nothing was added to the page.")?;
        let added = parse_body(&pages, markdown);
        if added.is_empty() {
            return Err("Nothing to append: the markdown is empty.".into());
        }
        // (The tables of the page as it will be are those a relation can name.)
        let mut whole = blocks.get(&id).cloned().unwrap_or_default();
        let there = whole.len();
        whole.extend(added);
        let relation_notes = link_relations_from(&pages, &blocks, &id, &mut whole, there);
        // Only the new blocks are written, after the ones there: the page
        // is not written again as a whole, so nothing typed into it in the
        // app at the same moment is lost.
        let added = whole.split_off(there);
        let count = added.len();
        let total = self.store.append_blocks(&id, &added, now_ms()).map_err(db_error)?;
        self.keep_row_templates(&id, markdown)?;
        let mut answer = json!({ "page_id": id, "appended_blocks": count, "total_blocks": total });
        Self::with_check(&mut answer, relation_notes.into_iter().chain(self.check(&id, markdown, there)).collect());
        Ok(pretty(&answer))
    }

    /// A row at the end of a table of a page, its cells given by the names
    /// of their columns. A date column left out gets today.
    fn add_table_row(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let values = named_cells(args, "values")?;
        let (pages, blocks) = self.load()?;
        live_page(&pages, &id)?;
        let mut page_blocks = blocks.get(&id).cloned().unwrap_or_default();
        let at = table_in(&page_blocks, args.get("table"))?;
        let mut table = crate::table::Table::parse(&page_blocks[at].data);
        let today = crate::when::today();
        // Today goes in the date column; a date among the values takes its place.
        let row = table.add_day_row(today, today).ok_or("The table is full: a table holds at most 200 rows.")?;
        let unknown = table.put_named(row, &values, today);
        if !unknown.is_empty() && unknown.len() == values.len() {
            return Err(format!("The table has no column {}. Its columns: {}.", quoted_names(&unknown), headings(&table)));
        }
        let own_page = Self::own_page_arg(args)?;
        // (A row with a page goes by a name of its own: see `Table::name_rows`.)
        if own_page.is_some() {
            table.name_rows();
        }
        let cells = row_text(&table, row);
        page_blocks[at].data = table.to_data();
        page_blocks[at].text = table.plain_text();
        self.save_blocks(&id, &page_blocks)?;
        self.name_row_pages(&pages, &page_blocks[at].id, &table)?;
        let mut answer = json!({ "page_id": id, "row": row, "cells": cells, "columns_not_found": unknown });
        self.row_page_answer(&mut answer, &id, &page_blocks[at].id, &table, row, own_page)?;
        Ok(pretty(&answer))
    }

    /// The `page` of add_table_row and update_table_row: the Markdown of
    /// the row's own page ("" for the page as Rune makes it).
    fn own_page_arg(args: &Value) -> Result<Option<String>, String> {
        let Some(markdown) = arg_str(args, "page") else { return Ok(None) };
        refuse_big_tables(markdown, "Nothing was changed.")?;
        Ok(Some(markdown.to_string()))
    }

    /// The row's page in the answer of a tool that changed a row: written
    /// first when the tool was given `page` (see `write_row_page`); its id
    /// when the row has one, to read or change it as any page.
    fn row_page_answer(&self, answer: &mut Value, on: &str, block: &str, table: &crate::table::Table, r: usize, own_page: Option<String>) -> Result<(), String> {
        let notes = match &own_page {
            Some(markdown) => self.write_row_page(on, block, table, r, markdown)?,
            None => Vec::new(),
        };
        let (pages, _) = self.load()?;
        if let Some((page, false)) = table.row_id(r).and_then(|row| crate::state::row_page_id(&pages, block, row)) {
            answer["row_page_id"] = json!(page);
        }
        Self::with_check(answer, notes);
        Ok(())
    }

    /// Write the page of row `r` of the table `block` on the page `on` (the
    /// table as it is kept, its rows with names to go by): made now with
    /// `markdown` when the row has none (with "", as Rune makes it when the
    /// row is opened: from its template), or written again with it. Returns
    /// the notes of `check`. (The page of a row was made only when the
    /// person opened it: the AI could not write a recipe's steps or a
    /// day's notes into the page of its row.)
    fn write_row_page(&self, on: &str, block: &str, table: &crate::table::Table, r: usize, markdown: &str) -> Result<Vec<String>, String> {
        let (pages, blocks) = self.load()?;
        let row = table.row_id(r).ok_or("The row has no name to go by.")?;
        if let Some((page, trashed)) = crate::state::row_page_id(&pages, block, row) {
            if trashed {
                let mut back = pages[&page].clone();
                back.trashed = false;
                back.updated_at = now_ms();
                self.save_page(&back)?;
            }
            if markdown.trim().is_empty() {
                return Ok(Vec::new());
            }
            let written = self.replace_page_content(&json!({ "page_id": page, "markdown": markdown }))?;
            let notes = serde_json::from_str::<Value>(&written).ok().and_then(|v| v.get("check").cloned()).and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
            return Ok(notes);
        }
        let mut page = Page::new(Some(on.to_string()), 0.0);
        page.space_id = pages.get(on).map(|p| p.space_id.clone()).unwrap_or_default();
        page.row_of = Some(format!("{block}/{row}"));
        page.title = table.title(r).to_string();
        let mut lines = if markdown.trim().is_empty() {
            crate::state::new_row_lines(&pages, &blocks, block, table)
        } else {
            non_empty(lines_of(&pages, &strip_title_heading(markdown, &page.title)))
        };
        let mut notes = match markdown.contains("relation") {
            true => link_relations(&HashMap::from_iter(pages.clone().into_iter().chain([(page.id.clone(), page.clone())])), &blocks, &page.id, &mut lines),
            false => Vec::new(),
        };
        self.save_page(&page)?;
        self.save_blocks(&page.id, &lines)?;
        self.keep_row_templates(&page.id, markdown)?;
        // (The AI made it: it may write it again until someone changes it, see `row_page_free`.)
        if let Ok(now) = self.store.load_page_blocks(&page.id) {
            self.seen.borrow_mut().insert(page.id.clone(), now);
        }
        notes.extend(self.check(&page.id, markdown, 0));
        Ok(notes)
    }

    /// Change cells of one row of a table, found by its number under the
    /// header or by the text of one of its cells.
    fn update_table_row(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        // (With only the row's page to write, no cell need change.)
        let values = match (args.get("values"), args.get("page")) {
            (None, Some(_)) => Vec::new(),
            _ => named_cells(args, "values")?,
        };
        let (pages, blocks) = self.load()?;
        live_page(&pages, &id)?;
        let mut page_blocks = blocks.get(&id).cloned().unwrap_or_default();
        let at = table_in(&page_blocks, args.get("table"))?;
        let mut table = crate::table::Table::parse(&page_blocks[at].data);
        let today = crate::when::today();
        let row = match (arg_u64(args, "row")?, args.get("match").and_then(Value::as_object)) {
            (Some(n), _) if n >= 1 && (n as usize) < table.rows.len() => n as usize,
            (Some(n), _) => return Err(format!("The table has no row {n}: it has {} rows under its header.", table.rows.len() - 1)),
            (None, Some(m)) => {
                let (name, text) = m.iter().next().ok_or("match is empty: give one column and the text of its cell.")?;
                let text = text.as_str().map(str::to_string).unwrap_or_else(|| text.to_string());
                let c = table.find_column(name).ok_or_else(|| format!("The table has no column “{name}”. Its columns: {}.", headings(&table)))?;
                table.find_row(c, &text, today).ok_or_else(|| {
                    let cells: Vec<String> = table.body().map(|r| format!("“{}”", table.shown(r, c))).take(12).collect();
                    format!("No row has “{text}” in the column “{name}”. Its rows have there: {}.", if cells.is_empty() { "nothing (the table has no rows)".to_string() } else { cells.join(", ") })
                })?
            }
            (None, None) => return Err("Say which row: row (its number under the header, from 1) or match ({\"Column\": \"text\"}).".into()),
        };
        let own_page = Self::own_page_arg(args)?;
        let unknown = if values.is_empty() { Vec::new() } else { table.put_named(row, &values, today) };
        if !unknown.is_empty() && unknown.len() == values.len() {
            return Err(format!("The table has no column {}. Its columns: {}.", quoted_names(&unknown), headings(&table)));
        }
        if own_page.is_some() {
            table.name_rows();
        }
        // (Before a cell is saved: a refusal changes nothing.)
        self.row_page_free(&pages, &blocks, &page_blocks[at].id, table.row_id(row), own_page.as_deref())?;
        let cells = row_text(&table, row);
        page_blocks[at].data = table.to_data();
        page_blocks[at].text = table.plain_text();
        self.save_blocks(&id, &page_blocks)?;
        self.name_row_pages(&pages, &page_blocks[at].id, &table)?;
        let mut answer = json!({ "page_id": id, "row": row, "cells": cells, "columns_not_found": unknown });
        self.row_page_answer(&mut answer, &id, &page_blocks[at].id, &table, row, own_page)?;
        Ok(pretty(&answer))
    }

    /// `page` (`markdown`) for the row `row` of the table `block`, whose
    /// page has lines: refused unless the AI read that page and no one
    /// changed it since (or made it in this run). The page of a row is
    /// where the person writes (a day's spending, a trade's notes), and the
    /// AI was given its id only after the write: it wrote over lines it
    /// had never seen.
    fn row_page_free(&self, pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, block: &str, row: Option<&str>, markdown: Option<&str>) -> Result<(), String> {
        let (Some(row), Some(markdown)) = (row, markdown) else { return Ok(()) };
        if markdown.trim().is_empty() {
            return Ok(());
        }
        let Some((page, _)) = crate::state::row_page_id(pages, block, row) else { return Ok(()) };
        let now = blocks.get(&page).map(Vec::as_slice).unwrap_or_default();
        if is_blank(now) || self.seen.borrow().get(&page).is_some_and(|read| read.as_slice() == now) {
            return Ok(());
        }
        Err(format!(
            "The row already has its own page ({page}) with lines in it, which may be the person's, so it was not written over. Nothing was changed. \
             Read it with read_page (page_id {page}), then change it with replace_page_content, or add to it with append_to_page."
        ))
    }

    /// Tick or clear a to-do, or set or take away its day, found by words of
    /// its text.
    fn set_todo(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let words = required_str(args, "text")?.trim().to_lowercase();
        let done = args.get("done").and_then(Value::as_bool);
        let due = match arg_str(args, "due").map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(day) => Some(Some(crate::when::parse_date(day).ok_or_else(|| format!("due must be a day as 2026-11-12, or empty to take the day away; not “{day}”."))?)),
        };
        let remind = match arg_str(args, "remind").map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(time) => Some(Some(crate::model::Block::remind_from_column(time).ok_or_else(|| format!("remind must be a time as 09:00 or 14:30, or empty to take the reminder away; not “{time}”."))?)),
        };
        if done.is_none() && due.is_none() && remind.is_none() {
            return Err("Say what to change: done (true or false), due (a day as 2026-11-12, or empty), remind (a time as 09:00, or empty), or more of them.".into());
        }
        let (pages, blocks) = self.load()?;
        live_page(&pages, &id)?;
        let mut page_blocks = blocks.get(&id).cloned().unwrap_or_default();
        let todos: Vec<usize> = (0..page_blocks.len()).filter(|&i| page_blocks[i].kind == BlockKind::Todo).collect();
        let i = todos
            .iter()
            .copied()
            .find(|&i| page_blocks[i].plain_text().trim().to_lowercase() == words)
            .or_else(|| todos.iter().copied().find(|&i| page_blocks[i].plain_text().to_lowercase().contains(&words)))
            .ok_or_else(|| match todos.len() {
                0 => "This page has no to-dos (lines - [ ] …). Add one with append_to_page.".to_string(),
                _ => format!("No to-do of this page has the words “{words}”. Its to-dos: {}.", todos.iter().take(12).map(|&i| format!("“{}”", page_blocks[i].plain_text())).collect::<Vec<_>>().join(", ")),
            })?;
        if let Some(done) = done {
            page_blocks[i].checked = done;
        }
        if let Some(due) = due {
            page_blocks[i].due = due;
            if due.is_none() {
                page_blocks[i].remind = None;
            }
        }
        if let Some(remind) = remind {
            page_blocks[i].remind = remind;
            // A reminder needs a day: today, or tomorrow when the time has passed.
            if let (Some(t), None) = (remind, page_blocks[i].due) {
                let today = crate::when::today();
                page_blocks[i].due = Some(if crate::when::at(today, t) > crate::model::now_ms() { today } else { crate::when::next_day(today) });
            }
        }
        let todo = &page_blocks[i];
        let answer = json!({
            "page_id": id,
            "todo": todo.plain_text(),
            "done": todo.checked,
            "due": todo.due.map(|d| d.format("%Y-%m-%d").to_string()),
            "remind": todo.remind.map(|t| t.format("%H:%M").to_string()),
        });
        self.save_blocks(&id, &page_blocks)?;
        Ok(pretty(&answer))
    }

    fn replace_page_content(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let markdown = required_str(args, "markdown")?;
        let (pages, blocks) = self.load()?;
        let mut page = live_page(&pages, &id)?.clone();
        // (Before the new name is saved: the new lines are refused after it.)
        not_locked(&blocks, &id)?;
        refuse_big_tables(markdown, "The page was not changed.")?;
        // Keep the page as it was, so the change can be undone in Page history.
        let old = blocks.get(&id).map(|b| b.as_slice()).unwrap_or_default();
        self.changed_since_read(&id, old)?;
        self.store.save_version(&id, now_ms(), &page.title, old).map_err(db_error)?;
        if let Some(title) = arg_str(args, "title") {
            page.title = title.trim().to_string();
            page.updated_at = now_ms();
            self.save_page(&page)?;
        }
        // (A line that comes back as it was is the block it was: a toggle
        // stays a toggle, a callout keeps its icon, and a line of text under
        // one keeps its step in. Markdown cannot say those, and each such
        // block became a plain bullet, a quote or a line at the edge.)
        let said = page_said(&pages, markdown, &page.title, old);
        let mut body = crate::model::keep_unchanged(old, keep_media(old, said.clone()));
        as_said(old, &said, &mut body);
        let mut body = non_empty(body);
        // A table of the page is the same table still (its view, the pages of its rows).
        keep_tables(old, &mut body);
        let relation_notes = link_relations(&pages, &blocks, &id, &mut body);
        self.save_blocks(&id, &body)?;
        for b in body.iter().filter(|b| b.kind == BlockKind::Table) {
            self.name_row_pages(&pages, &b.id, &crate::table::Table::parse(&b.data))?;
        }
        if arg_str(args, "title").is_some() {
            self.name_row_cell(&page, &blocks)?;
        }
        self.keep_row_templates(&id, markdown)?;
        let mut answer = json!({ "page_id": id, "title": page.display_title(), "blocks": body.len() });
        Self::with_check(&mut answer, relation_notes.into_iter().chain(self.check(&id, markdown, 0)).collect());
        Ok(pretty(&answer))
    }

    /// The page was read with `read_page`, and its lines are not what was
    /// read any more (nor what the tools wrote since): someone changed it in
    /// Rune meanwhile. The page written from what was read would take that
    /// change away (a line typed while the AI worked, the end of a meeting's
    /// transcript), so it is refused, with what to do.
    fn changed_since_read(&self, id: &str, now: &[Block]) -> Result<(), String> {
        match self.seen.borrow().get(id) {
            Some(read) if read.as_slice() != now => Err(
                "The page was changed in Rune after you read it (someone typed in it, or a meeting is being written into it), so it was not replaced: \
                 your Markdown does not have that change and would take it away. Nothing was changed. Read the page again with read_page and make your change on what it gives. \
                 To only add to a page that keeps changing, use append_to_page."
                    .into(),
            ),
            _ => Ok(()),
        }
    }

    /// The tables of a page whose Markdown said what the page of each row
    /// starts with (`page:` on the table line): each that has no template
    /// yet gets one (see `state::row_template`). An existing template's
    /// page is never changed this way. (Only when the Markdown has a
    /// `page:` part: else nothing is read.)
    fn keep_row_templates(&self, page_id: &str, markdown: &str) -> Result<(), String> {
        if !markdown.to_lowercase().contains("page:") {
            return Ok(());
        }
        let said = markdown::said_pages(markdown);
        if said.is_empty() {
            return Ok(());
        }
        let (mut pages, mut blocks) = self.load()?;
        let lines = blocks.get(page_id).cloned().unwrap_or_default();
        for (block, said) in crate::state::said_for_blocks(&lines, said) {
            let Some((table, holder, template)) = crate::state::row_template(&pages, &blocks, page_id, &block, &said) else { continue };
            self.save_page(&holder)?;
            self.save_blocks(&holder.id, &template)?;
            blocks.insert(holder.id.clone(), template);
            pages.insert(holder.id.clone(), holder);
            let mut own = blocks.get(page_id).cloned().unwrap_or_default();
            if let Some(b) = own.iter_mut().find(|b| b.id == block) {
                b.data = table.to_data();
                b.text = table.plain_text();
            }
            self.save_blocks(page_id, &own)?;
            blocks.insert(page_id.to_string(), own);
        }
        Ok(())
    }

    /// The pages of the rows of a table take the names their rows have now.
    fn name_row_pages(&self, pages: &HashMap<Id, Page>, block: &str, table: &crate::table::Table) -> Result<(), String> {
        for (id, title) in stale_row_titles(pages, block, table) {
            let Some(mut page) = pages.get(&id).cloned() else { continue };
            page.title = title;
            page.updated_at = now_ms();
            self.save_page(&page)?;
        }
        Ok(())
    }

    /// A page that is a row of a table got another name: the first cell of
    /// its row takes it. (`blocks`: the lines of every page, as they are kept.)
    fn name_row_cell(&self, page: &Page, blocks: &HashMap<Id, Vec<Block>>) -> Result<(), String> {
        let Some((block, row)) = page.row_of.as_deref().and_then(|key| key.split_once('/')) else { return Ok(()) };
        let Some(parent) = page.parent_id.as_deref() else { return Ok(()) };
        let mut lines = blocks.get(parent).cloned().unwrap_or_default();
        let Some(b) = lines.iter_mut().find(|b| b.id == block && b.kind == BlockKind::Table) else { return Ok(()) };
        let mut table = crate::table::Table::parse(&b.data);
        let Some(r) = table.row_with_id(row).filter(|&r| table.cell(r, 0) != page.title) else { return Ok(()) };
        table.set(r, 0, &page.title);
        b.data = table.to_data();
        b.text = table.plain_text();
        self.save_blocks(parent, &lines)
    }

    fn update_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, blocks) = self.load()?;
        let mut page = live_page(&pages, &id)?.clone();
        not_locked(&blocks, &id)?;
        let mut changed = Vec::new();
        let mut new_space = None;

        if let Some(title) = arg_str(args, "title") {
            page.title = title.trim().to_string();
            changed.push("title");
        }
        if let Some(icon) = args.get("icon") {
            page.icon = match icon {
                Value::Null => None,
                Value::String(s) => page_icon(s).map_err(|why| format!("{why} Nothing was changed."))?,
                _ => return Err("`icon` must be NAME:COLOUR, as book:blue, or empty to remove it.".into()),
            };
            changed.push("icon");
        }
        if let Some(fav) = args.get("favorite") {
            page.favorite = fav.as_bool().ok_or("`favorite` must be true or false.")?;
            changed.push("favorite");
        }
        changed.extend(page_look(args, &mut page)?);
        if let Some(parent) = args.get("parent_id") {
            let new_parent = match parent {
                Value::Null => None,
                Value::String(s) if s.trim().is_empty() => None,
                Value::String(s) => Some(normalize_id(s)),
                _ => return Err("`parent_id` must be a page id, or empty to move the page to the top level.".into()),
            };
            if let Some(np) = &new_parent {
                live_page(&pages, np)?;
                not_locked(&blocks, np)?;
                if descendants(&pages, &id).contains(np) {
                    return Err("A page can't be moved inside itself or one of its sub-pages.".into());
                }
            }
            if new_parent != page.parent_id {
                page.position = next_position(&pages, new_parent.as_deref());
                page.parent_id = new_parent;
                // (The page of a row that goes to another page is a page like any other there.)
                page.row_of = None;
                // A page put in a page of another space is in that space,
                // with its sub-pages (as a move in the app does it): left in
                // its old space, it was filed there by search and opened the
                // wrong space.
                if let Some(space) = page.parent_id.as_ref().and_then(|np| pages.get(np)).map(|p| p.space_id.clone()).filter(|s| *s != page.space_id) {
                    page.space_id = space.clone();
                    new_space = Some(space);
                }
            }
            changed.push("parent_id");
        }
        // To another space: to its top level, with the pages under it. (With a
        // parent in the same call, the parent says the space.)
        if let Some(space) = self.space_arg(args)? {
            match page.parent_id.as_ref().filter(|_| args.get("parent_id").is_some()) {
                Some(np) => {
                    let of_parent = pages.get(np).map(|p| p.space_id.clone()).unwrap_or_default();
                    if of_parent != space.id {
                        return Err(format!("The new parent page is not in the space \"{}\": a sub-page is in the space of its parent. Pass `space` or `parent_id`, not both. Nothing was changed.", space.display_name()));
                    }
                }
                // (To another space only: a sub-page named with its own space
                // was pulled out of its parent.)
                None => {
                    if page.space_id != space.id {
                        page.position = next_position(&pages, None);
                        page.parent_id = None;
                        page.row_of = None;
                        page.space_id = space.id.clone();
                        new_space = Some(space.id.clone());
                        changed.push("space");
                    }
                }
            }
        }
        if changed.is_empty() {
            return Err("Nothing to update: pass title, icon, cover, font, favorite, parent_id or space.".into());
        }
        page.updated_at = now_ms();
        self.save_page(&page)?;
        if let Some(space) = new_space {
            // (The list begins with the page itself, which is saved above:
            // its old copy must not be written over it.)
            for inner in descendants(&pages, &id).into_iter().filter(|inner| *inner != id) {
                if let Some(sub) = pages.get(&inner) {
                    self.save_page(&Page { space_id: space.clone(), ..sub.clone() })?;
                }
            }
        }
        // (The name of a page that is a row of a table is the first cell of its row.)
        if changed.contains(&"title") {
            let (_, blocks) = self.load()?;
            self.name_row_cell(&page, &blocks)?;
        }
        let (spaces, _) = self.spaces()?;
        Ok(pretty(&json!({
            "page_id": id,
            "updated": changed,
            "title": page.display_title(),
            "icon": page.icon,
            "cover": page.cover,
            "font": page.font,
            "favorite": page.favorite,
            "parent_id": page.parent_id,
            "space": Self::space_name(&spaces, &page.space_id),
        })))
    }

    fn trash_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, blocks) = self.load()?;
        let page = pages.get(&id).ok_or_else(|| not_found(&id))?;
        if page.trashed {
            return Ok(format!("\"{}\" is already in the Trash.", page.display_title()));
        }
        not_locked(&blocks, &id)?;
        let ids = descendants(&pages, &id);
        let now = now_ms();
        let mut count = 0;
        for pid in &ids {
            if let Some(p) = pages.get(pid).filter(|p| !p.trashed) {
                let mut p = p.clone();
                p.trashed = true;
                p.updated_at = now;
                self.save_page(&p)?;
                count += 1;
            }
        }
        Ok(pretty(&json!({
            "trashed_pages": count,
            "title": page.display_title(),
            "note": "Moved to the Trash (not deleted). Undo with restore_page or from the app's Trash.",
        })))
    }

    // ----- calendar --------------------------------------------------------

    /// An event for an agent. `first` is the first day of its repeating
    /// event (for words like "Every week on Tuesday").
    fn event_json(&self, e: &crate::model::Event, first: chrono::NaiveDate) -> Value {
        let end_shown = if e.all_day { e.end - 1 } else { e.end };
        // The calendar is one for every space; each event says which it is in.
        let space = self.store.load_spaces().ok().and_then(|all| all.into_iter().find(|s| s.id == e.space_id)).map(|s| s.display_name().to_string());
        json!({
            "id": e.id,
            "title": e.display_title(),
            "space": space,
            "start": crate::when::iso(e.start, e.all_day),
            "end": crate::when::iso(end_shown, e.all_day),
            "all_day": e.all_day,
            "color": e.color,
            "notes": e.notes,
            "notes_page": e.page_id.as_ref().map(|p| crate::inline::page_url(p)),
            "reminder_minutes": e.remind,
            "repeats": e.repeat.as_ref().map(|r| crate::repeat::describe(r, first)),
        })
    }

    /// The stored event an id names. A repeat's id ("<id>@<day>") names its
    /// repeating event.
    fn find_event(&self, id: &str) -> Result<crate::model::Event, String> {
        let (series, _) = crate::repeat::split_id(id.trim());
        self.store
            .load_events()
            .map_err(db_error)?
            .into_iter()
            .find(|e| e.id == series)
            .ok_or_else(|| format!("No event with id {id}. Use list_events to find event ids."))
    }

    /// `repeat` and `repeat_until` from the arguments, applied to `e`.
    fn set_repeat(e: &mut crate::model::Event, args: &Value) -> Result<(), String> {
        if let Some(v) = args.get("repeat") {
            let word = v.as_str().unwrap_or("none");
            let rule = crate::repeat::parse(word).map_err(|_| {
                format!("Couldn't read repeat \"{word}\". Use none, daily, weekdays, weekly, biweekly, monthly or yearly.")
            })?;
            e.repeat = rule.map(|mut r| {
                if let Some(old) = &e.repeat {
                    r.until = old.until.clone();
                    r.skip = old.skip.clone();
                }
                r
            });
        }
        if let Some(v) = args.get("repeat_until") {
            let until = match v.as_str().filter(|s| !s.trim().is_empty()) {
                None => None,
                Some(s) => Some(
                    crate::when::parse_iso(s)
                        .map(|(t, _)| crate::when::date_value(crate::when::date_of(t)))
                        .ok_or_else(|| format!("Couldn't read repeat_until \"{s}\". Use a date like 2026-12-20."))?,
                ),
            };
            if let Some(r) = &mut e.repeat {
                r.until = until;
            }
        }
        Ok(())
    }

    fn list_events(&self, args: &Value) -> ToolResult {
        let today = crate::when::today();
        let from = match arg_str(args, "from") {
            Some(s) => crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read the date \"{s}\". Use 2026-11-10 or 2026-11-10T14:00."))?.0,
            None => crate::when::midnight(today),
        };
        let to = match arg_str(args, "to") {
            Some(s) => {
                let (t, day) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read the date \"{s}\"."))?;
                if day {
                    crate::when::midnight(crate::when::next_day(crate::when::date_of(t)))
                } else {
                    t
                }
            }
            None => from + 30 * 24 * crate::when::HOUR,
        };
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).min(200) as usize;
        let stored = self.store.load_events().map_err(db_error)?;
        let firsts: HashMap<&str, chrono::NaiveDate> =
            stored.iter().map(|e| (e.id.as_str(), crate::when::date_of(e.start))).collect();
        let mut shown = crate::repeat::expand(&stored, from, to);
        shown.sort_by_key(|e| e.start);
        let list: Vec<Value> = shown
            .iter()
            .take(limit)
            .map(|e| {
                let first = firsts.get(crate::repeat::split_id(&e.id).0).copied().unwrap_or_else(|| crate::when::date_of(e.start));
                self.event_json(e, first)
            })
            .collect();
        Ok(pretty(&json!({
            "from": crate::when::iso(from, false),
            "to": crate::when::iso(to, false),
            "count": list.len(),
            "events": list,
            "timezone": "times are local to this Mac",
        })))
    }

    fn create_event(&self, args: &Value) -> ToolResult {
        let title = required_str(args, "title")?.trim().to_string();
        let start_s = required_str(args, "start")?;
        let (start, day) = crate::when::parse_iso(start_s).ok_or_else(|| format!("Couldn't read start \"{start_s}\". Use 2026-11-10 for an all-day event or 2026-11-10T14:00 for a time."))?;
        let all_day = args.get("all_day").and_then(Value::as_bool).unwrap_or(day);
        let end = match arg_str(args, "end") {
            Some(s) => {
                let (t, end_day) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read end \"{s}\"."))?;
                if all_day || end_day {
                    crate::when::midnight(crate::when::next_day(crate::when::date_of(t)))
                } else {
                    t
                }
            }
            None if all_day => crate::when::midnight(crate::when::next_day(crate::when::date_of(start))),
            None => start + crate::when::HOUR,
        };
        let start = if all_day { crate::when::midnight(crate::when::date_of(start)) } else { start };
        // The space it is named for, or the page's that holds its notes, or the one open in Rune.
        let notes_page = self.event_page(args)?;
        let space = match (self.space_arg(args)?, &notes_page) {
            (Some(space), _) => space.id,
            (None, Some(Some(page))) => self.load()?.0.get(page).map(|p| p.space_id.clone()).unwrap_or_default(),
            (None, _) => self.store.get_setting("space").unwrap_or_default(),
        };
        let mut e = crate::model::Event::new(space, title, start, end, all_day);
        e.page_id = notes_page.flatten();
        if let Some(n) = arg_str(args, "notes") {
            e.notes = n.to_string();
        }
        if let Some(c) = arg_str(args, "color").filter(|c| crate::ui::icons::ICON_COLORS.iter().any(|(id, _)| id == c)) {
            e.color = c.to_string();
        }
        e.remind = match args.get("reminder_minutes") {
            Some(Value::Null) => None,
            Some(v) => v.as_i64().filter(|m| *m >= 0),
            None => match self.store.get_setting("default_reminder") {
                Some(v) => crate::notify::parse_reminder(&v),
                None => Some(10),
            },
        };
        Self::set_repeat(&mut e, args)?;
        self.store.save_event(&e).map_err(db_error)?;
        Ok(pretty(&json!({ "created": self.event_json(&e, crate::when::date_of(e.start)) })))
    }

    /// The page that holds an event's notes (`page_id`: a page, or "" for
    /// none), when the call gives one: it shows under the event in the
    /// calendar. (The tools could not link an event to a page, so the flight
    /// of a trip plan was not with its page.)
    fn event_page(&self, args: &Value) -> Result<Option<Option<Id>>, String> {
        match args.get("page_id") {
            None => Ok(None),
            Some(_) => match optional_page_id(args, "page_id")? {
                None => Ok(Some(None)),
                Some(id) => {
                    live_page(&self.load()?.0, &id)?;
                    Ok(Some(Some(id)))
                }
            },
        }
    }

    fn update_event(&self, args: &Value) -> ToolResult {
        let id = required_str(args, "event_id")?;
        let mut series = self.find_event(id)?;
        let notes_page = self.event_page(args)?;
        // A change to one repeat goes to them all (moved by the same days and
        // time), as in the app.
        let day = crate::repeat::split_id(id.trim()).1.filter(|_| series.repeat.is_some());
        let mut e = match day {
            Some(d) => crate::repeat::at(&series, d),
            None => series.clone(),
        };
        if let Some(t) = arg_str(args, "title") {
            e.title = t.trim().to_string();
        }
        if let Some(n) = arg_str(args, "notes") {
            e.notes = n.to_string();
        }
        if let Some(c) = arg_str(args, "color").filter(|c| crate::ui::icons::ICON_COLORS.iter().any(|(id, _)| id == c)) {
            e.color = c.to_string();
        }
        if let Some(all_day) = args.get("all_day").and_then(Value::as_bool) {
            e.all_day = all_day;
        }
        match args.get("reminder_minutes") {
            Some(Value::Null) => e.remind = None,
            Some(v) => e.remind = v.as_i64().filter(|m| *m >= 0),
            None => {}
        }
        let duration = e.end - e.start;
        if let Some(s) = arg_str(args, "start") {
            let (t, _) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read start \"{s}\"."))?;
            e.start = if e.all_day { crate::when::midnight(crate::when::date_of(t)) } else { t };
            e.end = e.start + duration;
        }
        if let Some(s) = arg_str(args, "end") {
            let (t, day) = crate::when::parse_iso(s).ok_or_else(|| format!("Couldn't read end \"{s}\"."))?;
            e.end = if e.all_day || day { crate::when::midnight(crate::when::next_day(crate::when::date_of(t))) } else { t };
        }
        if e.end <= e.start {
            e.end = e.start + if e.all_day { 24 * crate::when::HOUR } else { crate::when::HOUR };
        }
        Self::set_repeat(&mut e, args)?;
        let e = match day {
            Some(d) => {
                let before = crate::repeat::at(&series, d);
                crate::repeat::apply(&mut series, &before, &e);
                series
            }
            None => e,
        };
        let mut e = e;
        if let Some(page) = notes_page {
            e.page_id = page;
        }
        e.updated_at = now_ms();
        self.store.save_event(&e).map_err(db_error)?;
        Ok(pretty(&json!({ "updated": self.event_json(&e, crate::when::date_of(e.start)) })))
    }

    fn delete_event(&self, args: &Value) -> ToolResult {
        let id = required_str(args, "event_id")?;
        let mut e = self.find_event(id)?;
        let all = args.get("all_repeats").and_then(Value::as_bool).unwrap_or(false);
        if let (Some(day), Some(rule), false) = (crate::repeat::split_id(id.trim()).1, e.repeat.as_mut(), all) {
            rule.skip.push(crate::when::date_value(day));
            e.updated_at = now_ms();
            self.store.save_event(&e).map_err(db_error)?;
            return Ok(format!("Deleted the {} repeat of \"{}\"; the other repeats stay.", day.format("%b %-d"), e.display_title()));
        }
        self.store.delete_event(&e.id).map_err(db_error)?;
        Ok(format!("Deleted the event \"{}\".", e.display_title()))
    }

    fn restore_page(&self, args: &Value) -> ToolResult {
        let id = required_page_id(args, "page_id")?;
        let (pages, _) = self.load()?;
        let page = pages.get(&id).ok_or_else(|| not_found(&id))?;
        if !page.trashed {
            return Ok(format!("\"{}\" is not in the Trash.", page.display_title()));
        }
        let parent_gone = page
            .parent_id
            .as_ref()
            .is_some_and(|parent| pages.get(parent).is_none_or(|pp| pp.trashed));
        let now = now_ms();
        let ids = descendants(&pages, &id);
        for pid in &ids {
            if let Some(p) = pages.get(pid) {
                let mut p = p.clone();
                p.trashed = false;
                p.updated_at = now;
                if parent_gone && p.id == id {
                    p.parent_id = None;
                }
                self.save_page(&p)?;
            }
        }
        Ok(pretty(&json!({
            "restored_pages": ids.len(),
            "title": page.display_title(),
            "moved_to_top_level": parent_gone,
        })))
    }
}

// ---------------------------------------------------------------------------
// Protocol pieces
// ---------------------------------------------------------------------------

/// Rune's logo, for the tools that show a connection with its icon (MCP
/// 2025-11-25 `icons`): the drawing, and a small picture for those that
/// take no drawings. Both are in the program, so they work offline.
fn icons() -> Value {
    use base64::Engine;
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    json!([
        { "src": format!("data:image/svg+xml;base64,{}", b64(include_bytes!("../assets/logo.svg"))), "mimeType": "image/svg+xml", "sizes": ["any"] },
        { "src": format!("data:image/png;base64,{}", b64(include_bytes!("../assets/logo-64.png"))), "mimeType": "image/png", "sizes": ["64x64"] },
    ])
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = if SUPPORTED_VERSIONS.contains(&requested) { requested } else { SUPPORTED_VERSIONS[0] };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "rune",
            "title": "Rune",
            "version": env!("CARGO_PKG_VERSION"),
            "websiteUrl": "https://github.com/capi-git/Rune",
            "icons": icons(),
        },
        "instructions": INSTRUCTIONS,
    })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_definitions() -> Value {
    let page_id = json!({ "type": "string", "description": "Page id (or a rune://page/<id> link)." });
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    let write = |idempotent: bool| {
        json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": idempotent, "openWorldHint": false })
    };
    json!([
        {
            "name": "search_pages",
            "title": "Search pages",
            "description": "Search the workspace by page title and content (case-insensitive; every word must appear). Returns page ids, titles, their path of parent pages and a snippet. An empty query returns the most recently edited pages.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Most results to return (default 10)." }
                },
                "required": ["query"]
            },
            "annotations": read_only
        },
        {
            "name": "list_pages",
            "title": "List pages",
            "description": "List the page tree: top-level pages (of every space, each with the name of its space; or of one space, with `space`), or the sub-pages of parent_id, nested `depth` levels deep (default 3). Also names the spaces and says which one is open in Rune. Trashed pages are left out.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "parent_id": page_id,
                    "space": { "type": "string", "description": "Only the top-level pages of the space with this name." },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 10, "description": "Levels of sub-pages to include (default 3)." }
                }
            },
            "annotations": read_only
        },
        {
            "name": "read_page",
            "title": "Read a page",
            "description": "Read a page as Markdown, after a short header with its id, title, icon, parent and path. Links to other pages appear as [Title](rune://page/<id>) on their own line.",
            "inputSchema": {
                "type": "object",
                "properties": { "page_id": page_id },
                "required": ["page_id"]
            },
            "annotations": read_only
        },
        {
            "name": "create_page",
            "title": "Create a page",
            "description": "Create a page with a title and optional Markdown content. With parent_id it becomes a sub-page and a link to it is added to the end of the parent page. A top-level page goes to the space named by `space`, or, with none named, to the space that is open in Rune. Returns the new page's id and its space.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Page title." },
                    "markdown": { "type": "string", "description": "Page content as Markdown (don't repeat the title as a heading)." },
                    "parent_id": { "type": "string", "description": "Optional parent page id; omit for a top-level page." },
                    "space": { "type": "string", "description": "For a top-level page: the name of the space it goes in (see list_pages). Omit to use the space that is open in Rune." },
                    "cover": { "type": "string", "description": "Optional band across the top: sand, clay, ember, moss, sea, slate, dawn, night, meadow, shore, stone, rust, honey, fern, lake or plum." },
                    "font": { "type": "string", "enum": ["sans", "serif", "mono"], "description": "Optional face of the title and headings (default sans)." },
                    "icon": { "type": "string", "description": "Optional icon, NAME:COLOUR, as book:blue (Rune draws its own icons, not emoji). NAME: file, note, book, bookmark, folder, calendar, check-circle, lightbulb, rocket, target, star, flame, leaf, briefcase, home, plane, palette, music, flask, wrench, chart, chat, coffee, globe, lock, heart, smile, bot, code, warning, info, pin, bell, inbox, map-pin, camera, gift, trophy or sparkles; COLOUR: gray, brown, orange, yellow, green, blue, purple, pink or red." }
                },
                "required": ["title"]
            },
            "annotations": write(false)
        },
        {
            "name": "append_to_page",
            "title": "Append to a page",
            "description": "Add Markdown content to the end of a page, keeping what is already there.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "markdown": { "type": "string", "description": "Content to add, as Markdown." }
                },
                "required": ["page_id", "markdown"]
            },
            "annotations": write(false)
        },
        {
            "name": "add_table_row",
            "title": "Add a row to a table",
            "description": "Add a row at the end of a table in a page (a log), without writing the page again. Give the cells by the names of their columns, for example {\"Date\": \"today\", \"km\": \"6\", \"Time\": \"34 min\"}. A date column left out gets today; days, times and ticks are written the table's way; a pace column is worked out by itself. With more than one table, say which with table (its number in the page, from 1, or a word of its header).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "values": { "type": "object", "description": "The cells: column name → text.", "additionalProperties": { "type": ["string", "number", "boolean"] } },
                    "table": { "type": ["integer", "string"], "description": "Which table of the page (default the first): its number from 1, or a word of its header." },
                    "page": { "type": "string", "description": "Optional: the row's own page as Markdown (a recipe's steps, a day's notes), made with it; \"\" makes it as Rune does when the row is opened (from the template). The answer gives its row_page_id." }
                },
                "required": ["page_id", "values"]
            },
            "annotations": write(false)
        },
        {
            "name": "update_table_row",
            "title": "Change a row of a table",
            "description": "Change cells of one row of a table in a page, without writing the page again, or write the row's own page (page). Find the row by row (its number under the header, from 1) or by match (one cell, as {\"Date\": \"2026-12-01\"}), and give the new cells in values by the names of their columns.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "values": { "type": "object", "description": "The new cells: column name → text.", "additionalProperties": { "type": ["string", "number", "boolean"] } },
                    "row": { "type": "integer", "minimum": 1, "description": "The row's number under the header, from 1." },
                    "match": { "type": "object", "description": "One column and the text of its cell in the row, as {\"Date\": \"2026-12-01\"}." },
                    "table": { "type": ["integer", "string"], "description": "Which table of the page (default the first): its number from 1, or a word of its header." },
                    "page": { "type": "string", "description": "Optional: the row's own page as Markdown, made with it; \"\" makes it as Rune does when the row is opened. A row page that has lines is written again only after you read it (read_page); else nothing is changed. The answer gives its row_page_id." }
                },
                "required": ["page_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "set_todo",
            "title": "Tick a to-do, set its day or a reminder",
            "description": "Tick or clear a to-do of a page, set or take away the day it is due, or set a reminder at a time of that day (Rune sends a notification then), without writing the page again. The to-do is found by words of its text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "text": { "type": "string", "description": "Words of the to-do's text." },
                    "done": { "type": "boolean", "description": "true to tick it, false to clear it." },
                    "due": { "type": "string", "description": "The day it is due, as 2026-11-12; empty to take the day away." },
                    "remind": { "type": "string", "description": "A reminder at this time of its day, as 09:00 or 14:30 (with no day: today, or tomorrow when the time has passed); empty to take the reminder away." }
                },
                "required": ["page_id", "text"]
            },
            "annotations": write(true)
        },
        {
            "name": "replace_page_content",
            "title": "Replace a page's content",
            "description": "Replace everything in a page's body with new Markdown (optionally renaming it). Read the page first and keep its [Title](rune://page/<id>) link lines to keep links to sub-pages; sub-pages themselves are never removed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "markdown": { "type": "string", "description": "The page's full new content, as Markdown." },
                    "title": { "type": "string", "description": "Optional new title." }
                },
                "required": ["page_id", "markdown"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "update_page",
            "title": "Update page details",
            "description": "Change a page's title, icon, cover, face or favorite flag, move it under another page (parent_id; empty string moves it to the top level), or move it to the top level of another space (space). Its sub-pages go with it. Content is unchanged.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_id": page_id,
                    "title": { "type": "string" },
                    "icon": { "type": "string", "description": "NAME:COLOUR as for create_page, or empty to remove the icon." },
                    "favorite": { "type": "boolean" },
                    "parent_id": { "type": "string", "description": "New parent page id, or empty for top level." },
                    "space": { "type": "string", "description": "The name of the space to move the page to (to its top level)." },
                    "cover": { "type": "string", "description": "A cover as for create_page, or empty to take it away." },
                    "font": { "type": "string", "enum": ["sans", "serif", "mono"] }
                },
                "required": ["page_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "trash_page",
            "title": "Move a page to the Trash",
            "description": "Move a page and its sub-pages to the Trash. This is reversible (restore_page); pages are never permanently deleted by this server.",
            "inputSchema": {
                "type": "object",
                "properties": { "page_id": page_id },
                "required": ["page_id"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "restore_page",
            "title": "Restore a page from the Trash",
            "description": "Bring a trashed page and its sub-pages back. If its parent is gone it returns at the top level.",
            "inputSchema": {
                "type": "object",
                "properties": { "page_id": page_id },
                "required": ["page_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "list_events",
            "title": "List calendar events",
            "description": "Events in the Rune calendar between two dates (default: the next 30 days), from every space; each event names its space. Times are local to this Mac, written like 2026-11-10T14:00; all-day events use plain dates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Start of the range: 2026-11-10 or 2026-11-10T09:00. Default: today." },
                    "to": { "type": "string", "description": "End of the range (inclusive for a plain date). Default: 30 days after from." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                }
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "create_event",
            "title": "Add a calendar event",
            "description": "Add an event to the calendar: in the space named by space, else the space of its page_id, else the one open in Rune. Give start as 2026-11-10T14:00 for a timed event (an hour long unless end is given) or 2026-11-10 for an all-day event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "start": { "type": "string", "description": "2026-11-10T14:00 (local time) or 2026-11-10 (all day)" },
                    "end": { "type": "string", "description": "Optional end, same format. For all-day events, the last day." },
                    "all_day": { "type": "boolean" },
                    "notes": { "type": "string" },
                    "color": { "type": "string", "enum": ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"] },
                    "reminder_minutes": { "type": ["integer", "null"], "minimum": 0, "description": "Minutes before the start for a Mac notification (0 = at the start; null = none). Default: the app's setting, normally 10." },
                    "repeat": { "type": "string", "enum": ["none", "daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"], "description": "How the event repeats, counted from its start. Default: none." },
                    "repeat_until": { "type": ["string", "null"], "description": "The last day it may repeat on, like 2026-12-20. Default: no end." },
                    "space": { "type": "string", "description": "The name of the space it goes in (see list_pages)." },
                    "page_id": { "type": "string", "description": "A page that holds its notes (a trip plan for its flight, the agenda of a meeting): it shows under the event in the calendar." }
                },
                "required": ["title", "start"]
            },
            "annotations": write(false)
        },
        {
            "name": "update_event",
            "title": "Change a calendar event",
            "description": "Change an event's title, time, notes, colour or how it repeats. Moving the start keeps the length unless end is also given. Given one repeat of a repeating event (an id ending in @<date>), the change goes to every repeat, moved by the same amount.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": { "type": "string" },
                    "title": { "type": "string" },
                    "start": { "type": "string" },
                    "end": { "type": "string" },
                    "all_day": { "type": "boolean" },
                    "notes": { "type": "string" },
                    "color": { "type": "string", "enum": ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"] },
                    "reminder_minutes": { "type": ["integer", "null"], "minimum": 0 },
                    "repeat": { "type": "string", "enum": ["none", "daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"] },
                    "repeat_until": { "type": ["string", "null"], "description": "The last day it may repeat on (null: no end)." },
                    "page_id": { "type": "string", "description": "A page that holds its notes, or empty for none." }
                },
                "required": ["event_id"]
            },
            "annotations": write(true)
        },
        {
            "name": "delete_event",
            "title": "Delete a calendar event",
            "description": "Remove an event from the calendar for good. Given one repeat of a repeating event (an id ending in @<date>), only that repeat is removed unless all_repeats is true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": { "type": "string" },
                    "all_repeats": { "type": "boolean", "description": "With a repeat's id: remove every repeat. Default: false." }
                },
                "required": ["event_id"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        }
    ])
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A locked page is changed only in Rune, with its password. A tool that
/// would change it (its name, its place, the Trash, a page made or put
/// inside it) refuses before it changes anything. Only the store refused
/// before, and only the lines: the page then had its new name, or a
/// sub-page with no line to it, and it could be moved and put in the Trash.
fn not_locked(blocks: &HashMap<Id, Vec<Block>>, id: &str) -> Result<(), String> {
    if blocks.get(id).is_some_and(|lines| crate::lock::hides(lines, id)) {
        return Err(format!("{} Nothing was changed.", crate::store::REFUSED_LOCKED));
    }
    Ok(())
}

fn db_error(e: rusqlite::Error) -> String {
    format!("Workspace database error: {e}")
}

fn not_found(id: &str) -> String {
    format!("No page with id {id}. Use search_pages or list_pages to find page ids.")
}

/// A page's icon as a tool gives it: NAME:COLOUR, a NAME alone (in gray),
/// or an emoji that Rune has an icon for; `None` for empty (no icon).
/// Rune draws only its own icons: any other emoji showed as a gray page,
/// and the tool was told it was fine. An icon Rune does not have is an
/// error that names those it has.
fn page_icon(given: &str) -> Result<Option<String>, String> {
    let given = given.trim();
    if given.is_empty() {
        return Ok(None);
    }
    match crate::ui::icons::known_icon(&given.to_lowercase()) {
        Some((name, color)) => Ok(Some(crate::ui::icons::format_icon(&name, &color))),
        None => Err(format!(
            "Rune has no icon “{given}”: it draws its own icons, not emoji. Give NAME:COLOUR, as book:blue; NAME one of {}; COLOUR one of {}.",
            crate::ui::icons::PAGE_ICONS.join(", "),
            crate::ui::icons::ICON_COLORS.iter().map(|(id, _)| *id).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The look a tool gives a page: `cover` (one of the app's own covers, by
/// name; "" for none) and `font` (sans, serif or mono), set on `page`.
/// Which were given, for the answer. (The tools could set neither, so a
/// page asked for "with a cover" or "in a serif face" was made without.)
fn page_look(args: &Value, page: &mut Page) -> Result<Vec<&'static str>, String> {
    let mut set = Vec::new();
    if let Some(cover) = arg_str(args, "cover").map(|c| c.trim().to_lowercase()) {
        let name = cover.strip_prefix("preset:").unwrap_or(&cover);
        page.cover = match name {
            "" | "none" => None,
            _ if crate::model::covers::css(name).is_some() => Some(format!("preset:{name}")),
            _ => return Err(format!("Rune has no cover “{cover}”. The covers: {}; or empty for none. Nothing was changed.", crate::model::covers::PRESETS.iter().map(|(id, ..)| *id).collect::<Vec<_>>().join(", "))),
        };
        page.cover_y = 50;
        set.push("cover");
    }
    if let Some(font) = arg_str(args, "font").map(|f| f.trim().to_lowercase()) {
        page.font = match font.as_str() {
            "" | "sans" | "default" => String::new(),
            "serif" | "mono" => font,
            _ => return Err(format!("A page's face is sans, serif or mono, not “{font}”. Nothing was changed.")),
        };
        set.push("font");
    }
    Ok(set)
}

/// The charts of a page that draw no table, or name a column their table
/// does not have (else they drew the first date and number columns, which
/// may be others), each as a note for the AI tool (see `Server::check`).
/// The table is found as the app finds it (`AppState::chart_source`).
fn chart_notes(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, page_id: &str, first: usize) -> Vec<String> {
    let lines = blocks.get(page_id).map(Vec::as_slice).unwrap_or_default();
    let space = pages.get(page_id).map(|p| p.space_id.clone());
    let mut notes = Vec::new();
    for (at, b) in lines.iter().enumerate().skip(first).filter(|(_, b)| b.kind == BlockKind::Chart) {
        let chart = crate::chart::Chart::parse(&b.data);
        let from = chart.from.trim();
        let name = if chart.title.trim().is_empty() { "A chart".to_string() } else { format!("The chart “{}”", chart.title.trim()) };
        let table = if from.is_empty() || from == page_id {
            lines[..at].iter().rev().find(|b| b.kind == BlockKind::Table).or_else(|| lines.iter().find(|b| b.kind == BlockKind::Table))
        } else {
            let want = from.to_lowercase();
            let mut found: Vec<&Page> = pages.values().filter(|p| !p.trashed && (p.id == from || p.title.trim().to_lowercase() == want)).collect();
            found.sort_by_key(|p| (p.id != from, Some(&p.space_id) != space.as_ref(), std::cmp::Reverse(p.updated_at)));
            found.first().and_then(|p| blocks.get(&p.id)).and_then(|bs| bs.iter().find(|b| b.kind == BlockKind::Table))
        };
        let Some(table) = table.map(|b| crate::table::Table::parse(&b.data)) else {
            notes.push(if from.is_empty() {
                format!("{name} draws no table: this page has none. Put the chart under its table, or give \"from\": the title of the page whose first table it draws.")
            } else {
                format!("{name} draws no table: no page called “{from}” has one. \"from\" is the title or id of a page whose first table it draws.")
            });
            continue;
        };
        for (key, column) in [("x", &chart.x), ("y", &chart.y)] {
            if !column.trim().is_empty() && table.find_column(column).is_none() {
                notes.push(format!("{name} has \"{key}\": “{column}”, but its table has no such column. Its columns: {}.", headings(&table)));
            }
        }
    }
    notes
}

/// The cells a tool was given, as (column, text) pairs.
fn named_cells(args: &Value, key: &str) -> Result<Vec<(String, String)>, String> {
    let values = args.get(key).and_then(Value::as_object).ok_or_else(|| format!("{key} must be an object of column name → text."))?;
    if values.is_empty() {
        return Err(format!("{key} is empty: give at least one cell."));
    }
    Ok(values
        .iter()
        .map(|(k, v)| {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Bool(true) => "x".to_string(),
                Value::Bool(false) | Value::Null => String::new(),
                other => other.to_string(),
            };
            (k.clone(), text)
        })
        .collect())
}

/// The index of the table a tool means: the one with this number (from 1),
/// the first whose header has this word, or the first.
fn table_in(blocks: &[Block], which: Option<&Value>) -> Result<usize, String> {
    let tables: Vec<usize> = (0..blocks.len()).filter(|&i| blocks[i].kind == BlockKind::Table).collect();
    if tables.is_empty() {
        return Err("This page has no table. Add one with append_to_page (a Markdown pipe table, with a columns line for its kinds).".into());
    }
    match which {
        Some(Value::Number(n)) => {
            let n = n.as_u64().unwrap_or(0) as usize;
            tables.get(n.wrapping_sub(1)).copied().ok_or_else(|| format!("The page has {} tables; there is no table {n}.", tables.len()))
        }
        Some(Value::String(word)) if !word.trim().is_empty() => {
            if let Ok(n) = word.trim().parse::<usize>() {
                return table_in(blocks, Some(&json!(n)));
            }
            tables
                .iter()
                .copied()
                .find(|&i| crate::table::Table::parse(&blocks[i].data).find_column(word).is_some())
                .ok_or_else(|| {
                    let each: Vec<String> = tables.iter().map(|&i| format!("({})", headings(&crate::table::Table::parse(&blocks[i].data)))).collect();
                    format!("No table of this page has a column “{word}”. Its tables have the columns {}; or give the table's number, from 1.", each.join(" and "))
                })
        }
        _ => Ok(tables[0]),
    }
}

/// One row of a table as column → what it shows.
fn row_text(table: &crate::table::Table, row: usize) -> Value {
    let mut out = serde_json::Map::new();
    for c in 0..table.columns() {
        out.insert(table.heading(c), json!(table.shown(row, c)));
    }
    Value::Object(out)
}

fn headings(table: &crate::table::Table) -> String {
    (0..table.columns()).map(|c| table.heading(c)).collect::<Vec<_>>().join(", ")
}

fn quoted_names(names: &[String]) -> String {
    names.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ")
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(format!("`{key}` must be a string.")),
        None => Err(format!("Missing required argument `{key}`.")),
    }
}

fn arg_u64(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be a positive whole number.")),
    }
}

/// Accept a bare id or a link to the page (rune://page/<id>, or the older rune://page/<id>).
fn normalize_id(raw: &str) -> String {
    let s = raw.trim();
    crate::inline::page_id_of(s).unwrap_or(s).trim_end_matches('/').to_string()
}

fn required_page_id(args: &Value, key: &str) -> Result<String, String> {
    let id = normalize_id(required_str(args, key)?);
    if id.is_empty() {
        return Err(format!("`{key}` can't be empty."));
    }
    Ok(id)
}

fn optional_page_id(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(normalize_id(s))),
        Some(_) => Err(format!("`{key}` must be a page id string.")),
    }
}

/// The page, if it exists and is not in the Trash.
fn live_page<'a>(pages: &'a HashMap<Id, Page>, id: &str) -> Result<&'a Page, String> {
    let page = pages.get(id).ok_or_else(|| not_found(id))?;
    if page.trashed {
        return Err(format!(
            "\"{}\" ({id}) is in the Trash. Restore it with restore_page first.",
            page.display_title()
        ));
    }
    Ok(page)
}

fn path_of(pages: &HashMap<Id, Page>, id: &str) -> String {
    let mut parts: Vec<String> = ancestors(pages, id).iter().map(|p| p.display_title().to_string()).collect();
    if let Some(p) = pages.get(id) {
        parts.push(p.display_title().to_string());
    }
    parts.join(" / ")
}

fn next_position(pages: &HashMap<Id, Page>, parent: Option<&str>) -> f64 {
    children_of(pages, parent).last().map(|p| p.position + 1.0).unwrap_or(1.0)
}

/// A page whose only block is an empty paragraph (what a new page starts with).
fn is_blank(blocks: &[Block]) -> bool {
    blocks.is_empty() || (blocks.len() == 1 && blocks[0].kind == BlockKind::Text && blocks[0].text.trim().is_empty())
}

fn non_empty(blocks: Vec<Block>) -> Vec<Block> {
    if blocks.is_empty() {
        vec![Block::text("")]
    } else {
        blocks
    }
}

/// The Markdown given for the page `old` (titled `title`) as its lines,
/// with a fence around all of it taken away once. Not when the page is one
/// code block of Markdown (a README draft): Rune gave it as that one fence,
/// and unwrapped as a reply in a fence is, it became lines of the page.
fn page_said(pages: &HashMap<Id, Page>, markdown: &str, title: &str, old: &[Block]) -> Vec<Block> {
    let mut real = old.iter().filter(|b| !(b.kind == BlockKind::Text && b.text.trim().is_empty()));
    let one_md_code = matches!((real.next(), real.next()), (Some(b), None) if b.kind == BlockKind::Code
        && b.language.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("markdown") || l.eq_ignore_ascii_case("md")));
    if one_md_code {
        lines_of(pages, &strip_title_only(markdown, title))
    } else {
        lines_of(pages, &strip_title_heading(markdown, title))
    }
}

/// Agents often start content with the page title as a heading; the title is
/// already shown above the page, so drop that one heading.
/// (A fence of Markdown around all of it is taken away first: see
/// `markdown::unfenced`.)
pub(crate) fn strip_title_heading(markdown: &str, title: &str) -> String {
    let markdown = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);
    strip_title_only(markdown::unfenced(markdown), title)
}

/// The same, with a fence around all of it kept.
fn strip_title_only(markdown: &str, title: &str) -> String {
    let markdown = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);
    let trimmed = markdown.trim_start();
    if let Some(first) = trimmed.lines().next() {
        let heading = first.trim_start_matches('#');
        if first.starts_with('#') && heading.starts_with(' ') && !title.is_empty() && heading.trim().eq_ignore_ascii_case(title.trim()) {
            return trimmed[first.len()..].trim_start_matches(['\r', '\n']).to_string();
        }
    }
    markdown.to_string()
}

/// Markdown → blocks. A line that is only a link to an existing page becomes a
/// page link block, so read → edit → write round-trips keep sub-page links.
/// The address a picture or drawing has in a page's Markdown.
const BLOCK_PREFIX: &str = crate::inline::BLOCK_URL;

/// `[Image](rune://block/<id>)` → `<id>`.
fn media_token(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix('[')?;
    let (_, rest) = rest.split_once("](")?;
    let id = rest.strip_prefix(BLOCK_PREFIX).or_else(|| rest.strip_prefix(crate::inline::OLD_BLOCK_URL))?.strip_suffix(')')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(id)
}

/// A page's pictures and drawings cannot be written as Markdown. When new
/// Markdown takes the place of a page, each line that names one of them
/// (`[Image](rune://block/<id>)`) becomes that picture or drawing again,
/// and those the Markdown does not name are kept at the end of the page:
/// replacing a page never loses a picture or a drawing.
pub(crate) fn keep_media(old: &[Block], new: Vec<Block>) -> Vec<Block> {
    let is_media = |b: &Block| matches!(b.kind, BlockKind::Image | BlockKind::Drawing);
    let mut used: Vec<&str> = Vec::new();
    let mut out: Vec<Block> = new
        .into_iter()
        .map(|b| {
            let found = (b.kind == BlockKind::Text).then(|| media_token(&crate::inline::plain(&b.text)).or_else(|| media_token(&b.text)).map(str::to_string)).flatten();
            match found.and_then(|id| old.iter().find(|o| o.id == id && is_media(o))) {
                Some(original) if !used.contains(&original.id.as_str()) => {
                    used.push(original.id.as_str());
                    let mut kept = original.clone();
                    kept.indent = b.indent;
                    kept
                }
                _ => b,
            }
        })
        .collect();
    out.extend(old.iter().filter(|o| is_media(o) && !used.contains(&o.id.as_str())).cloned());
    out
}

/// The name a relation column writes for the table `block` (see
/// `Table::from_names`): its name, else the title of its page when it is
/// the first table there; `None` when neither names it well, and the
/// column is then written as `relation` alone (a rewrite keeps its table).
fn table_name(pages: &HashMap<Id, Page>, all: &HashMap<Id, Vec<Block>>, block: &str) -> Option<String> {
    let (page, lines) = all.iter().find(|(_, lines)| lines.iter().any(|b| b.id == block && b.kind == BlockKind::Table))?;
    let table = lines.iter().find(|b| b.id == block).map(|b| crate::table::Table::parse(&b.data))?;
    if !table.name.trim().is_empty() {
        return Some(table.name.trim().to_string());
    }
    let first = lines.iter().find(|b| b.kind == BlockKind::Table).is_some_and(|b| b.id == block);
    pages.get(page).filter(|p| first && !p.trashed && !p.title.trim().is_empty()).map(|p| p.title.trim().to_string())
}

/// The relation columns of the tables in `lines` (the page `page_id` as
/// it is about to be kept; `all`, the lines of every page) that hold the
/// name of a table, as Markdown says it (`relation Companies`): each gets
/// that table's block, found by its name, else by the title of its page
/// (its first table), this space first. One that names no table that is
/// there is left with none (the column then asks the person to choose),
/// and a note says so. (Markdown could not say which table a relation
/// names: a contact list linked to companies needed the person to choose
/// the table in the column's menu.)
pub(crate) fn link_relations(pages: &HashMap<Id, Page>, all: &HashMap<Id, Vec<Block>>, page_id: &str, lines: &mut [Block]) -> Vec<String> {
    link_relations_from(pages, all, page_id, lines, 0)
}

/// The same for the tables from line `first` on (those an append adds:
/// the tables there before are only what a relation can name, and a note
/// about one of them asked the AI to put right what it did not write).
///
/// A column that named a table (`all` holds the page as it was) keeps it
/// while the name written is still that table's name: a rewrite of the
/// page wrote it by its name (see `table_name`), and found by the name
/// again it could become another table of the same name, or none when the
/// table is on a page in the trash or the page of a row.
pub(crate) fn link_relations_from(pages: &HashMap<Id, Page>, all: &HashMap<Id, Vec<Block>>, page_id: &str, lines: &mut [Block], first: usize) -> Vec<String> {
    use crate::table::{Kind, Table};
    let mut notes = Vec::new();
    let here: Vec<(Id, Table)> = lines.iter().filter(|b| b.kind == BlockKind::Table).map(|b| (b.id.clone(), Table::parse(&b.data))).collect();
    if !here.iter().any(|(_, t)| (0..t.columns()).any(|c| t.kind(c) == Kind::Relation)) {
        return notes;
    }
    let was = |block: &str, heading: &str| -> Option<String> {
        let old = all.get(page_id)?.iter().find(|b| b.id == block && b.kind == BlockKind::Table).map(|b| Table::parse(&b.data))?;
        let c = old.find_heading(heading).filter(|&c| old.kind(c) == Kind::Relation)?;
        Some(old.column(c).from.trim().to_string()).filter(|f| !f.is_empty())
    };
    let is_table = |id: &str| here.iter().any(|(h, _)| h == id) || all.values().flatten().any(|b| b.id == id && b.kind == BlockKind::Table);
    let space = pages.get(page_id).map(|p| p.space_id.clone());
    // Every table there is: those of this page as it is about to be, then
    // those of the other pages, this space first and the newest first.
    let mut others: Vec<&Page> = pages.values().filter(|p| !p.trashed && p.id != page_id && p.row_of.is_none()).collect();
    others.sort_by_key(|p| (Some(&p.space_id) != space.as_ref(), std::cmp::Reverse(p.updated_at)));
    // (Each: its block, its name, the title of its page, whether it is the first table there.)
    let mut tables: Vec<(Id, String, String, bool)> = here.iter().enumerate().map(|(n, (id, t))| (id.clone(), t.name.trim().to_string(), pages.get(page_id).map(|p| p.title.trim().to_string()).unwrap_or_default(), n == 0)).collect();
    for p in others {
        for (n, b) in all.get(&p.id).into_iter().flatten().filter(|b| b.kind == BlockKind::Table).enumerate() {
            tables.push((b.id.clone(), Table::parse(&b.data).name.trim().to_string(), p.title.trim().to_string(), n == 0));
        }
    }
    for b in lines.iter_mut().skip(first).filter(|b| b.kind == BlockKind::Table) {
        let mut table = Table::parse(&b.data);
        let mut changed = false;
        let relations: Vec<usize> = (0..table.columns()).filter(|&c| table.kind(c) == Kind::Relation).collect();
        for c in relations {
            let from = table.column(c).from.trim().to_string();
            if from.is_empty() {
                notes.push(format!("The relation column “{}” of {} names no table, so the person must choose one. Write the name of the table whose rows it names after relation, as relation Companies.", table.heading(c), table.called()));
                continue;
            }
            if is_table(&from) {
                continue;
            }
            let kept = was(&b.id, &table.heading(c)).filter(|old| is_table(old) && table_name(pages, all, old).is_some_and(|n| n.to_lowercase() == from.to_lowercase()));
            if let Some(old) = kept {
                table.set_from(c, &old);
                changed = true;
                continue;
            }
            let same = |a: &str| a.to_lowercase() == from.to_lowercase();
            let found = tables.iter().find(|(id, name, ..)| same(name) && *id != b.id).or_else(|| tables.iter().find(|(id, _, title, first)| *first && same(title) && *id != b.id));
            match found {
                Some((id, ..)) => table.set_from(c, id),
                None => {
                    let named: Vec<String> = tables.iter().filter(|(id, name, ..)| !name.is_empty() && *id != b.id).take(12).map(|(_, name, ..)| format!("“{name}”")).collect();
                    notes.push(format!(
                        "No table is named “{from}”, so the relation column “{}” of {} names none. Make that table first (a table line <!-- table: {from} --> names it), then write this table again.{}",
                        table.heading(c),
                        table.called(),
                        if named.is_empty() { String::new() } else { format!(" The tables with names: {}.", named.join(", ")) }
                    ));
                    table.set_from(c, "");
                }
            }
            changed = true;
        }
        if changed {
            b.data = table.to_data();
        }
    }
    notes
}

/// After `keep_unchanged`, which keeps the old block for each line that
/// reads the same (`merged[i]` is from `said[i]`, the blocks the Markdown
/// was read as): what the Markdown says that such a block must take all
/// the same. A chart is only what its Markdown says, its JSON, and it reads
/// as its title: a chart put right (another column, another shape) with
/// the same title stayed the old chart, and the tool said that it was done.
///
/// Lines side by side: the Markdown says them (`markdown::COLUMN`), and
/// what it says is taken when it says any or the lines it was written
/// from (`old`) had any; keep_unchanged kept the columns of the old lines,
/// so lines could not be put side by side or under each other again.
pub(crate) fn as_said(old: &[Block], said: &[Block], merged: &mut [Block]) {
    for (now, said) in merged.iter_mut().zip(said) {
        if now.kind == BlockKind::Chart && said.kind == BlockKind::Chart {
            now.data = said.data.clone();
            now.text = said.text.clone();
        }
        // The table a relation column names, by its name (a table that
        // reads the same is kept as it was, with the table it named).
        // `link_relations` then finds the table of that name.
        if now.kind == BlockKind::Table && said.kind == BlockKind::Table {
            use crate::table::{Kind, Table};
            let told = Table::parse(&said.data);
            let named: Vec<(String, String)> = (0..told.columns()).filter(|&c| told.kind(c) == Kind::Relation && !told.column(c).from.trim().is_empty()).map(|c| (told.heading(c), told.column(c).from)).collect();
            if !named.is_empty() {
                let mut table = Table::parse(&now.data);
                for (heading, from) in named {
                    if let Some(c) = table.find_heading(&heading).filter(|&c| table.kind(c) == Kind::Relation) {
                        table.set_from(c, &from);
                    }
                }
                now.data = table.to_data();
            }
        }
    }
    if said.iter().any(|b| b.place.col > 0) || !crate::columns::sets(old).is_empty() {
        for (now, said) in merged.iter_mut().zip(said) {
            now.place.col = said.place.col;
        }
        crate::columns::tidy(merged);
    }
}

/// Markdown with a table that is more than a table holds is refused, with
/// the most a table holds and what to do. (Read all the same, the columns
/// over the most were cut and their cells lost, and the tool said that it
/// worked.) `nothing` says what was not done.
fn refuse_big_tables(markdown: &str, nothing: &str) -> Result<(), String> {
    let (most_columns, most_rows) = (crate::table::MOST_COLUMNS, crate::table::MOST_ROWS - 1);
    let Some(&(columns, rows)) = markdown::tables_over_most(markdown).first() else { return Ok(()) };
    let mut has = Vec::new();
    let mut to_do = Vec::new();
    if columns > most_columns {
        has.push(format!("{columns} columns"));
        to_do.push("Write it with fewer columns, or as two tables that both have the first column.");
    }
    if rows > most_rows {
        has.push(format!("{rows} rows under its header"));
        to_do.push("Write its rows as more than one table (one for each month or year, say), each with its header and its comment lines.");
    }
    Err(format!("A table holds at most {most_columns} columns and {most_rows} rows under its header, and a table of this Markdown has {}. {nothing} {}", has.join(" and "), to_do.join(" ")))
}

pub(crate) fn parse_body(pages: &HashMap<Id, Page>, markdown: &str) -> Vec<Block> {
    lines_of(pages, markdown::unfenced(markdown))
}

/// The same, with a fence around all of it read as a code block.
fn lines_of(pages: &HashMap<Id, Page>, markdown: &str) -> Vec<Block> {
    markdown::to_blocks(markdown)
        .into_iter()
        .map(|b| match page_link_target(&b.text) {
            Some(target) if b.kind == BlockKind::Text && pages.contains_key(&target) => {
                Block::page_link(target).with_indent(b.indent)
            }
            _ => b,
        })
        .collect()
}

/// `[Title](rune://page/<id>)` → `<id>`.
pub(crate) fn page_link_target(text: &str) -> Option<Id> {
    let t = text.trim();
    if !t.starts_with('[') || !t.ends_with(')') {
        return None;
    }
    let (at, len) = [LINK_PREFIX, crate::inline::OLD_PAGE_URL].iter().filter_map(|p| t.rfind(&format!("]({p}")).map(|at| (at, p.len()))).max()?;
    let id = &t[at + 2 + len..t.len() - 1];
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// Blocks → Markdown, with # / ## / ### headings (the title is reported
/// separately) and page links as `[Title](rune://page/<id>)`.
pub(crate) fn render_body(pages: &HashMap<Id, Page>, blocks: &[Block]) -> String {
    render_body_with(pages, None, blocks)
}

/// The same; with `all` (the lines of every page), a table says what its
/// rollup columns take from the pages of its rows, and what those pages
/// start with (see `state::roll_up`).
pub(crate) fn render_body_with(pages: &HashMap<Id, Page>, all: Option<&HashMap<Id, Vec<Block>>>, blocks: &[Block]) -> String {
    let as_text = |b: &Block, text: String| {
        let mut t = Block::text(text);
        t.indent = b.indent;
        t.place.col = b.place.col;
        t
    };
    let converted: Vec<Block> = blocks
        .iter()
        .map(|b| match b.kind {
            // (A line break in a heading: the lines after it with room
            // before them, read back as more of the heading, see `markdown::read`.)
            BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3 => {
                let marks = "#".repeat(crate::model::heading_level(b.kind).unwrap_or(1) as usize);
                as_text(b, format!("{marks} {}", b.text.replace('\n', &format!("\n{}  ", "  ".repeat(b.indent as usize)))))
            }
            BlockKind::PageLink => {
                let target = b.target.clone().unwrap_or_default();
                let title = match pages.get(&target) {
                    Some(p) if p.trashed => format!("{} (in Trash)", p.display_title()),
                    Some(p) => p.display_title().to_string(),
                    None => "Missing page".to_string(),
                };
                let title = title.replace('[', "\\[").replace(']', "\\]");
                as_text(b, format!("[{title}]({LINK_PREFIX}{target})"))
            }
            // With the block's id, so that replace_page_content can put the
            // same picture or drawing back (see `keep_media`).
            BlockKind::Drawing => as_text(b, format!("[Drawing]({BLOCK_PREFIX}{})", b.id)),
            BlockKind::Image => as_text(b, format!("[Image]({BLOCK_PREFIX}{})", b.id)),
            BlockKind::Table => match all {
                Some(all) => {
                    let mut table = crate::table::Table::parse(&b.data);
                    table.beside = b.place.beside;
                    crate::state::roll_up(pages, all, &b.id, &mut table);
                    let relations: Vec<usize> = (0..table.columns()).filter(|&c| table.kind(c) == crate::table::Kind::Relation).collect();
                    for c in relations {
                        let from = table.column(c).from;
                        if let Some(name) = table_name(pages, all, &from) {
                            table.from_names.insert(from, name);
                        }
                    }
                    as_text(b, table.to_markdown(""))
                }
                None => as_text(b, crate::markdown::table_markdown(b, "")),
            },
            BlockKind::Chart => as_text(b, crate::chart::Chart::parse(&b.data).to_markdown()),
            _ => b.clone(),
        })
        .collect();
    let md = markdown::from_blocks("", &converted, |_| String::new());
    if md.trim().is_empty() {
        "(This page is empty.)".to_string()
    } else {
        md
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// About 140 characters of `text` around the byte offset `at` in its lowercase form.
fn snippet_around(text: &str, lower: &str, at: usize) -> String {
    let char_at = lower[..at.min(lower.len())].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let start = char_at.saturating_sub(40);
    let end = (char_at + 100).min(chars.len());
    let mut out: String = chars[start.min(chars.len())..end].iter().collect::<String>().replace('\n', " ");
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Milliseconds since the Unix epoch → `2026-11-09T07:52:00Z`.
fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// What to give an AI tool so it can use this workspace: the connector is
/// this program started with `--mcp` (see Settings ▸ AI ▸ Connect AI tools).
pub mod connect {
    /// The name the tools list the connector under.
    pub const NAME: &str = "rune";

    /// Where this program is (the app's own executable).
    pub fn program() -> String {
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "rune".into())
    }

    fn shell_quoted(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    fn json_quoted(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
    }

    /// The command as typed in Terminal.
    pub fn command(program: &str) -> String {
        format!("{} --mcp", shell_quoted(program))
    }

    /// For Claude Code: run once in Terminal.
    pub fn claude_code(program: &str) -> String {
        format!("claude mcp add --scope user {NAME} -- {} --mcp", shell_quoted(program))
    }

    /// For Codex: lines for ~/.codex/config.toml.
    pub fn codex(program: &str) -> String {
        format!("[mcp_servers.{NAME}]\ncommand = {}\nargs = [\"--mcp\"]\n", json_quoted(program))
    }

    /// For the Claude app, Cursor, VS Code and most others: the JSON their
    /// MCP settings file takes.
    pub fn json(program: &str) -> String {
        format!("{{\n  \"mcpServers\": {{\n    \"{NAME}\": {{\n      \"command\": {},\n      \"args\": [\"--mcp\"]\n    }}\n  }}\n}}\n", json_quoted(program))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new(Store::open_in_memory().unwrap())
    }

    fn request(server: &mut Server, id: i64, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        let reply = server.handle_line(&line).expect("a reply");
        let v: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], id);
        v
    }

    /// Call a tool and return (text, is_error).
    fn call(server: &mut Server, name: &str, args: Value) -> (String, bool) {
        let v = request(server, 99, "tools/call", json!({ "name": name, "arguments": args }));
        let result = &v["result"];
        (result["content"][0]["text"].as_str().unwrap().to_string(), result["isError"].as_bool().unwrap())
    }

    fn json_of(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_locked_page_is_refused_before_anything_of_it_is_changed() {
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Diary", "markdown": "The code is 4711" }));
        let diary = json_of(&made)["id"].as_str().unwrap().to_string();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Plan", "markdown": "Plain" }));
        let plan = json_of(&made)["id"].as_str().unwrap().to_string();
        // Locked in the app (the connector is another program: it holds no key).
        let lines = s.store.load_page_blocks(&diary).unwrap();
        s.store.save_blocks(&diary, &[crate::lock::seal_with(&crate::lock::test_key("pw-pw-pw"), &diary, &lines)]).unwrap();
        let before = s.store.load().unwrap();
        let refused = |out: (String, bool)| assert!(out.1 && out.0.contains("The page is locked") && out.0.contains("Nothing was changed"), "{}", out.0);
        refused(call(&mut s, "replace_page_content", json!({ "page_id": diary, "title": "New name", "markdown": "New lines" })));
        refused(call(&mut s, "update_page", json!({ "page_id": diary, "title": "New name" })));
        refused(call(&mut s, "update_page", json!({ "page_id": diary, "parent_id": plan })));
        refused(call(&mut s, "update_page", json!({ "page_id": plan, "parent_id": diary })));
        refused(call(&mut s, "trash_page", json!({ "page_id": diary })));
        refused(call(&mut s, "create_page", json!({ "title": "Inside", "parent_id": diary })));
        assert!(call(&mut s, "append_to_page", json!({ "page_id": diary, "markdown": "More" })).1);
        assert!(s.store.load().unwrap() == before, "no name, place, page or line changed");
        // It is read as a locked page, with none of its words.
        let (read, err) = call(&mut s, "read_page", json!({ "page_id": diary }));
        assert!(!err && read.contains("Locked page") && !read.contains("4711"), "{read}");
    }

    #[test]
    fn a_table_over_what_a_table_holds_is_refused_with_the_most_it_holds() {
        let mut s = server();
        let wide = |n: usize| {
            let line = |word: &str| format!("| {} |", (1..=n).map(|i| format!("{word}{i}")).collect::<Vec<_>>().join(" | "));
            format!("{}\n|{}\n{}", line("C"), " --- |".repeat(n), line("v"))
        };
        let long = |n: usize| {
            let rows: Vec<String> = (1..=n).map(|i| format!("| r{i} | {i} |")).collect();
            format!("| Name | Amount |\n| --- | --- |\n{}\n<!-- columns: text count | number sum -->\n<!-- table: Long | sort Amount down -->", rows.join("\n"))
        };
        // Too wide: no page is made, and the answer says the most and what to do.
        let (said, err) = call(&mut s, "create_page", json!({ "title": "Wide", "markdown": wide(14) }));
        assert!(err && said.contains("at most 12 columns") && said.contains("has 14 columns") && said.contains("No page was made"), "{said}");
        let (list, _) = call(&mut s, "list_pages", json!({}));
        assert!(!list.contains("Wide"), "{list}");
        // Too long: the same, for the rows.
        let (said, err) = call(&mut s, "create_page", json!({ "title": "Long", "markdown": long(260) }));
        assert!(err && said.contains("199 rows under its header") && said.contains("has 260 rows") && said.contains("more than one table"), "{said}");
        // What a table holds is taken whole: 12 columns, 199 rows, with their kinds and name.
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Fits", "markdown": format!("{}\n\n{}", wide(12), long(199)) }));
        assert!(!err, "{made}");
        let fits = json_of(&made)["id"].as_str().unwrap().to_string();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": fits }));
        assert!(page.contains("v12") && page.contains("| r199 | 199 |") && page.contains("<!-- table: Long | sort Amount down -->"), "{page}");
        // The other tools that take Markdown refuse too, and leave the page as it is.
        let (said, err) = call(&mut s, "append_to_page", json!({ "page_id": fits, "markdown": wide(13) }));
        assert!(err && said.contains("Nothing was added"), "{said}");
        let (said, err) = call(&mut s, "replace_page_content", json!({ "page_id": fits, "markdown": long(200) }));
        assert!(err && said.contains("The page was not changed") && said.contains("has 200 rows"), "{said}");
        let (after, _) = call(&mut s, "read_page", json!({ "page_id": fits }));
        assert_eq!(after, page);
        // Bars in a code block are no table.
        let code = format!("```\n{}\n```", wide(14));
        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": fits, "markdown": code }));
        assert!(!err);
    }

    #[test]
    fn a_template_page_made_by_hand_stays_through_the_ai_writing_its_page_again() {
        for rollup in [false, true] {
            let mut s = server();
            let kind = if rollup { "rollup Price sum USD sum" } else { "number USD sum" };
            let md = format!("Intro with a tpyo.\n\n| Trade | Cost |\n| --- | --- |\n| AAPL |  |\n<!-- columns: text | {kind} -->");
            let (made, _) = call(&mut s, "create_page", json!({ "title": "Trades", "markdown": md }));
            let id = json_of(&made)["id"].as_str().unwrap().to_string();
            // A template's page made by hand: a column with no heading, a
            // choice whose options came from typing, four places, rows, a
            // name, a sort, and words under the table.
            let (_, blocks) = s.load().unwrap();
            let mut lines = blocks[&id].clone();
            let at = lines.iter().position(|b| b.kind == BlockKind::Table).unwrap();
            let mut table = crate::table::Table::parse(&lines[at].data);
            table.name_rows();
            table.template = Some(Default::default());
            lines[at].data = table.to_data();
            s.save_blocks(&id, &lines).unwrap();
            let mut template = Page::new(Some(id.clone()), 0.0);
            template.row_of = Some(format!("{}/template", lines[at].id));
            template.title = "Template".into();
            let mut own = crate::table::Table::of(vec![vec!["Step".into(), "Price".into(), "When".into(), String::new()], vec!["Entry".into(), "185.25".into(), String::new(), "a".into()], vec!["Stop".into(), "180".into(), String::new(), String::new()]]);
            own.set_kind(1, crate::table::Kind::Number);
            own.set_format(1, "USD");
            own.set_places(1, Some(4));
            own.set_kind(2, crate::table::Kind::Choice);
            for (r, name) in [(1, "9:30"), (2, "Close; late")] {
                let made = own.add_option(2, name).unwrap();
                own.toggle_name(r, 2, &made);
            }
            own.name = "Plan".into();
            let mut tb = Block::new(BlockKind::Table, own.plain_text());
            tb.data = own.to_data();
            s.save_page(&template).unwrap();
            s.save_blocks(&template.id, &[tb, Block::text("Check the news first")]).unwrap();
            let (_, kept) = s.load().unwrap();
            // Ten times "fix the typo in the intro": the page as read, the
            // typo put right; every other time with words after `page:` of its own.
            for round in 0..10 {
                let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
                assert!(page.contains(&format!("row_page_template: {}", template.id)), "{page}");
                assert_eq!(page.contains("page: "), rollup, "{page}");
                let mut body = page.split("---\n\n").nth(1).unwrap().replace("tpyo", "typo");
                if round % 2 == 1 {
                    body = match body.find("<!-- table:") {
                        Some(at) => {
                            let end = at + body[at..].find("-->").unwrap();
                            format!("{} | Page: Item | Other text -->{}", body[..end].split(" | page:").next().unwrap().trim_end(), &body[end + 3..])
                        }
                        None => body.replace(&format!("{kind} -->"), &format!("{kind} -->\n<!-- table: | page: Item | Other text -->")),
                    };
                }
                let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body }));
                assert!(!err);
            }
            let (_, after) = s.load().unwrap();
            assert_eq!(after[&template.id], kept[&template.id], "the template's page is as it was (rollup: {rollup})");
            assert!(s.store.latest_version(&template.id).unwrap().is_none(), "and no copy of it in its history");
            let table_now = after[&id].iter().find(|b| b.kind == BlockKind::Table).unwrap();
            assert!(!table_now.data.contains("\"page\""));
            assert_eq!((table_now.id.clone(), crate::table::Table::parse(&table_now.data)), (kept[&id][at].id.clone(), crate::table::Table::parse(&kept[&id][at].data)), "the table is the table it was");
        }
    }

    #[test]
    fn page_makes_a_template_in_any_case_and_is_never_kept_in_a_table() {
        let mut s = server();
        let table = "| Day | Spent |\n| --- | --- |\n| Mon |  |\n<!-- columns: text | rollup Amount sum USD sum -->";
        // Written "Page:", by create_page, then by append_to_page.
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Week", "markdown": format!("{table}\n<!-- table: Week | Page: Item | Amount USD sum -->") }));
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": id, "markdown": format!("\n{}\n<!-- table: Next | page: Thing | Amount USD sum -->", table.replace("Day", "When")) }));
        assert!(!err);
        let (pages, blocks) = s.load().unwrap();
        let tables: Vec<&Block> = blocks[&id].iter().filter(|b| b.kind == BlockKind::Table).collect();
        assert_eq!(tables.len(), 2);
        for (b, first) in tables.iter().zip(["Item", "Thing"]) {
            assert!(!b.data.contains("\"page\""), "{}", b.data);
            assert!(crate::table::Table::parse(&b.data).template.is_some());
            let of = crate::state::template_page_id(&pages, &b.id).expect("a template's page");
            assert_eq!(crate::table::Table::parse(&blocks[&of][0].data).heading(0), first);
        }
        // Nothing of a `page:` part in any stored block of the notes.
        assert!(blocks.values().flatten().all(|b| !b.data.contains("\"page\"")));
    }

    #[test]
    fn a_budget_with_a_page_for_each_day_is_made_in_one_go() {
        let mut s = server();
        let md = "| Date | Budget | Spent | Left |\n| --- | --- | --- | --- |\n| 2026-11-01 | $60.00 |  |  |\n| 2026-11-02 | $60.00 |  |  |\n\
                  <!-- columns: date | number USD sum | rollup Amount sum USD sum | formula USD sum = Budget - Spent -->\n\
                  <!-- table: November | color Left below 0 red | page: Item | Amount USD sum | Category choice {Food; Fun; Bills} -->";
        let (made, err) = call(&mut s, "create_page", json!({ "title": "November budget", "markdown": md }));
        assert!(!err, "{made}");
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        // The page of each row starts with that table: it is the table's template now.
        let (pages, blocks) = s.load().unwrap();
        let table_block = blocks[&id].iter().find(|b| b.kind == BlockKind::Table).unwrap();
        assert!(!table_block.data.contains("\"page\":"), "kept in the template, not in the table");
        let table = crate::table::Table::parse(&table_block.data);
        assert!(table.template.is_some());
        assert_eq!(table.kind(2), crate::table::Kind::Rollup);
        let template = pages.values().find(|p| p.row_of.as_deref() == Some(format!("{}/template", table_block.id).as_str())).expect("a template page");
        let start = crate::table::Table::parse(&blocks[&template.id][0].data);
        assert_eq!((0..start.columns()).map(|c| start.heading(c)).collect::<Vec<_>>(), vec!["Item", "Amount", "Category"]);
        // Read back, the page says so, and the rollup is there.
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        assert!(page.contains("rollup Amount sum USD sum"), "{page}");
        assert!(page.contains("| page: Item text | Amount number USD sum | Category choice {Food:"), "{page}");
        // Written again as it was read: nothing changes, and nothing is lost.
        let body = page.split("---\n\n").nth(1).unwrap().to_string();
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body }));
        assert!(!err);
        let (pages_after, blocks_after) = s.load().unwrap();
        assert_eq!(pages_after.values().filter(|p| p.row_of.is_some()).count(), 1);
        assert_eq!(blocks_after[&template.id], blocks[&template.id]);
        // A kind changed and nothing else: the change is taken (the cells are the same).
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body.replace("rollup Amount sum USD sum", "rollup Amount highest USD") }));
        assert!(!err);
        let (_, blocks_after) = s.load().unwrap();
        let now = crate::table::Table::parse(&blocks_after[&id].iter().find(|b| b.kind == BlockKind::Table).unwrap().data);
        assert_eq!(now.column(2).way, crate::table::Total::Highest);
        assert!(now.template.is_some(), "what Markdown cannot say stays");
    }

    #[test]
    fn rows_of_a_log_and_to_dos_change_without_the_page_written_again() {
        let mut s = server();
        let md = "Runs of the week.\n\n| Date | km | Time | Pace |\n| --- | --- | --- | --- |\n| 2026-11-10 | 3 | 19:30 |  |\n<!-- columns: date | number km sum | time sum | pace average -->\n\n- [ ] Easy run, 4.5 km\n- [ ] Long run, 6 km";
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Run log", "markdown": md }));
        let log = json_of(&made)["id"].as_str().unwrap().to_string();
        // A row: today in the date column, a time in the table's way, the pace worked out.
        let (added, err) = call(&mut s, "add_table_row", json!({ "page_id": log, "values": { "km": 6, "time": "34 minutes", "Feel": "Good" } }));
        assert!(!err, "{added}");
        let added = json_of(&added);
        assert_eq!(added["row"], 2);
        assert_eq!(added["cells"]["km"], "6");
        assert_eq!(added["cells"]["Time"], "34:00");
        assert_eq!(added["cells"]["Pace"], "5:40");
        assert_eq!(added["cells"]["Date"], crate::table::full_day(crate::when::today()));
        assert_eq!(added["columns_not_found"], json!(["Feel"]));
        // A row found by its day, then by its number.
        let (changed, err) = call(&mut s, "update_table_row", json!({ "page_id": log, "match": { "Date": "2026-11-10" }, "values": { "km": "3.5" } }));
        assert!(!err, "{changed}");
        assert_eq!(json_of(&changed)["cells"]["Pace"], "5:34");
        let (_, err) = call(&mut s, "update_table_row", json!({ "page_id": log, "row": 9, "values": { "km": "1" } }));
        assert!(err);
        let (wrong, err) = call(&mut s, "add_table_row", json!({ "page_id": log, "values": { "Weight": "70" } }));
        assert!(err && wrong.contains("Its columns: Date, km, Time, Pace"), "{wrong}");
        // A to-do ticked and given a day.
        let (todo, err) = call(&mut s, "set_todo", json!({ "page_id": log, "text": "easy run", "done": true, "due": "2026-12-01" }));
        assert!(!err, "{todo}");
        assert_eq!(json_of(&todo)["todo"], "Easy run, 4.5 km");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": log }));
        assert!(page.contains("- [x] Easy run, 4.5 km 📅 2026-12-01"), "{page}");
        // A reminder at a time of its day; then taken away.
        let (todo, err) = call(&mut s, "set_todo", json!({ "page_id": log, "text": "easy run", "remind": "07:15" }));
        assert!(!err && json_of(&todo)["remind"] == "07:15", "{todo}");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": log }));
        assert!(page.contains("- [x] Easy run, 4.5 km 📅 2026-12-01 ⏰ 07:15"), "{page}");
        let (wrong, err) = call(&mut s, "set_todo", json!({ "page_id": log, "text": "easy run", "remind": "soon" }));
        assert!(err && wrong.contains("remind must be a time"), "{wrong}");
        let (todo, _) = call(&mut s, "set_todo", json!({ "page_id": log, "text": "easy run", "remind": "" }));
        assert!(json_of(&todo)["remind"].is_null(), "{todo}");
        // (A day is written the way its column writes, as if typed there.)
        let day = crate::table::full_day(chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap());
        assert!(page.contains(&format!("| {day} | 3.5 | 19:30 | 5:34 |")), "{page}");
        assert!(page.contains("<!-- columns: date | number km sum | time sum | pace average -->"), "{page}");
        // Money: a row is written the way its column writes, and the name of the table stays.
        let md = "| Item | Amount | Cost |\n| --- | --- | --- |\n| Rent | $2,000.00 |  |\n<!-- columns: text | number USD sum | formula USD = Amount * 2 -->\n<!-- table: Expenses -->";
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Budget", "markdown": md }));
        let budget = json_of(&made)["id"].as_str().unwrap().to_string();
        let (added, err) = call(&mut s, "add_table_row", json!({ "page_id": budget, "values": { "Item": "Dinner", "Amount": 65, "Cost": "9" } }));
        assert!(!err, "{added}");
        assert_eq!(json_of(&added)["cells"], json!({ "Item": "Dinner", "Amount": "$65.00", "Cost": "$130.00" }));
        let (changed, err) = call(&mut s, "update_table_row", json!({ "page_id": budget, "match": { "Item": "rent" }, "values": { "Amount": "€2100.5" } }));
        assert!(!err, "{changed}");
        assert_eq!(json_of(&changed)["cells"]["Amount"], "$2,100.50");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": budget }));
        assert!(page.contains("| Dinner | $65.00 | $130.00 |") && page.contains("<!-- columns: text | number USD sum | formula USD = Amount * 2 -->") && page.contains("<!-- table: Expenses -->"), "{page}");
        // The page written again: the table is the same block still, with what Markdown
        // cannot say (a sort, what its rows go by), and the page of a row keeps its row and takes its name.
        let (_, mut blocks) = s.store.load().unwrap();
        let lines = blocks.get_mut(&budget).unwrap();
        let at = lines.iter().position(|b| b.kind == BlockKind::Table).unwrap();
        let block_id = lines[at].id.clone();
        let mut table = crate::table::Table::parse(&lines[at].data);
        table.change_view(|v| v.sort_by(1, true));
        table.name_rows();
        let rent = table.row_id(1).unwrap().to_string();
        lines[at].data = table.to_data();
        s.store.save_blocks(&budget, lines).unwrap();
        let mut row_page = Page::new(Some(budget.clone()), 0.0);
        row_page.title = "Rent".into();
        row_page.row_of = Some(format!("{block_id}/{rent}"));
        s.store.save_page(&row_page).unwrap();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": budget }));
        let again = page.replace("| Rent |", "| Rent (flat) |").replace("| Dinner | $65.00 |", "| Dinner | $70.00 |");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": budget, "markdown": again }));
        assert!(!err);
        let (pages, blocks) = s.store.load().unwrap();
        let now = blocks[&budget].iter().find(|b| b.kind == BlockKind::Table).unwrap();
        let table_now = crate::table::Table::parse(&now.data);
        assert_eq!(now.id, block_id);
        assert_eq!((table_now.view.clone(), table_now.row_with_id(&rent), table_now.cell(2, 1)), (table.view.clone(), Some(1), "$70.00"));
        assert_eq!(pages[&row_page.id].title, "Rent (flat)");
        // A row changed by its tool: its page takes the name. A page of a row with another name: its row takes it.
        let (_, err) = call(&mut s, "update_table_row", json!({ "page_id": budget, "row": 1, "values": { "Item": "Rent, flat 2" } }));
        assert!(!err);
        assert_eq!(s.store.load().unwrap().0[&row_page.id].title, "Rent, flat 2");
        let (_, err) = call(&mut s, "update_page", json!({ "page_id": row_page.id, "title": "Rent" }));
        assert!(!err);
        let (_, blocks) = s.store.load().unwrap();
        assert_eq!(crate::table::Table::parse(&blocks[&budget].iter().find(|b| b.kind == BlockKind::Table).unwrap().data).cell(1, 0), "Rent");
        // A page with no table says so.
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Empty" }));
        let empty = json_of(&made)["id"].as_str().unwrap().to_string();
        let (no, err) = call(&mut s, "add_table_row", json!({ "page_id": empty, "values": { "a": "b" } }));
        assert!(err && no.contains("no table"), "{no}");
        // A chart written as a chart code block is a chart, and read back the same way.
        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": log, "markdown": "```chart\n{\"shape\": \"line\", \"y\": \"Pace\", \"by\": \"row\", \"add\": \"average\"}\n```" }));
        assert!(!err);
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": log }));
        assert!(page.contains("```chart\n{\"shape\":\"line\",\"by\":\"row\",\"y\":\"Pace\",\"add\":\"average\"}\n```"), "{page}");
    }

    #[test]
    fn what_an_ai_run_changes_is_written_down_and_can_be_taken_back() {
        let dir = std::env::temp_dir().join(format!("rune-journal-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("run.jsonl");
        // Before the run: one page with a line, and one event.
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Plan", "markdown": "Old line" }));
        let plan = json_of(&made)["id"].as_str().unwrap().to_string();
        let (made, _) = call(&mut s, "create_event", json!({ "title": "Run", "start": "2026-11-12T07:00" }));
        let run = json_of(&made)["created"]["id"].as_str().unwrap().to_string();
        assert!(read_journal(&journal).is_empty(), "nothing is written down with no file named");
        // The run: a new page, a change of the old page, a new name, a new event, a removed event. A read writes nothing.
        let mut s = Server::new(s.store).with_journal(journal.clone());
        call(&mut s, "read_page", json!({ "page_id": plan }));
        assert!(read_journal(&journal).is_empty());
        let (made, failed) = call(&mut s, "create_page", json!({ "title": "Workout A", "parent_id": plan, "markdown": "Ten squats" }));
        assert!(!failed, "{made}");
        let child = json_of(&made)["id"].as_str().unwrap().to_string();
        call(&mut s, "replace_page_content", json!({ "page_id": plan, "markdown": "New line" }));
        call(&mut s, "update_page", json!({ "page_id": plan, "title": "Workout Plan" }));
        call(&mut s, "create_event", json!({ "title": "Stretch", "start": "2026-11-13T08:00" }));
        call(&mut s, "delete_event", json!({ "event_id": run }));
        let changes = read_journal(&journal);
        let said = describe(&changes);
        assert_eq!(
            said,
            vec![
                "Made the page “Workout A”",
                "Changed the page “Plan”",
                "Named “Plan” “Workout Plan”",
                "Added “Stretch” to the calendar",
                "Removed “Run” from the calendar",
            ],
            "{changes:?}"
        );
        assert_eq!(count_line(&changes), "1 page made · 2 pages changed · 1 event added · 1 event changed");
        // Many things of one kind are one line.
        let many: Vec<Change> = (1..=8).map(|i| Change::PageMade { id: format!("w{i}"), title: format!("Week {i}") }).collect();
        assert_eq!(describe(&many), vec!["Made 8 pages, “Week 1” to “Week 8”"]);
        assert_eq!(count_line(&many), "8 pages made");
        let runs: Vec<Change> = (1..=5).map(|i| Change::EventMade { id: format!("e{i}"), title: format!("Run {i}") }).chain(many.iter().take(2).cloned()).collect();
        assert_eq!(describe(&runs), vec!["Added 5 events to the calendar", "Made the page “Week 1”", "Made the page “Week 2”"]);
        // The person writes a line on the page after the run: it is no part
        // of the run, and stays when the run is taken back.
        let (_, mut now) = s.store.load().unwrap();
        now.get_mut(&plan).unwrap().push(Block::text("Written since"));
        s.store.save_blocks(&plan, &now[&plan]).unwrap();
        // Taken back: the old page as it was, the new page in the Trash, the events as before.
        undo_changes(&s.store, &changes).unwrap();
        let (pages, blocks) = s.store.load().unwrap();
        assert_eq!(pages[&plan].title, "Plan");
        assert_eq!(blocks[&plan].iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["Old line", "Written since"]);
        assert!(pages[&child].trashed, "a page that the run made goes to the Trash");
        let events: Vec<String> = s.store.load_events().unwrap().into_iter().map(|e| e.title).collect();
        assert_eq!(events, vec!["Run"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_name_taken_back_leaves_what_the_person_changed_since() {
        let dir = std::env::temp_dir().join(format!("rune-page-back-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("run.jsonl");
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Plan" }));
        let plan = json_of(&made)["id"].as_str().unwrap().to_string();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Archive" }));
        let archive = json_of(&made)["id"].as_str().unwrap().to_string();
        // The run gives the page a new name and moves it into another page.
        let mut s = Server::new(s.store).with_journal(journal.clone());
        call(&mut s, "update_page", json!({ "page_id": plan, "title": "Workout Plan" }));
        let (moved, failed) = call(&mut s, "update_page", json!({ "page_id": plan, "parent_id": archive }));
        assert!(!failed, "{moved}");
        let changes = read_journal(&journal);
        // After the run the person makes it a favourite and gives it an icon.
        let (pages, _) = s.store.load().unwrap();
        assert_eq!((pages[&plan].title.as_str(), pages[&plan].parent_id.as_deref()), ("Workout Plan", Some(archive.as_str())));
        s.store.save_page(&Page { favorite: true, icon: Some("star:yellow".into()), ..pages[&plan].clone() }).unwrap();
        // Undo: the old name and the old place; the favourite and the icon stay.
        undo_changes(&s.store, &changes).unwrap();
        let (pages, _) = s.store.load().unwrap();
        let page = &pages[&plan];
        assert_eq!((page.title.as_str(), page.parent_id.as_deref()), ("Plan", None));
        assert!(page.favorite && page.icon.as_deref() == Some("star:yellow"), "{page:?}");
        // A record of a run from before this (no `left`) is read, and puts the whole page back as it did.
        let old: Change = serde_json::from_str(&serde_json::to_string(&json!({ "kind": "page_was", "page": page, "title": "x", "what": "renamed" })).unwrap()).unwrap();
        assert!(matches!(&old, Change::PageWas { left: None, .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_page_before_a_run_is_known_and_a_line_typed_meanwhile_is_seen() {
        let dir = std::env::temp_dir().join(format!("rune-before-run-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("run.jsonl");
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Plan", "markdown": "Old line" }));
        let plan = json_of(&made)["id"].as_str().unwrap().to_string();
        let (pages_then, blocks_then) = s.store.load().unwrap();
        let sent = render_body(&pages_then, &blocks_then[&plan]);
        // The run: a sub-page (its link is put on the page), a line added to
        // the page, the sub-page named again, and an event.
        let mut s = Server::new(s.store).with_journal(journal.clone());
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Week 1", "parent_id": plan }));
        let child = json_of(&made)["id"].as_str().unwrap().to_string();
        call(&mut s, "append_to_page", json!({ "page_id": plan, "markdown": "Added by the tool" }));
        call(&mut s, "update_page", json!({ "page_id": child, "title": "First week" }));
        call(&mut s, "create_event", json!({ "title": "Run", "start": "2026-11-12T07:00" }));
        let changes = read_journal(&journal);
        // Nothing but the tools wrote on the page: it is the page that was sent.
        let (mut pages, blocks) = s.store.load().unwrap();
        assert_ne!(render_body(&pages, &blocks[&plan]), sent);
        let was = before_run(&changes, &plan, &mut pages, blocks[&plan].clone()).expect("only the tools changed the page");
        assert_eq!(render_body(&pages, &was), sent);
        // A run that changed other things only: the page is as it is.
        let only_event: Vec<Change> = changes.iter().filter(|c| matches!(c, Change::EventMade { .. })).cloned().collect();
        let (mut pages, _) = s.store.load().unwrap();
        assert_eq!(before_run(&only_event, &plan, &mut pages, blocks_then[&plan].clone()), Some(blocks_then[&plan].clone()));
        // The person types a line after the tools' last change: it is seen.
        let (mut pages, mut blocks) = s.store.load().unwrap();
        blocks.get_mut(&plan).unwrap().push(Block::text("Typed meanwhile"));
        assert_eq!(before_run(&changes, &plan, &mut pages, blocks[&plan].clone()), None);
        // …and a line typed while the run changed only other things is in the page that is compared.
        let mut typed = blocks_then[&plan].clone();
        typed.push(Block::text("Typed meanwhile"));
        let (mut pages, _) = s.store.load().unwrap();
        let was = before_run(&only_event, &plan, &mut pages, typed).unwrap();
        assert_ne!(render_body(&pages, &was), sent);
        // A tool that left the page with no line at all: the empty line the
        // app shows there is that page; a line typed since is seen.
        let emptied = vec![Change::BlocksWere { id: plan.clone(), title: "Plan".into(), blocks: blocks_then[&plan].clone(), left: Vec::new() }];
        assert_eq!(before_run(&emptied, &plan, &mut pages, vec![Block::text("")]), Some(blocks_then[&plan].clone()));
        assert_eq!(before_run(&emptied, &plan, &mut pages, vec![Block::text("Typed meanwhile")]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_connector_reads_but_does_not_change_newer_notes() {
        let mut s = server();
        let (made, failed) = call(&mut s, "create_page", json!({ "title": "Before" }));
        assert!(!failed, "{made}");
        s.store.set_file_schema(crate::store::SCHEMA + 1);
        let (text, failed) = call(&mut s, "create_page", json!({ "title": "After" }));
        assert!(failed && text.contains("older than the Rune app"), "{text}");
        let (text, failed) = call(&mut s, "list_pages", json!({}));
        assert!(!failed && text.contains("Before") && !text.contains("After"), "{text}");
    }

    #[test]
    fn initialize_negotiates_version_and_lists_tools() {
        let mut s = server();
        let v = request(&mut s, 1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }));
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(v["result"]["serverInfo"]["name"], "rune");
        let icons = v["result"]["serverInfo"]["icons"].as_array().cloned().unwrap_or_default();
        assert!(icons.iter().any(|i| i["mimeType"] == "image/png" && i["src"].as_str().is_some_and(|s| s.starts_with("data:image/png;base64,iVBOR"))), "{icons:?}");
        assert!(icons.iter().any(|i| i["mimeType"] == "image/svg+xml"));
        assert!(v["result"]["capabilities"]["tools"].is_object());
        // Unknown versions get our newest.
        let v = request(&mut s, 2, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(v["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);

        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert_eq!(request(&mut s, 3, "ping", Value::Null)["result"], json!({}));

        let tools = request(&mut s, 4, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in ["search_pages", "list_pages", "read_page", "create_page", "append_to_page", "replace_page_content", "update_page", "trash_page", "restore_page"] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        for t in tools["result"]["tools"].as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn protocol_errors() {
        let mut s = server();
        let v: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
        assert_eq!(v["error"]["code"], -32700);
        assert_eq!(v["id"], Value::Null);
        assert_eq!(request(&mut s, 5, "resources/list", json!({}))["error"]["code"], -32601);
        assert_eq!(request(&mut s, 6, "tools/call", json!({ "name": "nope" }))["error"]["code"], -32602);
        // Unknown notifications and stray responses get no reply.
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"whatever"}"#).is_none());
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#).is_none());
        // Bad arguments are tool errors the model can read and fix.
        let (text, err) = call(&mut s, "read_page", json!({}));
        assert!(err && text.contains("page_id"));
        let (text, err) = call(&mut s, "read_page", json!({ "page_id": "missing" }));
        assert!(err && text.contains("No page"));
    }

    #[test]
    fn a_page_moved_into_a_page_of_another_space_goes_there_with_what_is_under_it() {
        let mut s = server();
        let home = Space::new("Home", "blue", 0.0);
        let side = Space::new("Side project", "green", 1.0);
        s.store.save_space(&home).unwrap();
        s.store.save_space(&side).unwrap();
        s.store.set_setting("space", &home.id).unwrap();
        let made = |s: &mut Server, args: Value| json_of(&call(s, "create_page", args).0)["id"].as_str().unwrap().to_string();
        let trip = made(&mut s, json!({ "title": "Trip" }));
        let list = made(&mut s, json!({ "title": "Packing", "parent_id": trip }));
        s.store.set_setting("space", &side.id).unwrap();
        let shelf = made(&mut s, json!({ "title": "Shelf" }));
        let (text, err) = call(&mut s, "update_page", json!({ "page_id": trip, "parent_id": shelf, "title": "Trip to Rome" }));
        assert!(!err, "{text}");
        let (pages, _) = s.store.load().unwrap();
        let p = &pages[&trip];
        assert_eq!((p.parent_id.as_deref(), p.title.as_str(), p.space_id.as_str()), (Some(shelf.as_str()), "Trip to Rome", side.id.as_str()), "the page itself is moved and named");
        assert_eq!(pages[&list].space_id, side.id, "and what is under it goes with it");
        assert_eq!(pages[&list].parent_id.as_deref(), Some(trip.as_str()));
    }

    #[test]
    fn a_new_page_goes_to_the_space_that_is_named() {
        let mut s = server();
        let work = Space::new("Work", "blue", 0.0);
        let own = Space::new("Personal projects", "green", 1.0);
        s.store.save_space(&work).unwrap();
        s.store.save_space(&own).unwrap();
        // Rune shows "Work"; the person asks for a page in the other space.
        s.store.set_setting("space", &work.id).unwrap();
        let (text, err) = call(&mut s, "create_page", json!({ "title": "Projects", "space": "personal" }));
        assert!(!err, "{text}");
        let made = json_of(&text);
        assert_eq!(made["space"], "Personal projects");
        let folder = made["id"].as_str().unwrap().to_string();
        let (pages, _) = s.store.load().unwrap();
        assert_eq!(pages[&folder].space_id, own.id);
        // With no space named: the space that is open, and the answer says which.
        let (text, _) = call(&mut s, "create_page", json!({ "title": "Inbox" }));
        assert_eq!(json_of(&text)["space"], "Work");
        // A part from the middle of a name is not that space ("son" in "Personal").
        let (text, err) = call(&mut s, "create_page", json!({ "title": "Lost", "space": "son" }));
        assert!(err, "{text}");
        // A page named with the space it is in stays under its parent.
        let (text, err) = call(&mut s, "create_page", json!({ "title": "Inside", "parent_id": folder }));
        assert!(!err, "{text}");
        let inside = json_of(&text)["id"].as_str().unwrap().to_string();
        let (text, err) = call(&mut s, "update_page", json!({ "page_id": inside, "title": "Inside 2", "space": "Personal projects" }));
        assert!(!err, "{text}");
        assert_eq!(s.store.load().unwrap().0[&inside].parent_id.as_deref(), Some(folder.as_str()));
        call(&mut s, "trash_page", json!({ "page_id": inside }));
        // A name that no space has changes nothing, and names the spaces.
        let (text, err) = call(&mut s, "create_page", json!({ "title": "Lost", "space": "Holidays" }));
        assert!(err && text.contains("\"Work\"") && text.contains("\"Personal projects\"") && text.contains("Nothing was changed"), "{text}");
        assert_eq!(s.store.load().unwrap().0.values().filter(|p| !p.trashed).count(), 2);
        // A sub-page is in the space of its parent; another space with it is refused.
        let (text, err) = call(&mut s, "create_page", json!({ "title": "Plan", "parent_id": folder, "space": "Work" }));
        assert!(err && text.contains("Nothing was made"), "{text}");
        let (text, err) = call(&mut s, "create_page", json!({ "title": "Plan", "parent_id": folder }));
        assert!(!err, "{text}");
        let plan = json_of(&text)["id"].as_str().unwrap().to_string();
        // The list names the spaces, the one that is open, and the space of each top-level page.
        let (text, _) = call(&mut s, "list_pages", json!({}));
        let listed = json_of(&text);
        let named: Vec<(String, bool)> = listed["spaces"].as_array().unwrap().iter().map(|sp| (sp["name"].as_str().unwrap().to_string(), sp["open_in_rune"].as_bool().unwrap())).collect();
        assert!(named.contains(&("Work".to_string(), true)) && named.contains(&("Personal projects".to_string(), false)), "{named:?}");
        assert_eq!(named.iter().filter(|(_, open)| *open).count(), 1);
        let of: Vec<(String, String)> = listed["pages"].as_array().unwrap().iter().map(|p| (p["title"].as_str().unwrap().to_string(), p["space"].as_str().unwrap().to_string())).collect();
        assert!(of.contains(&("Projects".to_string(), "Personal projects".to_string())) && of.contains(&("Inbox".to_string(), "Work".to_string())), "{of:?}");
        let (text, _) = call(&mut s, "list_pages", json!({ "space": "Work" }));
        assert_eq!(json_of(&text)["pages"].as_array().unwrap().len(), 1);
        // A page moved to another space goes to its top level, with what is under it.
        let (text, err) = call(&mut s, "update_page", json!({ "page_id": folder, "space": "Work" }));
        assert!(!err && json_of(&text)["space"] == "Work", "{text}");
        let (pages, _) = s.store.load().unwrap();
        assert_eq!((pages[&folder].space_id.as_str(), pages[&plan].space_id.as_str(), pages[&plan].parent_id.as_deref()), (work.id.as_str(), work.id.as_str(), Some(folder.as_str())));
        let (text, _) = call(&mut s, "read_page", json!({ "page_id": plan }));
        assert!(text.contains("space: \"Work\""), "{text}");
    }

    #[test]
    fn events_of_every_space_say_their_space() {
        let mut s = server();
        let home = Space::new("Home", "blue", 0.0);
        let side = Space::new("Side project", "green", 1.0);
        s.store.save_space(&home).unwrap();
        s.store.save_space(&side).unwrap();
        s.store.set_setting("space", &home.id).unwrap();
        let (text, err) = call(&mut s, "create_event", json!({ "title": "Dentist", "start": "2026-11-12T09:00" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["created"]["space"], "Home", "a new event is in the space that is open");
        let at = |t: &str| crate::when::parse_iso(t).unwrap().0;
        s.store.save_event(&Event::new(side.id.clone(), "Launch", at("2026-11-12T15:00"), at("2026-11-12T16:00"), false)).unwrap();
        let (text, err) = call(&mut s, "list_events", json!({ "from": "2026-11-12", "to": "2026-11-12" }));
        assert!(!err, "{text}");
        let listed = json_of(&text);
        assert_eq!(listed["count"], 2, "the list has the events of every space");
        let spaces: Vec<&str> = listed["events"].as_array().unwrap().iter().map(|e| e["space"].as_str().unwrap_or("")).collect();
        assert_eq!(spaces, ["Home", "Side project"]);
    }

    #[test]
    fn calendar_events_round_trip() {
        let mut s = server();
        let (text, err) = call(&mut s, "create_event", json!({ "title": "Design review", "start": "2026-11-12T14:00", "notes": "Bring sketches", "color": "green" }));
        assert!(!err, "{text}");
        let created = &json_of(&text)["created"];
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["start"], "2026-11-12T14:00");
        assert_eq!(created["end"], "2026-11-12T15:00");

        let (text, err) = call(&mut s, "create_event", json!({ "title": "Offsite", "start": "2026-11-13", "end": "2026-11-14" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["created"]["all_day"], true);
        assert_eq!(json_of(&text)["created"]["end"], "2026-11-14");

        let (text, err) = call(&mut s, "list_events", json!({ "from": "2026-11-12", "to": "2026-11-13" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["count"], 2);

        let (text, err) = call(&mut s, "update_event", json!({ "event_id": id, "start": "2026-11-12T16:30" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["updated"]["end"], "2026-11-12T17:30", "moving keeps the length");

        let (text, err) = call(&mut s, "delete_event", json!({ "event_id": id }));
        assert!(!err, "{text}");
        let (text, _) = call(&mut s, "list_events", json!({ "from": "2026-11-12", "to": "2026-11-12" }));
        assert_eq!(json_of(&text)["count"], 0);
        let (_, err) = call(&mut s, "create_event", json!({ "title": "x", "start": "next tuesday" }));
        assert!(err, "unreadable dates are reported, not guessed");
    }

    #[test]
    fn connector_snippets_are_well_formed() {
        let program = "/Applications/Rune.app/Contents/MacOS/Rune";
        assert_eq!(connect::command(program), "'/Applications/Rune.app/Contents/MacOS/Rune' --mcp");
        assert_eq!(
            connect::claude_code(program),
            "claude mcp add --scope user rune -- '/Applications/Rune.app/Contents/MacOS/Rune' --mcp"
        );
        let parsed: Value = serde_json::from_str(&connect::json(program)).unwrap();
        assert_eq!(parsed["mcpServers"]["rune"]["command"], program);
        assert_eq!(parsed["mcpServers"]["rune"]["args"], json!(["--mcp"]));
        let toml = connect::codex(program);
        assert!(toml.starts_with("[mcp_servers.rune]\ncommand = \"/Applications/Rune.app/Contents/MacOS/Rune\"\nargs = [\"--mcp\"]"), "{toml}");
        // A quote in the path cannot end the quoting early.
        assert_eq!(connect::command("/tmp/it's/rune"), "'/tmp/it'\\''s/rune' --mcp");
    }

    #[test]
    fn repeating_events_list_each_repeat() {
        let mut s = server();
        let (text, err) = call(&mut s, "create_event", json!({ "title": "Standup", "start": "2026-11-03T09:00", "repeat": "weekly", "repeat_until": "2026-11-24" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["created"]["repeats"], "Every week on Tuesday, until Nov 24, 2026");

        let (text, _) = call(&mut s, "list_events", json!({ "from": "2026-11-01", "to": "2026-11-30" }));
        let listed = json_of(&text);
        assert_eq!(listed["count"], 4, "{text}");
        let second = listed["events"][1]["id"].as_str().unwrap().to_string();
        assert!(second.ends_with("@2026-11-10"), "{second}");
        assert_eq!(listed["events"][1]["start"], "2026-11-10T09:00");

        // Moving one repeat moves them all.
        let (text, err) = call(&mut s, "update_event", json!({ "event_id": second, "start": "2026-11-10T10:00" }));
        assert!(!err, "{text}");
        assert_eq!(json_of(&text)["updated"]["start"], "2026-11-03T10:00");

        // Deleting one repeat keeps the rest.
        let (text, err) = call(&mut s, "delete_event", json!({ "event_id": second }));
        assert!(!err, "{text}");
        let (text, _) = call(&mut s, "list_events", json!({ "from": "2026-11-01", "to": "2026-11-30" }));
        assert_eq!(json_of(&text)["count"], 3, "{text}");

        let (_, err) = call(&mut s, "update_event", json!({ "event_id": second, "repeat": "sometimes" }));
        assert!(err, "unknown repeat words are reported");
    }

    #[test]
    fn create_read_search_edit_and_trash_round_trip() {
        let mut s = server();
        let (text, err) = call(&mut s, "create_page", json!({
            "title": "Trip plan",
            "icon": "✈️",
            "markdown": "# Trip plan\n\nPack early.\n\n## Checklist\n\n- [ ] Passport\n- [x] Tickets\n\n```rust\nfn main() {}\n```"
        }));
        assert!(!err, "{text}");
        let parent = json_of(&text)["id"].as_str().unwrap().to_string();

        let (text, err) = call(&mut s, "create_page", json!({ "title": "Packing", "parent_id": format!("notion2://page/{parent}") }));
        assert!(!err, "{text}");
        let child = json_of(&text)["id"].as_str().unwrap().to_string();

        let (page, err) = call(&mut s, "read_page", json!({ "page_id": parent }));
        assert!(!err);
        // (Kept as the icon Rune draws for it.)
        assert!(page.contains("title: \"Trip plan\"") && page.contains("icon: plane:blue"), "{page}");
        assert!(!page.contains("# Trip plan\n"), "title heading should be stripped: {page}");
        assert!(page.contains("## Checklist") && page.contains("- [ ] Passport") && page.contains("- [x] Tickets"), "{page}");
        assert!(page.contains("```rust\nfn main() {}\n```"), "{page}");
        let link = format!("[Packing](rune://page/{child})");
        assert!(page.contains(&link), "{page}");

        // Writing back what was read keeps the sub-page link as a link block.
        let body = page.split("---\n\n").nth(1).unwrap().to_string();
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": parent, "markdown": format!("{body}\n\nAdded line.") }));
        assert!(!err);
        let (_, blocks) = s.store.load().unwrap();
        assert!(blocks[&parent].iter().any(|b| b.kind == BlockKind::PageLink && b.target.as_deref() == Some(child.as_str())));
        assert!(blocks[&parent].iter().any(|b| b.kind == BlockKind::Heading2 && b.text == "Checklist"));

        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": child, "markdown": "- socks\n- charger" }));
        assert!(!err);
        let (_, blocks) = s.store.load().unwrap();
        assert_eq!(blocks[&child].len(), 2, "blank starter block is replaced");

        let (text, _) = call(&mut s, "search_pages", json!({ "query": "charger" }));
        let found = json_of(&text);
        assert_eq!(found["results"][0]["id"], child.as_str());
        assert_eq!(found["results"][0]["path"], "Trip plan / Packing");
        assert!(found["results"][0]["snippet"].as_str().unwrap().contains("charger"));

        let (text, _) = call(&mut s, "list_pages", json!({}));
        let tree = json_of(&text);
        assert_eq!(tree["pages"][0]["children"][0]["id"], child.as_str());

        let (_, err) = call(&mut s, "update_page", json!({ "page_id": child, "title": "Packing list", "favorite": true, "parent_id": "" }));
        assert!(!err);
        let (pages, _) = s.store.load().unwrap();
        assert_eq!(pages[&child].title, "Packing list");
        assert!(pages[&child].favorite && pages[&child].parent_id.is_none());
        let (text, err) = call(&mut s, "update_page", json!({ "page_id": parent, "parent_id": parent }));
        assert!(err && text.contains("inside itself"));

        let (text, err) = call(&mut s, "trash_page", json!({ "page_id": parent }));
        assert!(!err && json_of(&text)["trashed_pages"] == 1);
        let (_, err) = call(&mut s, "read_page", json!({ "page_id": parent }));
        assert!(err);
        let (text, _) = call(&mut s, "search_pages", json!({ "query": "passport" }));
        assert_eq!(json_of(&text)["count"], 0);
        let (_, err) = call(&mut s, "restore_page", json!({ "page_id": parent }));
        assert!(!err);
        let (pages, _) = s.store.load().unwrap();
        assert!(!pages[&parent].trashed);
    }

    #[test]
    fn serve_reads_lines_and_answers_requests_only() {
        let mut s = server();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#, "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
            "\n",
            r#"{"jsonrpc":"2.0","id":"b","method":"tools/list"}"#, "\n",
        );
        let mut out = Vec::new();
        s.serve(io::Cursor::new(input), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["id"], "b");
    }

    #[test]
    fn helpers() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_794_239_520_000), "2026-11-09T15:52:00Z");
        assert_eq!(page_link_target("[A](notion2://page/abc123)"), Some("abc123".into()));
        assert_eq!(page_link_target("[A](https://x.com)"), None);
        assert_eq!(page_link_target("see [A](notion2://page/abc)"), None);
        assert_eq!(strip_title_heading("# Hello\n\nbody", "hello"), "body");
        assert_eq!(strip_title_heading("## Other\nbody", "hello"), "## Other\nbody");
        assert_eq!(normalize_id(" notion2://page/abc/ "), "abc");
        let text = "Some text with Needle inside";
        let lower = text.to_lowercase();
        assert!(snippet_around(text, &lower, lower.find("needle").unwrap()).contains("Needle"));
    }

    #[test]
    fn what_the_markdown_could_not_say_comes_back_in_the_answer_to_put_right() {
        let mut s = server();
        // As a capable AI writes it, with the slips seen most: an empty line
        // before the comment lines, the two in the other order, a kind of
        // Notion's, a colour rule with "=", a formula with a column that is
        // not there, a chart of the wrong shape word and a chart of a column
        // its table does not have.
        let md = [
            "| Item | Price | Qty | Total | Paid |",
            "| --- | --- | --- | --- | --- |",
            "| Tea | 4 | 2 |  |  |",
            "",
            "<!-- table: Invoice | icon gift:green | colour Paid = yes green | sort Price down -->",
            "<!-- columns: text | currency | number sum | formula USD sum = Price * Qty * Tax | checkbox -->",
            "",
            "```chart",
            "{\"shape\": \"donut\", \"x\": \"Item\"}",
            "```",
            "",
            "```chart",
            "{\"shape\": \"bars\", \"x\": \"Item\", \"y\": \"Amount\"}",
            "```",
            "",
            "<!-- a note to myself -->",
        ]
        .join("\n");
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Invoice", "markdown": md }));
        assert!(!err, "{made}");
        let answer = json_of(&made);
        let check: Vec<String> = answer["check"].as_array().expect("a check").iter().map(|n| n.as_str().unwrap().to_string()).collect();
        let has = |words: &[&str]| check.iter().any(|n| words.iter().all(|w| n.contains(w)));
        assert!(has(&["“Price”", "“currency”", "which Rune does not know", "number USD"]), "{check:?}");
        assert!(has(&["“colour Paid = yes green”", "was not understood", "Item, Price, Qty, Total, Paid"]), "{check:?}");
        assert!(has(&["formula of the column “Total”", "no column \"Tax\"", "round"]), "{check:?}");
        assert!(has(&["chart block could not be read", "donut", "bars, line, pie, ring or days"]), "{check:?}");
        assert!(has(&["\"y\": “Amount”", "no such column", "Item, Price"]), "{check:?}");
        assert!(has(&["<!-- a note to myself -->", "under no table"]), "{check:?}");
        assert!(answer["to_do"].as_str().unwrap().contains("read_page"), "{made}");
        // The comment lines were read all the same, after the empty line and in that order.
        let id = answer["id"].as_str().unwrap().to_string();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        assert!(page.contains("<!-- columns: text | text currency | number sum | formula USD sum = Price * Qty * Tax | checkbox -->"), "{page}");
        assert!(page.contains("<!-- table: Invoice | icon gift:green | sort Price down -->"), "{page}");
        // Put right, the same tool says nothing more; and what read_page
        // gives is taken back with no note.
        let fixed = md
            .replace("colour Paid = yes green", "color Paid is ticked green")
            .replace("| currency |", "| number USD |")
            .replace(" * Tax", "")
            .replace("donut", "pie")
            .replace("\"y\": \"Amount\"", "\"y\": \"Total\"")
            .replace("<!-- a note to myself -->", "");
        let (done, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": fixed }));
        assert!(!err && json_of(&done).get("check").is_none(), "{done}");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        let body = page.split("---\n\n").nth(1).unwrap().to_string();
        let (again, _) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body }));
        assert!(json_of(&again).get("check").is_none(), "{again}");
        let (after, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        assert_eq!(after.split("---\n\n").nth(1), page.split("---\n\n").nth(1));
        // A chart of another page that has no table, by its title.
        let (more, _) = call(&mut s, "append_to_page", json!({ "page_id": id, "markdown": "```chart\n{\"shape\": \"line\", \"from\": \"Run log\"}\n```" }));
        assert!(json_of(&more)["check"][0].as_str().unwrap().contains("no page called “Run log” has one"), "{more}");
    }

    #[test]
    fn a_page_icon_is_one_that_rune_draws() {
        let mut s = server();
        let icon_of = |s: &mut Server, id: &str| {
            let (page, _) = call(s, "read_page", json!({ "page_id": id }));
            page.lines().find_map(|l| l.strip_prefix("icon: ")).unwrap().to_string()
        };
        // By its name and colour, by its name alone, and an emoji that Rune has.
        for (given, kept) in [("book:blue", "book:blue"), ("Plane", "plane:gray"), ("💡", "lightbulb:yellow")] {
            let (made, err) = call(&mut s, "create_page", json!({ "title": given, "icon": given }));
            assert!(!err && json_of(&made).get("check").is_none(), "{made}");
            let id = json_of(&made)["id"].as_str().unwrap().to_string();
            assert_eq!(icon_of(&mut s, &id), kept);
        }
        // An emoji Rune has no icon for showed as a gray page: the page is
        // made with none, and the answer says what Rune has.
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Workouts", "icon": "🏋️" }));
        assert!(!err, "{made}");
        let note = json_of(&made)["check"][0].as_str().unwrap().to_string();
        assert!(note.contains("no icon “🏋️”") && note.contains("flame") && note.contains("update_page"), "{note}");
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        assert_eq!(icon_of(&mut s, &id), "none");
        // update_page refuses it, and changes nothing.
        let (said, err) = call(&mut s, "update_page", json!({ "page_id": id, "title": "Gym", "icon": "🏋️" }));
        assert!(err && said.contains("Nothing was changed"), "{said}");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        assert!(page.contains("title: \"Workouts\""), "{page}");
        let (_, err) = call(&mut s, "update_page", json!({ "page_id": id, "icon": "flame:orange" }));
        assert!(!err);
        assert_eq!(icon_of(&mut s, &id), "flame:orange");
        // The names the tool's words list are the icons a page can have.
        let tools = tool_definitions();
        let words = tools.as_array().unwrap().iter().find(|t| t["name"] == "create_page").unwrap()["inputSchema"]["properties"]["icon"]["description"].as_str().unwrap().to_string();
        let listed = words.split("NAME: ").nth(1).unwrap().split(';').next().unwrap().replace(" or ", ", ");
        assert_eq!(listed.split(", ").collect::<Vec<_>>(), crate::ui::icons::PAGE_ICONS.to_vec());
    }

    #[test]
    fn lines_side_by_side_are_made_kept_and_put_under_each_other_again() {
        let mut s = server();
        // A week planner as an AI writes it: two sets of columns.
        let md = "Plan the week.\n\n<!-- column -->\n## Monday\n- [ ] Gym\n<!-- column -->\n## Tuesday\n- [ ] Read\n  - Chapter 3\n<!-- column -->\n## Wednesday\n- [ ] Call Sam\n<!-- end columns -->\n\n<!-- column 1 -->\n## Thursday\n<!-- column 2 -->\n## Friday\n- [ ] Ship\n<!-- /columns -->\n\nNotes under it.";
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Week", "markdown": md }));
        assert!(!err && json_of(&made).get("check").is_none(), "{made}");
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        let cols = |s: &Server| -> Vec<(String, u8)> {
            let (_, blocks) = s.load().unwrap();
            blocks[&id].iter().map(|b| (b.plain_text(), b.place.col)).collect()
        };
        let want = |pairs: &[(&str, u8)]| pairs.iter().map(|(t, c)| (t.to_string(), *c)).collect::<Vec<_>>();
        let planner = want(&[("Plan the week.", 0), ("Monday", 1), ("Gym", 1), ("Tuesday", 2), ("Read", 2), ("Chapter 3", 2), ("Wednesday", 3), ("Call Sam", 3), ("Thursday", 1), ("Friday", 2), ("Ship", 2), ("Notes under it.", 0)]);
        assert_eq!(cols(&s), planner);
        let (_, blocks) = s.load().unwrap();
        assert_eq!(crate::columns::sets(&blocks[&id]).len(), 2);
        // read_page says them as they were written, and gives back changes nothing.
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        let body = page.split("---\n\n").nth(1).unwrap().to_string();
        assert!(body.contains("Plan the week.\n\n<!-- column -->\n\n## Monday\n\n- [ ] Gym\n\n<!-- column -->\n\n## Tuesday"), "{body}");
        assert!(body.contains("- [ ] Call Sam\n\n<!-- end columns -->\n\n<!-- column -->\n\n## Thursday\n\n<!-- column -->\n\n## Friday\n\n- [ ] Ship\n\n<!-- end columns -->\n\nNotes under it."), "{body}");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body }));
        assert!(!err);
        assert_eq!(cols(&s), planner);
        // A to-do changed in a column stays in its column.
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body.replace("- [ ] Read", "- [x] Read a book") }));
        assert!(!err);
        assert_eq!(cols(&s)[4], ("Read a book".to_string(), 2));
        assert_eq!(cols(&s)[7], ("Call Sam".to_string(), 3));
        // Written with no marks, the lines are under each other again.
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        let plain = page.split("---\n\n").nth(1).unwrap().lines().filter(|l| !l.starts_with("<!-- column") && !l.starts_with("<!-- end")).collect::<Vec<_>>().join("\n");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": plain }));
        assert!(!err);
        assert!(cols(&s).iter().all(|(_, c)| *c == 0), "{:?}", cols(&s));
        // A page with no columns, rewritten with no marks, keeps none (and gains none).
        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": id, "markdown": "<!-- column -->\nLeft\n<!-- column -->\nRight\n<!-- end columns -->" }));
        assert!(!err);
        let last: Vec<(String, u8)> = cols(&s).into_iter().rev().take(2).collect();
        assert_eq!(last, want(&[("Right", 2), ("Left", 1)]));
    }

    #[test]
    fn a_relation_names_its_table_by_its_name() {
        let mut s = server();
        let block_of = |s: &Server, page: &str, n: usize| {
            let (_, blocks) = s.load().unwrap();
            blocks[page].iter().filter(|b| b.kind == BlockKind::Table).nth(n).unwrap().clone()
        };
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Companies", "markdown": "| Company | Town |\n| --- | --- |\n| Acme | Leeds |\n| Globex | York |\n<!-- columns: text | text -->\n<!-- table: Companies | icon briefcase:blue -->" }));
        let companies = json_of(&made)["id"].as_str().unwrap().to_string();
        let firms = block_of(&s, &companies, 0).id;
        // A contact list linked to its companies, in one call, as an AI writes it.
        let contacts_md = "| Name | Email | Company |\n| --- | --- | --- |\n| Ann | ann@acme.com | Acme |\n| Bo | bo@globex.com | Globex |\n<!-- columns: text | email | relation Companies -->\n<!-- table: Contacts | icon smile:green -->";
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Contacts", "markdown": contacts_md }));
        assert!(!err && json_of(&made).get("check").is_none(), "{made}");
        let contacts = json_of(&made)["id"].as_str().unwrap().to_string();
        let table = crate::table::Table::parse(&block_of(&s, &contacts, 0).data);
        assert_eq!(table.column(2).from, firms, "the column names the table Companies");
        // read_page says it by the table's name, and given back it changes nothing.
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": contacts }));
        assert!(page.contains("<!-- columns: text | email | relation Companies -->"), "{page}");
        let body = page.split("---\n\n").nth(1).unwrap().to_string();
        let before = s.load().unwrap();
        let (again, _) = call(&mut s, "replace_page_content", json!({ "page_id": contacts, "markdown": body }));
        assert!(json_of(&again).get("check").is_none(), "{again}");
        assert!(s.load().unwrap().1[&contacts] == before.1[&contacts]);
        // A name no table has: the column names none, and the answer says what to do.
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Deals", "markdown": "| Deal | Client |\n| --- | --- |\n| Big one | Initech |\n<!-- columns: text | relation Clients -->" }));
        let note = json_of(&made)["check"][0].as_str().unwrap().to_string();
        assert!(note.contains("No table is named “Clients”") && note.contains("“Companies”") && note.contains("“Contacts”"), "{note}");
        let deals = json_of(&made)["id"].as_str().unwrap().to_string();
        assert_eq!(crate::table::Table::parse(&block_of(&s, &deals, 0).data).column(1).from, "");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": deals }));
        assert!(page.contains("<!-- columns: text | relation -->"), "{page}");
        // Put right, with the same rows (the table reads the same but for its relation): it names Companies.
        let (fixed, _) = call(&mut s, "replace_page_content", json!({ "page_id": deals, "markdown": "| Deal | Client |\n| --- | --- |\n| Big one | Initech |\n<!-- columns: text | relation Companies -->" }));
        assert!(json_of(&fixed).get("check").is_none(), "{fixed}");
        assert_eq!(crate::table::Table::parse(&block_of(&s, &deals, 0).data).column(1).from, firms);
        // A table of the same page, and one with no name by the title of its page.
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Books", "markdown": "| Title | Author |\n| --- | --- |\n| Emma | Austen |\n<!-- columns: text | relation Authors -->\n<!-- table: Books -->\n\n| Author | Born |\n| --- | --- |\n| Austen | 1775 |\n<!-- table: Authors -->\n\n| Shelf | Firm |\n| --- | --- |\n| Top | Acme |\n<!-- columns: text | relation Companies -->" }));
        assert!(json_of(&made).get("check").is_none(), "{made}");
        let books = json_of(&made)["id"].as_str().unwrap().to_string();
        assert_eq!(crate::table::Table::parse(&block_of(&s, &books, 0).data).column(1).from, block_of(&s, &books, 1).id);
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Towns", "markdown": "| Town |\n| --- |\n| Leeds |" }));
        let towns = json_of(&made)["id"].as_str().unwrap().to_string();
        let (_, err) = call(&mut s, "append_to_page", json!({ "page_id": books, "markdown": "| Trip | Where |\n| --- | --- |\n| Spring | Leeds |\n<!-- columns: text | relation Towns -->" }));
        assert!(!err);
        assert_eq!(crate::table::Table::parse(&block_of(&s, &books, 3).data).column(1).from, block_of(&s, &towns, 0).id);
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": books }));
        assert!(page.contains("relation Towns -->") && page.contains("relation Authors -->"), "{page}");
    }

    #[test]
    fn the_ai_writes_the_page_of_a_row() {
        let mut s = server();
        // A recipe book: a table of recipes, each with its own page.
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Recipes", "markdown": "| Recipe | Minutes |\n| --- | --- |\n| Pancakes | 20 |\n| Soup | 40 |\n<!-- columns: text | number min average -->\n<!-- table: Recipes | gallery -->" }));
        let book = json_of(&made)["id"].as_str().unwrap().to_string();
        let steps = "## Ingredients\n- Flour\n- Milk\n\n## Steps\n1. Mix\n2. Fry";
        let (said, err) = call(&mut s, "update_table_row", json!({ "page_id": book, "match": { "Recipe": "Pancakes" }, "page": steps }));
        assert!(!err && json_of(&said).get("check").is_none(), "{said}");
        let pancakes = json_of(&said)["row_page_id"].as_str().expect("the id of the row's page").to_string();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": pancakes }));
        assert!(page.contains("title: \"Pancakes\"") && page.contains(&format!("parent_id: {book}")) && page.contains("## Ingredients\n\n- Flour") && page.contains("2. Fry"), "{page}");
        // It is the page of that row (the one the row opens), not a sub-page in the list.
        let (pages, blocks) = s.load().unwrap();
        let block = blocks[&book].iter().find(|b| b.kind == BlockKind::Table).unwrap();
        let table = crate::table::Table::parse(&block.data);
        assert_eq!(pages[&pancakes].row_of, Some(format!("{}/{}", block.id, table.row_id(1).unwrap())));
        assert!(table.row_id(2).is_some(), "every row goes by a name of its own now");
        let (list, _) = call(&mut s, "list_pages", json!({ "parent_id": book }));
        assert!(!list.contains("Pancakes"), "{list}");
        // Written again, it is the same page with the new words.
        let (said, _) = call(&mut s, "update_table_row", json!({ "page_id": book, "row": 1, "values": { "Minutes": "25" }, "page": format!("{steps}\n3. Serve") }));
        assert_eq!(json_of(&said)["row_page_id"].as_str(), Some(pancakes.as_str()));
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": pancakes }));
        assert!(page.contains("3. Serve"), "{page}");
        assert_eq!(s.load().unwrap().0.values().filter(|p| p.row_of.is_some()).count(), 1);
        // A new row with its page in one call; and a row with no page says none.
        let (said, err) = call(&mut s, "add_table_row", json!({ "page_id": book, "values": { "Recipe": "Salad", "Minutes": "10" }, "page": "## Steps\n1. Chop" }));
        assert!(!err && json_of(&said)["row_page_id"].is_string(), "{said}");
        let (said, _) = call(&mut s, "update_table_row", json!({ "page_id": book, "match": { "Recipe": "Soup" }, "values": { "Minutes": "45" } }));
        assert!(json_of(&said).get("row_page_id").is_none(), "{said}");

        // A budget: the page of a day made from the template, then with its
        // spending, which the month adds up.
        let md = "| Date | Budget | Spent |\n| --- | --- | --- |\n| 2026-11-01 | $60.00 |  |\n<!-- columns: date | number USD sum | rollup Amount sum USD sum -->\n<!-- table: November | page: Item | Amount number USD sum -->";
        let (made, _) = call(&mut s, "create_page", json!({ "title": "November", "markdown": md }));
        let month = json_of(&made)["id"].as_str().unwrap().to_string();
        let (said, _) = call(&mut s, "update_table_row", json!({ "page_id": month, "row": 1, "page": "" }));
        let day = json_of(&said)["row_page_id"].as_str().unwrap().to_string();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": day }));
        assert!(page.contains("| Item | Amount |") && page.contains("number USD sum"), "made from the template: {page}");
        let (_, err) = call(&mut s, "update_table_row", json!({ "page_id": month, "row": 1, "page": "| Item | Amount |\n| --- | --- |\n| Lunch | $12.50 |\n| Cinema | $8.00 |\n<!-- columns: text | number USD sum -->" }));
        assert!(!err);
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": month }));
        assert!(page.contains("| $60.00 | $20.50 |"), "the month adds up the day: {page}");
    }

    #[test]
    fn a_callout_made_and_changed_by_the_ai_stays_a_callout() {
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Trip", "markdown": "> 💡 Book early.\n\n> [!warning:red] Passport!" }));
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        let kinds = |s: &Server| -> Vec<(BlockKind, Option<String>, String)> { s.load().unwrap().1[&id].iter().map(|b| (b.kind, b.icon.clone(), b.text.clone())).collect() };
        assert_eq!(kinds(&s), vec![(BlockKind::Callout, Some("lightbulb:yellow".into()), "Book early.".into()), (BlockKind::Callout, Some("warning:red".into()), "Passport!".into())]);
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        assert!(page.contains("> [!lightbulb:yellow] Book early.\n\n> [!warning:red] Passport!"), "{page}");
        // Its words changed (else it was kept as it was): a callout still, with its icon.
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": "> [!lightbulb:yellow] Book in May.\n\n> [!warning:red] Passport!" }));
        assert!(!err);
        assert_eq!(kinds(&s)[0], (BlockKind::Callout, Some("lightbulb:yellow".into()), "Book in May.".into()));
    }

    #[test]
    fn a_page_gets_its_cover_and_face_and_an_event_its_space_and_page() {
        let mut s = server();
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Lisbon trip", "cover": "sea", "font": "serif", "icon": "plane:blue" }));
        assert!(!err, "{made}");
        let trip = json_of(&made)["id"].as_str().unwrap().to_string();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": trip }));
        assert!(page.contains("icon: plane:blue\ncover: sea\nfont: serif\n"), "{page}");
        let (said, err) = call(&mut s, "update_page", json!({ "page_id": trip, "cover": "pink" }));
        assert!(err && said.contains("no cover “pink”") && said.contains("meadow"), "{said}");
        let (_, err) = call(&mut s, "update_page", json!({ "page_id": trip, "cover": "", "font": "sans" }));
        assert!(!err);
        let (pages, _) = s.load().unwrap();
        assert_eq!((pages[&trip].cover.clone(), pages[&trip].font.clone()), (None, String::new()));
        // The covers the tool's words list are the app's.
        let tools = tool_definitions();
        let words = tools.as_array().unwrap().iter().find(|t| t["name"] == "create_page").unwrap()["inputSchema"]["properties"]["cover"]["description"].as_str().unwrap().to_string();
        let listed = words.split(": ").nth(1).unwrap().trim_end_matches('.').replace(" or ", ", ");
        assert_eq!(listed.split(", ").collect::<Vec<_>>(), crate::model::covers::PRESETS.iter().map(|(id, ..)| *id).collect::<Vec<_>>());

        // A flight with the trip's page, in the trip's space though another is open.
        let work = Space::new("Work", "blue", 0.0);
        let own = Space::new("Personal", "green", 1.0);
        s.store.save_space(&work).unwrap();
        s.store.save_space(&own).unwrap();
        let (_, err) = call(&mut s, "update_page", json!({ "page_id": trip, "space": "Personal" }));
        assert!(!err);
        s.store.set_setting("space", &work.id).unwrap();
        let (made, err) = call(&mut s, "create_event", json!({ "title": "Flight to Lisbon", "start": "2031-05-02T09:40", "page_id": trip }));
        assert!(!err, "{made}");
        let event = &json_of(&made)["created"];
        assert_eq!((event["space"].as_str(), event["notes_page"].as_str()), (Some("Personal"), Some(format!("{LINK_PREFIX}{trip}").as_str())));
        // Named for a space; and the page taken away again.
        let (made, _) = call(&mut s, "create_event", json!({ "title": "Stand-up", "start": "2031-05-03T09:00", "space": "work" }));
        let standup = json_of(&made)["created"]["id"].as_str().unwrap().to_string();
        assert_eq!(json_of(&made)["created"]["space"], "Work");
        let (_, err) = call(&mut s, "update_event", json!({ "event_id": standup, "page_id": trip }));
        assert!(!err);
        let (said, _) = call(&mut s, "update_event", json!({ "event_id": standup, "page_id": "" }));
        assert!(json_of(&said)["updated"]["notes_page"].is_null(), "{said}");
        let (said, err) = call(&mut s, "create_event", json!({ "title": "Lost", "start": "2031-05-03T09:00", "page_id": "nope" }));
        assert!(err && said.contains("No page with id nope"), "{said}");
    }

    #[test]
    fn the_examples_the_instructions_give_are_taken_with_no_note() {
        let mut s = server();
        // A win rate and overdue rows red, as the instructions say them.
        let md = "| Task | Due | Status | Done |\n| --- | --- | --- | --- |\n| Old | 2020-01-01 | Done |  |\n| New | 2099-01-01 | Waiting |  |\n<!-- columns: text | date | choice {Done:green; Waiting:yellow} | formula percent average = if(Status = \"Done\", 100, 0) -->\n<!-- table: Tasks | color Due below today red -->";
        let (made, err) = call(&mut s, "create_page", json!({ "title": "Tasks", "markdown": md }));
        assert!(!err && json_of(&made).get("check").is_none(), "{made}");
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        let (_, blocks) = s.load().unwrap();
        let table = crate::table::Table::parse(&blocks[&id].iter().find(|b| b.kind == BlockKind::Table).unwrap().data);
        assert_eq!((table.shown(1, 3), table.shown(2, 3)), ("100%".to_string(), "0%".to_string()));
        assert_eq!((table.color(1), table.color(2)), (Some("red"), None), "the day that has passed is red");
        // The budget the instructions give, each line on its own line, reads with no note.
        let budget = INSTRUCTIONS.split("For example a budget for a month, a row for each day of the month: ").nth(1).unwrap().split(" (each line on its own line)").next().unwrap().replace("| | ", "|\n| ").replace(" <!--", "\n<!--");
        assert!(markdown::notes(&budget).is_empty(), "{budget}: {:?}", markdown::notes(&budget));
    }

    #[test]
    fn the_icons_the_instructions_name_are_icons_rune_draws() {
        let list = INSTRUCTIONS.split("(icon NAME:COLOUR, NAME one of ").nth(1).unwrap().split(';').next().unwrap();
        for name in list.split(", ") {
            assert!(crate::ui::icons::paths(name.trim()).is_some(), "“{name}” is no icon of Rune's");
        }
        assert!(crate::ui::icons::known_icon(crate::model::CALLOUT_ICON).is_some());
        assert!(crate::ui::icons::known_icon("lightbulb:yellow").is_some());
    }

    #[test]
    fn the_page_of_a_row_with_the_persons_lines_is_not_written_over() {
        let mut s = server();
        let md = "| Date | Budget | Spent |\n| --- | --- | --- |\n| 2026-11-01 | $60.00 |  |\n<!-- columns: date | number USD sum | rollup Amount sum USD sum -->\n<!-- table: November | page: Item | Amount number USD sum -->";
        let (made, _) = call(&mut s, "create_page", json!({ "title": "November", "markdown": md }));
        let month = json_of(&made)["id"].as_str().unwrap().to_string();
        let (said, _) = call(&mut s, "update_table_row", json!({ "page_id": month, "row": 1, "page": "" }));
        let day = json_of(&said)["row_page_id"].as_str().unwrap().to_string();
        // The person writes a line of their own in the day's page, in Rune.
        let mut lines = s.load().unwrap().1[&day].clone();
        lines.push(Block::text("Coffee and the bus"));
        s.save_blocks(&day, &lines).unwrap();
        let spending = "| Item | Amount |\n| --- | --- |\n| Lunch | $12.50 |\n<!-- columns: text | number USD sum -->";
        let (said, err) = call(&mut s, "update_table_row", json!({ "page_id": month, "row": 1, "values": { "Budget": "$70.00" }, "page": spending }));
        assert!(err && said.contains("not written over") && said.contains(&day), "{said}");
        assert!(s.load().unwrap().1[&day].iter().any(|b| b.text == "Coffee and the bus"), "the person's line is there");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": month }));
        assert!(page.contains("$60.00") && !page.contains("$70.00"), "nothing was changed: {page}");
        // Read first, the AI may write it: it has seen what is there.
        call(&mut s, "read_page", json!({ "page_id": day }));
        let (said, err) = call(&mut s, "update_table_row", json!({ "page_id": month, "row": 1, "page": spending }));
        assert!(!err, "{said}");
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": day }));
        assert!(page.contains("Lunch"), "{page}");
    }

    #[test]
    fn a_rewrite_keeps_the_table_a_relation_names() {
        let mut s = server();
        let table_of = |s: &Server, page: &str| crate::table::Table::parse(&s.load().unwrap().1[page].iter().find(|b| b.kind == BlockKind::Table).unwrap().data);
        let firms_md = "| Company |\n| --- |\n| Acme |\n<!-- table: Companies -->";
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Firms", "markdown": firms_md }));
        let firms = json_of(&made)["id"].as_str().unwrap().to_string();
        let firms_table = s.load().unwrap().1[&firms].iter().find(|b| b.kind == BlockKind::Table).unwrap().id.clone();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Contacts", "markdown": "| Name | Company |\n| --- | --- |\n| Ann | Acme |\n<!-- columns: text | relation Companies -->" }));
        let contacts = json_of(&made)["id"].as_str().unwrap().to_string();
        assert_eq!(table_of(&s, &contacts).column(1).from, firms_table);
        // A newer table of the same name (a copy of the page), then a rewrite of the contacts.
        call(&mut s, "create_page", json!({ "title": "Firms copy", "markdown": firms_md }));
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": contacts }));
        let body = page.split("---\n\n").nth(1).unwrap().replace("| Ann |", "| Anna |");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": contacts, "markdown": body }));
        assert!(!err);
        assert_eq!(table_of(&s, &contacts).column(1).from, firms_table, "it names the table it named");
        // Its table in the trash: still that table.
        call(&mut s, "trash_page", json!({ "page_id": firms }));
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": contacts }));
        let body = page.split("---\n\n").nth(1).unwrap().replace("| Anna |", "| Ann |");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": contacts, "markdown": body }));
        assert!(!err);
        assert_eq!(table_of(&s, &contacts).column(1).from, firms_table);
    }

    #[test]
    fn a_rewrite_keeps_what_the_person_wrote_as_it_is() {
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Ideas", "markdown": "x" }));
        let id = json_of(&made)["id"].as_str().unwrap().to_string();
        // A quote that starts with an emoji and an item that starts with a triangle.
        let mut code = Block::new(BlockKind::Code, "# Draft\n\nHello");
        code.language = Some("markdown".into());
        s.save_blocks(&id, &[Block::new(BlockKind::Quote, "💡 idea"), Block::new(BlockKind::Bulleted, "▸ Next"), Block::text("End")]).unwrap();
        let kinds = |s: &Server, id: &str| -> Vec<(BlockKind, String)> { s.load().unwrap().1[id].iter().map(|b| (b.kind, b.text.clone())).collect() };
        let before = kinds(&s, &id);
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        let body = page.split("---\n\n").nth(1).unwrap().replace("End", "The end");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body }));
        assert!(!err);
        assert_eq!(kinds(&s, &id)[..2], before[..2], "still a quote and a bullet");
        // A page that is one code block of Markdown stays that code block.
        s.save_blocks(&id, &[code.clone()]).unwrap();
        let (page, _) = call(&mut s, "read_page", json!({ "page_id": id }));
        let body = page.split("---\n\n").nth(1).unwrap().replace("Hello", "Hello again");
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": body }));
        assert!(!err);
        assert_eq!(kinds(&s, &id), vec![(BlockKind::Code, "# Draft\n\nHello again".to_string())]);
        // A line written in the place of a closed toggle is not that toggle.
        let mut toggle = Block::new(BlockKind::Toggle, "Plans");
        toggle.collapsed = true;
        toggle.tint = "red".into();
        s.save_blocks(&id, &[Block::text("Intro"), toggle.clone(), Block::text("End")]).unwrap();
        call(&mut s, "read_page", json!({ "page_id": id }));
        let (_, err) = call(&mut s, "replace_page_content", json!({ "page_id": id, "markdown": "Intro\n\nA new thought\n\nEnd" }));
        assert!(!err);
        let (_, blocks) = s.load().unwrap();
        let line = blocks[&id].iter().find(|b| b.text == "A new thought").unwrap();
        assert!(line.id != toggle.id && line.tint.is_empty() && !line.collapsed, "{line:?}");
    }

    #[test]
    fn an_append_asks_nothing_about_what_was_there() {
        let mut s = server();
        let (made, _) = call(&mut s, "create_page", json!({ "title": "Deals", "markdown": "| Deal | Client |\n| --- | --- |\n| Big one | Initech |\n<!-- columns: text | relation Clients -->" }));
        assert!(json_of(&made).get("check").is_some(), "{made}");
        let deals = json_of(&made)["id"].as_str().unwrap().to_string();
        let (said, err) = call(&mut s, "append_to_page", json!({ "page_id": deals, "markdown": "One more line." }));
        assert!(!err && json_of(&said).get("check").is_none(), "{said}");
    }
}
