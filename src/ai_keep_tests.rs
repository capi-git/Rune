//! The AI writes a page again: round trips of one page that holds every kind
//! of thing a page can hold, through each way an AI changes a page (the
//! connector's tools and the AI box of the app). After each, the notes are
//! compared with what they must be, field by field, on the page and on every
//! page that hangs on it (the pages of rows, the template's page, sub-pages).
//! The rule they guard: an AI rewrite never changes or loses a thing that
//! its Markdown did not change.
//!
//! (The window's folded headings are not part of the page: they are the
//! window's `folds` setting, which no AI path reads or writes.)

use chrono::{NaiveDate, NaiveTime};
use serde_json::{json, Value};

use crate::mcp::Server;
use crate::model::{Block, BlockKind, Id, Page, Place};
use crate::store::{Loaded, Store};
use crate::table::{date::When, Table};

// ----- the page -------------------------------------------------------------

/// The ids of what the test page holds.
#[allow(dead_code)]
struct Ids {
    page: Id,
    parent: Id,
    sub: Id,
    locked: Id,
    other: Id,
    table_a: Id,
    table_b: Id,
    template: Id,
    row_pages: Vec<Id>,
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn line(kind: BlockKind, text: &str) -> Block {
    Block::new(kind, text)
}

fn table_block(md: &str) -> Block {
    let lines: Vec<&str> = md.lines().collect();
    let (table, _) = Table::from_markdown(&lines).expect("a table");
    let mut b = Block::new(BlockKind::Table, table.plain_text());
    b.place.beside = table.beside;
    b.data = table.to_data();
    b
}

fn set_table(b: &mut Block, table: &Table) {
    b.data = table.to_data();
    b.text = table.plain_text();
}

fn page(id_parent: Option<&Id>, title: &str) -> Page {
    let mut p = Page::new(id_parent.cloned(), 1.0);
    p.space_id = "space-1".into();
    p.title = title.into();
    p
}

/// A page with every kind of line, each with its settings that are not the
/// first ones, and the pages around it.
fn build(store: &Store) -> Ids {
    let parent = page(None, "Parent");
    let mut p = page(Some(&parent.id), "Everything");
    p.icon = Some("star:blue".into());
    p.cover = Some("preset:sea".into());
    p.cover_y = 30;
    p.font = "serif".into();
    p.favorite = true;
    let sub = page(Some(&p.id), "Sub page");
    let locked = page(Some(&p.id), "Diary");
    let other = page(None, "Other page");

    let mut blocks: Vec<Block> = Vec::new();
    let mut intro = line(BlockKind::Text, &format!("Intro with **bold**, _italic_, `code`, a [link](https://example.com) and [Other page](rune://page/{}): the quick fox.", other.id));
    intro.tint = "red blue-bg".into();
    blocks.push(intro);
    let mut h1 = line(BlockKind::Heading1, "Big heading");
    h1.tint = "green".into();
    blocks.push(h1);
    blocks.push(line(BlockKind::Heading2, "Middle heading"));
    blocks.push(line(BlockKind::Heading3, "Small heading"));
    for (n, kind) in [BlockKind::Bulleted, BlockKind::Numbered].into_iter().enumerate() {
        for depth in 0..3u8 {
            blocks.push(line(kind, &format!("List {n} depth {depth}")).with_indent(depth));
        }
    }
    let mut todo = line(BlockKind::Todo, "Call Sam about the **budget**");
    todo.due = Some(day(2026, 11, 20));
    todo.remind = NaiveTime::from_hms_opt(9, 0, 0);
    todo.tint = "orange".into();
    blocks.push(todo);
    let mut done = line(BlockKind::Todo, "Send the slides");
    done.checked = true;
    blocks.push(done);
    blocks.push(line(BlockKind::Todo, "A nested to-do").with_indent(1));
    let mut open = line(BlockKind::Toggle, "Open toggle");
    open.tint = "purple-bg".into();
    blocks.push(open);
    blocks.push(line(BlockKind::Text, "Text inside the open toggle").with_indent(1));
    blocks.push(line(BlockKind::Heading3, "Heading inside the toggle").with_indent(1));
    let mut shut = line(BlockKind::Toggle, "Closed toggle");
    shut.collapsed = true;
    blocks.push(shut);
    blocks.push(line(BlockKind::Bulleted, "Bullet in the closed toggle").with_indent(1));
    blocks.push(line(BlockKind::Text, "Words in the closed toggle").with_indent(1));
    let mut callout = line(BlockKind::Callout, "A callout with the app's icon");
    callout.icon = Some("flame:red".into());
    callout.tint = "yellow-bg".into();
    blocks.push(callout);
    let mut emoji = line(BlockKind::Callout, "A callout with an emoji");
    emoji.icon = Some("🔥".into());
    blocks.push(emoji);
    blocks.push(line(BlockKind::Quote, "A quote\non two lines"));
    let mut code = line(BlockKind::Code, "fn main() {\n    println!(\"hi\");\n}");
    code.language = Some("rust".into());
    blocks.push(code);
    let mut diagram = line(BlockKind::Code, "flowchart LR\n  A[Plan] --> B[Do]");
    diagram.language = Some("mermaid".into());
    blocks.push(diagram);
    blocks.push(line(BlockKind::Divider, ""));
    blocks.push(Block::page_link(sub.id.clone()));
    blocks.push(Block::page_link(locked.id.clone()));
    let mut picture = line(BlockKind::Image, "A caption");
    picture.data = "a1b2c3d4.png".into();
    picture.place = Place { w: 320, ..Place::default() };
    blocks.push(picture);
    let mut free = line(BlockKind::Image, "");
    free.data = "e5f6a7b8.png".into();
    free.place = Place { free: true, right: true, x: 12, w: 200, ..Place::default() };
    blocks.push(free);
    let mut drawing = line(BlockKind::Drawing, "");
    drawing.data = r#"{"type":"excalidraw","elements":[{"id":"x","type":"rectangle"}]}"#.into();
    blocks.push(drawing);
    blocks.push(line(BlockKind::Text, "https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
    blocks.push(line(BlockKind::Text, "12 × 4.5 ="));
    let mut left = line(BlockKind::Text, "Left column words");
    left.place.col = 1;
    blocks.push(left);
    let mut right = line(BlockKind::Text, "Right column words");
    right.place.col = 2;
    blocks.push(right);
    // A second set right under it, of three columns, with lines of other
    // kinds in them (as the marks `<!-- column -->` write them): a heading
    // and a to-do; a heading and a callout with the app's first icon; a
    // heading and a shut toggle with a line under it.
    for (col, kind, words) in [(1, BlockKind::Heading3, "Monday"), (1, BlockKind::Todo, "Gym at seven"), (2, BlockKind::Heading3, "Tuesday"), (2, BlockKind::Callout, "Rest day"), (3, BlockKind::Heading3, "Wednesday"), (3, BlockKind::Toggle, "Ideas to try")] {
        let mut b = line(kind, words);
        b.place.col = col;
        if kind == BlockKind::Callout {
            b.icon = Some(crate::model::CALLOUT_ICON.into());
        }
        if kind == BlockKind::Toggle {
            b.collapsed = true;
        }
        blocks.push(b);
    }
    let mut hike = line(BlockKind::Bulleted, "A hike").with_indent(1);
    hike.place.col = 3;
    blocks.push(hike);
    blocks.push(line(BlockKind::Text, &format!("[Other page](rune://page/{})", other.id)));
    blocks.push(line(BlockKind::Bulleted, "A bullet with words under it"));
    blocks.push(line(BlockKind::Text, "Words under the bullet").with_indent(1));
    // Lines with a line break in them (Shift-Return).
    blocks.push(line(BlockKind::Bulleted, "A bullet on\ntwo lines").with_indent(1));
    let mut two = line(BlockKind::Todo, "A to-do on\ntwo lines");
    two.due = Some(day(2026, 12, 1));
    blocks.push(two);
    let mut tall = line(BlockKind::Callout, "A callout on\ntwo lines");
    tall.icon = Some("💡".into());
    blocks.push(tall);
    blocks.push(line(BlockKind::Heading2, "A heading on\ntwo lines"));
    let mut toggle = line(BlockKind::Toggle, "A toggle with a table");
    toggle.collapsed = true;
    blocks.push(toggle);
    blocks.push(table_block("| Step | Done |\n| --- | --- |\n| One | x |\n<!-- columns: text | checkbox -->").with_indent(1));
    blocks.push(line(BlockKind::Text, "After the table in the toggle").with_indent(1));

    // A table with a column of each kind it can have (12), its name, icon
    // and every part of a view.
    let when = When { start: day(2026, 11, 20), time: NaiveTime::from_hms_opt(9, 0, 0), end: Some(day(2026, 11, 21)), end_time: NaiveTime::from_hms_opt(10, 30, 0), remind: Some(15) }.write(day(2026, 11, 14));
    let md_a = format!(
        "| Name | Amount | Share | Distance | When | Took | Pace | Done | Kind | Tags | Status | Double |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n\
         | Alpha | $1,200.50 | 40% | 5 | {when} | 30:00 |  | x | Long | Home, Food | In progress |  |\n\
         | Beta | -$20.00 | 75% | 12 | Sat 21 Nov 2026 | 45:00 |  |  | Short | Work | Done |  |\n\
         | Gamma | $3.00 | 10% | 7.5 |  | 1:02:30 |  |  | Long |  | Not started |  |\n\
         <!-- columns: text count | number USD sum signed | number percent ring goal 100 | number km bar goal 20 average | date | time sum | pace average | checkbox checked | choice {{Long:green; Short:red; Rest:blue}} | multi {{Home:blue; Food:orange; Work:purple}} | status | formula USD sum = Amount * 2 -->\n\
         <!-- table: Trades | icon target:green | board by Kind | sort Amount down | frozen 1 | hide Pace | filter Distance above 1 | color Status is Done green | locked | full -->"
    );
    let mut a = table_block(&md_a);
    let mut ta = Table::parse(&a.data);
    ta.name_rows();
    ta.nest(2, true);
    ta.nest(3, true);
    ta.toggle_closed(2);
    ta.set_places(3, Some(2));
    ta.template = Some(crate::table::Template { cells: vec![String::new(), "$10.00".into(), String::new(), String::new(), String::new(), String::new(), String::new(), String::new(), "Rest".into(), String::new(), "Not started".into(), String::new()] });
    set_table(&mut a, &ta);
    let ids_a: Vec<String> = (1..4).map(|r| ta.row_id(r).unwrap().to_string()).collect();
    blocks.push(a.clone());
    let chart = crate::chart::Chart::from_markdown(r#"{"shape":"line","x":"When","y":"Amount","by":"week","add":"sum","title":"Money"}"#).unwrap();
    let mut c = line(BlockKind::Chart, &chart.title);
    c.data = chart.to_data();
    blocks.push(c);
    let chart2 = crate::chart::Chart::from_markdown(r#"{"shape":"pie","x":"Kind","add":"count"}"#).unwrap();
    let mut c2 = line(BlockKind::Chart, &chart2.title);
    c2.data = chart2.to_data();
    blocks.push(c2);

    // The other kinds, beside it, in a gallery: a rollup of the pages of
    // its rows, and a relation to the first table.
    let md_b = "| Item | Site | Mail | Phone | Pictures | Cost | Trade | Note |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n\
         | Rent | https://example.com | a@example.com | +1 555 0100 | f1a2b3c4.png | | Alpha | first |\n\
         | Food | | | | | | Beta, Gamma | |\n\
         <!-- columns: text | link | email | phone | file | rollup Amount sum USD sum | relation | text -->\n\
         <!-- table: Costs | icon wrench:blue | beside | gallery | sort Item -->";
    let mut b = table_block(md_b);
    let mut tb = Table::parse(&b.data);
    tb.name_rows();
    tb.set_from(6, &a.id);
    set_table(&mut b, &tb);
    let ids_b: Vec<String> = (1..3).map(|r| tb.row_id(r).unwrap().to_string()).collect();
    blocks.push(b.clone());
    blocks.push(line(BlockKind::Text, "The end of the page."));
    crate::columns::tidy(&mut blocks);

    // The pages of rows, with words; one that holds a table the rollup takes from.
    let mut row_pages = Vec::new();
    let save = |p: &Page, bs: &[Block]| {
        store.save_page(p).unwrap();
        store.save_blocks(&p.id, bs).unwrap();
    };
    for (r, id) in ids_a.iter().enumerate().take(2) {
        let mut rp = page(Some(&p.id), ["Alpha", "Beta"][r]);
        rp.row_of = Some(format!("{}/{id}", a.id));
        save(&rp, &[line(BlockKind::Text, &format!("Notes on {}", rp.title))]);
        row_pages.push(rp.id);
    }
    let mut rp = page(Some(&p.id), "Rent");
    rp.row_of = Some(format!("{}/{}", b.id, ids_b[0]));
    save(&rp, &[table_block("| What | Amount |\n| --- | --- |\n| Flat | $500.00 |\n<!-- columns: text | number USD sum -->"), line(BlockKind::Text, "Paid on the first")]);
    row_pages.push(rp.id);
    let mut template = page(Some(&p.id), "Template");
    template.row_of = Some(format!("{}/template", a.id));
    save(&template, &[table_block("| Step | Price |\n| --- | --- |\n| Entry | $1.00 |\n<!-- columns: text | number USD -->"), line(BlockKind::Text, "Check the news first")]);

    save(&parent, &[Block::page_link(p.id.clone())]);
    save(&p, &blocks);
    save(&sub, &[line(BlockKind::Text, "Words of the sub-page")]);
    save(&other, &[line(BlockKind::Text, "Another page")]);
    let secret = [line(BlockKind::Text, "The code is 4711")];
    save(&locked, &[crate::lock::seal_with(&crate::lock::test_key("pw-pw-pw"), &locked.id, &secret)]);
    Ids { page: p.id, parent: parent.id, sub: sub.id, locked: locked.id, other: other.id, table_a: a.id, table_b: b.id, template: template.id, row_pages }
}

// ----- comparing ------------------------------------------------------------

fn label(b: &Block) -> String {
    let words: String = b.plain_text().chars().take(28).collect();
    format!("{} “{}”", b.kind.as_str(), words.replace('\n', " "))
}

fn page_diff(want: &Page, got: &Page) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = |name: &str, a: String, b: String| {
        if a != b {
            out.push(format!("page “{}”: {name} {a} → {b}", want.title));
        }
    };
    field("title", want.title.clone(), got.title.clone());
    field("icon", format!("{:?}", want.icon), format!("{:?}", got.icon));
    field("space", want.space_id.clone(), got.space_id.clone());
    field("parent", format!("{:?}", want.parent_id), format!("{:?}", got.parent_id));
    field("position", want.position.to_string(), got.position.to_string());
    field("favorite", want.favorite.to_string(), got.favorite.to_string());
    field("trashed", want.trashed.to_string(), got.trashed.to_string());
    field("created", want.created_at.to_string(), got.created_at.to_string());
    field("cover", format!("{:?} {}", want.cover, want.cover_y), format!("{:?} {}", got.cover, got.cover_y));
    field("font", want.font.clone(), got.font.clone());
    field("row_of", format!("{:?}", want.row_of), format!("{:?}", got.row_of));
    out
}

fn table_diff(want: &str, got: &str) -> Vec<String> {
    let (a, b) = (Table::parse(want), Table::parse(got));
    let mut out = Vec::new();
    if a.rows != b.rows {
        for r in 0..a.rows.len().max(b.rows.len()) {
            if a.rows.get(r) != b.rows.get(r) {
                out.push(format!("row {r} {:?} → {:?}", a.rows.get(r), b.rows.get(r)));
            }
        }
    }
    for c in 0..a.columns().max(b.columns()) {
        let (x, y) = (a.columns.get(c), b.columns.get(c));
        if x != y && !(x.is_none() && y.is_some_and(|y| *y == Default::default())) && !(y.is_none() && x.is_some_and(|x| *x == Default::default())) {
            out.push(format!("column {c} {x:?} → {y:?}"));
        }
    }
    if a.name != b.name || a.icon != b.icon {
        out.push(format!("name {} {} → {} {}", a.name, a.icon, b.name, b.icon));
    }
    if a.info != b.info {
        out.push(format!("rows' ids or nesting {:?} → {:?}", a.info, b.info));
    }
    if a.view != b.view {
        out.push(format!("view {:?} → {:?}", a.view, b.view));
    }
    if a.template != b.template {
        out.push(format!("template {:?} → {:?}", a.template, b.template));
    }
    out
}

fn block_diff(want: &Block, got: &Block) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = |name: &str, a: String, b: String| {
        if a != b {
            out.push(format!("{name} {a:?} → {b:?}"));
        }
    };
    field("kind", want.kind.as_str().into(), got.kind.as_str().into());
    field("text", want.text.clone(), got.text.clone());
    field("checked", want.checked.to_string(), got.checked.to_string());
    field("indent", want.indent.to_string(), got.indent.to_string());
    field("collapsed", want.collapsed.to_string(), got.collapsed.to_string());
    field("icon", format!("{:?}", want.icon), format!("{:?}", got.icon));
    field("language", format!("{:?}", want.language), format!("{:?}", got.language));
    field("target", format!("{:?}", want.target), format!("{:?}", got.target));
    field("place", format!("{:?}", want.place), format!("{:?}", got.place));
    field("due", format!("{:?}", want.due), format!("{:?}", got.due));
    field("remind", format!("{:?}", want.remind), format!("{:?}", got.remind));
    field("tint", want.tint.clone(), got.tint.clone());
    if want.kind == BlockKind::Table && got.kind == BlockKind::Table {
        out.extend(table_diff(&want.data, &got.data).into_iter().map(|d| format!("table: {d}")));
    } else if want.data != got.data {
        out.push(format!("data {:?} → {:?}", want.data, got.data));
    }
    out
}

/// A line made by the change has an id that starts with this: it is paired
/// with the line of the same kind and words.
const NEW: &str = "new-";

fn lines_diff(title: &str, want: &[Block], got: &[Block]) -> Vec<String> {
    let mut out = Vec::new();
    let mut paired: Vec<Option<usize>> = want.iter().map(|w| got.iter().position(|g| g.id == w.id)).collect();
    for (i, w) in want.iter().enumerate() {
        if paired[i].is_none() && w.id.starts_with(NEW) {
            paired[i] = (0..got.len()).find(|&j| !paired.contains(&Some(j)) && got[j].kind == w.kind && got[j].text == w.text);
        }
    }
    for (i, w) in want.iter().enumerate() {
        match paired[i] {
            Some(j) => out.extend(block_diff(w, &got[j]).into_iter().map(|d| format!("“{title}” {}: {d}", label(w)))),
            None => {
                // The same words under a new id: a line that lost what it was.
                match got.iter().enumerate().find(|(j, g)| !paired.contains(&Some(*j)) && g.plain_text() == w.plain_text()) {
                    Some((j, g)) => {
                        paired[i] = Some(j);
                        let d = block_diff(w, g);
                        out.push(format!("“{title}” {}: is a new block (new id){}", label(w), if d.is_empty() { String::new() } else { format!(": {}", d.join("; ")) }));
                    }
                    None => out.push(format!("“{title}” {}: is gone", label(w))),
                }
            }
        }
    }
    for (j, g) in got.iter().enumerate() {
        if !paired.contains(&Some(j)) {
            out.push(format!("“{title}” {}: was not there", label(g)));
        }
    }
    let order: Vec<usize> = paired.iter().flatten().copied().collect();
    if order.windows(2).any(|w| w[0] > w[1]) {
        out.push(format!("“{title}”: lines in another order"));
    }
    out
}

/// Every difference between the notes as they must be and as they are.
fn differences(want: &Loaded, got: &Loaded) -> Vec<String> {
    let mut out = Vec::new();
    let mut ids: Vec<&Id> = want.0.keys().chain(got.0.keys()).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        match (want.0.get(id), got.0.get(id)) {
            (Some(w), Some(g)) => {
                out.extend(page_diff(w, g));
                let none = Vec::new();
                out.extend(lines_diff(&w.title, want.1.get(id).unwrap_or(&none), got.1.get(id).unwrap_or(&none)));
            }
            (Some(w), None) => out.push(format!("page “{}” is gone", w.title)),
            (None, Some(g)) => out.push(format!("page “{}” was not there", g.title)),
            (None, None) => {}
        }
    }
    out
}

// ----- the changes an AI makes ----------------------------------------------

/// What the AI does to the page's Markdown, and what the notes must then be.
struct Change {
    name: &'static str,
    markdown: fn(&str, &Ids) -> String,
    want: fn(&mut Loaded, &Ids),
}

fn lines_of<'a>(notes: &'a mut Loaded, id: &str) -> &'a mut Vec<Block> {
    notes.1.get_mut(id).unwrap()
}

fn no_change(_: &mut Loaded, _: &Ids) {}

fn changes() -> Vec<Change> {
    vec![
        Change { name: "the same text back", markdown: |md, _| md.to_string(), want: no_change },
        Change { name: "the same text back with Windows line ends", markdown: |md, _| md.replace('\n', "\r\n"), want: no_change },
        Change { name: "the same text back after a byte order mark", markdown: |md, _| format!("\u{FEFF}{md}"), want: no_change },
        Change { name: "the same text back in a fence of Markdown", markdown: |md, _| format!("```markdown\n{md}\n```"), want: no_change },
        Change { name: "a line added at the end, in a fence of Markdown", markdown: |md, _| format!("```md\n{}\n\nA new line at the end.\n```", md.trim_end()), want: |n, ids| {
            let mut b = Block::text("A new line at the end.");
            b.id = format!("{NEW}1");
            lines_of(n, &ids.page).push(b);
        } },
        Change {
            name: "one word of one paragraph",
            markdown: |md, _| md.replacen("the quick fox", "the fast fox", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text.contains("the quick fox")).unwrap();
                b.text = b.text.replace("the quick fox", "the fast fox");
            },
        },
        Change {
            name: "one word in the closed toggle",
            markdown: |md, _| md.replacen("Words in the closed toggle", "Words kept in the closed toggle", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text == "Words in the closed toggle").unwrap();
                b.text = "Words kept in the closed toggle".into();
            },
        },
        Change {
            name: "one word of a coloured callout",
            markdown: |md, _| md.replacen("with the app's icon", "with its own icon", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text.contains("with the app's icon")).unwrap();
                b.text = b.text.replace("with the app's icon", "with its own icon");
            },
        },
        Change {
            name: "one word of the to-do with a day",
            markdown: |md, _| md.replacen("Call Sam about", "Phone Sam about", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text.contains("Call Sam about")).unwrap();
                b.text = b.text.replace("Call Sam about", "Phone Sam about");
            },
        },
        Change {
            name: "the reminder of the to-do",
            markdown: |md, _| md.replacen("\u{23F0} 09:00", "\u{23F0} 10:30", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text.contains("Call Sam about")).unwrap();
                b.remind = NaiveTime::from_hms_opt(10, 30, 0);
            },
        },
        Change {
            name: "one word of a toggle",
            markdown: |md, _| md.replacen("- ▸ Closed toggle", "- ▸ Closed toggle box", 1),
            want: |n, ids| lines_of(n, &ids.page).iter_mut().find(|b| b.text == "Closed toggle").unwrap().text = "Closed toggle box".into(),
        },
        Change {
            name: "one word of the callout with an emoji",
            markdown: |md, _| md.replacen("A callout with an emoji", "A callout with a sign", 1),
            want: |n, ids| lines_of(n, &ids.page).iter_mut().find(|b| b.text == "A callout with an emoji").unwrap().text = "A callout with a sign".into(),
        },
        Change {
            name: "a word made bold",
            markdown: |md, _| md.replacen("the quick fox", "the **quick** fox", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text.contains("the quick fox")).unwrap();
                b.text = b.text.replace("the quick fox", "the **quick** fox");
            },
        },
        Change {
            name: "a link's address",
            markdown: |md, _| md.replacen("(https://example.com)", "(https://example.org)", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.text.contains("(https://example.com)")).unwrap();
                b.text = b.text.replace("(https://example.com)", "(https://example.org)");
            },
        },
        Change {
            name: "a code block's language",
            markdown: |md, _| md.replacen("```rust", "```python", 1),
            want: |n, ids| lines_of(n, &ids.page).iter_mut().find(|b| b.language.as_deref() == Some("rust")).unwrap().language = Some("python".into()),
        },
        Change {
            name: "one word of the to-do on two lines",
            markdown: |md, _| md.replacen("A to-do on", "A task on", 1),
            want: |n, ids| lines_of(n, &ids.page).iter_mut().find(|b| b.text.starts_with("A to-do on")).unwrap().text = "A task on\ntwo lines".into(),
        },
        Change {
            name: "a line added in the closed toggle",
            markdown: |md, _| md.replacen("  Words in the closed toggle", "  Words in the closed toggle\n\n  New words in the toggle", 1),
            want: |n, ids| {
                let lines = lines_of(n, &ids.page);
                let at = lines.iter().position(|b| b.text == "Words in the closed toggle").unwrap();
                let mut b = Block::text("New words in the toggle").with_indent(1);
                b.id = format!("{NEW}1");
                lines.insert(at + 1, b);
            },
        },
        Change {
            name: "one cell of the table in a toggle",
            markdown: |md, _| md.replacen("| One | x |", "| One | |", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.kind == BlockKind::Table && b.text.contains("Step")).unwrap();
                let mut t = Table::parse(&b.data);
                t.set(1, 1, "");
                set_table(b, &t);
            },
        },
        Change {
            name: "a column of the first table renamed",
            markdown: |md, _| {
                let Some(start) = md.find("| Name | Amount |") else { return md.to_string() };
                let end = start + md[start..].find("<!-- table: Trades").unwrap();
                let end = end + md[end..].find("-->").unwrap();
                format!("{}{}{}", &md[..start], md[start..end].replace("Distance", "Km"), &md[end..])
            },
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.id == ids.table_a).unwrap();
                let mut t = Table::parse(&b.data);
                t.set(0, 3, "Km");
                set_table(b, &t);
            },
        },
        Change {
            name: "the comment lines of the first table left out",
            markdown: |md, _| {
                let Some(start) = md.find("| Name | Amount |") else { return md.to_string() };
                let cols = start + md[start..].find("\n<!-- columns:").unwrap();
                let end = cols + md[cols..].find("<!-- table: Trades").unwrap();
                let end = end + md[end..].find("-->").unwrap() + 3;
                format!("{}{}", &md[..cols], &md[end..])
            },
            want: no_change,
        },
        Change {
            name: "one cell of the first table",
            markdown: |md, _| md.replacen("| Beta | -$20.00 |", "| Beta | -$25.00 |", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.id == ids.table_a).unwrap();
                let mut t = Table::parse(&b.data);
                t.set(2, 1, "-$25.00");
                set_table(b, &t);
            },
        },
        Change {
            name: "the name of a row with a page",
            markdown: |md, _| md.replacen("| Beta |", "| Bravo |", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.id == ids.table_a).unwrap();
                let mut t = Table::parse(&b.data);
                t.set(2, 0, "Bravo");
                set_table(b, &t);
                // (Its page takes the name; see `name_row_pages`.)
                let beta = ids.row_pages[1].clone();
                n.0.get_mut(&beta).unwrap().title = "Bravo".into();
            },
        },
        Change {
            name: "the chart's shape",
            markdown: |md, _| md.replacen("\"shape\":\"pie\"", "\"shape\":\"bars\"", 1),
            want: |n, ids| {
                let b = lines_of(n, &ids.page).iter_mut().find(|b| b.data.contains("\"shape\":\"pie\"")).unwrap();
                b.data = b.data.replace("\"shape\":\"pie\"", "\"shape\":\"bars\"");
            },
        },
        Change {
            name: "a line added at the end",
            markdown: |md, _| format!("{}\n\nA new line at the end.", md.trim_end()),
            want: |n, ids| {
                let mut b = Block::text("A new line at the end.");
                b.id = format!("{NEW}1");
                lines_of(n, &ids.page).push(b);
            },
        },
        Change {
            name: "a line added at the top",
            markdown: |md, _| match md.strip_prefix("# Everything\n\n") {
                Some(rest) => format!("# Everything\n\nA new first line.\n\n{rest}"),
                None => format!("A new first line.\n\n{md}"),
            },
            want: |n, ids| {
                let mut b = Block::text("A new first line.");
                b.id = format!("{NEW}1");
                lines_of(n, &ids.page).insert(0, b);
            },
        },
        Change {
            name: "a picture's line left out",
            markdown: |md, _| md.lines().filter(|l| !l.starts_with("[Image](")).collect::<Vec<_>>().join("\n"),
            // (A picture that the Markdown does not name is kept at the end of the page.)
            want: |n, ids| {
                let lines = lines_of(n, &ids.page);
                let pictures: Vec<Block> = lines.iter().filter(|b| b.kind == BlockKind::Image).cloned().collect();
                lines.retain(|b| b.kind != BlockKind::Image);
                lines.extend(pictures.into_iter().map(|mut b| {
                    b.place.col = 0;
                    b
                }));
            },
        },
        Change {
            name: "one word in a column of the second set",
            markdown: |md, _| md.replacen("Rest day", "Rest and stretch day", 1),
            want: |n, ids| lines_of(n, &ids.page).iter_mut().find(|b| b.text == "Rest day").unwrap().text = "Rest and stretch day".into(),
        },
        Change {
            name: "a line added in the third column",
            markdown: |md, _| md.replacen("  - A hike", "  - A hike\n\n  - A swim", 1),
            want: |n, ids| {
                let lines = lines_of(n, &ids.page);
                let at = lines.iter().position(|b| b.text == "A hike").unwrap();
                let mut b = Block::new(BlockKind::Bulleted, "A swim").with_indent(1);
                b.id = format!("{NEW}col");
                b.place.col = 3;
                lines.insert(at + 1, b);
            },
        },
        Change {
            name: "the marks of the columns left out: the lines under each other",
            markdown: |md, _| md.lines().filter(|l| *l != crate::markdown::COLUMN && *l != crate::markdown::COLUMNS_END).collect::<Vec<_>>().join("\n"),
            want: |n, ids| lines_of(n, &ids.page).iter_mut().for_each(|b| b.place.col = 0),
        },
        Change {
            name: "a sub-page's link left out",
            markdown: |md, ids| md.lines().filter(|l| !l.contains(&format!("rune://page/{})", ids.sub)) || !l.starts_with('[')).collect::<Vec<_>>().join("\n"),
            want: |n, ids| lines_of(n, &ids.page).retain(|b| b.target.as_deref() != Some(ids.sub.as_str())),
        },
        Change {
            name: "the second table left out",
            markdown: |md, _| {
                let Some(start) = md.find("| Item | Site |") else { return md.to_string() };
                let name = start + md[start..].find("<!-- table: Costs").unwrap();
                let end = name + md[name..].find("-->").unwrap() + 3;
                format!("{}{}", &md[..start], &md[end..])
            },
            want: |n, ids| lines_of(n, &ids.page).retain(|b| b.id != ids.table_b),
        },
    ]
}

// ----- the ways an AI changes a page ----------------------------------------

fn tool(s: &mut Server, name: &str, args: Value) -> (String, bool) {
    let v = s.tool(name, &args);
    (v["content"][0]["text"].as_str().unwrap_or("").to_string(), v["isError"].as_bool().unwrap_or(false))
}

/// The page as `read_page` gives it, without its header.
fn read_body(s: &mut Server, page: &str) -> String {
    let (text, err) = tool(s, "read_page", json!({ "page_id": page }));
    assert!(!err, "{text}");
    let at = text.find("\n---\n\n").expect("a header");
    text[at + 6..].to_string()
}

/// The connector: read_page, the change, replace_page_content.
fn by_connector(change: &Change) -> (Loaded, Loaded) {
    let mut s = Server::new(Store::open_in_memory().unwrap());
    let ids = build(s.store());
    let before = s.store().load().unwrap();
    let md = (change.markdown)(&read_body(&mut s, &ids.page), &ids);
    let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": md }));
    assert!(!err, "{}: {said}", change.name);
    let mut want = before.clone();
    (change.want)(&mut want, &ids);
    (want, s.store().load().unwrap())
}

/// The AI box, a whole page: the page as the AI is given it, the change,
/// and the reply put in place of the page (see `ai_revise`). The pages of
/// rows that the app drops with such a change (`drop_lost_row_pages`) are
/// left out of the notes after it.
fn by_box(change: &Change) -> (Loaded, Loaded) {
    let store = Store::open_in_memory().unwrap();
    let ids = build(&store);
    let before = store.load().unwrap();
    let (pages, all) = &before;
    let given = crate::state::page_as_given_with(pages, all, &ids.page, &all[&ids.page]);
    let reply = (change.markdown)(&given, &ids);
    // (As the AI sends it: its first line says that it is the page again.)
    let output = match reply.strip_prefix('\u{FEFF}') {
        // (The mark inside the fence, as some write it.)
        None if reply.starts_with("```md\n") => reply.replacen("```md\n", &format!("```md\n{}\n", crate::ai::REVISE_MARK), 1),
        Some(rest) => format!("\u{FEFF}{}\n{rest}", crate::ai::REVISE_MARK),
        None => format!("{}{}{reply}", crate::ai::REVISE_MARK, if reply.contains('\r') { "\r\n" } else { "\n" }),
    };
    let body = crate::ai::split_revision(&crate::ai::clean_reply(&output).0).expect("a page again");
    let (body, _) = crate::ai::split_pages(&body);
    let title = pages[&ids.page].title.clone();
    let new = crate::mcp::parse_body(pages, &crate::mcp::strip_title_heading(&body, &title));
    let mut lines = crate::state::revised_page(&all[&ids.page], new);
    // (A relation column that names its table by its name finds it, as in `ai_revise`.)
    crate::mcp::link_relations(pages, all, &ids.page, &mut lines);
    crate::columns::tidy(&mut lines);
    let mut got = before.clone();
    dropped_rows(&mut got, &all[&ids.page], &lines);
    got.1.insert(ids.page.clone(), lines);
    let mut want = before.clone();
    (change.want)(&mut want, &ids);
    // (The box gives a row's page the new name too: `name_row_pages_of`.)
    for b in got.1[&ids.page].clone().iter().filter(|b| b.kind == BlockKind::Table) {
        for (id, title) in crate::state::stale_row_titles(&got.0, &b.id, &Table::parse(&b.data)) {
            got.0.get_mut(&id).unwrap().title = title;
        }
    }
    (want, got)
}

/// The pages of the rows that went with a change of the window's page go
/// too (`drop_row_pages`): those with words to the Trash, the others for good.
fn dropped_rows(notes: &mut Loaded, before: &[Block], after: &[Block]) {
    for (block, rows) in crate::state::lost_rows(before, after) {
        for row in rows {
            let key = format!("{block}/{row}");
            let Some(id) = notes.0.values().find(|p| p.row_of.as_deref() == Some(key.as_str())).map(|p| p.id.clone()) else { continue };
            if notes.1.get(&id).is_some_and(|bs| bs.iter().any(|b| b.kind != BlockKind::Text || !b.text.trim().is_empty())) {
                notes.0.get_mut(&id).unwrap().trashed = true;
            } else {
                notes.0.remove(&id);
            }
        }
    }
}

/// The AI box with a part of the page picked (from the open toggle to the
/// words of the closed one): the part as the AI is given it, the change,
/// and the reply put in place of the part (see `ai_revise`). None when the
/// change does not touch the part.
fn by_part(change: &Change) -> Option<(Loaded, Loaded)> {
    let store = Store::open_in_memory().unwrap();
    let ids = build(&store);
    let before = store.load().unwrap();
    let (pages, all) = &before;
    let lines = &all[&ids.page];
    let from = lines.iter().position(|b| b.text == "Open toggle").unwrap();
    let to = lines.iter().position(|b| b.text == "Words in the closed toggle").unwrap();
    let part: Vec<Id> = lines[from..=to].iter().map(|b| b.id.clone()).collect();
    let given = crate::state::part_as_given(pages, lines, &part);
    let reply = (change.markdown)(&given, &ids);
    // (A line added to the part goes to its end or top, not the page's.)
    if (reply == given && change.name != "the same text back") || change.name.starts_with("a line added at") {
        return None;
    }
    let new = crate::mcp::parse_body(pages, &crate::mcp::strip_title_heading(&reply, ""));
    let mut after = lines.clone();
    crate::state::revise_part(&mut after, &part, new).expect("the part is one run");
    crate::mcp::link_relations(pages, all, &ids.page, &mut after);
    crate::columns::tidy(&mut after);
    let mut got = before.clone();
    dropped_rows(&mut got, lines, &after);
    got.1.insert(ids.page.clone(), after);
    let mut want = before.clone();
    (change.want)(&mut want, &ids);
    Some((want, got))
}

/// The AI box asked to rewrite one line (Improve, Shorter, Translate…): it
/// is given the line's words, and its reply takes the line's place (see
/// `ai_accept`). `reply` gives the reply from the words given.
fn by_rewrite(find: &str, reply: fn(&str) -> String) -> (Loaded, Loaded) {
    let store = Store::open_in_memory().unwrap();
    let ids = build(&store);
    let before = store.load().unwrap();
    let lines = &before.1[&ids.page];
    let anchor = lines.iter().find(|b| b.text == find).unwrap_or_else(|| panic!("no line “{find}”")).clone();
    let answer = reply(&anchor.text);
    let mut after = lines.clone();
    crate::state::accept_reply(&mut after, &anchor.id, true, &answer);
    crate::columns::tidy(&mut after);
    let mut got = before.clone();
    got.1.insert(ids.page.clone(), after);
    let mut want = before.clone();
    let b = lines_of(&mut want, &ids.page).iter_mut().find(|b| b.id == anchor.id).unwrap();
    b.text = if anchor.kind == BlockKind::Code { answer.trim_matches('\n').to_string() } else { crate::inline::canonical(answer.trim()) };
    (want, got)
}

/// Rewrites of one line of each kind: the line, and the reply to its words.
fn rewrites() -> Vec<(&'static str, fn(&str) -> String)> {
    vec![
        ("Call Sam about the **budget**", |t| format!("{t} today")),
        ("A callout with the app's icon", |t| t.replace("app's", "own")),
        ("A callout with an emoji", |t| t.replace("an emoji", "a sign")),
        ("Closed toggle", |t| format!("{t} box")),
        ("Big heading", |_| "Large heading".into()),
        ("Words in the closed toggle", |t| t.replace("Words", "All the words")),
        ("A bullet on\ntwo lines", |t| t.replace("two", "three")),
        ("A quote\non two lines", |t| t.replace("two", "three")),
        ("fn main() {\n    println!(\"hi\");\n}", |t| t.replace("hi", "hello")),
        ("Left column words", |t| format!("{t} again")),
    ]
}

fn report(path: &str, change: &Change, found: &[String]) -> String {
    if found.is_empty() {
        return String::new();
    }
    format!("## {path}: {}\n{}\n", change.name, found.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"))
}

// ----- the tests -------------------------------------------------------------

/// Each test lists what changed that the change did not ask for, by way
/// and change, and passes when the list is empty.
fn none_lost(all: String) {
    assert!(all.is_empty(), "An AI rewrite changed what its Markdown did not:\n{all}");
}

#[test]
fn every_kind_of_thing_on_a_page_stays_when_the_connector_writes_it_again() {
    let mut all = String::new();
    for change in changes() {
        let (want, got) = by_connector(&change);
        all.push_str(&report("connector", &change, &differences(&want, &got)));
    }
    none_lost(all);
}

#[test]
fn every_kind_of_thing_on_a_page_stays_when_the_ai_box_writes_it_again() {
    let mut all = String::new();
    for change in changes() {
        let (want, got) = by_box(&change);
        let mut found = differences(&want, &got);
        // (The window drops the pages of the rows of a table that the page
        // no longer has, as a table deleted by hand does: those with words
        // go to the Trash. The connector leaves them to the next start.)
        if change.name == "the second table left out" {
            found.retain(|f| f != "page “Rent”: trashed false → true");
        }
        all.push_str(&report("box", &change, &found));
    }
    none_lost(all);
}

#[test]
fn every_kind_of_thing_in_a_picked_part_stays_when_the_ai_box_writes_it_again() {
    let mut all = String::new();
    for change in changes() {
        if let Some((want, got)) = by_part(&change) {
            all.push_str(&report("box, a part", &change, &differences(&want, &got)));
        }
    }
    none_lost(all);
}

#[test]
fn a_line_that_the_ai_rewrites_stays_the_line_it_was_with_new_words() {
    let mut all = String::new();
    for (find, reply) in rewrites() {
        let (want, got) = by_rewrite(find, reply);
        let found = differences(&want, &got);
        if !found.is_empty() {
            all.push_str(&format!("## box, rewrite of one line: {find:?}\n{}\n", found.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")));
        }
    }
    none_lost(all);
}

#[test]
fn a_page_changed_in_rune_after_the_ai_read_it_is_not_written_over() {
    let mut s = Server::new(Store::open_in_memory().unwrap());
    let ids = build(s.store());
    // A person types in the page while the AI works on what it read.
    let read = read_body(&mut s, &ids.page);
    let mut lines = s.store().load_page_blocks(&ids.page).unwrap();
    let typed = lines.iter_mut().find(|b| b.text == "The end of the page.").unwrap();
    typed.text = "The end of the page, typed while the AI worked.".into();
    s.store().save_blocks(&ids.page, &lines).unwrap();
    let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": read.replacen("the quick fox", "the fast fox", 1) }));
    assert!(err && said.contains("changed in Rune after you read it") && said.contains("Nothing was changed"), "{said}");
    assert_eq!(s.store().load_page_blocks(&ids.page).unwrap(), lines, "the page is as the person left it");
    // Read again, the change goes in, with the person's words.
    let read = read_body(&mut s, &ids.page);
    let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": read.replacen("the quick fox", "the fast fox", 1) }));
    assert!(!err, "{said}");
    let now = s.store().load_page_blocks(&ids.page).unwrap();
    assert!(now.iter().any(|b| b.text.contains("typed while the AI worked")) && now.iter().any(|b| b.text.contains("the fast fox")));
    // What the AI's own tools change since its read is no change of another.
    let read = read_body(&mut s, &ids.page);
    let (_, err) = tool(&mut s, "append_to_page", json!({ "page_id": ids.page, "markdown": "Added by the AI." }));
    assert!(!err);
    let (_, err) = tool(&mut s, "update_table_row", json!({ "page_id": ids.page, "table": "Name", "row": 1, "values": { "Distance": "6" } }));
    assert!(!err);
    let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": format!("{}\n\nAdded by the AI, now longer.", read.replacen("| Alpha | $1,200.50 | 40% | 5 |", "| Alpha | $1,200.50 | 40% | 6 |", 1)) }));
    assert!(!err, "{said}");
    // But a line typed after that is.
    let read = read_body(&mut s, &ids.page);
    let (_, err) = tool(&mut s, "set_todo", json!({ "page_id": ids.page, "text": "Send the slides", "done": false }));
    assert!(!err);
    let mut lines = s.store().load_page_blocks(&ids.page).unwrap();
    lines.push(Block::text("Typed after the AI ticked a to-do"));
    s.store().save_blocks(&ids.page, &lines).unwrap();
    let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": read }));
    assert!(err && said.contains("read_page"), "{said}");
    assert_eq!(s.store().load_page_blocks(&ids.page).unwrap(), lines);
}

#[test]
fn a_reply_in_a_fence_of_markdown_is_the_markdown_in_it() {
    let store = Store::open_in_memory().unwrap();
    let ids = build(&store);
    let mut lines = store.load_page_blocks(&ids.page).unwrap();
    let todo = lines.iter().find(|b| b.text.starts_with("Call Sam")).unwrap().clone();
    crate::state::accept_reply(&mut lines, &todo.id, true, "```markdown\nCall Sam about the **budget** today\n```");
    let now = lines.iter().find(|b| b.id == todo.id).unwrap();
    assert_eq!(*now, Block { text: "Call Sam about the **budget** today".into(), ..todo });
    // A code line's reply in its own fence is its code; a fence of no language or another stays code.
    let code = lines.iter().find(|b| b.language.as_deref() == Some("rust")).unwrap().clone();
    crate::state::accept_reply(&mut lines, &code.id, true, "```rust\nfn main() {}\n```");
    assert_eq!(lines.iter().find(|b| b.id == code.id).unwrap().text, "fn main() {}");
    for kept in ["```\n# Not a page\n```", "```python\nprint(1)\n```", "```markdown\nan open fence"] {
        assert_eq!(crate::markdown::unfenced(kept), kept);
    }
}

#[test]
fn a_locked_page_takes_nothing_from_any_tool() {
    let mut s = Server::new(Store::open_in_memory().unwrap());
    let ids = build(s.store());
    let before = s.store().load().unwrap();
    for (name, args) in [
        ("replace_page_content", json!({ "page_id": ids.locked, "markdown": "New words" })),
        ("append_to_page", json!({ "page_id": ids.locked, "markdown": "More" })),
        ("add_table_row", json!({ "page_id": ids.locked, "values": { "Name": "x" } })),
        ("update_table_row", json!({ "page_id": ids.locked, "row": 1, "values": { "Name": "x" } })),
        ("set_todo", json!({ "page_id": ids.locked, "text": "code", "done": true })),
        ("update_page", json!({ "page_id": ids.locked, "title": "Open diary" })),
        ("create_page", json!({ "title": "Inside", "parent_id": ids.locked })),
    ] {
        let (said, err) = tool(&mut s, name, args);
        assert!(err, "{name}: {said}");
    }
    let (read, _) = tool(&mut s, "read_page", json!({ "page_id": ids.locked }));
    assert!(!read.contains("4711"), "{read}");
    assert!(differences(&before, &s.store().load().unwrap()).is_empty());
}

#[test]
fn a_table_with_the_most_rows_goes_through_a_rewrite_whole() {
    let mut s = Server::new(Store::open_in_memory().unwrap());
    let ids = build(s.store());
    // The first table with as many rows as a table holds, each named.
    let mut lines = s.store().load_page_blocks(&ids.page).unwrap();
    let b = lines.iter_mut().find(|b| b.id == ids.table_a).unwrap();
    let mut t = Table::parse(&b.data);
    while t.rows.len() < crate::table::MOST_ROWS {
        let r = t.add_row(t.rows.len() - 1).expect("room for a row");
        t.set(r, 0, &format!("Row {r}"));
        t.set(r, 3, &format!("{r}"));
    }
    assert!(t.add_row(t.rows.len() - 1).is_none(), "the table is full");
    t.name_rows();
    set_table(b, &t);
    s.store().save_blocks(&ids.page, &lines).unwrap();
    let before = s.store().load().unwrap();
    // Written again as read, and with one cell changed.
    for (from, to) in [("", ""), ("| Row 150 |", "| Row 150 changed |")] {
        let read = read_body(&mut s, &ids.page);
        let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": read.replacen(from, to, 1) }));
        assert!(!err, "{said}");
        let mut want = before.clone();
        if !from.is_empty() {
            let b = lines_of(&mut want, &ids.page).iter_mut().find(|b| b.id == ids.table_a).unwrap();
            let mut t = Table::parse(&b.data);
            let r = t.body().find(|&r| t.cell(r, 0) == "Row 150").unwrap();
            t.set(r, 0, "Row 150 changed");
            set_table(b, &t);
        }
        let found = differences(&want, &s.store().load().unwrap());
        assert!(found.is_empty(), "{found:#?}");
    }
    // One row more than it holds is refused, and nothing is changed.
    let read = read_body(&mut s, &ids.page);
    let now = s.store().load().unwrap();
    let more = read.replacen("| Alpha |", "| One more | | | | | | | | | | | |\n| Alpha |", 1);
    let (said, err) = tool(&mut s, "replace_page_content", json!({ "page_id": ids.page, "markdown": more }));
    assert!(err && said.contains("199 rows under its header") && said.contains("The page was not changed"), "{said}");
    assert!(differences(&now, &s.store().load().unwrap()).is_empty());
}

/// What the round trips above go through holds the forms the AI can write
/// (columns by their marks, a callout with its icon, a toggle, a relation
/// by its table's name, the cover and face of the page), as `read_page` and
/// the AI box give them: they are proven to come back as they were.
#[test]
fn what_the_ai_is_given_holds_every_form_it_can_write() {
    let mut s = Server::new(Store::open_in_memory().unwrap());
    let ids = build(s.store());
    let (text, _) = tool(&mut s, "read_page", json!({ "page_id": ids.page }));
    for form in ["cover: sea\nfont: serif\n", "<!-- column -->\n\nLeft column words\n\n<!-- column -->\n\nRight column words\n\n<!-- end columns -->\n\n<!-- column -->\n\n### Monday", "> [!lightbulb:yellow] Rest day", "> [!flame:red] A callout with the app's icon", "- ▸ Closed toggle", "- ▸ Ideas to try\n\n  - A hike\n\n<!-- end columns -->", "| relation Trades | text -->"] {
        assert!(text.contains(form), "read_page lacks {form:?}:\n{text}");
    }
    let (pages, all) = s.store().load().unwrap();
    let given = crate::state::page_as_given_with(&pages, &all, &ids.page, &all[&ids.page]);
    assert!(given.contains("<!-- column -->") && given.contains("> [!lightbulb:yellow] Rest day") && given.contains("relation Trades"), "{given}");
}

/// A line the person changed while the AI wrote stays as they left it,
/// when the AI's page put in its place left that line as it was; a line
/// both changed refuses the reply.
#[test]
fn what_the_person_wrote_while_the_ai_wrote_is_kept() {
    let p = |t: &str| crate::mcp::parse_body(&Default::default(), t);
    let sent = vec![line(BlockKind::Heading1, "Plan"), line(BlockKind::Text, "Old intro"), line(BlockKind::Bulleted, "Milk"), line(BlockKind::Text, "End")];
    let ids: Vec<Id> = sent.iter().map(|b| b.id.clone()).collect();
    // The person, meanwhile: "Milk" ticked off into "Milk and eggs", a line
    // added after it, "End" taken away.
    let mut now = sent.clone();
    now[2].text = "Milk and eggs".into();
    now.insert(3, line(BlockKind::Text, "Typed meanwhile"));
    now.remove(4);
    // The AI: a new intro, and a line added at the foot.
    let reply = p("## Plan\n\nA better intro\n\n- Milk\n\nEnd\n\nA tip from the AI");
    let merged = crate::state::merged_page(&sent, &now, reply).expect("no line changed by both");
    let words: Vec<&str> = merged.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(words, vec!["Plan", "A better intro", "Milk and eggs", "Typed meanwhile", "A tip from the AI"]);
    assert_eq!(merged[0].id, ids[0]);
    assert_eq!(merged[2].id, ids[2], "the person's line is the line it was");
    // A line changed by both: refused.
    let reply = p("## Plan\n\nOld intro\n\n- Bread\n\nEnd");
    assert!(crate::state::merged_page(&sent, &now, reply).is_none());
    // A line the person took away and the AI changed: refused.
    let reply = p("## Plan\n\nOld intro\n\n- Milk\n\nThe end");
    assert!(crate::state::merged_page(&sent, &now, reply).is_none());
    // A table the person typed in, a paragraph the AI changed: both kept.
    let mut table = line(BlockKind::Table, "");
    table.data = Table::parse(r#"{"rows":[["Item","Amount"],["Lunch","12"]]}"#).to_data();
    let sent = vec![line(BlockKind::Text, "Spending"), table.clone()];
    let mut now = sent.clone();
    let mut typed = Table::parse(&table.data);
    typed.rows.push(vec!["Bus".into(), "3".into()]);
    now[1].data = typed.to_data();
    let reply = p(&format!("Spending this week\n\n{}", crate::markdown::table_markdown(&table, "")));
    let merged = crate::state::merged_page(&sent, &now, reply).expect("apart");
    assert_eq!(merged[0].text, "Spending this week");
    assert_eq!(merged[1].id, table.id);
    assert!(Table::parse(&merged[1].data).rows.iter().any(|r| r[0] == "Bus"), "the row typed meanwhile is there");
}
