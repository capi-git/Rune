//! Markdown in and out: typing shortcuts at the start of a block, turning AI
//! replies into blocks, and turning a page into text for AI context.

use crate::model::{heading_level, Block, BlockKind};

/// A Markdown-style shortcut typed at the start of a text block.
#[derive(Debug, PartialEq)]
pub struct Shortcut {
    pub kind: BlockKind,
    /// Byte length of the prefix to remove.
    pub prefix_len: usize,
    pub checked: bool,
}

pub fn detect_shortcut(text: &str) -> Option<Shortcut> {
    let simple: &[(&str, BlockKind)] = &[
        ("### ", BlockKind::Heading3),
        ("## ", BlockKind::Heading2),
        ("# ", BlockKind::Heading1),
        ("- ", BlockKind::Bulleted),
        ("* ", BlockKind::Bulleted),
        ("+ ", BlockKind::Bulleted),
        ("[] ", BlockKind::Todo),
        ("[ ] ", BlockKind::Todo),
        ("> ", BlockKind::Toggle),
        ("\" ", BlockKind::Quote),
        // The Mac makes a typed quote sign curly ("smart quotes").
        ("\u{201C} ", BlockKind::Quote),
        ("\u{201D} ", BlockKind::Quote),
        ("```", BlockKind::Code),
    ];
    for (prefix, kind) in simple {
        if text.starts_with(prefix) {
            return Some(Shortcut { kind: *kind, prefix_len: prefix.len(), checked: false });
        }
    }
    for prefix in ["[x] ", "[X] "] {
        if text.starts_with(prefix) {
            return Some(Shortcut { kind: BlockKind::Todo, prefix_len: prefix.len(), checked: true });
        }
    }
    if text == "---" {
        return Some(Shortcut { kind: BlockKind::Divider, prefix_len: 3, checked: false });
    }
    numbered_prefix(text).map(|len| Shortcut { kind: BlockKind::Numbered, prefix_len: len, checked: false })
}

/// Length of a "12. " or "3) " prefix.
fn numbered_prefix(text: &str) -> Option<usize> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = &text.as_bytes()[digits..];
    if rest.len() >= 2 && (rest[0] == b'.' || rest[0] == b')') && rest[1] == b' ' {
        Some(digits + 2)
    } else {
        None
    }
}

/// If the text ends with a `/command` being typed, return the byte index of the
/// slash and the query typed after it.
pub fn slash_query(text: &str) -> Option<(usize, String)> {
    let slash = text.rfind('/')?;
    let before_ok = slash == 0 || text[..slash].ends_with(char::is_whitespace);
    let query = &text[slash + 1..];
    if before_ok && query.chars().count() <= 24 && !query.contains(char::is_whitespace) {
        Some((slash, query.to_string()))
    } else {
        None
    }
}

/// If the text ends with the start of a link to a page being typed (`@name`
/// or `[[name`), return the byte index where that starts, whether it is the
/// `[[` form, and the name typed so far. A name can have spaces (up to four
/// words). `@` counts only at the start of a word, so an email address or
/// "@ 5 pm" is left alone.
pub fn mention_query(text: &str) -> Option<(usize, bool, String)> {
    let fits = |q: &str| q.chars().count() <= 40 && !q.starts_with(char::is_whitespace) && q.split_whitespace().count() <= 4;
    let wiki = text.rfind("[[").filter(|&at| {
        let q = &text[at + 2..];
        fits(q) && !q.contains(']') && !q.contains('[')
    });
    let at_sign = text.rfind('@').filter(|&at| {
        let before_ok = at == 0 || text[..at].ends_with(|c: char| c.is_whitespace() || c == '(');
        before_ok && fits(&text[at + 1..])
    });
    match (wiki, at_sign) {
        (Some(w), Some(a)) if a > w => Some((a, false, text[a + 1..].to_string())),
        (Some(w), _) => Some((w, true, text[w + 2..].to_string())),
        (None, Some(a)) => Some((a, false, text[a + 1..].to_string())),
        (None, None) => None,
    }
}

/// What an AI sent as one code block of Markdown (some wrap all that they
/// write so): the Markdown inside it. Anything else, as it is. (Read as it
/// came, a page written again was one code block: every line, table and
/// picture's place went, and with the tables the pages of their rows.)
pub fn unfenced(md: &str) -> &str {
    let t = md.trim();
    let Some((first, rest)) = t.split_once('\n') else { return md };
    let fence = first.bytes().take_while(|b| *b == b'`').count();
    let lang = first[fence..].trim();
    if fence < 3 || !(lang.eq_ignore_ascii_case("markdown") || lang.eq_ignore_ascii_case("md")) {
        return md;
    }
    match rest.trim_end().rsplit_once('\n') {
        Some((inner, last)) if last.trim().len() == fence && last.trim().bytes().all(|b| b == b'`') => inner,
        _ => md,
    }
}

/// Convert Markdown (for example an AI reply) into blocks.
pub fn to_blocks(md: &str) -> Vec<Block> {
    read(md, false, &mut Found::default())
}

/// The tables of this Markdown whose table line says what the page of each
/// row starts with (`page:`), in their order, each with that table in
/// `Table::page`. (It is not kept in the blocks: see `state::row_template`.)
pub fn said_pages(md: &str) -> Vec<crate::table::Table> {
    let mut found = Found::default();
    read(md, false, &mut found);
    found.said
}

/// The tables of this Markdown that are more than a table holds: for each,
/// its columns and its rows under the header (see `Table::over_most`).
pub fn tables_over_most(md: &str) -> Vec<(usize, usize)> {
    let mut found = Found::default();
    read(md, false, &mut found);
    found.over
}

/// What in this Markdown could not be read as it was meant, one plain
/// sentence each: a part of a table's comment lines, a chart, a comment
/// line under no table. (Read in silence, it became words on the page, a
/// text column or nothing, and the AI tool that wrote it never knew.)
pub fn notes(md: &str) -> Vec<String> {
    let mut found = Found::default();
    read(md, false, &mut found);
    found.notes
}

/// As `to_blocks`, for words that are pasted: each line is a line of the
/// page. (In a Markdown file, lines one under the other are one paragraph;
/// a list of names copied from another program is not.)
pub fn to_blocks_by_line(md: &str) -> Vec<Block> {
    read(md, true, &mut Found::default())
}

/// What reading Markdown finds besides the blocks.
#[derive(Default)]
struct Found {
    /// Tables over what a table holds (see `tables_over_most`).
    over: Vec<(usize, usize)>,
    /// Tables that say what the page of each row starts with (see `said_pages`).
    said: Vec<crate::table::Table>,
    /// What could not be read (see `notes`).
    notes: Vec<String>,
}

fn read<'a>(md: &'a str, each_line: bool, found: &mut Found) -> Vec<Block> {
    // (A mark of the order of bytes that some programs put first is no word.)
    let md = md.strip_prefix('\u{FEFF}').unwrap_or(md);
    let mut out: Vec<Block> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut quote: Vec<String> = Vec::new();
    let mut code: Option<(Option<String>, Vec<String>)> = None;
    // How many backticks opened the code block: a longer fence holds lines
    // that begin with three (see the writer), and ends only at its like.
    let mut fence = 3;
    // Where a column of lines side by side starts (true) or the set of
    // columns ends (false): before the block at that number.
    let mut marks: Vec<(usize, bool)> = Vec::new();

    // How deep the paragraph being read stands (see where it starts).
    let mut para_indent = 0u8;
    // The list item or heading read last, while the lines under it with
    // more room before them are more of its words (see the writer): its
    // place in `out`, its room, and its words as written.
    let mut item: Option<(usize, usize, String)> = None;

    fn flush(out: &mut Vec<Block>, paragraph: &mut Vec<String>, quote: &mut Vec<String>, indent: u8) {
        if !paragraph.is_empty() {
            out.push(Block::text(clean_inline(&paragraph.join("\n"))).with_indent(indent));
            paragraph.clear();
        }
        if !quote.is_empty() {
            match callout_of(quote) {
                Some((icon, text)) => {
                    let mut b = Block::new(BlockKind::Callout, clean_inline(&text));
                    b.icon = Some(icon);
                    out.push(b);
                }
                None => out.push(Block::new(BlockKind::Quote, clean_inline(unguarded(&quote.join("\n"))))),
            }
            quote.clear();
        }
    }

    let all: Vec<&'a str> = md.lines().collect();
    let mut skip = 0usize;
    // The table read last had more rows than a table holds: the line its
    // other rows start at, with its header and its line of dashes.
    let mut more_rows: Option<(usize, &'a str, &'a str)> = None;
    for (at, raw) in all.iter().copied().enumerate() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        // A table: its header, the line of dashes, then its rows.
        if code.is_none() && raw.trim_start().starts_with('|') {
            // The rows over the most of the table above are a table of
            // their own, under the same header. (They were one paragraph
            // of bars and words.)
            let rest: Option<Vec<&'a str>> = more_rows.filter(|(from, ..)| *from == at).map(|(_, head, rule)| [head, rule].into_iter().chain(all[at..].iter().copied()).collect());
            let (lines, lead) = match &rest {
                Some(lines) => (lines.as_slice(), 2),
                None => (&all[at..], 0),
            };
            if lead == 0 {
                found.over.extend(crate::table::Table::over_most(lines));
            }
            // (The rows over the most come again under the same comment
            // lines: what those say is noted once.)
            let mut notes = Vec::new();
            if let Some((table, taken)) = crate::table::Table::from_markdown_noting(lines, &mut notes) {
                if lead == 0 {
                    found.notes.extend(notes);
                }
                flush(&mut out, &mut paragraph, &mut quote, para_indent);
                item = None;
                let mut block = Block::new(BlockKind::Table, table.plain_text());
                block.data = table.to_data();
                block.place.beside = table.beside;
                out.push(block);
                if table.page.is_some() {
                    found.said.push(table);
                }
                skip = taken - lead - 1;
                let cut = taken == crate::table::MOST_ROWS + 1 && lines.get(taken).is_some_and(|l| l.trim_start().starts_with('|'));
                more_rows = cut.then(|| (at + taken - lead, lines[0], lines[1]));
                continue;
            }
        }
        if let Some((lang, lines)) = code.as_mut() {
            let ends = if fence > 3 { raw.trim().len() >= fence && raw.trim().bytes().all(|b| b == b'`') } else { raw.trim_start().starts_with("```") };
            if ends {
                let text = lines.join("\n");
                let is_chart = lang.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("chart"));
                let chart = is_chart.then(|| crate::chart::Chart::from_markdown(&text)).flatten();
                if is_chart && chart.is_none() {
                    found.notes.push(crate::chart::Chart::why_not(&text));
                }
                match chart {
                    Some(chart) => {
                        let mut b = Block::new(BlockKind::Chart, chart.title.trim());
                        b.data = chart.to_data();
                        out.push(b);
                    }
                    None => {
                        let mut b = Block::new(BlockKind::Code, text);
                        b.language = lang.take();
                        out.push(b);
                    }
                }
                code = None;
            } else {
                lines.push(raw.to_string());
            }
            continue;
        }

        // (A tab is one step in, as two spaces are.)
        let leading: usize = raw.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 2 } else { 1 }).sum();
        let indent = (leading / 2).min(4) as u8;
        let line = raw.trim();

        if let Some(starts) = column_mark(line) {
            flush(&mut out, &mut paragraph, &mut quote, para_indent);
            item = None;
            marks.push((out.len(), starts));
            continue;
        }
        if line.starts_with("```") {
            flush(&mut out, &mut paragraph, &mut quote, para_indent);
            item = None;
            fence = line.bytes().take_while(|b| *b == b'`').count();
            let lang = line[fence..].trim();
            code = Some(((!lang.is_empty()).then(|| lang.to_string()), Vec::new()));
            continue;
        }
        if line.is_empty() {
            flush(&mut out, &mut paragraph, &mut quote, para_indent);
            item = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix('>') {
            item = None;
            if !paragraph.is_empty() {
                flush(&mut out, &mut paragraph, &mut quote, para_indent);
            }
            quote.push(rest.trim_start().to_string());
            continue;
        }

        let block = if line == "---" || line == "***" || line == "___" {
            Some((Block::new(BlockKind::Divider, ""), ""))
        } else if let Some(rest) = line.strip_prefix("#### ").or_else(|| line.strip_prefix("### ")) {
            Some((Block::new(BlockKind::Heading3, clean_inline(rest)), rest))
        } else if let Some(rest) = line.strip_prefix("## ") {
            Some((Block::new(BlockKind::Heading2, clean_inline(rest)), rest))
        } else if let Some(rest) = line.strip_prefix("# ") {
            Some((Block::new(BlockKind::Heading1, clean_inline(rest)), rest))
        } else if let Some((checked, rest)) = todo_item(line) {
            let (text, due, remind) = todo_parts(rest);
            let mut b = Block::new(BlockKind::Todo, text).with_indent(indent);
            b.checked = checked;
            b.due = due;
            b.remind = remind;
            Some((b, rest))
        } else if let Some(rest) = ["- ", "* ", "+ ", "• "].iter().find_map(|p| line.strip_prefix(p)) {
            // A toggle: an item that starts with its triangle (see `TOGGLE_MARK`).
            match TOGGLE_MARKS.iter().find_map(|m| rest.strip_prefix(m)) {
                Some(words) => Some((Block::new(BlockKind::Toggle, clean_inline(words.trim_start())).with_indent(indent), words.trim_start())),
                None => Some((Block::new(BlockKind::Bulleted, clean_inline(unguarded(rest))).with_indent(indent), unguarded(rest))),
            }
        } else {
            numbered_prefix(line)
                .map(|len| (Block::new(BlockKind::Numbered, clean_inline(&line[len..])).with_indent(indent), &line[len..]))
        };

        match block {
            Some((mut b, rest)) => {
                flush(&mut out, &mut paragraph, &mut quote, para_indent);
                // (A heading stands as deep as the line it is written under
                // allows, as a paragraph does: see below.)
                if !each_line && heading_level(b.kind).is_some() {
                    b.indent = indent.min(out.last().map_or(0, |l| l.indent.saturating_add(1)));
                }
                if b.kind != BlockKind::Divider {
                    item = Some((out.len(), leading, rest.to_string()));
                }
                out.push(b);
            }
            None => {
                // More words of the list item or heading above, on a line
                // of their own with more room before it (a line break in it,
                // typed with Shift-Return; see the writer).
                if let Some((at, _, words)) = item.as_mut().filter(|(_, room, _)| leading > *room && !each_line && paragraph.is_empty() && quote.is_empty()) {
                    words.push('\n');
                    words.push_str(line);
                    let b = &mut out[*at];
                    match b.kind {
                        BlockKind::Todo => (b.text, b.due, b.remind) = todo_parts(words),
                        _ => b.text = clean_inline(words),
                    }
                    continue;
                }
                item = None;
                if !quote.is_empty() || (each_line && !paragraph.is_empty()) {
                    flush(&mut out, &mut paragraph, &mut quote, para_indent);
                }
                // A paragraph with room before it stands under the line
                // above it, as deep as that allows (the writer puts the room
                // of its depth before it, as before a list item). (In pasted
                // words each line is a line of the page, at the edge.)
                if paragraph.is_empty() {
                    para_indent = if each_line { 0 } else { indent.min(out.last().map_or(0, |l| l.indent.saturating_add(1))) };
                }
                if line.starts_with("<!--") && line.ends_with("-->") {
                    found.notes.push(format!("The line “{line}” is under no table, so it shows as words on the page. A table's comment lines go right under its last row."));
                }
                paragraph.push(line.to_string());
            }
        }
    }
    if let Some((lang, lines)) = code {
        let mut b = Block::new(BlockKind::Code, lines.join("\n"));
        b.language = lang;
        out.push(b);
    }
    flush(&mut out, &mut paragraph, &mut quote, para_indent);
    if !marks.is_empty() {
        put_in_columns(&mut out, &marks);
    }
    out
}

/// A quote that is a callout (a box with an icon): its first line starts
/// with the icon in `[!…]` (as the writer writes it: `> [!lightbulb:yellow]
/// Book early`; GitHub's and Obsidian's `[!TIP]`, `[!WARNING]` too) or with
/// an emoji Rune has an icon for (`> 💡 Book early`). The icon, as a page
/// keeps it, and the words. (A callout came back as a quote, and the AI
/// could make none.)
fn callout_of(lines: &[String]) -> Option<(String, String)> {
    let first = lines.first()?.trim_start();
    let (icon, rest) = match first.strip_prefix("[!").and_then(|r| r.split_once(']')) {
        Some((said, rest)) => (callout_icon(said.trim()), rest),
        None => {
            let (word, rest) = first.split_once(char::is_whitespace).unwrap_or((first, ""));
            let emoji = !word.is_empty() && word.chars().all(|c| !c.is_alphanumeric() && !c.is_ascii());
            (emoji.then(|| crate::ui::icons::known_icon(word)).flatten().map(|(n, c)| crate::ui::icons::format_icon(&n, &c))?, rest)
        }
    };
    let text: Vec<&str> = std::iter::once(rest.trim()).filter(|t| !t.is_empty()).chain(lines[1..].iter().map(String::as_str)).collect();
    Some((icon, text.join("\n")))
}

/// The icon a callout's `[!…]` says: an icon of Rune's (`lightbulb:yellow`,
/// or an emoji it has), or a kind of GitHub's and Obsidian's; else Rune's
/// first icon for a callout.
fn callout_icon(said: &str) -> String {
    let low = said.to_lowercase();
    let kind = match low.as_str() {
        "note" | "info" => Some("info:blue"),
        "tip" | "hint" => Some("lightbulb:yellow"),
        "important" => Some("star:purple"),
        "warning" | "attention" => Some("warning:orange"),
        "caution" | "danger" | "error" => Some("warning:red"),
        "success" | "check" | "done" => Some("check-circle:green"),
        "question" | "faq" => Some("chat:blue"),
        _ => None,
    };
    kind.map(str::to_string)
        .or_else(|| crate::ui::icons::known_icon(&low).map(|(n, c)| crate::ui::icons::format_icon(&n, &c)))
        .unwrap_or_else(|| crate::model::CALLOUT_ICON.to_string())
}

/// The line that starts each column of lines that stand side by side, and
/// the line that ends the set (see `columns`):
///
/// ```text
/// <!-- column -->
/// ## Monday
/// <!-- column -->
/// ## Tuesday
/// <!-- end columns -->
/// ```
pub const COLUMN: &str = "<!-- column -->";
pub const COLUMNS_END: &str = "<!-- end columns -->";

/// Whether a line is a mark of columns: `Some(true)` for one that starts a
/// column, `Some(false)` for one that ends the set. Read with some give
/// ("column 2", "/columns"), as an AI tool may write them.
fn column_mark(line: &str) -> Option<bool> {
    let words = line.strip_prefix("<!--")?.strip_suffix("-->")?.trim().to_lowercase();
    let words = words.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        ["column" | "columns"] => Some(true),
        ["column", n] if n.chars().all(|c| c.is_ascii_digit()) => Some(true),
        ["end", "column" | "columns"] | ["columns" | "column", "end"] | ["/column" | "/columns"] => Some(false),
        _ => None,
    }
}

/// Give each block the column the marks put it in (see `column_mark`), as
/// a page keeps it (`Place::col`, see `columns`).
fn put_in_columns(blocks: &mut [Block], marks: &[(usize, bool)]) {
    let mut col = 0u8;
    let mut next = marks.iter().peekable();
    for (i, b) in blocks.iter_mut().enumerate() {
        while let Some(&(_, starts)) = next.next_if(|(at, _)| *at <= i) {
            col = if starts { (col + 1).min(crate::columns::MOST) } else { 0 };
        }
        b.place.col = col;
    }
    crate::columns::tidy(blocks);
}

/// A toggle (a line whose lines under it fold away) is a list item that
/// starts with a triangle: "- ▸ What is Rune?", its lines indented under
/// it. (Markdown had none: a toggle came back as a bullet, and the AI
/// could make none.) The other triangles are read too.
pub const TOGGLE_MARK: &str = "▸";
const TOGGLE_MARKS: [&str; 4] = ["▸ ", "▶ ", "▸\u{FE0F} ", "▶\u{FE0F} "];

/// The words of a quote or a list item as the writer keeps them from
/// being read as more than that: a quote whose words start with an emoji
/// Rune has an icon for (`> 💡 idea`) was read back as a callout, and an
/// item that starts with a triangle (`- ▸ Next`) as a toggle; so the
/// writer puts a backslash before it (see `render`).
fn guarded(text: &str, reads_as_more: bool) -> String {
    if reads_as_more { format!("\\{text}") } else { text.to_string() }
}

/// The words with that backslash taken away. (Before any other sign that
/// is not ASCII: in the stored form a backslash of the person's is
/// written twice.)
fn unguarded(text: &str) -> &str {
    match text.strip_prefix('\\') {
        Some(rest) if rest.chars().next().is_some_and(|c| !c.is_ascii()) => rest,
        _ => text,
    }
}

/// The sign before the day a to-do is due, at the end of its line:
/// "- [ ] Call Sam 📅 2026-11-12". Other note apps write it the same way.
pub const DUE_MARK: &str = "📅";

/// The sign before a to-do's reminder time, after its day.
pub const REMIND_MARK: &str = "⏰";

/// A to-do's words and day, and the time of its reminder when the line ends
/// with one ("… 📅 2026-11-12 ⏰ 09:00").
fn split_remind(text: &str) -> (&str, Option<chrono::NaiveTime>) {
    let Some(at) = text.rfind(REMIND_MARK) else { return (text, None) };
    match Block::remind_from_column(&text[at + REMIND_MARK.len()..]) {
        Some(time) => (text[..at].trim_end(), Some(time)),
        None => (text, None),
    }
}

/// A to-do's words, and the day it is due when its line ends with one.
fn split_due(text: &str) -> (&str, Option<chrono::NaiveDate>) {
    let Some(at) = text.rfind(DUE_MARK) else { return (text, None) };
    match Block::due_from_column(&text[at + DUE_MARK.len()..]) {
        Some(day) => (text[..at].trim_end(), Some(day)),
        None => (text, None),
    }
}

/// A to-do's words as written after its box: its words, and the day and
/// the reminder at their end.
fn todo_parts(rest: &str) -> (String, Option<chrono::NaiveDate>, Option<chrono::NaiveTime>) {
    let (words, remind) = split_remind(rest);
    let (words, due) = split_due(words);
    // (A time with no day before it is no reminder: the words stay as written.)
    let (words, due) = if remind.is_some() && due.is_none() { split_due(rest) } else { (words, due) };
    // (A time with no day is no reminder: it needs a day.)
    (clean_inline(words), due, remind.filter(|_| due.is_some()))
}

fn todo_item(line: &str) -> Option<(bool, &str)> {
    for bullet in ["- ", "* "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            if let Some(r) = rest.strip_prefix("[ ] ") {
                return Some((false, r));
            }
            if let Some(r) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
                return Some((true, r));
            }
        }
    }
    None
}

/// Keep the reply's bold, italic, code and links, written in the stored form.
fn clean_inline(s: &str) -> String {
    crate::inline::canonical(s)
}

/// Render a page as Markdown (used as AI context and for "Copy as Markdown").
pub fn from_blocks(title: &str, blocks: &[Block], link_title: impl Fn(&str) -> String) -> String {
    render(title, blocks, |b| match b.kind {
        BlockKind::PageLink => Some(format!("[{}]", link_title(b.target.as_deref().unwrap_or("")))),
        _ => None,
    })
}

/// Render a page as Markdown. `special` may write a block's whole line itself
/// (an export writes page links and drawings as links to files).
pub fn render(title: &str, blocks: &[Block], mut special: impl FnMut(&Block) -> Option<String>) -> String {
    let mut out = String::new();
    if !title.trim().is_empty() {
        out.push_str(&format!("# {}\n\n", title.trim()));
    }
    let numbers = crate::model::list_numbers(blocks);
    // Lines side by side: a mark before each column, and one after the set.
    let layout = crate::columns::layout(blocks);
    let mut was: Option<(usize, usize)> = None;
    for ((b, n), at) in blocks.iter().zip(numbers).zip(layout) {
        if was.is_some_and(|(set, _)| at.map(|(s, _)| s) != Some(set)) {
            out.push_str(COLUMNS_END);
            out.push_str("\n\n");
        }
        if at.is_some() && at != was {
            out.push_str(COLUMN);
            out.push_str("\n\n");
        }
        was = at;
        let pad = "  ".repeat(b.indent as usize);
        // (The lines of a list item or a heading after its first, with room
        // before them: they are read back as more of its words, not as a
        // paragraph of their own.)
        let more = |text: &str| text.replace('\n', &format!("\n{pad}  "));
        let line = if let Some(line) = special(b) {
            line
        } else {
            match b.kind {
            BlockKind::Text => format!("{pad}{}", b.text),
            BlockKind::Heading1 => format!("## {}", more(&b.text)),
            BlockKind::Heading2 => format!("### {}", more(&b.text)),
            BlockKind::Heading3 => format!("#### {}", more(&b.text)),
            BlockKind::Bulleted => format!("{pad}- {}", guarded(&more(&b.text), TOGGLE_MARKS.iter().any(|m| b.text.starts_with(m)))),
            BlockKind::Toggle => format!("{pad}- {TOGGLE_MARK} {}", more(&b.text)),
            BlockKind::Numbered => format!("{pad}{}. {}", n.unwrap_or(1), more(&b.text)),
            BlockKind::Todo => {
                let due = b.due.map(|d| format!(" {DUE_MARK} {}", d.format("%Y-%m-%d"))).unwrap_or_default()
                    + &b.remind.filter(|_| b.due.is_some()).map(|t| format!(" {REMIND_MARK} {}", t.format("%H:%M"))).unwrap_or_default();
                format!("{pad}- [{}] {}{due}", if b.checked { "x" } else { " " }, more(&b.text))
            }
            BlockKind::Quote => {
                let as_callout = b.text.chars().next().is_some_and(|c| !c.is_ascii()) && callout_of(&[b.text.lines().next().unwrap_or_default().to_string()]).is_some();
                guarded(&b.text, as_callout).lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")
            }
            // With its icon, so that it is read back as a callout (see
            // `callout_of`), and each line with its mark: a second line with
            // none was read back as a paragraph of its own.
            BlockKind::Callout => {
                let icon = b.icon.as_deref().map(str::trim).filter(|i| !i.is_empty()).unwrap_or(crate::model::CALLOUT_ICON);
                format!("> [!{}] {}", icon.replace([']', '\n'], ""), b.text.split('\n').collect::<Vec<_>>().join("\n> "))
            }
            // (A fence longer than any run of backticks that begins a line of
            // the code: else such a line ended the block when it was read.)
            BlockKind::Code => {
                let longest = b.text.lines().map(|l| l.trim_start().bytes().take_while(|b| *b == b'`').count()).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                format!("{fence}{}\n{}\n{fence}", b.language.as_deref().unwrap_or(""), b.text)
            }
            BlockKind::Divider => "---".to_string(),
            BlockKind::Drawing => format!("{pad}[Drawing]"),
            BlockKind::Image if b.text.trim().is_empty() => format!("{pad}[Image]"),
            BlockKind::Image => format!("{pad}[Image: {}]", b.text.trim()),
            BlockKind::Table => table_markdown(b, &pad),
            BlockKind::Chart => crate::chart::Chart::parse(&b.data).to_markdown(),
            BlockKind::PageLink => "[Page]".to_string(),
            // (What is in a locked page is kept from anything that reads it.)
            BlockKind::Locked => "[Locked page: it opens only in Rune, with its password]".to_string(),
            }
        };
        out.push_str(&line);
        out.push_str("\n\n");
    }
    if was.is_some() {
        out.push_str(COLUMNS_END);
    }
    out.trim_end().to_string()
}

/// A table block as Markdown. That it stands beside the table above it is
/// kept with the block, and said on the table's comment line.
pub fn table_markdown(b: &Block, pad: &str) -> String {
    let mut table = crate::table::Table::parse(&b.data);
    table.beside = b.place.beside;
    table.to_markdown(pad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_lines_are_lines_of_the_page() {
        let words = |bs: &[Block]| bs.iter().map(|b| (b.kind, b.indent, b.text.clone())).collect::<Vec<_>>();
        assert_eq!(words(&to_blocks_by_line("Alice\nBob\nCarol")), vec![(BlockKind::Text, 0, "Alice".to_string()), (BlockKind::Text, 0, "Bob".to_string()), (BlockKind::Text, 0, "Carol".to_string())]);
        // In a Markdown file they are one paragraph, as before.
        assert_eq!(to_blocks("Alice\nBob").len(), 1);
        // A list nested with a tab keeps its step.
        assert_eq!(words(&to_blocks_by_line("- one\n\t- two")), vec![(BlockKind::Bulleted, 0, "one".to_string()), (BlockKind::Bulleted, 1, "two".to_string())]);
        // Code and tables stay whole.
        let code = to_blocks_by_line("```\na\nb\n```\nafter");
        assert_eq!((code.len(), code[0].kind, code[0].text.as_str()), (2, BlockKind::Code, "a\nb"));
    }

    #[test]
    fn code_that_holds_a_fence_is_read_back_whole() {
        let mut code = Block::new(BlockKind::Code, "Write it so:\n```js\nlet a = 1;\n```\nand so on.");
        code.language = Some("md".to_string());
        let blocks = vec![Block::text("before"), code.clone(), Block::text("after")];
        let md = from_blocks("", &blocks, |t| t.to_string());
        let back = to_blocks(&md);
        assert_eq!(back.iter().map(|b| (b.kind, b.text.as_str())).collect::<Vec<_>>(), vec![(BlockKind::Text, "before"), (BlockKind::Code, code.text.as_str()), (BlockKind::Text, "after")], "{md}");
        assert_eq!(back[1].language.as_deref(), Some("md"));
        // Plain code keeps the fence of three.
        assert!(from_blocks("", &[Block::new(BlockKind::Code, "x")], |t| t.to_string()).contains("```\nx\n```"));
    }

    #[test]
    fn a_chart_and_a_typed_table_there_and_back() {
        let md = "| Date | km |\n| --- | --- |\n| Tue 1 Dec | 6 |\n<!-- columns: date | number km sum -->\n\n```chart\n{\"shape\": \"bars\", \"y\": \"km\", \"by\": \"week\"}\n```\n\n```rust\nfn main() {}\n```";
        let blocks = to_blocks(md);
        let kinds: Vec<BlockKind> = blocks.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [BlockKind::Table, BlockKind::Chart, BlockKind::Code]);
        let table = crate::table::Table::parse(&blocks[0].data);
        assert_eq!(table.kind(1), crate::table::Kind::Number);
        assert_eq!(table.total(1), Some(("Sum", "6".to_string())));
        assert_eq!(crate::chart::Chart::parse(&blocks[1].data).y, "km");
        let back = from_blocks("", &blocks, |_| String::new());
        assert_eq!(to_blocks(&back).iter().map(|b| (b.kind, b.data.clone())).collect::<Vec<_>>(), blocks.iter().map(|b| (b.kind, b.data.clone())).collect::<Vec<_>>());
        // A chart block that is not a chart stays code.
        assert_eq!(to_blocks("```chart\nnot json\n```")[0].kind, BlockKind::Code);
    }

    #[test]
    fn a_named_table_beside_another_there_and_back() {
        let md = "| Item | Amount |\n| --- | --- |\n| Salary | $3,500.00 |\n<!-- columns: text | number USD sum -->\n<!-- table: Income | icon plus:green -->\n\n| Item | Amount |\n| --- | --- |\n| Rent | $2,000.00 |\n<!-- columns: text | number USD sum -->\n<!-- table: Expenses | beside -->\n\nAfter";
        let blocks = to_blocks(md);
        assert_eq!(blocks.iter().map(|b| b.kind).collect::<Vec<_>>(), [BlockKind::Table, BlockKind::Table, BlockKind::Text]);
        assert_eq!((blocks[0].place.beside, blocks[1].place.beside), (false, true));
        let (income, expenses) = (crate::table::Table::parse(&blocks[0].data), crate::table::Table::parse(&blocks[1].data));
        assert_eq!((income.name.as_str(), income.icon.as_str(), expenses.name.as_str()), ("Income", "plus:green", "Expenses"));
        assert_eq!(expenses.total(1), Some(("Sum", "$2,000.00".to_string())));
        // The name of a table is in the words of its block, for search.
        assert!(blocks[1].text.starts_with("Expenses"));
        // (That it stands beside is kept with the block, not in the table.)
        assert!(!blocks[1].data.contains("beside"));
        let back = from_blocks("", &blocks, |_| String::new());
        assert!(back.contains("<!-- table: Expenses | beside -->") && back.contains("<!-- table: Income | icon plus:green -->"), "{back}");
        let again = to_blocks(&back);
        assert_eq!(again.iter().map(|b| (b.kind, b.data.clone(), b.place)).collect::<Vec<_>>(), blocks.iter().map(|b| (b.kind, b.data.clone(), b.place)).collect::<Vec<_>>());
    }

    #[test]
    fn lines_with_a_line_break_and_lines_inside_others_come_back_whole() {
        let mut todo = Block::new(BlockKind::Todo, "Pack\nthe bags");
        todo.due = chrono::NaiveDate::from_ymd_opt(2026, 11, 20);
        let mut callout = Block::new(BlockKind::Callout, "Two\nlines");
        callout.icon = Some("\u{1F4A1}".into());
        let blocks = vec![
            Block::new(BlockKind::Bulleted, "A bullet\non two lines"),
            Block::new(BlockKind::Bulleted, "Nested").with_indent(1),
            Block::text("Words under it").with_indent(2),
            todo,
            callout,
            Block::new(BlockKind::Heading2, "A heading\nbroken"),
            Block::text("At the edge"),
        ];
        let md = from_blocks("", &blocks, |_| String::new());
        let back = to_blocks(&md);
        let seen = |bs: &[Block]| bs.iter().map(|b| (b.kind, b.text.clone(), b.indent, b.due)).collect::<Vec<_>>();
        let mut want = seen(&blocks);
        // (A callout comes back as a callout, with its icon: see `callout_of`.)
        assert_eq!(back[4].icon.as_deref(), Some("lightbulb:yellow"));
        // (This writer puts headings a step down: the page's name is its `#`.)
        want[5].0 = BlockKind::Heading3;
        assert_eq!(seen(&back), want, "{md}");
        // A paragraph stands at most one step under the line above it; pasted lines are at the edge.
        assert_eq!(to_blocks("Top\n\n      Deep").iter().map(|b| b.indent).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(to_blocks_by_line("  one\n  two").iter().map(|b| b.indent).collect::<Vec<_>>(), [0, 0]);
        // A mark of the order of bytes first is no word.
        assert_eq!(to_blocks("\u{FEFF}# Plan")[0].text, "Plan");
    }

    #[test]
    fn shortcuts_are_detected() {
        assert_eq!(detect_shortcut("# ").unwrap().kind, BlockKind::Heading1);
        assert_eq!(detect_shortcut("### x").unwrap().kind, BlockKind::Heading3);
        assert_eq!(detect_shortcut("- ").unwrap().kind, BlockKind::Bulleted);
        assert_eq!(detect_shortcut("12. ").unwrap(), Shortcut { kind: BlockKind::Numbered, prefix_len: 4, checked: false });
        assert!(detect_shortcut("[x] ").unwrap().checked);
        assert_eq!(detect_shortcut("> ").unwrap().kind, BlockKind::Toggle);
        assert_eq!(detect_shortcut("\" ").unwrap().kind, BlockKind::Quote);
        assert_eq!(detect_shortcut("```").unwrap().kind, BlockKind::Code);
        assert_eq!(detect_shortcut("---").unwrap().kind, BlockKind::Divider);
        assert!(detect_shortcut("hello").is_none());
        assert!(detect_shortcut("1.5 apples").is_none());
    }

    #[test]
    fn a_link_to_a_page_starts_with_an_at_sign_or_two_brackets() {
        assert_eq!(mention_query("@"), Some((0, false, String::new())));
        assert_eq!(mention_query("see @Read"), Some((4, false, "Read".into())));
        assert_eq!(mention_query("see @Reading List"), Some((4, false, "Reading List".into())));
        assert_eq!(mention_query("(@Apollo"), Some((1, false, "Apollo".into())));
        assert_eq!(mention_query("[[Meeting notes"), Some((0, true, "Meeting notes".into())));
        assert_eq!(mention_query("as in [["), Some((6, true, String::new())));
        // The later of the two counts.
        assert_eq!(mention_query("[[a @b"), Some((4, false, "b".into())));
        assert_eq!(mention_query("@a [[b"), Some((3, true, "b".into())));
        // Not a link being typed.
        assert_eq!(mention_query("mail jack@example.com"), None);
        assert_eq!(mention_query("meet @ 5 pm"), None);
        assert_eq!(mention_query("@one two three four five"), None);
        assert_eq!(mention_query("[[done]] and more"), None);
        assert_eq!(mention_query("plain words"), None);
        assert_eq!(mention_query("an é@name"), None);
        assert_eq!(mention_query("é @name"), Some((3, false, "name".into())));
    }

    #[test]
    fn slash_query_needs_word_boundary() {
        assert_eq!(slash_query("/"), Some((0, String::new())));
        assert_eq!(slash_query("hello /h1"), Some((6, "h1".into())));
        assert_eq!(slash_query("and/or"), None);
        assert_eq!(slash_query("/to do"), None);
    }

    #[test]
    fn markdown_becomes_blocks() {
        let md = "# Plan\n\nIntro line one\nline two\n\n- a\n  - nested\n1. first\n2) second\n- [ ] task\n- [x] done\n\n> quoted\n> more\n\n```rust\nfn main() {}\n```\n---\n**Bold** end";
        let blocks = to_blocks(md);
        let got: Vec<(BlockKind, &str, u8)> =
            blocks.iter().map(|b| (b.kind, b.text.as_str(), b.indent)).collect();
        assert_eq!(
            got,
            vec![
                (BlockKind::Heading1, "Plan", 0),
                (BlockKind::Text, "Intro line one\nline two", 0),
                (BlockKind::Bulleted, "a", 0),
                (BlockKind::Bulleted, "nested", 1),
                (BlockKind::Numbered, "first", 0),
                (BlockKind::Numbered, "second", 0),
                (BlockKind::Todo, "task", 0),
                (BlockKind::Todo, "done", 0),
                (BlockKind::Quote, "quoted\nmore", 0),
                (BlockKind::Code, "fn main() {}", 0),
                (BlockKind::Divider, "", 0),
                (BlockKind::Text, "**Bold** end", 0),
            ]
        );
        assert!(blocks[7].checked);
        assert_eq!(blocks[9].language.as_deref(), Some("rust"));
    }

    #[test]
    fn unterminated_code_fence_is_kept() {
        let blocks = to_blocks("```\nlet x = 1;");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Code);
        assert_eq!(blocks[0].text, "let x = 1;");
    }

    #[test]
    fn a_curly_quote_sign_starts_a_quote_too() {
        // The Mac makes a typed quote sign curly; the shortcut must not need the straight one.
        for typed in ["\" ", "\u{201C} ", "\u{201D} "] {
            let got = detect_shortcut(typed).expect(typed);
            assert_eq!((got.kind, got.prefix_len), (BlockKind::Quote, typed.len()));
        }
    }

    #[test]
    fn a_to_do_keeps_its_day_through_markdown() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let mut todo = Block::new(BlockKind::Todo, "Call Sam");
        todo.due = Some(day);
        let md = from_blocks("Plan", &[todo, Block::new(BlockKind::Todo, "No day")], |_| String::new());
        assert_eq!(md, "# Plan\n\n- [ ] Call Sam \u{1F4C5} 2026-11-12\n\n- [ ] No day");
        let back = to_blocks("- [ ] Call Sam \u{1F4C5} 2026-11-12\n- [x] Done one \u{1F4C5}2026-11-13\n- [ ] No day\n- [ ] Not a day \u{1F4C5} soon");
        assert_eq!((back[0].text.as_str(), back[0].due), ("Call Sam", Some(day)));
        assert_eq!((back[1].text.as_str(), back[1].due, back[1].checked), ("Done one", day.succ_opt(), true));
        assert_eq!((back[2].text.as_str(), back[2].due), ("No day", None));
        // A sign with no day after it is only text.
        assert_eq!((back[3].text.as_str(), back[3].due), ("Not a day \u{1F4C5} soon", None));
    }

    #[test]
    fn a_to_do_keeps_its_reminder_through_markdown() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let nine = chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let mut todo = Block::new(BlockKind::Todo, "Call Sam");
        todo.due = Some(day);
        todo.remind = Some(nine);
        let md = from_blocks("Plan", &[todo], |_| String::new());
        assert_eq!(md, "# Plan\n\n- [ ] Call Sam \u{1F4C5} 2026-11-12 \u{23F0} 09:00");
        let back = to_blocks("- [ ] Call Sam \u{1F4C5} 2026-11-12 \u{23F0} 09:00\n- [ ] At 2 \u{1F4C5} 2026-11-12 \u{23F0} 14:30\n- [ ] No day \u{23F0} 09:00\n- [ ] Not a time \u{1F4C5} 2026-11-12 \u{23F0} soon");
        assert_eq!((back[0].text.as_str(), back[0].due, back[0].remind), ("Call Sam", Some(day), Some(nine)));
        assert_eq!(back[1].remind, chrono::NaiveTime::from_hms_opt(14, 30, 0));
        // A time needs a day; a sign with no time after it is only text (and the day is not read past it).
        assert_eq!((back[2].text.as_str(), back[2].remind, back[2].due), ("No day \u{23F0} 09:00", None, None));
        assert_eq!(back[3].remind, None);
        // A done to-do sends no reminder.
        let mut done = back[0].clone();
        assert!(done.remind_at().is_some());
        done.checked = true;
        assert_eq!(done.remind_at(), None);
    }

    #[test]
    fn a_diagram_goes_through_markdown_as_it_is() {
        let md = "```mermaid\nflowchart LR\n  A[Plan] --> B[Enter]\n  B -. back .-> A\n```";
        let blocks = to_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!((blocks[0].kind, blocks[0].language.as_deref()), (BlockKind::Code, Some("mermaid")));
        assert_eq!(blocks[0].text, "flowchart LR\n  A[Plan] --> B[Enter]\n  B -. back .-> A");
        assert!(from_blocks("Steps", &blocks, |id| id.to_string()).contains(md), "{}", from_blocks("Steps", &blocks, |id| id.to_string()));
    }

    #[test]
    fn page_renders_to_markdown() {
        let mut todo = Block::new(BlockKind::Todo, "ship");
        todo.checked = true;
        let blocks = vec![Block::new(BlockKind::Heading1, "Goals"), todo, Block::new(BlockKind::Numbered, "one")];
        let md = from_blocks("Plan", &blocks, |_| String::new());
        assert_eq!(md, "# Plan\n\n## Goals\n\n- [x] ship\n\n1. one");
    }

    #[test]
    fn lines_side_by_side_go_to_markdown_and_back() {
        let mut blocks = vec![Block::text("Top"), Block::new(BlockKind::Heading2, "Left"), Block::new(BlockKind::Bulleted, "a"), Block::new(BlockKind::Heading2, "Right"), Block::new(BlockKind::Todo, "b"), Block::text("Under")];
        for (i, col) in [(1, 1), (2, 1), (3, 2), (4, 2)] {
            blocks[i].place.col = col;
        }
        let md = from_blocks("", &blocks, |t| t.to_string());
        assert_eq!(md, "Top\n\n<!-- column -->\n\n### Left\n\n- a\n\n<!-- column -->\n\n### Right\n\n- [ ] b\n\n<!-- end columns -->\n\nUnder");
        let back = to_blocks(&md);
        assert_eq!(back.iter().map(|b| (b.plain_text(), b.place.col)).collect::<Vec<_>>(), blocks.iter().map(|b| (b.plain_text(), b.place.col)).collect::<Vec<_>>());
        // A page that ends in a set ends with the mark; one column alone is no set.
        let md = from_blocks("", &blocks[..5], |t| t.to_string());
        assert!(md.ends_with("- [ ] b\n\n<!-- end columns -->"), "{md}");
        assert!(to_blocks("<!-- column -->\nOnly one\n<!-- end columns -->").iter().all(|b| b.place.col == 0));
        // A fourth column goes in the third; a mark that is not one shows as it is.
        let four = to_blocks("<!-- column -->\nA\n<!-- column -->\nB\n<!-- column -->\nC\n<!-- column -->\nD");
        assert_eq!(four.iter().map(|b| b.place.col).collect::<Vec<_>>(), [1, 2, 3, 3]);
        assert_eq!(to_blocks("<!-- columns: text | date -->")[0].text, "<!-- columns: text | date -->");
    }

    #[test]
    fn a_callout_goes_to_markdown_and_back_with_its_icon() {
        // As an AI writes one: with an emoji Rune has, with a kind of
        // GitHub's, with Rune's own icon; a quote stays a quote.
        let blocks = to_blocks("> 💡 Book early: prices go up.\n\n> [!WARNING]\n> Bring the **passport**.\n> And the visa.\n\n> [!plane:blue] Flight at 9.\n\n> To be or not to be.\n\n> — Hamlet");
        let seen: Vec<(BlockKind, Option<&str>, &str)> = blocks.iter().map(|b| (b.kind, b.icon.as_deref(), b.text.as_str())).collect();
        assert_eq!(
            seen,
            vec![
                (BlockKind::Callout, Some("lightbulb:yellow"), "Book early: prices go up."),
                (BlockKind::Callout, Some("warning:orange"), "Bring the **passport**.\nAnd the visa."),
                (BlockKind::Callout, Some("plane:blue"), "Flight at 9."),
                (BlockKind::Quote, None, "To be or not to be."),
                (BlockKind::Quote, None, "— Hamlet"),
            ]
        );
        // Written, each says its icon, and read back it is the same.
        let md = from_blocks("", &blocks, |t| t.to_string());
        assert!(md.starts_with("> [!lightbulb:yellow] Book early: prices go up.\n\n> [!warning:orange] Bring the **passport**.\n> And the visa.\n\n> [!plane:blue] Flight at 9."), "{md}");
        let back = to_blocks(&md);
        assert_eq!(back.iter().map(|b| (b.kind, b.icon.clone(), b.text.clone())).collect::<Vec<_>>(), blocks.iter().map(|b| (b.kind, b.icon.clone(), b.text.clone())).collect::<Vec<_>>());
        // A callout of before, with no icon kept, or an emoji kept as its icon.
        let mut old = Block::new(BlockKind::Callout, "Old one");
        old.icon = None;
        let mut legacy = Block::new(BlockKind::Callout, "Older one");
        legacy.icon = Some("📌".into());
        let back = to_blocks(&from_blocks("", &[old, legacy], |t| t.to_string()));
        assert_eq!(back.iter().map(|b| (b.kind, b.icon.as_deref())).collect::<Vec<_>>(), vec![(BlockKind::Callout, Some("lightbulb:yellow")), (BlockKind::Callout, Some("pin:red"))]);
    }

    #[test]
    fn a_toggle_goes_to_markdown_and_back() {
        // An FAQ as an AI writes it: each question a toggle, its answer under it.
        let blocks = to_blocks("- ▸ What is Rune?\n  - A notes app.\n- ▶ Does it sync?\n  - [ ] Ask Sam\n- A plain bullet");
        let seen: Vec<(BlockKind, u8, &str)> = blocks.iter().map(|b| (b.kind, b.indent, b.text.as_str())).collect();
        assert_eq!(seen, vec![(BlockKind::Toggle, 0, "What is Rune?"), (BlockKind::Bulleted, 1, "A notes app."), (BlockKind::Toggle, 0, "Does it sync?"), (BlockKind::Todo, 1, "Ask Sam"), (BlockKind::Bulleted, 0, "A plain bullet")]);
        let md = from_blocks("", &blocks, |t| t.to_string());
        assert!(md.starts_with("- ▸ What is Rune?\n\n  - A notes app.\n\n- ▸ Does it sync?"), "{md}");
        let back = to_blocks(&md);
        assert_eq!(back.iter().map(|b| (b.kind, b.indent, b.text.clone())).collect::<Vec<_>>(), blocks.iter().map(|b| (b.kind, b.indent, b.text.clone())).collect::<Vec<_>>());
    }
}
