//! Core data model: pages and the flat, indented block list that makes up a page.
//!
//! A page's blocks are stored as one ordered list. Nesting is expressed with an
//! `indent` level: a block "owns" every following block whose indent is deeper,
//! which is how toggles hide their children and how drag-and-drop moves a block
//! together with its children.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Id = String;

pub const MAX_INDENT: u8 = 8;

/// Icon a new callout starts with ("name:color", see `ui/icons.rs`).
pub const CALLOUT_ICON: &str = "lightbulb:yellow";

pub fn new_id() -> Id {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockKind {
    Text,
    Heading1,
    Heading2,
    Heading3,
    Bulleted,
    Numbered,
    Todo,
    Toggle,
    Quote,
    Callout,
    Code,
    Divider,
    PageLink,
    /// An Excalidraw canvas; its scene is in `Block::data`.
    Drawing,
    /// A picture; `Block::data` names its file (see files.rs). Empty until
    /// one is chosen.
    Image,
    /// A simple table; its cells are in `Block::data` (see table.rs), and
    /// their words are mirrored in `Block::text` for search.
    Table,
    /// A chart of a table; what it draws is in `Block::data` (see chart.rs).
    Chart,
}

impl BlockKind {
    pub const TURN_INTO: [BlockKind; 11] = [
        BlockKind::Text,
        BlockKind::Heading1,
        BlockKind::Heading2,
        BlockKind::Heading3,
        BlockKind::Bulleted,
        BlockKind::Numbered,
        BlockKind::Todo,
        BlockKind::Toggle,
        BlockKind::Quote,
        BlockKind::Callout,
        BlockKind::Code,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BlockKind::Text => "text",
            BlockKind::Heading1 => "heading1",
            BlockKind::Heading2 => "heading2",
            BlockKind::Heading3 => "heading3",
            BlockKind::Bulleted => "bulleted",
            BlockKind::Numbered => "numbered",
            BlockKind::Todo => "todo",
            BlockKind::Toggle => "toggle",
            BlockKind::Quote => "quote",
            BlockKind::Callout => "callout",
            BlockKind::Code => "code",
            BlockKind::Divider => "divider",
            BlockKind::PageLink => "page",
            BlockKind::Drawing => "drawing",
            BlockKind::Image => "image",
            BlockKind::Table => "table",
            BlockKind::Chart => "chart",
        }
    }

    pub fn parse(s: &str) -> BlockKind {
        match s {
            "heading1" => BlockKind::Heading1,
            "heading2" => BlockKind::Heading2,
            "heading3" => BlockKind::Heading3,
            "bulleted" => BlockKind::Bulleted,
            "numbered" => BlockKind::Numbered,
            "todo" => BlockKind::Todo,
            "toggle" => BlockKind::Toggle,
            "quote" => BlockKind::Quote,
            "callout" => BlockKind::Callout,
            "code" => BlockKind::Code,
            "divider" => BlockKind::Divider,
            "drawing" => BlockKind::Drawing,
            "image" => BlockKind::Image,
            "table" => BlockKind::Table,
            "chart" => BlockKind::Chart,
            "page" => BlockKind::PageLink,
            _ => BlockKind::Text,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BlockKind::Text => "Text",
            BlockKind::Heading1 => "Heading 1",
            BlockKind::Heading2 => "Heading 2",
            BlockKind::Heading3 => "Heading 3",
            BlockKind::Bulleted => "Bulleted list",
            BlockKind::Numbered => "Numbered list",
            BlockKind::Todo => "To-do list",
            BlockKind::Toggle => "Toggle list",
            BlockKind::Quote => "Quote",
            BlockKind::Callout => "Callout",
            BlockKind::Code => "Code",
            BlockKind::Divider => "Divider",
            BlockKind::PageLink => "Page",
            BlockKind::Drawing => "Drawing",
            BlockKind::Image => "Image",
            BlockKind::Table => "Table",
            BlockKind::Chart => "Chart",
        }
    }

    /// Name of the block's icon (see `ui/icons.rs`).
    pub fn icon(self) -> &'static str {
        match self {
            BlockKind::Text => "text",
            BlockKind::Heading1 => "heading-1",
            BlockKind::Heading2 => "heading-2",
            BlockKind::Heading3 => "heading-3",
            BlockKind::Bulleted => "list-bullet",
            BlockKind::Numbered => "list-numbered",
            BlockKind::Todo => "checkbox",
            BlockKind::Toggle => "toggle-list",
            BlockKind::Quote => "quote",
            BlockKind::Callout => "callout",
            BlockKind::Code => "code",
            BlockKind::Divider => "divider",
            BlockKind::PageLink => "file",
            BlockKind::Drawing => "drawing",
            BlockKind::Image => "image",
            BlockKind::Table => "table",
            BlockKind::Chart => "chart-bars",
        }
    }

    /// Pressing Enter in these blocks continues the same kind of block.
    pub fn continues_on_enter(self) -> bool {
        matches!(
            self,
            BlockKind::Bulleted | BlockKind::Numbered | BlockKind::Todo | BlockKind::Toggle
        )
    }

    /// Blocks that hold editable text.
    pub fn is_editable(self) -> bool {
        !matches!(self, BlockKind::Divider | BlockKind::PageLink | BlockKind::Drawing | BlockKind::Image | BlockKind::Table | BlockKind::Chart)
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            BlockKind::Text => "Type '/' for commands, ⌘J for AI…",
            BlockKind::Heading1 => "Heading 1",
            BlockKind::Heading2 => "Heading 2",
            BlockKind::Heading3 => "Heading 3",
            BlockKind::Bulleted | BlockKind::Numbered => "List",
            BlockKind::Todo => "To-do",
            BlockKind::Toggle => "Toggle",
            BlockKind::Quote => "Empty quote",
            BlockKind::Callout => "Type something…",
            BlockKind::Code => "Code",
            BlockKind::Divider | BlockKind::PageLink | BlockKind::Drawing | BlockKind::Image | BlockKind::Table | BlockKind::Chart => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: Id,
    pub kind: BlockKind,
    pub text: String,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub indent: u8,
    #[serde(default)]
    pub collapsed: bool,
    /// Emoji shown on callouts.
    #[serde(default)]
    pub icon: Option<String>,
    /// Language label on code blocks.
    #[serde(default)]
    pub language: Option<String>,
    /// Page a `PageLink` block points to.
    #[serde(default)]
    pub target: Option<Id>,
    /// A `Drawing` block's scene (Excalidraw JSON). Empty for other blocks.
    #[serde(default)]
    pub data: String,
    /// Where a picture sits and how wide it is, when that is not simply in
    /// line at its own size.
    #[serde(default, skip_serializing_if = "Place::is_plain")]
    pub place: Place,
    /// The day a to-do is due. It shows at the end of the to-do's line and
    /// in that day of the calendar. Other kinds of line do not use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<chrono::NaiveDate>,
    /// A to-do with a day can have a time on that day: Rune sends a Mac
    /// notification then (a reminder). None: no reminder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind: Option<chrono::NaiveTime>,
}

/// Where a picture sits. A free picture is at one side of the page, at the
/// line it was let go at: it keeps its spot in the list of blocks (so it
/// stays with the text around it), and the lines after it run beside it
/// where there is room, or below it. Nothing goes under or over it.
///
/// For a table, only `beside` is used: the table stands at the right of the
/// table above it, on the same line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    #[serde(default)]
    pub free: bool,
    /// At the right side of the page's column (else the left).
    #[serde(default)]
    pub right: bool,
    /// How far it is from that side's edge of the column (points). Less
    /// than zero hangs it out into the margin of the page.
    #[serde(default)]
    pub x: i32,
    /// Not used now (a first form of free pictures kept a height here);
    /// kept so that files from then still read.
    #[serde(default)]
    pub y: i32,
    /// Its width in points; 0 is the picture's own width.
    #[serde(default)]
    pub w: u32,
    /// A table that stands beside the table above it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub beside: bool,
}

impl Place {
    /// The narrowest and the widest a picture can be made.
    pub const NARROWEST: u32 = 40;
    pub const WIDEST: u32 = 4000;
    /// How far from the edge of the column a free picture can be.
    const FARTHEST: i32 = 20_000;

    pub fn is_plain(&self) -> bool {
        *self == Place::default()
    }

    /// With every number inside what a page can show.
    pub fn tidy(mut self) -> Place {
        if self.w != 0 {
            self.w = self.w.clamp(Place::NARROWEST, Place::WIDEST);
        }
        self.y = 0;
        if self.free {
            self.x = self.x.clamp(-Place::FARTHEST, Place::FARTHEST);
        } else {
            self.x = 0;
            self.right = false;
        }
        self
    }

    /// As kept in the workspace file: nothing for a plain place.
    pub fn to_column(&self) -> String {
        if self.is_plain() { String::new() } else { serde_json::to_string(self).unwrap_or_default() }
    }

    pub fn from_column(text: &str) -> Place {
        serde_json::from_str::<Place>(text).map(Place::tidy).unwrap_or_default()
    }
}

impl Block {
    pub fn new(kind: BlockKind, text: impl Into<String>) -> Block {
        Block {
            id: new_id(),
            kind,
            text: text.into(),
            checked: false,
            indent: 0,
            collapsed: false,
            icon: if kind == BlockKind::Callout { Some(CALLOUT_ICON.into()) } else { None },
            language: None,
            target: None,
            data: String::new(),
            place: Place::default(),
            due: None,
            remind: None,
        }
    }

    /// The reminder's time as the notes file keeps it: "09:00", or nothing.
    pub fn remind_column(&self) -> String {
        self.remind.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
    }

    pub fn remind_from_column(s: &str) -> Option<chrono::NaiveTime> {
        chrono::NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
    }

    /// When a to-do's reminder goes off (ms), if it has one and is not done.
    pub fn remind_at(&self) -> Option<i64> {
        (self.kind == BlockKind::Todo && !self.checked).then_some(())?;
        Some(crate::when::at(self.due?, self.remind?))
    }

    /// The due day as the notes file keeps it: "2026-11-12", or nothing.
    pub fn due_column(&self) -> String {
        self.due.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
    }

    pub fn due_from_column(s: &str) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
    }

    /// A picture that is off the lines of the page (see `Place`).
    pub fn floats(&self) -> bool {
        self.kind == BlockKind::Image && self.place.free
    }

    pub fn text(text: impl Into<String>) -> Block {
        Block::new(BlockKind::Text, text)
    }

    pub fn with_indent(mut self, indent: u8) -> Block {
        self.indent = indent.min(MAX_INDENT);
        self
    }

    /// The text as shown, without formatting markers.
    pub fn plain_text(&self) -> String {
        crate::inline::plain_for(self.kind, &self.text)
    }

    pub fn page_link(target: Id) -> Block {
        let mut b = Block::new(BlockKind::PageLink, "");
        b.target = Some(target);
        b
    }

    /// Change the block's kind, keeping its text, and set kind-specific defaults.
    /// Code blocks store raw text; everything else stores inline Markdown.
    pub fn turn_into(&mut self, kind: BlockKind) {
        self.text = crate::inline::convert(&self.text, self.kind, kind);
        self.kind = kind;
        if kind == BlockKind::Callout && self.icon.is_none() {
            self.icon = Some(CALLOUT_ICON.into());
        }
        if kind != BlockKind::Todo {
            self.checked = false;
        }
        if kind != BlockKind::Toggle {
            self.collapsed = false;
        }
        if kind != BlockKind::Image {
            self.place = Place::default();
        }
    }
}

/// A calendar event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Id,
    pub space_id: Id,
    pub title: String,
    /// Start and end in milliseconds since 1970 (UTC). An all-day event runs
    /// from local midnight of its first day to local midnight after its last.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// Colour (see `ui/icons.rs` ICON_COLORS).
    pub color: String,
    pub notes: String,
    /// The link to join its call by ("" for none). It has a field of its
    /// own; an older event can have the link in its notes (see `join_url`).
    pub join: String,
    /// A page holding longer notes for the event.
    pub page_id: Option<Id>,
    /// Minutes before the start to send a reminder (None: no reminder).
    pub remind: Option<i64>,
    /// How the event repeats (None: it happens once). `start` and `end` are
    /// its first time; see repeat.rs.
    pub repeat: Option<crate::repeat::Repeat>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Event {
    pub fn new(space_id: impl Into<String>, title: impl Into<String>, start: i64, end: i64, all_day: bool) -> Event {
        let now = now_ms();
        Event {
            id: new_id(),
            space_id: space_id.into(),
            title: title.into(),
            start,
            end: end.max(start + 1),
            all_day,
            color: "blue".into(),
            notes: String::new(),
            join: String::new(),
            page_id: None,
            remind: None,
            repeat: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// The link to join the call of this event by: the one in its own
    /// field, or else the link of a meeting found in its notes.
    pub fn join_url(&self) -> Option<String> {
        let own = self.join.trim();
        if own.is_empty() {
            return crate::meeting::join_link(&self.notes);
        }
        // Typed without "https://": a link all the same.
        Some(if own.contains("://") { own.to_string() } else { format!("https://{own}") })
    }

    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            "New event"
        } else {
            &self.title
        }
    }

    /// Whether the event overlaps [from, to).
    pub fn overlaps(&self, from: i64, to: i64) -> bool {
        self.start < to && self.end > from
    }
}

/// A space (workspace): its own set of pages, favourites, Home and Trash.
#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    pub id: Id,
    pub name: String,
    /// Badge colour (see `ui/icons.rs` ICON_COLORS).
    pub color: String,
    pub position: f64,
    pub created_at: i64,
}

impl Space {
    pub fn new(name: impl Into<String>, color: impl Into<String>, position: f64) -> Space {
        Space { id: new_id(), name: name.into(), color: color.into(), position, created_at: now_ms() }
    }

    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            "Untitled space"
        } else {
            &self.name
        }
    }

    /// First letter, for the badge.
    pub fn initial(&self) -> String {
        self.display_name().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: Id,
    /// The space the page lives in (sub-pages share their parent's).
    pub space_id: Id,
    pub parent_id: Option<Id>,
    pub title: String,
    pub icon: Option<String>,
    pub position: f64,
    pub favorite: bool,
    pub trashed: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// The band across the top of the page: "preset:<name>" for one of the
    /// app's own colours and gradients (see `covers`), or "file:<name>" for
    /// a picture of the person's (see files.rs).
    pub cover: Option<String>,
    /// Which part of a cover picture shows: 0 its top … 100 its bottom.
    pub cover_y: u8,
    /// The face of the page's title and headings: "" (the app's own),
    /// "serif" or "mono". A serif page also has a larger first paragraph
    /// and sets its quotes large.
    pub font: String,
    /// For the page of a row of a table: the id of the table's block (it
    /// is in the page `parent_id`), a bar, and what the row goes by there
    /// (`table::RowInfo::id`); "template" in place of a row for the page of
    /// the table's template. Such a page is not in the sidebar: it opens
    /// from its row.
    #[serde(default)]
    pub row_of: Option<Id>,
}

/// The faces a page can have, with their names.
pub const PAGE_FONTS: [(&str, &str); 3] = [("", "Sans"), ("serif", "Serif"), ("mono", "Mono")];

/// How many minutes a page of this many words takes to read (none for a
/// page that takes less than one).
pub fn minutes_to_read(words: usize) -> Option<usize> {
    const WORDS_A_MINUTE: usize = 230;
    (words >= WORDS_A_MINUTE).then(|| words.div_ceil(WORDS_A_MINUTE))
}

/// The app's own covers: soft colours and gradients, by name, with the CSS
/// that paints them. Quiet tones that sit well behind text, in both themes.
pub mod covers {
    pub const PRESETS: &[(&str, &str, &str)] = &[
        ("sand", "Sand", "linear-gradient(135deg, #efe4cf 0%, #dcc7a3 55%, #bfa27c 100%)"),
        ("clay", "Clay", "linear-gradient(135deg, #f0c9ae 0%, #d98f6c 55%, #a85a43 100%)"),
        ("ember", "Ember", "linear-gradient(120deg, #f6d29a 0%, #e58a45 50%, #a8402f 100%)"),
        ("moss", "Moss", "linear-gradient(135deg, #d5e3bd 0%, #8fb58a 55%, #456f5f 100%)"),
        ("sea", "Sea", "linear-gradient(135deg, #bfe3e0 0%, #6aa9bd 55%, #2f5f85 100%)"),
        ("slate", "Slate", "linear-gradient(135deg, #cfd6dd 0%, #8794a3 55%, #46505e 100%)"),
        ("dawn", "Dawn", "linear-gradient(120deg, #f7e3c8 0%, #f0b9a8 45%, #b9a3c4 100%)"),
        ("night", "Night", "linear-gradient(135deg, #1f2a44 0%, #2f4f6f 55%, #5f8f8a 100%)"),
        ("meadow", "Meadow", "radial-gradient(120% 140% at 15% 20%, #f3e7b1 0%, rgba(243, 231, 177, 0) 55%), linear-gradient(135deg, #b9d99a 0%, #5f9f86 100%)"),
        ("shore", "Shore", "radial-gradient(110% 130% at 85% 15%, #f6dcc0 0%, rgba(246, 220, 192, 0) 55%), linear-gradient(135deg, #9fd0d8 0%, #4f86a8 100%)"),
        ("stone", "Stone", "#b9b4ab"),
        ("rust", "Rust", "#c0623f"),
        ("honey", "Honey", "#e0b24a"),
        ("fern", "Fern", "#6f9f6a"),
        ("lake", "Lake", "#4f86b5"),
        ("plum", "Plum", "#8a5f86"),
    ];

    /// The CSS that paints a preset cover.
    pub fn css(name: &str) -> Option<&'static str> {
        PRESETS.iter().find(|(id, _, _)| *id == name).map(|(_, _, css)| *css)
    }
}

impl Page {
    pub fn new(parent_id: Option<Id>, position: f64) -> Page {
        let now = now_ms();
        Page {
            id: new_id(),
            space_id: String::new(),
            parent_id,
            title: String::new(),
            icon: None,
            position,
            favorite: false,
            trashed: false,
            created_at: now,
            updated_at: now,
            cover: None,
            cover_y: 50,
            font: String::new(),
            row_of: None,
        }
    }

    /// The picture of a cover that is the person's own, by its file name.
    pub fn cover_file(&self) -> Option<&str> {
        self.cover.as_deref()?.strip_prefix("file:").filter(|name| crate::files::valid_name(name))
    }

    /// The CSS of a cover that is one of the app's own.
    pub fn cover_css(&self) -> Option<&'static str> {
        covers::css(self.cover.as_deref()?.strip_prefix("preset:")?)
    }

    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            "Untitled"
        } else {
            &self.title
        }
    }

    /// Stored icon ("name:color"), or the plain page icon.
    pub fn display_icon(&self) -> &str {
        self.icon.as_deref().unwrap_or("file")
    }
}

// ---------------------------------------------------------------------------
// Text position helpers. The web view reports caret positions in UTF-16 code
// units; Rust strings are indexed by bytes.
// ---------------------------------------------------------------------------

pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

pub fn utf16_to_byte(s: &str, pos: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in s.char_indices() {
        if units >= pos {
            return byte;
        }
        units += ch.len_utf16();
    }
    s.len()
}

// ---------------------------------------------------------------------------
// Structural helpers on a page's block list.
// ---------------------------------------------------------------------------

pub fn index_of(blocks: &[Block], id: &str) -> Option<usize> {
    blocks.iter().position(|b| b.id == id)
}

/// Exclusive end of the block at `idx` plus all of its nested children.
pub fn subtree_end(blocks: &[Block], idx: usize) -> usize {
    let level = blocks[idx].indent;
    let mut end = idx + 1;
    while end < blocks.len() && blocks[end].indent > level {
        end += 1;
    }
    end
}

/// Which blocks are visible (not hidden inside a collapsed toggle).
pub fn visibility(blocks: &[Block]) -> Vec<bool> {
    let mut out = Vec::with_capacity(blocks.len());
    let mut hidden_below: Option<u8> = None;
    for b in blocks {
        if let Some(level) = hidden_below {
            if b.indent > level {
                out.push(false);
                continue;
            }
            hidden_below = None;
        }
        out.push(true);
        if b.kind == BlockKind::Toggle && b.collapsed {
            hidden_below = Some(b.indent);
        }
    }
    out
}

/// The number shown on each numbered-list block (None for other blocks).
pub fn list_numbers(blocks: &[Block]) -> Vec<Option<usize>> {
    let mut counters = [0usize; MAX_INDENT as usize + 1];
    blocks
        .iter()
        .map(|b| {
            // A free picture is not in the list: the count goes on past it.
            if b.floats() {
                return None;
            }
            let level = b.indent.min(MAX_INDENT) as usize;
            for c in counters.iter_mut().skip(level + 1) {
                *c = 0;
            }
            if b.kind == BlockKind::Numbered {
                counters[level] += 1;
                Some(counters[level])
            } else {
                counters[level] = 0;
                None
            }
        })
        .collect()
}

/// Whether the block at `idx` has nested children.
pub fn has_children(blocks: &[Block], idx: usize) -> bool {
    subtree_end(blocks, idx) > idx + 1
}

/// Where the caret should go after an edit: block id and UTF-16 offset
/// (`usize::MAX` means "end of the text").
#[derive(Clone, Debug, PartialEq)]
pub struct Caret {
    pub id: Id,
    pub pos: usize,
}

impl Caret {
    pub fn start(id: &str) -> Caret {
        Caret { id: id.to_string(), pos: 0 }
    }
    pub fn end(id: &str) -> Caret {
        Caret { id: id.to_string(), pos: usize::MAX }
    }
}

/// Enter pressed at visible UTF-16 offset `pos` in block `idx` whose current
/// (stored) text is `value`.
pub fn press_enter(blocks: &mut Vec<Block>, idx: usize, pos: usize, value: &str) -> Caret {
    blocks[idx].text = value.to_string();
    let current = blocks[idx].clone();
    let visible = crate::inline::len_for(current.kind, &current.text);

    // Enter on an empty list item leaves the list (un-nest first, then plain text).
    if current.kind.continues_on_enter() && visible == 0 {
        if current.indent > 0 {
            outdent(blocks, idx);
        } else {
            blocks[idx].turn_into(BlockKind::Text);
        }
        return Caret::start(&current.id);
    }

    let next_kind = if current.kind.continues_on_enter() {
        current.kind
    } else {
        BlockKind::Text
    };

    // Enter at the very start of a non-empty block inserts an empty block above.
    if pos == 0 && visible > 0 {
        let above = Block::new(next_kind, "").with_indent(current.indent);
        blocks.insert(idx, above);
        return Caret::start(&current.id);
    }

    let (before, after) = crate::inline::split_for(current.kind, &current.text, pos);
    blocks[idx].text = before;

    let (insert_at, indent) = if current.kind == BlockKind::Toggle && !current.collapsed {
        // An open toggle gets its first child.
        (idx + 1, current.indent + 1)
    } else if current.kind == BlockKind::Toggle {
        (subtree_end(blocks, idx), current.indent)
    } else {
        (idx + 1, current.indent)
    };
    let kind = if current.kind == BlockKind::Toggle && !current.collapsed {
        BlockKind::Text
    } else {
        next_kind
    };
    let after = crate::inline::convert(&after, current.kind, kind);
    let new_block = Block::new(kind, after).with_indent(indent);
    let new_id = new_block.id.clone();
    blocks.insert(insert_at, new_block);
    Caret::start(&new_id)
}

/// Backspace pressed with the caret at the very start of block `idx`.
pub fn press_backspace_at_start(blocks: &mut Vec<Block>, idx: usize, value: &str) -> Option<Caret> {
    blocks[idx].text = value.to_string();
    let current = blocks[idx].clone();

    if current.kind != BlockKind::Text && current.kind.is_editable() {
        blocks[idx].turn_into(BlockKind::Text);
        return Some(Caret::start(&current.id));
    }
    if current.indent > 0 {
        outdent(blocks, idx);
        return Some(Caret::start(&current.id));
    }

    let vis = visibility(blocks);
    // A free picture is not a line: Backspace joins with the line before it.
    let prev = (0..idx).rev().find(|&i| vis[i] && !blocks[i].floats())?;
    match blocks[prev].kind {
        BlockKind::Divider => {
            blocks.remove(prev);
            Some(Caret::start(&current.id))
        }
        BlockKind::PageLink | BlockKind::Drawing | BlockKind::Image | BlockKind::Table | BlockKind::Chart => {
            if current.plain_text().is_empty() {
                // (With no line of text above it, the caret has no place to
                // go: the empty line stays. Taken out, it left the page with
                // no caret at all.)
                let target = (0..prev + 1).rev().find(|&i| blocks[i].kind.is_editable())?;
                let id = blocks[target].id.clone();
                blocks.remove(idx);
                ensure_not_empty(blocks);
                Some(Caret::end(&id))
            } else {
                None
            }
        }
        _ => {
            let join = crate::inline::len_for(blocks[prev].kind, &blocks[prev].text);
            blocks[prev].text = crate::inline::join_for(blocks[prev].kind, &blocks[prev].text, current.kind, &current.text);
            blocks.remove(idx);
            Some(Caret { id: blocks[prev].id.clone(), pos: join })
        }
    }
}

/// Nest the block (and its children) one level deeper, under the block above.
pub fn indent(blocks: &mut [Block], idx: usize) -> bool {
    if idx == 0 {
        return false;
    }
    let prev_indent = blocks[idx - 1].indent;
    if blocks[idx].indent > prev_indent || blocks[idx].indent >= MAX_INDENT {
        return false;
    }
    let end = subtree_end(blocks, idx);
    for b in &mut blocks[idx..end] {
        b.indent = (b.indent + 1).min(MAX_INDENT);
    }
    true
}

/// Move the block (and its children) one level shallower.
pub fn outdent(blocks: &mut [Block], idx: usize) -> bool {
    if blocks[idx].indent == 0 {
        return false;
    }
    let end = subtree_end(blocks, idx);
    for b in &mut blocks[idx..end] {
        b.indent = b.indent.saturating_sub(1);
    }
    true
}

/// Move a block with its children before or after another block.
pub fn move_block(blocks: &mut Vec<Block>, from_id: &str, to_id: &str, after: bool) -> bool {
    let Some(from) = index_of(blocks, from_id) else { return false };
    let from_end = subtree_end(blocks, from);
    let Some(to) = index_of(blocks, to_id) else { return false };
    if to >= from && to < from_end {
        return false; // can't drop a block inside itself
    }
    let target_indent = blocks[to].indent;
    let group: Vec<Block> = blocks.drain(from..from_end).collect();
    let delta = target_indent as i16 - group[0].indent as i16;
    let group: Vec<Block> = group
        .into_iter()
        .map(|mut b| {
            b.indent = (b.indent as i16 + delta).clamp(0, MAX_INDENT as i16) as u8;
            b
        })
        .collect();
    let to = index_of(blocks, to_id).expect("target survives the drain");
    let insert_at = if after { subtree_end(blocks, to) } else { to };
    for (offset, b) in group.into_iter().enumerate() {
        blocks.insert(insert_at + offset, b);
    }
    true
}

/// Swap a block with the previous sibling (same nesting level).
pub fn move_up(blocks: &mut Vec<Block>, idx: usize) -> bool {
    let level = blocks[idx].indent;
    let mut i = idx;
    while i > 0 {
        i -= 1;
        if blocks[i].indent < level {
            return false;
        }
        // Free pictures are not lines to change places with.
        if blocks[i].indent == level && !blocks[i].floats() {
            let (from, to) = (blocks[idx].id.clone(), blocks[i].id.clone());
            return move_block(blocks, &from, &to, false);
        }
    }
    false
}

/// Swap a block with the next sibling (same nesting level).
pub fn move_down(blocks: &mut Vec<Block>, idx: usize) -> bool {
    let level = blocks[idx].indent;
    let mut end = subtree_end(blocks, idx);
    while end < blocks.len() && blocks[end].indent == level && blocks[end].floats() {
        end += 1;
    }
    if end >= blocks.len() || blocks[end].indent != level {
        return false;
    }
    let (from, to) = (blocks[idx].id.clone(), blocks[end].id.clone());
    move_block(blocks, &from, &to, true)
}

/// A page was written anew as Markdown (by the AI, to change it) and read
/// back as `new`. Markdown cannot say all that a block is (a toggle comes
/// back as a bullet, a callout as a quote, and every block gets a new id), so
/// each block of `new` whose words are the same as a block of `old`, in the
/// same order, is the old block again: the lines that did not change stay
/// what they were, and only what the change touched is new.
pub fn keep_unchanged(old: &[Block], new: Vec<Block>) -> Vec<Block> {
    let key = |b: &Block| match b.kind {
        BlockKind::Image | BlockKind::Drawing => format!("media:{}", b.id),
        BlockKind::PageLink => format!("page:{}", b.target.as_deref().unwrap_or("")),
        BlockKind::Divider => "divider".to_string(),
        _ => format!("text:{}", b.plain_text().trim()),
    };
    let a: Vec<String> = old.iter().map(key).collect();
    let b: Vec<String> = new.iter().map(key).collect();
    // The longest run of lines that both have, in order.
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut same: Vec<Option<usize>> = vec![None; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            same[j] = Some(i);
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let mut out: Vec<Block> = new
        .into_iter()
        .zip(same)
        .map(|(fresh, was)| {
            let Some(was) = was.map(|i| &old[i]) else { return fresh };
            // What Markdown cannot say stays as it was; what it can say
            // (a line made a heading, a to-do ticked, its nesting) is taken.
            let lost_in_markdown = matches!((was.kind, fresh.kind), (BlockKind::Toggle, BlockKind::Bulleted) | (BlockKind::Callout, BlockKind::Quote));
            if was.kind == fresh.kind || lost_in_markdown {
                let mut kept = was.clone();
                // (Markdown says how deep a list item is, and no more: a
                // line of text, a heading or code under a toggle or in a
                // list comes back with no step in. Taken from there, every
                // line of a meeting's transcript left its closed toggle.)
                if matches!(fresh.kind, BlockKind::Bulleted | BlockKind::Numbered | BlockKind::Todo) {
                    kept.indent = fresh.indent;
                }
                if kept.kind == BlockKind::Todo {
                    kept.checked = fresh.checked;
                    kept.due = fresh.due;
                }
                kept
            } else {
                Block { id: was.id.clone(), ..fresh }
            }
        })
        .collect();
    // A table with a changed cell has other words, so above it came out as
    // a new block. It is the same table still.
    keep_tables(old, &mut out);
    out
}

/// A page was written anew as Markdown and read back as `new`. Each table
/// of `new` that is no block of `old` is the table of `old` it was written
/// from, when there is one: it takes that table's id, and from it what
/// Markdown cannot say (its view, what its rows go by, so that the pages of
/// the rows find them: see `Table::adopt`). The two are paired by the name
/// of the table, then by the headings they share (the most first). Tables
/// that are left on both sides pair in their order only when as many are
/// left old as new: a table that was added takes nothing from another.
pub fn keep_tables(old: &[Block], new: &mut [Block]) {
    use crate::table::Table;
    let fresh: Vec<usize> = (0..new.len()).filter(|&i| new[i].kind == BlockKind::Table && !old.iter().any(|o| o.id == new[i].id)).collect();
    let spare: Vec<&Block> = old.iter().filter(|o| o.kind == BlockKind::Table && !new.iter().any(|b| b.id == o.id)).collect();
    if fresh.is_empty() || spare.is_empty() {
        return;
    }
    let mut now: Vec<Table> = fresh.iter().map(|&i| Table::parse(&new[i].data)).collect();
    let was: Vec<Table> = spare.iter().map(|b| Table::parse(&b.data)).collect();
    let headings = |t: &Table| -> Vec<String> { (0..t.columns()).map(|c| t.cell(0, c).trim().to_lowercase()).filter(|h| !h.is_empty()).collect() };
    let (heads_now, heads_was): (Vec<_>, Vec<_>) = (now.iter().map(headings).collect(), was.iter().map(headings).collect());
    let mut likeness: Vec<(usize, usize, usize)> = Vec::new();
    for n in 0..now.len() {
        for w in 0..was.len() {
            let named = !now[n].name.is_empty() && now[n].name.to_lowercase() == was[w].name.to_lowercase();
            let shared = heads_now[n].iter().filter(|h| heads_was[w].contains(h)).count();
            if named || shared > 0 {
                likeness.push((if named { 1000 } else { 0 } + shared, n, w));
            }
        }
    }
    // (The most alike first; among equals, those that come first.)
    likeness.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut pair: Vec<Option<usize>> = vec![None; now.len()];
    let mut taken = vec![false; was.len()];
    for (_, n, w) in likeness {
        if pair[n].is_none() && !taken[w] {
            pair[n] = Some(w);
            taken[w] = true;
        }
    }
    let left_now: Vec<usize> = (0..now.len()).filter(|&n| pair[n].is_none()).collect();
    let left_was: Vec<usize> = (0..was.len()).filter(|&w| !taken[w]).collect();
    if left_now.len() == left_was.len() {
        for (n, w) in left_now.into_iter().zip(left_was) {
            pair[n] = Some(w);
        }
    }
    for (n, w) in pair.into_iter().enumerate().filter_map(|(n, w)| Some((n, w?))) {
        let b = &mut new[fresh[n]];
        now[n].adopt(&was[w]);
        b.id = spare[w].id.clone();
        b.data = now[n].to_data();
        b.text = now[n].plain_text();
        b.place.beside |= spare[w].place.beside;
    }
}

/// Put a picture at a place. A free picture goes to the line it was let go
/// at, so that it stays with that text: before the block `to`, or after it
/// and all that is nested in it. Only the picture moves, and it takes the
/// nesting of that line. Returns whether anything changed.
pub fn place_picture(blocks: &mut Vec<Block>, id: &str, place: Place, to: Option<&str>, after: bool) -> bool {
    let Some(from) = index_of(blocks, id).filter(|&i| blocks[i].kind == BlockKind::Image) else { return false };
    let before = blocks.clone();
    blocks[from].place = place.tidy();
    if let Some(to) = to.filter(|to| *to != id && index_of(blocks, to).is_some()) {
        let mut picture = blocks.remove(from);
        let at = index_of(blocks, to).expect("the line is still there");
        picture.indent = blocks[at].indent;
        let at = if after { subtree_end(blocks, at) } else { at };
        blocks.insert(at, picture);
    }
    *blocks != before
}

/// Copy a block and its children right after the original. Returns the copy's id.
pub fn duplicate(blocks: &mut Vec<Block>, idx: usize) -> Id {
    let end = subtree_end(blocks, idx);
    let copies: Vec<Block> = blocks[idx..end]
        .iter()
        .cloned()
        .map(|mut b| {
            b.id = new_id();
            b
        })
        .collect();
    let first = copies[0].id.clone();
    for (offset, b) in copies.into_iter().enumerate() {
        blocks.insert(end + offset, b);
    }
    first
}

/// Delete a block and its children. Returns ids of pages linked from removed blocks.
pub fn delete(blocks: &mut Vec<Block>, idx: usize) -> Vec<Id> {
    let end = subtree_end(blocks, idx);
    let removed: Vec<Block> = blocks.drain(idx..end).collect();
    ensure_not_empty(blocks);
    removed.into_iter().filter_map(|b| b.target).collect()
}

/// A page always has at least one block to type into.
pub fn ensure_not_empty(blocks: &mut Vec<Block>) {
    if blocks.is_empty() {
        blocks.push(Block::text(""));
    }
}

/// Previous visible block that can take the caret.
pub fn prev_editable(blocks: &[Block], idx: usize) -> Option<usize> {
    let vis = visibility(blocks);
    (0..idx).rev().find(|&i| vis[i] && blocks[i].kind.is_editable())
}

/// Next visible block that can take the caret.
pub fn next_editable(blocks: &[Block], idx: usize) -> Option<usize> {
    let vis = visibility(blocks);
    (idx + 1..blocks.len()).find(|&i| vis[i] && blocks[i].kind.is_editable())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(blocks: &[Block]) -> Vec<(BlockKind, &str, u8)> {
        blocks.iter().map(|b| (b.kind, b.text.as_str(), b.indent)).collect()
    }

    #[test]
    fn utf16_positions_map_to_bytes() {
        let s = "a😀b";
        assert_eq!(utf16_len(s), 4);
        assert_eq!(utf16_to_byte(s, 0), 0);
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 6);
        assert_eq!(utf16_to_byte(s, 99), 6);
    }

    #[test]
    fn enter_splits_text_and_continues_lists() {
        let mut blocks = vec![Block::new(BlockKind::Bulleted, "hello world")];
        let caret = press_enter(&mut blocks, 0, 5, "hello world");
        assert_eq!(
            kinds(&blocks),
            vec![(BlockKind::Bulleted, "hello", 0), (BlockKind::Bulleted, " world", 0)]
        );
        assert_eq!(caret, Caret::start(&blocks[1].id));

        let mut blocks = vec![Block::new(BlockKind::Heading1, "Title")];
        press_enter(&mut blocks, 0, 5, "Title");
        assert_eq!(blocks[1].kind, BlockKind::Text);
    }

    #[test]
    fn enter_on_empty_list_item_leaves_the_list() {
        let mut blocks = vec![Block::new(BlockKind::Todo, "").with_indent(1)];
        press_enter(&mut blocks, 0, 0, "");
        assert_eq!(kinds(&blocks), vec![(BlockKind::Todo, "", 0)]);
        press_enter(&mut blocks, 0, 0, "");
        assert_eq!(kinds(&blocks), vec![(BlockKind::Text, "", 0)]);
    }

    #[test]
    fn enter_at_start_inserts_above() {
        let mut blocks = vec![Block::text("abc")];
        let id = blocks[0].id.clone();
        let caret = press_enter(&mut blocks, 0, 0, "abc");
        assert_eq!(kinds(&blocks), vec![(BlockKind::Text, "", 0), (BlockKind::Text, "abc", 0)]);
        assert_eq!(caret, Caret::start(&id));
    }

    #[test]
    fn enter_in_open_toggle_creates_child() {
        let mut blocks = vec![Block::new(BlockKind::Toggle, "Details")];
        press_enter(&mut blocks, 0, 7, "Details");
        assert_eq!(kinds(&blocks), vec![(BlockKind::Toggle, "Details", 0), (BlockKind::Text, "", 1)]);
    }

    #[test]
    fn backspace_unformats_then_outdents_then_merges() {
        let mut blocks = vec![Block::text("one"), Block::new(BlockKind::Quote, "two").with_indent(1)];
        press_backspace_at_start(&mut blocks, 1, "two");
        assert_eq!(blocks[1].kind, BlockKind::Text);
        press_backspace_at_start(&mut blocks, 1, "two");
        assert_eq!(blocks[1].indent, 0);
        let caret = press_backspace_at_start(&mut blocks, 1, "two").unwrap();
        assert_eq!(kinds(&blocks), vec![(BlockKind::Text, "onetwo", 0)]);
        assert_eq!(caret.pos, 3);
        assert!(press_backspace_at_start(&mut blocks, 0, "onetwo").is_none());
    }

    #[test]
    fn enter_and_backspace_keep_inline_formatting() {
        let mut blocks = vec![Block::text("**hello** world")];
        let caret = press_enter(&mut blocks, 0, 3, "**hello** world");
        assert_eq!(blocks[0].text, "**hel**");
        assert_eq!(blocks[1].text, "**lo** world");
        assert_eq!(caret, Caret::start(&blocks[1].id));
        let caret = press_backspace_at_start(&mut blocks, 1, "**lo** world").unwrap();
        assert_eq!(kinds(&blocks), vec![(BlockKind::Text, "**hello** world", 0)]);
        assert_eq!(caret.pos, 3);
    }

    #[test]
    fn code_blocks_keep_raw_text() {
        let mut b = Block::text("a\\*b");
        b.turn_into(BlockKind::Code);
        assert_eq!(b.text, "a*b");
        b.turn_into(BlockKind::Text);
        assert_eq!(b.text, "a\\*b");
    }

    #[test]
    fn backspace_after_divider_removes_it() {
        let mut blocks = vec![Block::text("a"), Block::new(BlockKind::Divider, ""), Block::text("b")];
        press_backspace_at_start(&mut blocks, 2, "b");
        assert_eq!(kinds(&blocks), vec![(BlockKind::Text, "a", 0), (BlockKind::Text, "b", 0)]);
    }

    #[test]
    fn indent_requires_a_parent_and_moves_children() {
        let mut blocks = vec![
            Block::text("a"),
            Block::text("b"),
            Block::text("c").with_indent(1),
        ];
        assert!(!indent(&mut blocks, 0));
        assert!(indent(&mut blocks, 1));
        assert_eq!(blocks.iter().map(|b| b.indent).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(!indent(&mut blocks, 1), "can't nest deeper than one level below the parent");
        assert!(outdent(&mut blocks, 1));
        assert_eq!(blocks.iter().map(|b| b.indent).collect::<Vec<_>>(), vec![0, 0, 1]);
    }

    #[test]
    fn collapsed_toggle_hides_children() {
        let mut t = Block::new(BlockKind::Toggle, "t");
        t.collapsed = true;
        let blocks = vec![t, Block::text("child").with_indent(1), Block::text("after")];
        assert_eq!(visibility(&blocks), vec![true, false, true]);
    }

    #[test]
    fn numbering_restarts_after_other_blocks() {
        let blocks = vec![
            Block::new(BlockKind::Numbered, "a"),
            Block::new(BlockKind::Numbered, "b"),
            Block::new(BlockKind::Numbered, "b1").with_indent(1),
            Block::new(BlockKind::Numbered, "c"),
            Block::text("break"),
            Block::new(BlockKind::Numbered, "x"),
        ];
        assert_eq!(
            list_numbers(&blocks),
            vec![Some(1), Some(2), Some(1), Some(3), None, Some(1)]
        );
    }

    #[test]
    fn move_block_carries_children() {
        let mut blocks = vec![
            Block::text("a"),
            Block::text("a1").with_indent(1),
            Block::text("b"),
        ];
        let (a, b) = (blocks[0].id.clone(), blocks[2].id.clone());
        assert!(move_block(&mut blocks, &a, &b, true));
        assert_eq!(
            kinds(&blocks),
            vec![(BlockKind::Text, "b", 0), (BlockKind::Text, "a", 0), (BlockKind::Text, "a1", 1)]
        );
        let a1 = blocks[2].id.clone();
        let a = blocks[1].id.clone();
        assert!(!move_block(&mut blocks, &a, &a1, false), "can't drop into own child");
    }

    #[test]
    fn move_up_and_down_swap_siblings() {
        let mut blocks = vec![Block::text("a"), Block::text("b"), Block::text("b1").with_indent(1)];
        assert!(move_up(&mut blocks, 1));
        assert_eq!(kinds(&blocks)[0], (BlockKind::Text, "b", 0));
        assert_eq!(kinds(&blocks)[1], (BlockKind::Text, "b1", 1));
        assert!(move_down(&mut blocks, 0));
        assert_eq!(kinds(&blocks)[0], (BlockKind::Text, "a", 0));
    }

    fn picture(place: Place) -> Block {
        let mut b = Block::new(BlockKind::Image, "");
        b.data = "abcdefghij0123456789.png".into();
        b.place = place;
        b
    }

    #[test]
    fn a_picture_goes_to_the_line_it_is_let_go_at() {
        let free = Place { free: true, right: true, x: 42, y: 0, w: 300, beside: false };
        let mut blocks = vec![
            picture(Place::default()),
            Block::text("a"),
            Block::new(BlockKind::Toggle, "b"),
            Block::text("b1").with_indent(1),
            Block::text("c"),
        ];
        let (pic, b, b1, c) = (blocks[0].id.clone(), blocks[2].id.clone(), blocks[3].id.clone(), blocks[4].id.clone());
        // Before a nested line: it takes that nesting, so the toggle keeps its lines.
        assert!(place_picture(&mut blocks, &pic, free, Some(&b1), false));
        assert_eq!(blocks.iter().map(|x| (x.text.as_str(), x.indent, x.floats())).collect::<Vec<_>>(), vec![
            ("a", 0, false),
            ("b", 0, false),
            ("", 1, true),
            ("b1", 1, false),
            ("c", 0, false),
        ]);
        assert_eq!(blocks[2].place, free);
        // After a block is after all that is nested in it.
        assert!(place_picture(&mut blocks, &pic, free, Some(&b), true));
        assert_eq!(blocks.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["a", "b", "b1", "", "c"]);
        assert_eq!(blocks[3].indent, 0);
        // The same place again changes nothing; an unknown line moves nothing.
        assert!(!place_picture(&mut blocks, &pic, free, Some(&c), false));
        assert!(!place_picture(&mut blocks, &pic, free, Some("gone"), false));
        assert!(!place_picture(&mut blocks, &c, free, None, false), "only pictures have a place");
        // Back in line: it keeps its width and its spot in the list.
        assert!(place_picture(&mut blocks, &pic, Place { free: false, right: true, x: 9, y: 9, w: 300, beside: false }, None, false));
        assert_eq!(blocks[3].place, Place { free: false, right: false, x: 0, y: 0, w: 300, beside: false });
        assert!(!blocks[3].floats());
        // Numbers out of range are brought in.
        assert_eq!(Place { free: true, right: true, x: 999_999, y: -999_999, w: 3, beside: false }.tidy(), Place { free: true, right: true, x: 20_000, y: 0, w: 40, beside: false });
        // A file from the first form of free pictures (with a height) still reads.
        assert_eq!(Place::from_column(r#"{"free":true,"x":120,"y":64,"w":300}"#), Place { free: true, right: false, x: 120, y: 0, w: 300, beside: false });
        assert_eq!(Place::from_column(&free.to_column()), free);
        assert_eq!(Place::default().to_column(), "");
        assert_eq!(Place::from_column("not json"), Place::default());
        // Another kind of block has no place.
        blocks[3].place = free;
        blocks[3].turn_into(BlockKind::Text);
        assert!(blocks[3].place.is_plain());
    }

    #[test]
    fn a_page_written_anew_keeps_the_lines_that_did_not_change() {
        let mut toggle = Block::new(BlockKind::Toggle, "Details");
        toggle.collapsed = true;
        let mut todo = Block::new(BlockKind::Todo, "Buy shoes");
        todo.checked = false;
        let old = vec![
            Block::new(BlockKind::Heading2, "Plan"),
            Block::text("Monday: run **5 km**"),
            toggle.clone(),
            Block::text("hidden line").with_indent(1),
            todo.clone(),
            picture(Place { free: true, right: true, x: 0, y: 0, w: 200, beside: false }),
            Block::text("Old closing line"),
        ];
        // As read back from the AI's Markdown: new ids, the toggle a bullet,
        // one line changed, one gone, one added, the to-do ticked.
        let mut ticked = Block::new(BlockKind::Todo, "Buy shoes");
        ticked.checked = true;
        let new = vec![
            Block::new(BlockKind::Heading2, "Plan"),
            Block::text("Monday: swim"),
            Block::new(BlockKind::Bulleted, "Details"),
            Block::text("hidden line").with_indent(1),
            ticked,
            old[5].clone(),
            Block::text("A new line"),
        ];
        let out = keep_unchanged(&old, new);
        let ids = |bs: &[Block]| bs.iter().map(|b| b.id.clone()).collect::<Vec<_>>();
        assert_eq!(out.len(), 7);
        // The heading, the toggle (still a closed toggle), its line, the to-do and the picture are the old blocks.
        assert_eq!(out[0], old[0]);
        assert_eq!((out[2].kind, out[2].collapsed, out[2].id.as_str()), (BlockKind::Toggle, true, old[2].id.as_str()));
        assert_eq!(out[3], old[3]);
        assert_eq!((out[4].id.as_str(), out[4].checked), (old[4].id.as_str(), true));
        assert_eq!(out[5], old[5]);
        // The changed line and the added line are new blocks; the old closing line is gone.
        assert_eq!(out[1].text, "Monday: swim");
        assert!(!ids(&old).contains(&out[1].id) && !ids(&old).contains(&out[6].id));
        assert!(!out.iter().any(|b| b.text == "Old closing line"));
        // A line whose kind the Markdown changed keeps its id and takes the new kind.
        let old = vec![Block::text("Title")];
        let out = keep_unchanged(&old, vec![Block::new(BlockKind::Heading1, "Title")]);
        assert_eq!((out[0].kind, out[0].id.as_str()), (BlockKind::Heading1, old[0].id.as_str()));
        assert!(keep_unchanged(&old, Vec::new()).is_empty());
    }

    #[test]
    fn a_table_written_again_is_the_same_table() {
        use crate::table::{Table, Total};
        // A page with two tables; the first has a view and rows that go by names.
        let make = |rows: &[[&str; 2]]| Table::of(rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect());
        let mut budget = make(&[["Item", "Amount"], ["Rent", "$2,000.00"], ["Food", "$150.00"]]);
        budget.set_kind(1, crate::table::Kind::Number);
        budget.set_format(1, "USD");
        budget.set_total(1, Some(Total::Sum));
        budget.change_view(|v| v.sort_by(1, true));
        budget.name_rows();
        let other = make(&[["Name", "Note"], ["a", "b"]]);
        let block = |t: &Table, beside: bool| {
            let mut b = Block::new(BlockKind::Table, t.plain_text());
            b.data = t.to_data();
            b.place.beside = beside;
            b
        };
        let old = vec![Block::text("Intro"), block(&budget, false), block(&other, true), Block::text("End")];
        // The AI changes one cell of the first table and leaves the rest.
        let md = crate::markdown::from_blocks("", &old, |_| String::new()).replace("$150.00", "$175.00");
        let out = keep_unchanged(&old, crate::markdown::to_blocks(&md));
        assert_eq!(out.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), old.iter().map(|b| b.id.as_str()).collect::<Vec<_>>());
        let changed = Table::parse(&out[1].data);
        assert_eq!(changed.cell(2, 1), "$175.00");
        assert_eq!(changed.total(1), Some(("Sum", "$2,175.00".to_string())));
        // What Markdown cannot say stayed: the sort, and what the rows go by.
        assert_eq!(changed.view, budget.view);
        assert_eq!((changed.row_id(1), changed.row_id(2)), (budget.row_id(1), budget.row_id(2)));
        // The table that did not change is the old block as it was, beside the first.
        assert_eq!(out[2], old[2]);
        assert!(out[2].place.beside && !out[1].place.beside);
    }

    #[test]
    fn a_table_put_above_another_takes_nothing_from_it() {
        use crate::table::Table;
        let make = |name: &str, rows: &[[&str; 2]]| {
            let mut t = Table::of(rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect());
            t.name = name.to_string();
            t
        };
        let block = |t: &Table| {
            let mut b = Block::new(BlockKind::Table, t.plain_text());
            b.data = t.to_data();
            b
        };
        let mut budget = make("", &[["Item", "Amount"], ["Rent", "2000"], ["Food", "150"]]);
        budget.change_view(|v| v.sort_by(1, true));
        budget.name_rows();
        let mut plans = make("Plans", &[["Name", "Note"], ["Trip", "May"]]);
        plans.name_rows();
        let old = vec![Block::text("Intro"), block(&budget), block(&plans)];
        // The AI puts a new table first, changes a cell of each of the two, and gives the second other headings.
        let md = crate::markdown::from_blocks("", &old, |_| String::new());
        let md = md.replace("| Food | 150 |", "| Food | 175 |").replace("| Name | Note |", "| What | When |").replace("| Trip | May |", "| Trip | June |");
        let md = format!("| Item | Shop |\n| --- | --- |\n| Milk | Corner |\n\n{md}");
        let out = keep_unchanged(&old, crate::markdown::to_blocks(&md));
        let tables: Vec<&Block> = out.iter().filter(|b| b.kind == BlockKind::Table).collect();
        assert_eq!(tables.len(), 3);
        // The new table is a new block; the two others are the blocks they were, with what Markdown cannot say.
        assert!(old.iter().all(|o| o.id != tables[0].id));
        assert_eq!((tables[1].id.as_str(), tables[2].id.as_str()), (old[1].id.as_str(), old[2].id.as_str()));
        let (new_one, budget_now, plans_now) = (Table::parse(&tables[0].data), Table::parse(&tables[1].data), Table::parse(&tables[2].data));
        assert!(new_one.view.is_plain() && !new_one.has_ids());
        assert_eq!((budget_now.view.clone(), budget_now.row_id(2), budget_now.cell(2, 1)), (budget.view.clone(), budget.row_id(2), "175"));
        assert_eq!((plans_now.name.as_str(), plans_now.row_id(1), plans_now.cell(1, 1)), ("Plans", plans.row_id(1), "June"));
        // Through the connector (no line of the page is kept as it was) the tables are found the same way.
        let mut fresh = crate::markdown::to_blocks(&md);
        keep_tables(&old, &mut fresh);
        let ids: Vec<&str> = fresh.iter().filter(|b| b.kind == BlockKind::Table).map(|b| b.id.as_str()).collect();
        assert_eq!(&ids[1..], [old[1].id.as_str(), old[2].id.as_str()]);
        // A table that is taken out and another put in its place: one old, one new, so they pair (the id stays; no row is the same row).
        let mut swapped = crate::markdown::to_blocks("| Colour | Code |\n| --- | --- |\n| Red | 1 |");
        keep_tables(&old[..2], &mut swapped);
        assert_eq!(swapped[0].id, old[1].id);
        assert!(!Table::parse(&swapped[0].data).has_ids());
    }

    #[test]
    fn a_free_picture_is_not_a_line() {
        let free = picture(Place { free: true, right: false, x: 0, y: 0, w: 200, beside: false });
        // The count of a numbered list goes on past it.
        let blocks = vec![Block::new(BlockKind::Numbered, "one"), free.clone(), Block::new(BlockKind::Numbered, "two")];
        assert_eq!(list_numbers(&blocks), vec![Some(1), None, Some(2)]);
        let in_line = vec![Block::new(BlockKind::Numbered, "one"), picture(Place::default()), Block::new(BlockKind::Numbered, "two")];
        assert_eq!(list_numbers(&in_line), vec![Some(1), None, Some(1)]);
        // Backspace at the start of a line joins it with the line before the picture.
        let mut blocks = vec![Block::text("one"), free.clone(), Block::text("two")];
        let caret = press_backspace_at_start(&mut blocks, 2, "two").unwrap();
        assert_eq!(blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["onetwo", ""]);
        assert_eq!((caret.id.as_str(), caret.pos), (blocks[0].id.as_str(), 3));
        assert!(blocks[1].floats());
        // Move up and down change places with lines, not with free pictures.
        let mut blocks = vec![Block::text("a"), free.clone(), Block::text("b")];
        assert!(move_up(&mut blocks, 2));
        assert_eq!(blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["b", "a", ""]);
        let mut blocks = vec![Block::text("a"), free, Block::text("b")];
        assert!(move_down(&mut blocks, 0));
        assert_eq!(blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["", "b", "a"]);
    }

    #[test]
    fn duplicate_and_delete_handle_subtrees() {
        let mut blocks = vec![Block::text("a"), Block::text("a1").with_indent(1)];
        duplicate(&mut blocks, 0);
        assert_eq!(blocks.len(), 4);
        assert_ne!(blocks[0].id, blocks[2].id);
        delete(&mut blocks, 0);
        delete(&mut blocks, 0);
        assert_eq!(kinds(&blocks), vec![(BlockKind::Text, "", 0)]);
    }
}
