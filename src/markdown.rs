//! Markdown in and out: typing shortcuts at the start of a block, turning AI
//! replies into blocks, and turning a page into text for AI context.

use crate::model::{Block, BlockKind};

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

/// Convert Markdown (for example an AI reply) into blocks.
pub fn to_blocks(md: &str) -> Vec<Block> {
    read(md, false)
}

/// As `to_blocks`, for words that are pasted: each line is a line of the
/// page. (In a Markdown file, lines one under the other are one paragraph;
/// a list of names copied from another program is not.)
pub fn to_blocks_by_line(md: &str) -> Vec<Block> {
    read(md, true)
}

fn read(md: &str, each_line: bool) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut quote: Vec<String> = Vec::new();
    let mut code: Option<(Option<String>, Vec<String>)> = None;
    // How many backticks opened the code block: a longer fence holds lines
    // that begin with three (see the writer), and ends only at its like.
    let mut fence = 3;

    fn flush(out: &mut Vec<Block>, paragraph: &mut Vec<String>, quote: &mut Vec<String>) {
        if !paragraph.is_empty() {
            out.push(Block::text(clean_inline(&paragraph.join("\n"))));
            paragraph.clear();
        }
        if !quote.is_empty() {
            out.push(Block::new(BlockKind::Quote, clean_inline(&quote.join("\n"))));
            quote.clear();
        }
    }

    let all: Vec<&str> = md.lines().collect();
    let mut skip = 0usize;
    for (at, raw) in all.iter().copied().enumerate() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        // A table: its header, the line of dashes, then its rows.
        if code.is_none() && raw.trim_start().starts_with('|') {
            if let Some((table, taken)) = crate::table::Table::from_markdown(&all[at..]) {
                flush(&mut out, &mut paragraph, &mut quote);
                let mut block = Block::new(BlockKind::Table, table.plain_text());
                block.data = table.to_data();
                block.place.beside = table.beside;
                out.push(block);
                skip = taken - 1;
                continue;
            }
        }
        if let Some((lang, lines)) = code.as_mut() {
            let ends = if fence > 3 { raw.trim().len() >= fence && raw.trim().bytes().all(|b| b == b'`') } else { raw.trim_start().starts_with("```") };
            if ends {
                let text = lines.join("\n");
                let chart = lang.as_deref().filter(|l| l.eq_ignore_ascii_case("chart")).and_then(|_| crate::chart::Chart::from_markdown(&text));
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

        if line.starts_with("```") {
            flush(&mut out, &mut paragraph, &mut quote);
            fence = line.bytes().take_while(|b| *b == b'`').count();
            let lang = line[fence..].trim();
            code = Some(((!lang.is_empty()).then(|| lang.to_string()), Vec::new()));
            continue;
        }
        if line.is_empty() {
            flush(&mut out, &mut paragraph, &mut quote);
            continue;
        }
        if let Some(rest) = line.strip_prefix('>') {
            if !paragraph.is_empty() {
                flush(&mut out, &mut paragraph, &mut quote);
            }
            quote.push(rest.trim_start().to_string());
            continue;
        }

        let block = if line == "---" || line == "***" || line == "___" {
            Some(Block::new(BlockKind::Divider, ""))
        } else if let Some(rest) = line.strip_prefix("#### ").or_else(|| line.strip_prefix("### ")) {
            Some(Block::new(BlockKind::Heading3, clean_inline(rest)))
        } else if let Some(rest) = line.strip_prefix("## ") {
            Some(Block::new(BlockKind::Heading2, clean_inline(rest)))
        } else if let Some(rest) = line.strip_prefix("# ") {
            Some(Block::new(BlockKind::Heading1, clean_inline(rest)))
        } else if let Some((checked, rest)) = todo_item(line) {
            let (words, remind) = split_remind(rest);
            let (words, due) = split_due(words);
            // (A time with no day before it is no reminder: the words stay as written.)
            let (rest, due) = if remind.is_some() && due.is_none() { split_due(rest) } else { (words, due) };
            let mut b = Block::new(BlockKind::Todo, clean_inline(rest)).with_indent(indent);
            b.checked = checked;
            b.due = due;
            // (A time with no day is no reminder: it needs a day.)
            b.remind = remind.filter(|_| due.is_some());
            Some(b)
        } else if let Some(rest) = ["- ", "* ", "+ ", "• "].iter().find_map(|p| line.strip_prefix(p)) {
            Some(Block::new(BlockKind::Bulleted, clean_inline(rest)).with_indent(indent))
        } else {
            numbered_prefix(line)
                .map(|len| Block::new(BlockKind::Numbered, clean_inline(&line[len..])).with_indent(indent))
        };

        match block {
            Some(b) => {
                flush(&mut out, &mut paragraph, &mut quote);
                out.push(b);
            }
            None => {
                if !quote.is_empty() || (each_line && !paragraph.is_empty()) {
                    flush(&mut out, &mut paragraph, &mut quote);
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
    flush(&mut out, &mut paragraph, &mut quote);
    out
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
    for (b, n) in blocks.iter().zip(numbers) {
        let pad = "  ".repeat(b.indent as usize);
        let line = if let Some(line) = special(b) {
            line
        } else {
            match b.kind {
            BlockKind::Text => format!("{pad}{}", b.text),
            BlockKind::Heading1 => format!("## {}", b.text),
            BlockKind::Heading2 => format!("### {}", b.text),
            BlockKind::Heading3 => format!("#### {}", b.text),
            BlockKind::Bulleted | BlockKind::Toggle => format!("{pad}- {}", b.text),
            BlockKind::Numbered => format!("{pad}{}. {}", n.unwrap_or(1), b.text),
            BlockKind::Todo => {
                let due = b.due.map(|d| format!(" {DUE_MARK} {}", d.format("%Y-%m-%d"))).unwrap_or_default()
                    + &b.remind.filter(|_| b.due.is_some()).map(|t| format!(" {REMIND_MARK} {}", t.format("%H:%M"))).unwrap_or_default();
                format!("{pad}- [{}] {}{due}", if b.checked { "x" } else { " " }, b.text)
            }
            BlockKind::Quote => b.text.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n"),
            // The icon is written only when it is a sign of its own (an
            // emoji), not the app's name for one of its icons ("lightbulb:yellow").
            BlockKind::Callout => match b.icon.as_deref().filter(|i| !i.is_empty() && !i.is_ascii()) {
                Some(icon) => format!("> {icon} {}", b.text),
                None => format!("> {}", b.text),
            },
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
            }
        };
        out.push_str(&line);
        out.push_str("\n\n");
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
}
