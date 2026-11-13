//! Inline formatting inside a block: bold, italic, strikethrough, inline code
//! and links.
//!
//! A block's text is stored as a strict subset of Markdown, so it stays readable
//! on disk and exports cleanly: `**bold**`, `_italic_`, `~~strike~~`,
//! `` `code` `` and `[label](url)`. Every other character with a meaning is
//! escaped with a backslash, so anything a person types round-trips exactly.
//! Bold and italic use different characters so that overlapping formatting is
//! never ambiguous. Markdown from elsewhere (an AI reply, a paste) is read with
//! the usual alternatives (`*italic*`, `__bold__`) and CommonMark's rule that
//! underscores inside a word are literal (`snake_case`).
//!
//! Code blocks are the exception: their text is stored raw, with no escaping.
//!
//! Positions are counted in UTF-16 code units of the visible text, the unit the
//! web view reports caret positions in. `assets/bridge.js` mirrors `to_md` and
//! `to_html`; keep the two in step.

use crate::model::{utf16_len, utf16_to_byte, BlockKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

impl Marks {
    fn and(self, o: Marks) -> Marks {
        Marks {
            bold: self.bold && o.bold,
            italic: self.italic && o.italic,
            strike: self.strike && o.strike,
            code: self.code && o.code,
        }
    }
    const ALL: Marks = Marks { bold: true, italic: true, strike: true, code: true };
}

/// A stretch of text with the same formatting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub marks: Marks,
    pub link: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bold,
    Italic,
    Strike,
}

impl Kind {
    fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Debug)]
enum Tok {
    Text(String),
    /// A run of `*`, `_` or `~` that may open or close formatting.
    Delims { ch: char, len: usize, open: bool, close: bool },
    Code(String),
    Link { label: Vec<Tok>, url: String },
}

fn tokenize(chars: &[char], foreign: bool) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, toks: &mut Vec<Tok>| {
        if !buf.is_empty() {
            toks.push(Tok::Text(std::mem::take(buf)));
        }
    };
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        match c {
            '\\' if i + 1 < n && chars[i + 1].is_ascii_punctuation() => {
                buf.push(chars[i + 1]);
                i += 2;
            }
            '`' => match find_code_end(chars, i + 1) {
                Some(end) => {
                    flush(&mut buf, &mut toks);
                    toks.push(Tok::Code(unescape_code(&chars[i + 1..end])));
                    i = end + 1;
                }
                None => {
                    buf.push(c);
                    i += 1;
                }
            },
            '[' => match find_link(chars, i) {
                Some((label_end, url_end)) => {
                    flush(&mut buf, &mut toks);
                    let label = tokenize(&chars[i + 1..label_end], foreign);
                    let raw: String = chars[label_end + 2..url_end].iter().collect();
                    let url = raw.split_whitespace().next().unwrap_or("").to_string();
                    toks.push(Tok::Link { label, url });
                    i = url_end + 1;
                }
                None => {
                    buf.push(c);
                    i += 1;
                }
            },
            '*' | '_' | '~' => {
                let mut len = 1;
                while i + len < n && chars[i + len] == c {
                    len += 1;
                }
                let prev = (i > 0).then(|| chars[i - 1]);
                let next = chars.get(i + len).copied();
                let mut open = next.is_some_and(|ch| !ch.is_whitespace());
                let mut close = prev.is_some_and(|ch| !ch.is_whitespace());
                let intraword = prev.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
                if foreign && c == '_' && intraword {
                    open = false;
                    close = false;
                }
                let usable = if c == '~' { len == 2 || len == 4 } else { len <= 3 };
                if usable && (open || close) {
                    flush(&mut buf, &mut toks);
                    toks.push(Tok::Delims { ch: c, len, open, close });
                } else {
                    buf.extend(std::iter::repeat_n(c, len));
                }
                i += len;
            }
            _ => {
                buf.push(c);
                i += 1;
            }
        }
    }
    flush(&mut buf, &mut toks);
    toks
}

fn find_code_end(chars: &[char], from: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        match chars[j] {
            '\\' if matches!(chars.get(j + 1), Some('`') | Some('\\')) => j += 2,
            '`' => return Some(j),
            _ => j += 1,
        }
    }
    None
}

fn unescape_code(chars: &[char]) -> String {
    let mut out = String::new();
    let mut j = 0;
    while j < chars.len() {
        if chars[j] == '\\' && matches!(chars.get(j + 1), Some('`') | Some('\\')) {
            out.push(chars[j + 1]);
            j += 2;
        } else {
            out.push(chars[j]);
            j += 1;
        }
    }
    out
}

/// For a `[` at `start`, the index of its `]` and of the `)` closing `(url)`.
fn find_link(chars: &[char], start: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut j = start;
    while j < chars.len() {
        match chars[j] {
            '\\' => {
                j += 2;
                continue;
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        j += 1;
    }
    if j >= chars.len() || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' {
        if chars[k] == '\n' {
            return None;
        }
        k += 1;
    }
    let url: String = chars.get(j + 2..k)?.iter().collect();
    (k < chars.len() && !url.trim().is_empty()).then_some((j, k))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Open,
    Close,
}

/// One use of part of a delimiter run.
#[derive(Clone, Copy, Debug)]
struct Use {
    kind: Kind,
    width: usize,
    role: Role,
    matched: bool,
}

fn width(ch: char, kind: Kind) -> usize {
    match (ch, kind) {
        ('~', _) => 2,
        (_, Kind::Bold) => 2,
        _ => 1,
    }
}

/// Pair openers with closers, CommonMark style but simpler: a closing run
/// closes the innermost matching opener; openers left unpaired become text.
fn pair(toks: &[Tok]) -> Vec<(Vec<Use>, usize)> {
    let mut uses: Vec<(Vec<Use>, usize)> = toks.iter().map(|_| (Vec::new(), 0)).collect();
    let mut stack: Vec<(usize, usize, char)> = Vec::new(); // (token, use index, char)
    for (t, tok) in toks.iter().enumerate() {
        let Tok::Delims { ch, len, open, close } = *tok else { continue };
        let mut left = len;
        if close {
            while left > 0 {
                let Some(pos) = stack.iter().rposition(|s| s.2 == ch) else { break };
                let (ot, ou, _) = stack[pos];
                let kind = uses[ot].0[ou].kind;
                let w = width(ch, kind);
                if left < w {
                    break;
                }
                stack.truncate(pos);
                uses[ot].0[ou].matched = true;
                uses[t].0.push(Use { kind, width: w, role: Role::Close, matched: true });
                left -= w;
            }
        }
        if open && left > 0 {
            let kinds: &[Kind] = match (ch, left) {
                ('~', 2) => &[Kind::Strike],
                ('~', _) => &[],
                (_, 1) => &[Kind::Italic],
                (_, 2) => &[Kind::Bold],
                (_, 3) => &[Kind::Bold, Kind::Italic],
                _ => &[],
            };
            for &kind in kinds {
                let w = width(ch, kind);
                uses[t].0.push(Use { kind, width: w, role: Role::Open, matched: false });
                stack.push((t, uses[t].0.len() - 1, ch));
                left -= w;
            }
        }
        uses[t].1 = left;
    }
    uses
}

fn push_run(out: &mut Vec<Run>, text: &str, marks: Marks, link: Option<&str>) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.marks == marks && last.link.as_deref() == link {
            last.text.push_str(text);
            return;
        }
    }
    out.push(Run { text: text.to_string(), marks, link: link.map(str::to_string) });
}

fn build(toks: &[Tok], base: Marks, link: Option<&str>, out: &mut Vec<Run>) {
    let uses = pair(toks);
    let mut depth = [0usize; 3];
    let marks_now = |d: &[usize; 3]| Marks {
        bold: base.bold || d[0] > 0,
        italic: base.italic || d[1] > 0,
        strike: base.strike || d[2] > 0,
        code: base.code,
    };
    for (t, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Text(s) => push_run(out, s, marks_now(&depth), link),
            Tok::Code(s) => {
                let mut m = marks_now(&depth);
                m.code = true;
                push_run(out, s, m, link);
            }
            Tok::Link { label, url } => build(label, marks_now(&depth), Some(url), out),
            Tok::Delims { ch, .. } => {
                for u in &uses[t].0 {
                    if u.matched {
                        let slot = u.kind.slot();
                        match u.role {
                            Role::Open => depth[slot] += 1,
                            Role::Close => depth[slot] = depth[slot].saturating_sub(1),
                        }
                    } else {
                        let raw: String = std::iter::repeat_n(*ch, u.width).collect();
                        push_run(out, &raw, marks_now(&depth), link);
                    }
                }
                let left = uses[t].1;
                if left > 0 {
                    let raw: String = std::iter::repeat_n(*ch, left).collect();
                    push_run(out, &raw, marks_now(&depth), link);
                }
            }
        }
    }
}

fn parse_with(md: &str, foreign: bool) -> Vec<Run> {
    let chars: Vec<char> = md.chars().collect();
    let mut out = Vec::new();
    build(&tokenize(&chars, foreign), Marks::default(), None, &mut out);
    out
}

/// Parse a block's stored text into formatted runs.
pub fn parse(md: &str) -> Vec<Run> {
    parse_with(md, false)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Merge neighbours with the same formatting, drop empty runs, and move
/// whitespace at the edges of bold/italic/strike out of the formatting so the
/// Markdown stays valid (`**a** b`, never `**a **b`).
pub fn normalize(runs: &[Run]) -> Vec<Run> {
    let merged = merge(runs);
    let mut split: Vec<Run> = Vec::new();
    for (i, r) in merged.iter().enumerate() {
        if r.marks.code {
            split.push(r.clone());
            continue;
        }
        let prev = i.checked_sub(1).and_then(|p| merged.get(p)).map(|p| p.marks).unwrap_or_default();
        let next = merged.get(i + 1).map(|n| n.marks).unwrap_or_default();
        let strip = |m: Marks, other: Marks| Marks { code: false, ..m.and(other) };
        let trimmed_start = r.text.trim_start();
        if trimmed_start.is_empty() {
            split.push(Run { marks: strip(r.marks, prev.and(next)), ..r.clone() });
            continue;
        }
        let lead = &r.text[..r.text.len() - trimmed_start.len()];
        let core = trimmed_start.trim_end();
        let trail = &trimmed_start[core.len()..];
        if !lead.is_empty() {
            split.push(Run { text: lead.into(), marks: strip(r.marks, prev), link: r.link.clone() });
        }
        split.push(Run { text: core.into(), marks: r.marks, link: r.link.clone() });
        if !trail.is_empty() {
            split.push(Run { text: trail.into(), marks: strip(r.marks, next), link: r.link.clone() });
        }
    }
    merge(&split)
}

fn merge(runs: &[Run]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for r in runs {
        push_run(&mut out, &r.text, r.marks, r.link.as_deref());
    }
    out
}

fn has(m: Marks, k: Kind) -> bool {
    match k {
        Kind::Bold => m.bold,
        Kind::Italic => m.italic,
        Kind::Strike => m.strike,
    }
}

fn delim(k: Kind) -> &'static str {
    match k {
        Kind::Bold => "**",
        Kind::Italic => "_",
        Kind::Strike => "~~",
    }
}

const ORDER: [Kind; 3] = [Kind::Bold, Kind::Italic, Kind::Strike];

fn emit(out: &mut String, open: &mut Vec<Kind>, base: usize, run: &Run) {
    while open.len() > base && open[base..].iter().any(|k| !has(run.marks, *k)) {
        out.push_str(delim(open.pop().expect("non-empty")));
    }
    for k in ORDER {
        if has(run.marks, k) && !open.contains(&k) {
            open.push(k);
            out.push_str(delim(k));
        }
    }
    if run.marks.code {
        out.push('`');
        out.push_str(&escape_code(&run.text));
        out.push('`');
    } else {
        out.push_str(&escape_text(&run.text));
    }
}

fn close_to(out: &mut String, open: &mut Vec<Kind>, base: usize) {
    while open.len() > base {
        out.push_str(delim(open.pop().expect("non-empty")));
    }
}

/// Write runs as stored inline Markdown.
pub fn to_md(runs: &[Run]) -> String {
    let runs = normalize(runs);
    let mut out = String::new();
    let mut open: Vec<Kind> = Vec::new();
    let mut i = 0;
    while i < runs.len() {
        let Some(url) = runs[i].link.clone() else {
            emit(&mut out, &mut open, 0, &runs[i]);
            i += 1;
            continue;
        };
        let mut j = i;
        while j < runs.len() && runs[j].link.as_deref() == Some(url.as_str()) {
            j += 1;
        }
        let common = runs[i..j].iter().fold(Marks::ALL, |acc, r| acc.and(r.marks));
        while open.iter().any(|k| !has(common, *k)) {
            out.push_str(delim(open.pop().expect("non-empty")));
        }
        out.push('[');
        let base = open.len();
        for r in &runs[i..j] {
            emit(&mut out, &mut open, base, r);
        }
        close_to(&mut out, &mut open, base);
        out.push_str("](");
        out.push_str(&encode_url(&url));
        out.push(')');
        i = j;
    }
    close_to(&mut out, &mut open, 0);
    out
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '~' | '`' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn escape_code(s: &str) -> String {
    s.replace('\\', "\\\\").replace('`', "\\`")
}

fn encode_url(url: &str) -> String {
    url.trim().replace(' ', "%20").replace('(', "%28").replace(')', "%29")
}

// ---------------------------------------------------------------------------
// Helpers used by the editor
// ---------------------------------------------------------------------------

/// Plain text to stored Markdown (escaping anything with a meaning).
pub fn escape(plain: &str) -> String {
    escape_text(plain)
}

/// The visible text of stored Markdown.
pub fn plain(md: &str) -> String {
    parse(md).into_iter().map(|r| r.text).collect()
}

/// Re-write Markdown from elsewhere (an AI reply, a paste) in the stored form.
pub fn canonical(md: &str) -> String {
    to_md(&parse_with(md, true))
}

fn split_runs(runs: &[Run], at: usize) -> (Vec<Run>, Vec<Run>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut seen = 0;
    for r in runs {
        let len = utf16_len(&r.text);
        if seen + len <= at {
            left.push(r.clone());
        } else if seen >= at {
            right.push(r.clone());
        } else {
            let byte = utf16_to_byte(&r.text, at - seen);
            left.push(Run { text: r.text[..byte].to_string(), ..r.clone() });
            right.push(Run { text: r.text[byte..].to_string(), ..r.clone() });
        }
        seen += len;
    }
    (left, right)
}

/// Split stored Markdown at a visible UTF-16 offset, keeping formatting on both sides.
pub fn split_at(md: &str, at: usize) -> (String, String) {
    let (l, r) = split_runs(&parse(md), at);
    (to_md(&l), to_md(&r))
}

/// A link with these words, as stored Markdown.
pub fn link(text: &str, url: &str) -> String {
    to_md(&[Run { text: text.to_string(), marks: Marks::default(), link: Some(url.to_string()) }])
}

/// The address of a page in a link: `rune://page/<id>`. Notes written
/// before the name Rune link with `notion2://page/<id>`: both are read,
/// and new links are written with the first.
pub const PAGE_URL: &str = "rune://page/";
pub const OLD_PAGE_URL: &str = "notion2://page/";
/// The address of a picture or a drawing in a page's Markdown (both read).
pub const BLOCK_URL: &str = "rune://block/";
pub const OLD_BLOCK_URL: &str = "notion2://block/";

/// The link to a page.
/// A part of a plain text (a cell of a table, the notes of an event) that
/// can hold links to pages, written as Markdown: `[Name](rune://page/<id>)`.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Words(String),
    /// A link to a page: the name it was written with, and the page.
    Page { name: String, id: String },
}

/// `text` as its words and its links to pages, in order. Any other
/// Markdown in it is words (a cell shows what was typed).
pub fn page_parts(text: &str) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    let mut words = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let name = &after[..close];
            let address = &after[close + 2..];
            let end = address.find(')')?;
            let id = address[..end].strip_prefix(PAGE_URL)?;
            let whole = !name.contains(['[', ']', '\n']) && !id.is_empty() && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
            whole.then(|| (name.to_string(), id.to_string(), open + 1 + close + 2 + end + 1))
        });
        match link {
            Some((name, id, len)) => {
                words.push_str(&rest[..open]);
                if !words.is_empty() {
                    out.push(Part::Words(std::mem::take(&mut words)));
                }
                out.push(Part::Page { name, id });
                rest = &rest[len..];
            }
            None => {
                words.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    words.push_str(rest);
    if !words.is_empty() {
        out.push(Part::Words(words));
    }
    out
}

/// `text` as its words, with each link to a page as the name of the page:
/// the name `name_of` gives for it (the name the page has now), or else the
/// name the link was written with. This is what a sort, a filter or a
/// search reads of a plain text that can hold such links.
pub fn with_page_names(text: &str, name_of: impl Fn(&str) -> Option<String>) -> String {
    if !text.contains("](rune://page/") {
        return text.to_string();
    }
    page_parts(text)
        .into_iter()
        .map(|part| match part {
            Part::Words(words) => words,
            Part::Page { name, id } => name_of(&id).unwrap_or(name),
        })
        .collect()
}

/// Whether a plain text holds a link to a page (see `page_parts`).
pub fn has_page_link(text: &str) -> bool {
    text.contains("](rune://page/") && page_parts(text).iter().any(|p| matches!(p, Part::Page { .. }))
}

pub fn page_url(id: &str) -> String {
    format!("{PAGE_URL}{id}")
}

/// The page a link is to, in either form.
pub fn page_id_of(url: &str) -> Option<&str> {
    let url = url.trim();
    url.strip_prefix(PAGE_URL).or_else(|| url.strip_prefix(OLD_PAGE_URL)).map(|id| id.trim_end_matches('/')).filter(|id| !id.is_empty())
}

/// The pages that stored Markdown links to, by id (in the order of the text).
pub fn page_links(md: &str) -> Vec<String> {
    if !md.contains(PAGE_URL) && !md.contains(OLD_PAGE_URL) {
        return Vec::new();
    }
    parse(md).iter().filter_map(|r| page_id_of(r.link.as_deref()?)).map(str::to_string).collect()
}

/// Whether stored Markdown has a link to a page, in either form.
pub fn links_to_page(md: &str, id: &str) -> bool {
    links_to(md, &page_url(id)) || links_to(md, &format!("{OLD_PAGE_URL}{id}"))
}

/// Whether stored Markdown has a link to this address.
pub fn links_to(md: &str, url: &str) -> bool {
    md.contains(url) && parse(md).iter().any(|r| r.link.as_deref() == Some(url))
}

/// Give the links to `url` whose words are `old` the words `new` (a page
/// was renamed). `None` if nothing changes.
pub fn retitle_links(md: &str, url: &str, old: &str, new: &str) -> Option<String> {
    if !md.contains(url) {
        return None;
    }
    let mut runs = parse(md);
    let mut changed = false;
    for r in runs.iter_mut().filter(|r| r.link.as_deref() == Some(url) && r.text == old) {
        r.text = new.to_string();
        changed = true;
    }
    changed.then(|| to_md(&runs))
}

/// Join two pieces of stored Markdown.
pub fn concat(a: &str, b: &str) -> String {
    let mut runs = parse(a);
    runs.extend(parse(b));
    to_md(&runs)
}

// ----- the same operations for a block, which knows whether it is code -----

pub fn plain_for(kind: BlockKind, text: &str) -> String {
    if kind == BlockKind::Code {
        text.to_string()
    } else {
        plain(text)
    }
}

pub fn len_for(kind: BlockKind, text: &str) -> usize {
    utf16_len(&plain_for(kind, text))
}

pub fn split_for(kind: BlockKind, text: &str, at: usize) -> (String, String) {
    if kind == BlockKind::Code {
        let b = utf16_to_byte(text, at);
        (text[..b].to_string(), text[b..].to_string())
    } else {
        split_at(text, at)
    }
}

/// Convert a block's text when it turns from or into a code block.
pub fn convert(text: &str, from: BlockKind, to: BlockKind) -> String {
    match (from == BlockKind::Code, to == BlockKind::Code) {
        (true, false) => escape(text),
        (false, true) => plain(text),
        _ => text.to_string(),
    }
}

/// Append block `b` (of kind `kb`) to block `a` (of kind `ka`), in `a`'s format.
pub fn join_for(ka: BlockKind, a: &str, kb: BlockKind, b: &str) -> String {
    let b = convert(b, kb, ka);
    if ka == BlockKind::Code {
        format!("{a}{b}")
    } else {
        concat(a, &b)
    }
}

// ---------------------------------------------------------------------------
// HTML for the editor
// ---------------------------------------------------------------------------

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Links the editor may open: the web, mail, phone numbers and this app's
/// own page links.
pub fn safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:", PAGE_URL, OLD_PAGE_URL].iter().any(|p| lower.starts_with(p))
        // A phone number, as a phone cell of a table writes it: digits only.
        || lower.strip_prefix("tel:").is_some_and(|n| !n.is_empty() && n.trim_start_matches('+').bytes().all(|b| b.is_ascii_digit()))
}

fn run_html(r: &Run) -> String {
    let mut s = html_escape(&r.text);
    if r.marks.code {
        s = format!("<code>{s}</code>");
    }
    if r.marks.strike {
        s = format!("<s>{s}</s>");
    }
    if r.marks.italic {
        s = format!("<em>{s}</em>");
    }
    if r.marks.bold {
        s = format!("<strong>{s}</strong>");
    }
    s
}

/// The web addresses written out in plain text, as (start, end) places in
/// it. An address begins with http:// or https:// at the start of a word and
/// runs to the next space; the signs that end a sentence are not part of it.
pub fn bare_urls(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("http") {
        let start = from + found;
        let rest = &text[start..];
        let begins_word = text[..start].chars().next_back().map_or(true, |c| c.is_whitespace() || c == '(');
        if begins_word && (rest.starts_with("http://") || rest.starts_with("https://")) {
            let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let url = rest[..len].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"']);
            if url.len() > "https://".len() {
                out.push((start, start + url.len()));
                from = start + url.len();
                continue;
            }
        }
        from = start + "http".len();
    }
    out
}

/// A run that is not a link. A web address written out in it shows as a
/// link all the same (a line written before addresses became links when
/// they are typed); the page then reads it back as one.
fn plain_run_html(r: &Run) -> String {
    if r.marks.code || !r.text.contains("http") {
        return run_html(r);
    }
    let piece = |text: &str| if text.is_empty() { String::new() } else { run_html(&Run { text: text.to_string(), ..r.clone() }) };
    let mut out = String::new();
    let mut at = 0;
    for (a, b) in bare_urls(&r.text) {
        out.push_str(&piece(&r.text[at..a]));
        out.push_str(&format!("<a href=\"{}\">{}</a>", html_escape(&r.text[a..b]), piece(&r.text[a..b])));
        at = b;
    }
    out.push_str(&piece(&r.text[at..]));
    out
}

/// HTML shown in the block editor for stored text.
pub fn to_html(kind: BlockKind, text: &str) -> String {
    let mut out = String::new();
    if kind == BlockKind::Code {
        out.push_str(&html_escape(text));
    } else {
        let runs = parse(text);
        let mut i = 0;
        while i < runs.len() {
            let Some(url) = runs[i].link.clone() else {
                out.push_str(&plain_run_html(&runs[i]));
                i += 1;
                continue;
            };
            let mut j = i;
            while j < runs.len() && runs[j].link.as_deref() == Some(url.as_str()) {
                j += 1;
            }
            out.push_str(&format!("<a href=\"{}\">", html_escape(&url)));
            for r in &runs[i..j] {
                out.push_str(&run_html(r));
            }
            out.push_str("</a>");
            i = j;
        }
    }
    // A trailing line break needs something after it to show as an empty line.
    if out.ends_with('\n') {
        out.push_str("<br>");
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_plain_text_gives_its_links_to_pages() {
        use super::{has_page_link, page_parts, Part};
        let words = |s: &str| Part::Words(s.to_string());
        let page = |name: &str, id: &str| Part::Page { name: name.to_string(), id: id.to_string() };
        assert_eq!(page_parts("See [Plan](rune://page/ab12) and [Notes](rune://page/cd34-x)."), vec![words("See "), page("Plan", "ab12"), words(" and "), page("Notes", "cd34-x"), words(".")]);
        assert_eq!(page_parts("[Plan](rune://page/ab12)"), vec![page("Plan", "ab12")]);
        // Other links, and brackets that are no link, are words.
        let plain = "a [b] c [site](https://x.example) [x](rune://page/) [y](rune://page/a b)";
        assert_eq!(page_parts(plain), vec![words(plain)]);
        assert!(has_page_link("x [P](rune://page/1)") && !has_page_link(plain) && !has_page_link("](rune://page/"));
        assert_eq!(page_parts(""), Vec::<Part>::new());
        assert_eq!(page_parts("[[Plan](rune://page/ab12)"), vec![words("["), page("Plan", "ab12")]);
    }

    #[test]
    fn page_links_are_read_in_both_forms_and_written_in_the_new_one() {
        assert_eq!(super::page_url("abc"), "rune://page/abc");
        assert_eq!(super::page_id_of("rune://page/abc/"), Some("abc"));
        assert_eq!(super::page_id_of("notion2://page/abc"), Some("abc"));
        assert_eq!(super::page_id_of("https://example.com"), None);
        let md = format!("{} and {}", super::link("New", "rune://page/n1"), super::link("Old", "notion2://page/o1"));
        assert_eq!(super::page_links(&md), ["n1", "o1"]);
        assert!(super::links_to_page(&md, "o1") && super::links_to_page(&md, "n1") && !super::links_to_page(&md, "x"));
        assert!(super::safe_url("rune://page/n1") && super::safe_url("notion2://page/o1"));
    }

    use super::*;

    #[test]
    fn a_web_address_written_out_shows_as_a_link() {
        assert_eq!(bare_urls("see https://youtu.be/abc, then (http://x.org/a?b=1). No: xhttps://no.com https://"), vec![(4, 24), (32, 50)]);
        assert_eq!(
            to_html(BlockKind::Text, "see https://youtu.be/abc now"),
            "see <a href=\"https://youtu.be/abc\">https://youtu.be/abc</a> now"
        );
        // One that is a link already is not made one a second time; code is left as it is.
        assert_eq!(to_html(BlockKind::Text, "[site](https://a.com)"), "<a href=\"https://a.com\">site</a>");
        assert_eq!(to_html(BlockKind::Text, "`https://a.com`"), "<code>https://a.com</code>");
        assert_eq!(to_html(BlockKind::Code, "https://a.com"), "https://a.com");
    }

    fn r(text: &str, f: &str) -> Run {
        Run {
            text: text.into(),
            marks: Marks {
                bold: f.contains('b'),
                italic: f.contains('i'),
                strike: f.contains('s'),
                code: f.contains('c'),
            },
            link: None,
        }
    }

    fn roundtrip(runs: Vec<Run>) {
        let md = to_md(&runs);
        assert_eq!(parse(&md), normalize(&runs), "md was {md:?}");
    }

    #[test]
    fn plain_text_escapes_and_round_trips() {
        for s in ["2*3*4", "snake_case_name", "a ~ b ~~ c", "[not](a link)", "back\\slash", "`tick`", "**x**", "_y_"] {
            let md = escape(s);
            assert_eq!(plain(&md), s, "md was {md:?}");
            assert_eq!(parse(&md).len(), 1);
        }
    }

    #[test]
    fn formats_round_trip() {
        roundtrip(vec![r("bold", "b"), r(" then ", ""), r("italic", "i")]);
        roundtrip(vec![r("a", "i"), r("b", "bi"), r("c", "b")]);
        roundtrip(vec![r("a", "b"), r("b", "i")]);
        roundtrip(vec![r("a", "i"), r("b", "b")]);
        roundtrip(vec![r("x", "bis")]);
        roundtrip(vec![r("in", ""), r("word", "i"), r("italic", "")]);
        roundtrip(vec![r("code ` with \\ tick", "c"), r(" and ", ""), r("struck", "s")]);
        roundtrip(vec![r("bold code", "bc")]);
        let mut link = r("site", "b");
        link.link = Some("https://example.com/a".into());
        roundtrip(vec![r("see ", ""), link.clone(), r(" now", "")]);
        link.link = Some("https://example.com/a b(c)".into());
        assert_eq!(to_md(&[link]), "[**site**](https://example.com/a%20b%28c%29)");
    }

    #[test]
    fn writes_the_expected_markdown() {
        assert_eq!(to_md(&[r("a", "b"), r(" b", "")]), "**a** b");
        assert_eq!(to_md(&[r("a ", "b"), r("b", "")]), "**a** b");
        assert_eq!(to_md(&[r("x", "bi")]), "**_x_**");
        assert_eq!(to_md(&[r("in", ""), r("word", "i"), r("x", "")]), "in_word_x");
        assert_eq!(to_md(&[r("a", "c")]), "`a`");
        let mut l = r("here", "");
        l.link = Some("https://x.dev".into());
        assert_eq!(to_md(&[l]), "[here](https://x.dev)");
    }

    #[test]
    fn reads_common_markdown_from_ai_replies() {
        let foreign = |s: &str| parse(&canonical(s));
        assert_eq!(plain(&canonical("**Note:** use _this_ and `that`")), "Note: use this and that");
        assert!(foreign("__b__")[0].marks.bold);
        assert!(foreign("*i* and ***both***")[0].marks.italic);
        assert!(foreign("***both***").iter().all(|r| r.marks.bold && r.marks.italic));
        assert_eq!(foreign("a * b * c").len(), 1, "spaced asterisks stay literal");
        assert_eq!(foreign("my_var_name").len(), 1, "underscores inside words stay literal");
        assert_eq!(plain(&canonical("unclosed **bold")), "unclosed **bold");
        let runs = foreign("see [the docs](https://d.dev) now");
        assert_eq!(runs[1].link.as_deref(), Some("https://d.dev"));
        assert_eq!(runs[1].text, "the docs");
    }

    #[test]
    fn split_and_join_keep_formatting() {
        let md = to_md(&[r("hello", "b"), r(" world", "")]);
        let (a, b) = split_at(&md, 3);
        assert_eq!(a, "**hel**");
        assert_eq!(b, "**lo** world");
        assert_eq!(concat(&a, &b), md);
        assert_eq!(len_for(BlockKind::Text, &md), 11);
        let (x, y) = split_for(BlockKind::Code, "a*b", 1);
        assert_eq!((x.as_str(), y.as_str()), ("a", "*b"));
    }

    #[test]
    fn code_blocks_convert() {
        assert_eq!(convert("a*b", BlockKind::Code, BlockKind::Text), "a\\*b");
        assert_eq!(convert("**a**", BlockKind::Text, BlockKind::Code), "a");
        assert_eq!(join_for(BlockKind::Code, "x", BlockKind::Text, "**y**"), "xy");
    }

    #[test]
    fn html_is_escaped_and_formatted() {
        let html = to_html(BlockKind::Text, "**<b>** [x](https://a.b)");
        assert_eq!(html, "<strong>&lt;b&gt;</strong> <a href=\"https://a.b\">x</a>");
        assert_eq!(to_html(BlockKind::Code, "a<b\n"), "a&lt;b\n<br>");
        assert!(safe_url("https://a") && !safe_url("javascript:alert(1)"));
        assert!(safe_url("tel:+15550102000") && !safe_url("tel:") && !safe_url("tel:1;open"));
    }
}
