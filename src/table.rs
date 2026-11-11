//! Tables: rows of cells, with the first row as the header. A table is one
//! block; its cells are kept in the block's `data` (JSON), and it reads and
//! writes Markdown's pipe tables.
//!
//! A column can have a kind (a date, a number, a time, a pace, a checkbox, a
//! choice, a link, a formula…), a unit, a format (money, percent) and a
//! total that shows at the foot of the table. Cells stay text: the kind says
//! how to read them. A pace is not typed in; it is worked out from the first
//! number column and the first time column of its row (a distance and a
//! time). A formula is worked out from the cells of its row.
//!
//! A table can also have a name, and a view: which rows show (filters), in
//! what order (sorts), in what layout (see view.rs). A row can have a page
//! of its own, and can be nested under the row above it (a sub-item).
//!
//! In Markdown the kinds are on a comment line right under the table, and
//! the name on a second one, so they survive a page that is written again:
//!
//! ```text
//! | Item | Amount |
//! | --- | --- |
//! | Rent | $2,000.00 |
//! <!-- columns: text | number USD sum -->
//! <!-- table: Expenses -->
//! ```

use crate::formula::{Formula, Value};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::cell::OnceCell;

pub mod choice;
pub mod money;
pub mod view;
pub use choice::{Choice, Color};
pub use view::{Layout, Op, Rule, View};

/// The most rows and columns a table may have.
pub const MOST_ROWS: usize = 200;
pub const MOST_COLUMNS: usize = 12;
/// How deep a row can be nested under other rows.
pub const MOST_NESTING: u8 = 3;

/// How the cells of a column are read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Text,
    Number,
    Date,
    /// A length of time: "34:00", "1:02:30", "34 min".
    Time,
    /// Time for each unit of distance, worked out (see the module notes).
    Pace,
    Checkbox,
    /// One of a few words ("Easy", "Long"), shown as a quiet tag.
    Choice,
    /// Some of a few words ("Home, Food"), each a tag.
    Multi,
    /// Where something stands: one word of a few, with a dot in its tag.
    /// It starts with "Not started", "In progress" and "Done".
    Status,
    /// A web address, opened by a click.
    Link,
    Email,
    Phone,
    /// Pictures kept with the notes (see files.rs); the cell holds their names.
    File,
    /// Worked out from other cells of the row by `Column::formula`.
    Formula,
    /// Rows of another table (`Column::from`), by their names.
    Relation,
}

impl Kind {
    pub const ALL: [Kind; 15] = [
        Kind::Text,
        Kind::Number,
        Kind::Choice,
        Kind::Multi,
        Kind::Status,
        Kind::Date,
        Kind::Time,
        Kind::Pace,
        Kind::Checkbox,
        Kind::Link,
        Kind::Email,
        Kind::Phone,
        Kind::File,
        Kind::Formula,
        Kind::Relation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Number => "number",
            Kind::Date => "date",
            Kind::Time => "time",
            Kind::Pace => "pace",
            Kind::Checkbox => "checkbox",
            Kind::Choice => "choice",
            Kind::Multi => "multi",
            Kind::Status => "status",
            Kind::Link => "link",
            Kind::Email => "email",
            Kind::Phone => "phone",
            Kind::File => "file",
            Kind::Formula => "formula",
            Kind::Relation => "relation",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "Text",
            Kind::Number => "Number",
            Kind::Date => "Date",
            Kind::Time => "Time",
            Kind::Pace => "Pace",
            Kind::Checkbox => "Checkbox",
            Kind::Choice => "Choice",
            Kind::Multi => "Several choices",
            Kind::Status => "Status",
            Kind::Link => "Link",
            Kind::Email => "Email",
            Kind::Phone => "Phone",
            Kind::File => "Picture",
            Kind::Formula => "Formula",
            Kind::Relation => "Row of another table",
        }
    }

    /// Name of its icon (see `ui/icons.rs`).
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Number => "hash",
            Kind::Date => "calendar",
            Kind::Time => "clock",
            Kind::Pace => "gauge",
            Kind::Checkbox => "checkbox",
            Kind::Choice => "choice",
            Kind::Multi => "choices",
            Kind::Status => "status",
            Kind::Link => "link",
            Kind::Email => "mail",
            Kind::Phone => "phone",
            Kind::File => "image",
            Kind::Formula => "sigma",
            Kind::Relation => "arrow-up-right",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        let s = s.trim();
        // (Notion's words for the two, which an AI is apt to write.)
        match s.to_lowercase().as_str() {
            "select" => return Some(Kind::Choice),
            "tags" | "multi-select" | "multiselect" => return Some(Kind::Multi),
            _ => {}
        }
        Kind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(s))
    }

    /// Its cells hold words from a list of options with colours.
    pub fn has_options(self) -> bool {
        matches!(self, Kind::Choice | Kind::Multi | Kind::Status)
    }

    /// The totals that mean something for this kind of column.
    pub fn totals(self) -> &'static [Total] {
        match self {
            Kind::Number | Kind::Time | Kind::Formula => &[Total::Sum, Total::Average, Total::Median, Total::Highest, Total::Lowest, Total::Range, Total::Count, Total::Unique, Total::Empty],
            Kind::Pace => &[Total::Average, Total::Highest, Total::Lowest],
            Kind::Checkbox => &[Total::Checked],
            _ => &[Total::Count, Total::Unique, Total::Empty],
        }
    }

    /// Its cells are amounts, which charts can draw and totals add up.
    pub fn is_amount(self) -> bool {
        matches!(self, Kind::Number | Kind::Time | Kind::Pace | Kind::Formula)
    }

    /// Its cells are worked out, not typed.
    pub fn is_worked_out(self) -> bool {
        matches!(self, Kind::Pace | Kind::Formula)
    }
}

/// What shows at the foot of a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Total {
    Count,
    Sum,
    Average,
    /// The middle amount.
    Median,
    Highest,
    Lowest,
    /// From the lowest to the highest.
    Range,
    /// How many different values there are.
    Unique,
    /// How many cells have nothing in them.
    Empty,
    /// How many boxes are ticked, of how many rows.
    Checked,
}

impl Total {
    pub const ALL: [Total; 10] = [Total::Count, Total::Sum, Total::Average, Total::Median, Total::Highest, Total::Lowest, Total::Range, Total::Unique, Total::Empty, Total::Checked];

    pub fn name(self) -> &'static str {
        match self {
            Total::Count => "count",
            Total::Sum => "sum",
            Total::Average => "average",
            Total::Median => "median",
            Total::Highest => "highest",
            Total::Lowest => "lowest",
            Total::Range => "range",
            Total::Unique => "unique",
            Total::Empty => "empty",
            Total::Checked => "checked",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Total::Count => "Count",
            Total::Sum => "Sum",
            Total::Average => "Average",
            Total::Median => "Median",
            Total::Highest => "Highest",
            Total::Lowest => "Lowest",
            Total::Range => "Range",
            Total::Unique => "Different values",
            Total::Empty => "Empty cells",
            Total::Checked => "Ticked",
        }
    }

    /// The short word written before the total at the foot.
    pub fn short(self) -> &'static str {
        match self {
            Total::Sum => "Sum",
            Total::Average => "Avg",
            Total::Median => "Med",
            Total::Highest => "Max",
            Total::Lowest => "Min",
            Total::Range => "Range",
            Total::Count | Total::Unique | Total::Empty | Total::Checked => "",
        }
    }

    pub fn parse(s: &str) -> Option<Total> {
        let s = s.trim();
        Total::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(s)).or(match s.to_ascii_lowercase().as_str() {
            "avg" | "mean" => Some(Total::Average),
            "max" | "most" => Some(Total::Highest),
            "min" | "least" => Some(Total::Lowest),
            "total" => Some(Total::Sum),
            "med" => Some(Total::Median),
            "different" => Some(Total::Unique),
            _ => None,
        })
    }
}

/// How a number shows in its cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Show {
    #[default]
    Number,
    /// The number, and a bar that is full at the column's goal.
    Bar,
    /// The number, and a ring that is closed at the column's goal.
    Ring,
}

impl Show {
    pub const ALL: [Show; 3] = [Show::Number, Show::Bar, Show::Ring];

    pub fn label(self) -> &'static str {
        match self {
            Show::Number => "Number",
            Show::Bar => "Bar",
            Show::Ring => "Ring",
        }
    }

    fn is_number(&self) -> bool {
        *self == Show::Number
    }
}

/// How one column is read, and what shows at its foot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Column {
    #[serde(default)]
    pub kind: Kind,
    /// Written after the heading's name in the column menu ("km").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<Total>,
    /// How its numbers are written: "" as typed, "grouped", "percent", or
    /// the code of a kind of money ("USD"). See money.rs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub format: String,
    /// Places after the point, when that is not the format's own number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub places: Option<u8>,
    #[serde(default, skip_serializing_if = "Show::is_number")]
    pub show: Show,
    /// The amount at which a bar is full or a ring closed (else the highest
    /// amount of the column, or 100 for a percent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<f64>,
    /// The words a choice column offers, in their order. See choice.rs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    /// What a formula column works out ("Price * Qty"). See formula.rs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub formula: String,
    /// The table (its block's id) whose rows a relation column names.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    /// Amounts above zero show green and below it red (a gain and a loss).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed: bool,
}

/// What a row has besides its cells.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RowInfo {
    /// What the row goes by, whatever its place among the rows: the page of
    /// a row finds its row by this (see `Page::row_of`). "" until a row of
    /// the table is opened as a page the first time; then every row has one,
    /// and each new row gets one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// How deep it is nested under the rows above it (0: not at all).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub indent: u8,
    /// The rows nested in it are folded away.
    #[serde(default, skip_serializing_if = "is_false")]
    pub closed: bool,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// What a new row starts with. (The lines its page starts with are those of
/// the template's own page: see `Page::row_of`.)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Template {
    /// One for each column; an empty one leaves the cell empty.
    #[serde(default)]
    pub cells: Vec<String>,
}

/// A name for a row that no other row has.
fn fresh_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// The formulas of the columns, read once for each table in memory.
#[derive(Clone, Debug, Default)]
struct Parsed(OnceCell<Vec<Option<Formula>>>);

impl PartialEq for Parsed {
    /// (Not part of what a table is: two tables with the same cells are the same.)
    fn eq(&self, _: &Parsed) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Every row has the same number of cells; the first row is the header.
    pub rows: Vec<Vec<String>>,
    /// One for each column, or none at all when every column is plain text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    /// Its name, shown above it ("" for none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The icon beside its name ("name:color", see `ui/icons.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// One for each row (the header too), or none at all when no row has a
    /// page or is nested.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub info: Vec<RowInfo>,
    #[serde(default, skip_serializing_if = "View::is_plain")]
    pub view: View,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,
    /// It stands beside the table above it. This is kept with the block
    /// (`Place::beside`), not in the table: it is here only on the way to
    /// and from Markdown.
    #[serde(skip)]
    pub beside: bool,
    #[serde(skip)]
    parsed: Parsed,
}

impl Default for Table {
    /// A new table: a header and two rows, three columns wide.
    fn default() -> Table {
        Table::of(vec![vec![String::new(); 3]; 3])
    }
}

impl Table {
    /// A plain table of these rows.
    pub fn of(rows: Vec<Vec<String>>) -> Table {
        let mut table = Table { rows, columns: Vec::new(), name: String::new(), icon: String::new(), info: Vec::new(), view: View::default(), template: None, beside: false, parsed: Parsed::default() };
        table.square();
        table
    }

    /// The table kept in a block (a new one if there is none yet, or it
    /// cannot be read).
    pub fn parse(data: &str) -> Table {
        let mut table: Table = serde_json::from_str(data).unwrap_or_default();
        table.square();
        table
    }

    pub fn to_data(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn columns(&self) -> usize {
        self.rows.first().map(Vec::len).unwrap_or(0)
    }

    /// Make every row as wide as the widest, with at least one cell at all;
    /// keep one description for each column and each row (or none), and
    /// nothing that names a column or a nesting that is not there.
    fn square(&mut self) {
        self.rows.truncate(MOST_ROWS);
        let width = self.rows.iter().map(Vec::len).max().unwrap_or(0).clamp(1, MOST_COLUMNS);
        if self.rows.is_empty() {
            self.rows.push(Vec::new());
        }
        for row in &mut self.rows {
            row.resize(width, String::new());
        }
        if self.columns.iter().all(|c| *c == Column::default()) {
            self.columns.clear();
        } else {
            self.columns.resize(width, Column::default());
        }
        if !self.info.is_empty() {
            self.info.resize(self.rows.len(), RowInfo::default());
            // The header and the first row are not nested, and no row is
            // more than one step deeper than the row above it.
            let mut above = 0;
            for (r, info) in self.info.iter_mut().enumerate() {
                let most = if r <= 1 { 0 } else { (above + 1).min(MOST_NESTING) };
                info.indent = info.indent.min(most);
                above = info.indent;
            }
            // (The header is no row to open.)
            self.info[0].id.clear();
            // (After that, since putting this right can leave nothing to keep.)
            if self.info.iter().all(|i| *i == RowInfo::default()) {
                self.info.clear();
            }
        }
        if let Some(template) = &mut self.template {
            template.cells.resize(width, String::new());
        }
        self.view.tidy(width);
        self.sync_options();
        self.parsed = Parsed::default();
    }

    /// Put text in a cell. Line ends become spaces: a cell is one line.
    pub fn set(&mut self, row: usize, column: usize, text: &str) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(column)) {
            *cell = text.replace(['\n', '\r'], " ");
            if self.kind(column).has_options() {
                self.sync_options();
            }
        }
    }

    // ----- rows ------------------------------------------------------------

    /// What a row has besides its cells.
    pub fn info(&self, r: usize) -> RowInfo {
        self.info.get(r).cloned().unwrap_or_default()
    }

    fn info_mut(&mut self, r: usize) -> Option<&mut RowInfo> {
        if r == 0 || r >= self.rows.len() {
            return None;
        }
        if self.info.len() < self.rows.len() {
            self.info.resize(self.rows.len(), RowInfo::default());
        }
        self.info.get_mut(r)
    }

    /// How deep a row is nested.
    pub fn indent(&self, r: usize) -> u8 {
        self.info.get(r).map(|i| i.indent).unwrap_or(0)
    }

    /// The row after `r` and the rows nested in it.
    pub fn subtree_end(&self, r: usize) -> usize {
        let level = self.indent(r);
        let mut end = r + 1;
        while end < self.rows.len() && self.indent(end) > level {
            end += 1;
        }
        end
    }

    /// Rows are nested in this row.
    pub fn has_children(&self, r: usize) -> bool {
        r >= 1 && r + 1 < self.rows.len() && self.indent(r + 1) > self.indent(r)
    }

    /// What a row goes by, if the rows of the table have such names yet.
    pub fn row_id(&self, r: usize) -> Option<&str> {
        self.info.get(r).map(|i| i.id.as_str()).filter(|id| !id.is_empty())
    }

    /// The row that goes by `id`.
    pub fn row_with_id(&self, id: &str) -> Option<usize> {
        (!id.is_empty()).then(|| self.info.iter().position(|i| i.id == id)).flatten()
    }

    /// The rows have names of their own (see `RowInfo::id`).
    pub fn has_ids(&self) -> bool {
        self.info.iter().any(|i| !i.id.is_empty())
    }

    /// This table is an older form of `now` (a kept version of the page
    /// put back): its rows take what the rows of `now` with the same names
    /// go by, each once, so that the pages of the rows still find them. A
    /// row that finds none gets a name of its own if the others have one.
    /// True if a row got a name.
    pub fn keep_row_names(&mut self, now: &Table) -> bool {
        let mut left: Vec<usize> = (1..now.rows.len()).filter(|&r| now.row_id(r).is_some_and(|id| self.row_with_id(id).is_none())).collect();
        let mut took = false;
        for r in 1..self.rows.len() {
            let name = self.title(r).to_lowercase();
            if self.row_id(r).is_some() || name.is_empty() {
                continue;
            }
            if let Some(i) = left.iter().position(|&o| now.title(o).to_lowercase() == name) {
                let id = now.row_id(left.remove(i)).unwrap_or_default().to_string();
                if let Some(info) = self.info_mut(r) {
                    info.id = id;
                    took = true;
                }
            }
        }
        if self.has_ids() {
            took |= self.name_rows();
        }
        self.square();
        took
    }

    /// Give every row under the header that has none a name of its own.
    /// True if a row got one.
    pub fn name_rows(&mut self) -> bool {
        let lacking: Vec<usize> = (1..self.rows.len()).filter(|&r| self.row_id(r).is_none()).collect();
        for &r in &lacking {
            if let Some(info) = self.info_mut(r) {
                info.id = fresh_id();
            }
        }
        !lacking.is_empty()
    }

    /// The name of a row: its first cell.
    pub fn title(&self, r: usize) -> &str {
        self.cell(r, 0).trim()
    }

    fn insert_cells(&mut self, at: usize, cells: Vec<String>, indent: u8) {
        let at = at.min(self.rows.len());
        // (In a table whose rows have names, a new row has one from the start.)
        let id = if self.has_ids() { fresh_id() } else { String::new() };
        if !self.info.is_empty() || indent > 0 {
            self.info.resize(self.rows.len(), RowInfo::default());
            self.info.insert(at, RowInfo { id, indent, closed: false });
        }
        self.rows.insert(at, cells);
    }

    /// A new row after `row` and the rows nested in it (at the end if there
    /// is no such row). It starts with what the template says. Returns its
    /// number, or `None` when the table is full.
    pub fn add_row(&mut self, row: usize) -> Option<usize> {
        if self.rows.len() >= MOST_ROWS {
            return None;
        }
        let (at, indent) = match row {
            0 => (1, 0),
            r if r < self.rows.len() => (self.subtree_end(r), self.indent(r)),
            _ => (self.rows.len(), 0),
        };
        let template = self.template.as_ref().map(|t| t.cells.clone()).unwrap_or_default();
        self.insert_cells(at, vec![String::new(); self.columns()], indent);
        let r = at.min(self.rows.len() - 1);
        // What the template says is written the way each column writes: a
        // "today" in a date column is the day the row was made.
        let today = crate::when::today();
        for (c, text) in template.iter().enumerate().take(self.columns()) {
            if !text.trim().is_empty() && !self.kind(c).is_worked_out() {
                self.put(r, c, text, today);
            }
        }
        Some(r)
    }

    /// A copy of a row (its cells, not its page: the copy is a row of its
    /// own) right after it.
    pub fn duplicate_row(&mut self, r: usize) -> Option<usize> {
        if r == 0 || r >= self.rows.len() || self.rows.len() >= MOST_ROWS {
            return None;
        }
        let at = self.subtree_end(r);
        self.insert_cells(at, self.rows[r].clone(), self.indent(r));
        Some(at)
    }

    /// Take a row out; the rows nested in it move out one step. The last
    /// row stays.
    pub fn remove_row(&mut self, row: usize) {
        if self.rows.len() > 1 && row < self.rows.len() {
            for r in row + 1..self.subtree_end(row) {
                if let Some(info) = self.info.get_mut(r) {
                    info.indent = info.indent.saturating_sub(1);
                }
            }
            self.rows.remove(row);
            if row < self.info.len() {
                self.info.remove(row);
            }
            self.square();
        }
    }

    /// Move a row, with the rows nested in it, to before row `to` (the
    /// number of rows puts it last). It is nested as deep as the row it
    /// lands before (at the end: not nested), so a row put between two
    /// nested rows is one of them, and takes none of them from the row
    /// they are nested in. Returns where it is now.
    pub fn move_row(&mut self, from: usize, to: usize) -> Option<usize> {
        let deep = if to >= 1 && to < self.rows.len() { self.indent(to) } else { 0 };
        self.move_row_as(from, to, deep)
    }

    /// Move a row to after row `after` and the rows nested in it, as deep
    /// as that row.
    pub fn move_row_after(&mut self, from: usize, after: usize) -> Option<usize> {
        self.move_row_as(from, self.subtree_end(after), self.indent(after))
    }

    /// Move a row, with the rows nested in it (they stay as deep under
    /// it), to before row `to`, nested `now` deep.
    fn move_row_as(&mut self, from: usize, to: usize, now: u8) -> Option<usize> {
        let n = self.rows.len();
        if from == 0 || from >= n {
            return None;
        }
        let end = self.subtree_end(from);
        let to = to.clamp(1, n);
        let (was, now) = (self.indent(from), now.min(MOST_NESTING));
        if (from..=end).contains(&to) {
            // It is at that place: it can still be nested another way there
            // (let go on the lower half of the nested row above it).
            if now == was {
                return None;
            }
            for r in from..end {
                if let Some(info) = self.info_mut(r) {
                    info.indent = (info.indent - was + now).min(MOST_NESTING);
                }
            }
            self.square();
            return Some(from);
        }
        if !self.info.is_empty() || now > 0 {
            self.info.resize(n, RowInfo::default());
        }
        let at = if to > from { to - (end - from) } else { to };
        let cells: Vec<Vec<String>> = self.rows.drain(from..end).collect();
        self.rows.splice(at..at, cells);
        if !self.info.is_empty() {
            let mut infos: Vec<RowInfo> = self.info.drain(from..end).collect();
            for info in &mut infos {
                info.indent = (info.indent - was + now).min(MOST_NESTING);
            }
            self.info.splice(at..at, infos);
        }
        self.square();
        Some(at)
    }

    /// Nest a row (and the rows nested in it) one step deeper, under the
    /// row above it, or move it out one step.
    pub fn nest(&mut self, r: usize, deeper: bool) {
        if r == 0 || r >= self.rows.len() {
            return;
        }
        let now = self.indent(r);
        let can = if deeper { r >= 2 && now <= self.indent(r - 1) && now < MOST_NESTING } else { now > 0 };
        if !can {
            return;
        }
        for i in r..self.subtree_end(r) {
            if let Some(info) = self.info_mut(i) {
                info.indent = if deeper { info.indent + 1 } else { info.indent - 1 };
            }
        }
        self.square();
    }

    /// Fold the rows nested in a row away, or show them again.
    pub fn toggle_closed(&mut self, r: usize) {
        if let Some(info) = self.info_mut(r) {
            info.closed = !info.closed;
        }
        self.square();
    }

    // ----- columns come and go ----------------------------------------------

    /// The cells of every row, and of the template.
    fn all_rows(&mut self) -> impl Iterator<Item = &mut Vec<String>> {
        self.rows.iter_mut().chain(self.template.iter_mut().map(|t| &mut t.cells))
    }

    /// A new empty column at place `at`. False when the table is full.
    pub fn insert_column(&mut self, at: usize) -> bool {
        if self.columns() >= MOST_COLUMNS {
            return false;
        }
        let at = at.min(self.columns());
        for row in self.all_rows() {
            row.insert(at.min(row.len()), String::new());
        }
        if !self.columns.is_empty() {
            self.columns.insert(at.min(self.columns.len()), Column::default());
        }
        self.view.remap(|c| Some(if c >= at { c + 1 } else { c }));
        self.square();
        true
    }

    /// A new empty column after `column`.
    pub fn add_column(&mut self, column: usize) {
        self.insert_column(column + 1);
    }

    /// Take a column out. The last column stays.
    pub fn remove_column(&mut self, column: usize) {
        if self.columns() > 1 && column < self.columns() {
            for row in self.all_rows() {
                if column < row.len() {
                    row.remove(column);
                }
            }
            if column < self.columns.len() {
                self.columns.remove(column);
            }
            self.view.remap(|c| match c.cmp(&column) {
                std::cmp::Ordering::Less => Some(c),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(c - 1),
            });
            self.square();
        }
    }

    /// A copy of a column right after it.
    pub fn duplicate_column(&mut self, c: usize) -> bool {
        if c >= self.columns() || !self.insert_column(c + 1) {
            return false;
        }
        for row in self.all_rows() {
            row[c + 1] = row[c].clone();
        }
        self.rows[0][c + 1] = format!("{} 2", self.heading(c));
        let col = self.column(c);
        if col != Column::default() {
            if let Some(copy) = self.column_mut(c + 1) {
                *copy = col;
            }
        }
        self.square();
        true
    }

    /// The words of the table, for search and for the AI.
    pub fn plain_text(&self) -> String {
        let mut out: Vec<String> = Vec::with_capacity(self.rows.len() + 1);
        if !self.name.is_empty() {
            out.push(self.name.clone());
        }
        out.extend(self.rows.iter().map(|row| row.join(" ")));
        out.join("\n")
    }

    // ----- columns --------------------------------------------------------

    /// How column `c` is read.
    pub fn column(&self, c: usize) -> Column {
        self.columns.get(c).cloned().unwrap_or_default()
    }

    pub fn kind(&self, c: usize) -> Kind {
        self.columns.get(c).map(|col| col.kind).unwrap_or_default()
    }

    /// Some column has a kind, a unit or a total.
    pub fn is_typed(&self) -> bool {
        !self.columns.is_empty()
    }

    fn column_mut(&mut self, c: usize) -> Option<&mut Column> {
        let width = self.columns();
        if c >= width {
            return None;
        }
        if self.columns.len() < width {
            self.columns.resize(width, Column::default());
        }
        self.columns.get_mut(c)
    }

    /// Read column `c` as `kind`. What means nothing for the new kind (a
    /// total, a format, a formula) goes.
    pub fn set_kind(&mut self, c: usize, kind: Kind) {
        // (A cell of several choices made a single choice keeps the first.)
        if self.kind(c) == Kind::Multi && matches!(kind, Kind::Choice | Kind::Status) {
            for r in 1..self.rows.len() {
                if let Some(first) = choice::names_in(Kind::Multi, &self.rows[r][c]).into_iter().next() {
                    self.rows[r][c] = first;
                }
            }
        }
        if let Some(col) = self.column_mut(c) {
            col.kind = kind;
            if !kind.has_options() {
                col.options.clear();
            }
            if col.total.is_some_and(|t| !kind.totals().contains(&t)) {
                col.total = None;
            }
            if !matches!(kind, Kind::Number | Kind::Formula) {
                col.format.clear();
                col.places = None;
                col.show = Show::Number;
                col.goal = None;
            }
            if kind != Kind::Formula {
                col.formula.clear();
            }
            if kind != Kind::Relation {
                col.from.clear();
            }
        }
        self.square();
    }

    pub fn set_total(&mut self, c: usize, total: Option<Total>) {
        if let Some(col) = self.column_mut(c) {
            col.total = total.filter(|t| col.kind.totals().contains(t));
        }
        self.square();
    }

    pub fn set_unit(&mut self, c: usize, unit: &str) {
        if let Some(col) = self.column_mut(c) {
            let unit = unit.split_whitespace().collect::<Vec<_>>().join("");
            col.unit = (!unit.is_empty()).then_some(unit);
        }
        self.square();
    }

    /// How the numbers of column `c` are written ("" as typed, "grouped",
    /// "percent", "USD"…). The numbers it has are written again that way.
    pub fn set_format(&mut self, c: usize, format: &str) {
        let format = money::known_format(format).unwrap_or("");
        if let Some(col) = self.column_mut(c).filter(|col| matches!(col.kind, Kind::Number | Kind::Formula)) {
            col.format = format.to_string();
        }
        self.square();
        self.rewrite(c);
    }

    /// Places after the point for column `c` (none: the format's own).
    pub fn set_places(&mut self, c: usize, places: Option<u8>) {
        if let Some(col) = self.column_mut(c).filter(|col| matches!(col.kind, Kind::Number | Kind::Formula)) {
            col.places = places.map(|p| p.min(6));
        }
        self.square();
        self.rewrite(c);
    }

    /// Show the numbers of column `c` with a bar or a ring, full at `goal`.
    pub fn set_show(&mut self, c: usize, show: Show, goal: Option<f64>) {
        if let Some(col) = self.column_mut(c).filter(|col| matches!(col.kind, Kind::Number | Kind::Formula)) {
            col.show = show;
            col.goal = goal.filter(|g| g.is_finite() && *g > 0.0);
        }
        self.square();
    }

    pub fn set_formula(&mut self, c: usize, formula: &str) {
        if let Some(col) = self.column_mut(c).filter(|col| col.kind == Kind::Formula) {
            // (A "=" in front is a habit from spreadsheets: it is not part of the formula.)
            let formula = formula.trim();
            col.formula = formula.strip_prefix('=').unwrap_or(formula).trim().to_string();
        }
        self.square();
    }

    /// What is wrong with the formula of column `c`, in one sentence.
    pub fn formula_error(&self, c: usize) -> Option<String> {
        let col = self.column(c);
        if col.kind != Kind::Formula || col.formula.is_empty() {
            return None;
        }
        let formula = match Formula::parse(&col.formula) {
            Ok(formula) => formula,
            Err(why) => return Some(why),
        };
        formula.names().into_iter().find(|name| self.find_heading(name).is_none_or(|o| o == c)).map(|name| format!("There is no column \"{name}\"."))
    }

    /// The table whose rows relation column `c` names.
    pub fn set_from(&mut self, c: usize, table: &str) {
        if let Some(col) = self.column_mut(c).filter(|col| col.kind == Kind::Relation) {
            col.from = table.to_string();
        }
        self.square();
    }

    /// Write every number of column `c` again, as its format writes numbers.
    fn rewrite(&mut self, c: usize) {
        if self.kind(c) != Kind::Number {
            return;
        }
        for r in 1..self.rows.len() {
            if let Some(tidy) = self.tidy_number(c, &self.rows[r][c]) {
                self.rows[r][c] = tidy;
            }
        }
    }

    /// A number as column `c` writes it: money with its sign and its own
    /// places, other numbers with the places they were typed with. A typed
    /// number is never made shorter: "5.125" keeps its three places in a
    /// column of two (the cell is where the number is kept).
    ///
    /// Only a cell that is one number is written again: "2 bags" stays as
    /// it was typed (and is still read as 2).
    fn tidy_number(&self, c: usize, text: &str) -> Option<String> {
        let col = self.column(c);
        // (A word after the number that the heading of the column has, as
        // "6 km" under "Distance (km)", is its unit: it is left out.)
        let heading = self.heading(c).to_lowercase();
        let in_heading = |rest: &str| {
            let rest = rest.trim().to_lowercase();
            !rest.is_empty() && heading.split(|ch: char| !ch.is_alphanumeric()).any(|word| word == rest)
        };
        let read = whole_number(text, col.unit.as_deref().unwrap_or("")).or_else(|| read_number(text).filter(|read| in_heading(read.rest)))?;
        let own = col.places.map(usize::from).or_else(|| money::currency(&col.format).map(|cur| cur.places as usize));
        Some(money::write(&col.format, read.value, Some(own.map(|p| p.max(read.needed)).unwrap_or(read.typed))))
    }

    /// The column with this heading (any case), or whose unit is these
    /// words, or whose heading holds them ("km" finds "Distance (km)"), or
    /// the column with this number (from 1).
    pub fn find_column(&self, name: &str) -> Option<usize> {
        let want = name.trim().to_lowercase();
        if want.is_empty() {
            return None;
        }
        let heads: Vec<String> = self.rows.first().map(|r| r.iter().map(|h| h.trim().to_lowercase()).collect()).unwrap_or_default();
        heads
            .iter()
            .position(|h| *h == want)
            .or_else(|| (0..heads.len()).find(|&c| self.column(c).unit.is_some_and(|u| u.to_lowercase() == want)))
            .or_else(|| heads.iter().position(|h| !h.is_empty() && (h.contains(&want) || want.contains(h.as_str()))))
            .or_else(|| want.parse::<usize>().ok().filter(|n| (1..=heads.len()).contains(n)).map(|n| n - 1))
    }

    /// The column with exactly this heading (any case).
    pub fn find_heading(&self, name: &str) -> Option<usize> {
        let want = name.trim().to_lowercase();
        self.rows.first()?.iter().position(|h| !want.is_empty() && h.trim().to_lowercase() == want)
    }

    /// The heading of column `c`, or "Column 3" when it has none.
    pub fn heading(&self, c: usize) -> String {
        let h = self.rows.first().and_then(|r| r.get(c)).map(|h| h.trim().to_string()).unwrap_or_default();
        if h.is_empty() {
            format!("Column {}", c + 1)
        } else {
            h
        }
    }

    /// The first column of a kind.
    pub fn first_of(&self, kind: Kind) -> Option<usize> {
        (0..self.columns()).find(|&c| self.kind(c) == kind)
    }

    // ----- values ---------------------------------------------------------

    /// The rows under the header that have something in them.
    pub fn body(&self) -> impl Iterator<Item = usize> + '_ {
        (1..self.rows.len()).filter(|&r| !self.is_blank(r))
    }

    /// A row with nothing in any cell.
    pub fn is_blank(&self, r: usize) -> bool {
        self.rows.get(r).is_none_or(|row| row.iter().all(|c| c.trim().is_empty()))
    }

    pub fn cell(&self, r: usize, c: usize) -> &str {
        self.rows.get(r).and_then(|row| row.get(c)).map(String::as_str).unwrap_or("")
    }

    /// The formulas of the columns, read once.
    fn formulas(&self) -> &[Option<Formula>] {
        self.parsed.0.get_or_init(|| self.columns.iter().map(|col| (col.kind == Kind::Formula).then(|| Formula::parse(&col.formula).ok()).flatten()).collect())
    }

    /// What a cell is worth to a formula. A formula that reads itself, or
    /// formulas that read each other in a ring, are worth nothing.
    fn worth(&self, r: usize, c: usize, depth: u8) -> Value {
        let text = self.cell(r, c).trim();
        let amount = |v: Option<f64>| v.map(Value::Number).unwrap_or(Value::Empty);
        match self.kind(c) {
            Kind::Formula => match self.formulas().get(c) {
                Some(Some(formula)) if depth < 8 => formula.eval(&|name| self.find_heading(name).filter(|&o| o != c).map(|o| self.worth(r, o, depth + 1))).unwrap_or(Value::Empty),
                _ => Value::Empty,
            },
            Kind::Checkbox => Value::Bool(checked(text)),
            Kind::Number => amount(number(text)),
            Kind::Time => amount(seconds(text)),
            Kind::Pace => amount(self.pace(r)),
            _ if text.is_empty() => Value::Empty,
            _ => Value::Text(text.to_string()),
        }
    }

    /// The amount in a cell: a number, a time in seconds, a pace in seconds
    /// for each unit, 1 or 0 for a checkbox, what a formula works out.
    pub fn value(&self, r: usize, c: usize) -> Option<f64> {
        match self.kind(c) {
            Kind::Time => seconds(self.cell(r, c)),
            Kind::Pace => self.pace(r),
            Kind::Checkbox => Some(if checked(self.cell(r, c)) { 1.0 } else { 0.0 }),
            Kind::Formula => self.worth(r, c, 0).number().filter(|v| v.is_finite()),
            _ => number(self.cell(r, c)),
        }
    }

    /// The pace of a row: its first time column over its first number column.
    pub fn pace(&self, r: usize) -> Option<f64> {
        let distance = number(self.cell(r, self.first_of(Kind::Number)?))?;
        let time = seconds(self.cell(r, self.first_of(Kind::Time)?))?;
        (distance > 0.0).then(|| time / distance)
    }

    /// The day in a cell of a date column.
    pub fn day(&self, r: usize, c: usize, today: NaiveDate) -> Option<NaiveDate> {
        day(self.cell(r, c), today)
    }

    /// What a cell shows: its text, or what is worked out for it.
    pub fn shown(&self, r: usize, c: usize) -> String {
        match self.kind(c) {
            Kind::Pace if r > 0 => self.pace(r).map(clock).unwrap_or_default(),
            Kind::Formula if r > 0 => match self.worth(r, c, 0) {
                Value::Number(v) if v.is_finite() => self.format(c, v),
                Value::Text(text) => text,
                Value::Bool(yes) => if yes { "Yes" } else { "No" }.to_string(),
                _ => String::new(),
            },
            _ => self.cell(r, c).to_string(),
        }
    }

    /// For a row with rows nested in it and nothing typed in an amount
    /// cell: the sum of that column over the nested rows.
    pub fn rolled_up(&self, r: usize, c: usize) -> Option<String> {
        if !matches!(self.kind(c), Kind::Number | Kind::Time) || !self.cell(r, c).trim().is_empty() || !self.has_children(r) {
            return None;
        }
        let values: Vec<f64> = (r + 1..self.subtree_end(r)).filter_map(|o| self.value(o, c)).collect();
        (!values.is_empty()).then(|| self.format(c, values.iter().sum()))
    }

    /// How many places after the point the numbers of a column use: its own
    /// number of places, or as many as its cells were typed with (two at
    /// the most).
    pub fn places(&self, c: usize) -> usize {
        if let Some(places) = self.column(c).places {
            return places as usize;
        }
        self.body().filter_map(|r| read_number(self.cell(r, c))).map(|read| read.typed).max().unwrap_or(0).min(2)
    }

    /// An amount of column `c` written as the column writes it.
    pub fn format(&self, c: usize, v: f64) -> String {
        let col = self.column(c);
        let own = col.places.map(usize::from);
        match col.kind {
            Kind::Time | Kind::Pace => clock(v),
            _ if money::currency(&col.format).is_some() => money::write(&col.format, v, own),
            // What a formula works out has no typed places: as few as the
            // number needs, two at the most.
            Kind::Formula if own.is_none() => {
                let cents = (v * 100.0).round() as i64;
                let places = if cents % 100 == 0 { 0 } else if cents % 10 == 0 { 1 } else { 2 };
                money::write(&col.format, v, Some(places))
            }
            _ => money::write(&col.format, v, Some(self.places(c))),
        }
    }

    /// The amount at which a bar or a ring of column `c` is full: the
    /// column's goal, or 100 for a percent, or the highest amount it has.
    pub fn full_at(&self, c: usize) -> f64 {
        let col = self.column(c);
        col.goal.unwrap_or_else(|| if col.format == money::PERCENT { 100.0 } else { self.body().filter_map(|r| self.value(r, c)).fold(0.0, f64::max) })
    }

    /// What shows at the foot of column `c`: the short word and the amount.
    /// Only the rows that pass the filters count.
    pub fn total(&self, c: usize) -> Option<(&'static str, String)> {
        let total = self.column(c).total?;
        let rows = self.counted();
        // (A table with nothing in its rows yet has nothing to add up.)
        if rows.iter().all(|&r| self.is_blank(r)) {
            return None;
        }
        let filled = |r: usize| !self.shown(r, c).trim().is_empty();
        let mut values: Vec<f64> = rows.iter().filter_map(|&r| self.value(r, c)).collect();
        let (low, high) = (values.iter().copied().reduce(f64::min), values.iter().copied().reduce(f64::max));
        let text = match total {
            Total::Count => {
                let n = rows.iter().filter(|&&r| filled(r)).count();
                if self.view.filter.is_empty() {
                    format!("{n} {}", if n == 1 { "row" } else { "rows" })
                } else {
                    format!("{n} of {} rows", self.body().count())
                }
            }
            Total::Unique => {
                let mut seen: Vec<String> = rows.iter().map(|&r| self.shown(r, c).trim().to_lowercase()).filter(|t| !t.is_empty()).collect();
                seen.sort();
                seen.dedup();
                format!("{} different", seen.len())
            }
            Total::Empty => format!("{} empty", rows.iter().filter(|&&r| !filled(r)).count()),
            Total::Checked => {
                let n = rows.iter().filter(|&&r| checked(self.cell(r, c))).count();
                format!("{n} of {}", rows.len())
            }
            Total::Sum if !values.is_empty() => self.format(c, values.iter().sum()),
            Total::Average if !values.is_empty() => self.format(c, values.iter().sum::<f64>() / values.len() as f64),
            Total::Median if !values.is_empty() => {
                values.sort_by(f64::total_cmp);
                let mid = values.len() / 2;
                self.format(c, if values.len() % 2 == 1 { values[mid] } else { (values[mid - 1] + values[mid]) / 2.0 })
            }
            Total::Highest => self.format(c, high?),
            Total::Lowest => self.format(c, low?),
            Total::Range => self.format(c, high? - low?),
            Total::Sum | Total::Average | Total::Median => return None,
        };
        Some((total.short(), text))
    }

    // ----- what is typed ------------------------------------------------------

    /// A cell was typed in and left. A sign in it can say what its column
    /// holds ("$3,500" in a plain column makes the column US Dollar; a date
    /// makes it a date column), when the other cells of the column agree.
    /// Then the cell is written the way its column writes.
    pub fn settle(&mut self, r: usize, c: usize, today: NaiveDate) {
        if r == 0 || c >= self.columns() {
            return;
        }
        let text = self.cell(r, c).trim().to_string();
        if text.is_empty() {
            return;
        }
        let col = self.column(c);
        // (The first column holds the names of the rows: it stays text.)
        if c > 0 && col.format.is_empty() && matches!(col.kind, Kind::Text | Kind::Number) {
            let others = |reads: &dyn Fn(&str) -> bool| self.body().filter(|&o| o != r).map(|o| self.cell(o, c).trim()).all(|t| t.is_empty() || reads(t));
            // (Only a cell that is one number, in a column of such cells:
            // "3/10" and "2 bags" beside a "50%" stay words.)
            let one_number = |t: &str| whole_number(t, col.unit.as_deref().unwrap_or("")).is_some();
            if let Some(format) = money::typed_format(&text).filter(|_| one_number(&text) && (col.kind == Kind::Number || others(&one_number))) {
                self.set_kind(c, Kind::Number);
                self.set_format(c, format);
                return;
            }
            // (And only a cell that is one day: "Mark 3 done" is words.)
            let a_day = !matches!(text.to_lowercase().as_str(), "today" | "yesterday" | "tomorrow") && whole_day(&text, today).is_some();
            if col.kind == Kind::Text && a_day && others(&|t| whole_day(t, today).is_some()) {
                self.set_kind(c, Kind::Date);
                for o in 1..self.rows.len() {
                    self.tidy(o, c, today);
                }
                return;
            }
        }
        self.tidy(r, c, today);
    }

    /// Write a typed cell the way its column writes: money and numbers with
    /// a format, days, times. Other cells stay as they were typed.
    pub fn tidy(&mut self, r: usize, c: usize, today: NaiveDate) {
        if r == 0 || r >= self.rows.len() || c >= self.columns() {
            return;
        }
        let text = self.cell(r, c).trim().to_string();
        let col = self.column(c);
        let tidy = match col.kind {
            Kind::Number if !col.format.is_empty() || col.places.is_some() => self.tidy_number(c, &text),
            Kind::Date => whole_day(&text, today).map(|d| format_day(d, today)),
            Kind::Time => seconds(&text).map(clock),
            _ => None,
        };
        if let Some(tidy) = tidy {
            self.set(r, c, &tidy);
        }
    }

    // ----- rows by the names of their columns ------------------------------

    /// Write `text` in a cell, in the form its column reads best: a day as
    /// "Tue 1 Dec", a time as "34:00", a tick as "x", money with its sign.
    pub fn put(&mut self, r: usize, c: usize, text: &str, today: NaiveDate) {
        let text = text.trim();
        let tidy = match self.kind(c) {
            Kind::Date => whole_day(text, today).map(|d| format_day(d, today)),
            Kind::Time => seconds(text).map(clock),
            Kind::Number => self.tidy_number(c, text),
            Kind::Checkbox => Some(if checked(text) { "x".to_string() } else { String::new() }),
            k if k.has_options() => Some(self.as_options(c, text)),
            _ => None,
        };
        let text = tidy.unwrap_or_else(|| text.to_string());
        self.set(r, c, &text);
    }

    /// Give row `r` what the filters ask a cell to be ("Category is Food"),
    /// where the cell is empty: a new row then stays in view.
    pub fn fill_for_filters(&mut self, r: usize, today: NaiveDate) {
        for rule in self.view.filter.clone() {
            if rule.op == Op::Is && !rule.text.trim().is_empty() && self.cell(r, rule.column).is_empty() && !self.kind(rule.column).is_worked_out() {
                self.put(r, rule.column, &rule.text, today);
            }
        }
    }

    /// Fill a row from (column, text) pairs. Names no column has are
    /// returned, and nothing is written for them.
    pub fn put_named(&mut self, r: usize, values: &[(String, String)], today: NaiveDate) -> Vec<String> {
        let mut unknown = Vec::new();
        for (name, text) in values {
            match self.find_column(name) {
                Some(c) if !self.kind(c).is_worked_out() => self.put(r, c, text, today),
                Some(_) => {}
                None => unknown.push(name.clone()),
            }
        }
        unknown
    }

    /// A row at the end for `day` (an empty last row is used again), with
    /// the day in the first date column. Returns its number, or `None` when
    /// the table is full.
    pub fn add_day_row(&mut self, day: NaiveDate, today: NaiveDate) -> Option<usize> {
        let empty_last = self.rows.len() > 1 && self.is_blank(self.rows.len() - 1);
        let r = if empty_last { self.rows.len() - 1 } else { self.add_row(self.rows.len())? };
        if let Some(c) = self.first_of(Kind::Date) {
            self.set(r, c, &format_day(day, today));
        }
        Some(r)
    }

    /// The first row (under the header) whose cell in column `c` says
    /// `text`; for a date column, the first row of that day.
    pub fn find_row(&self, c: usize, text: &str, today: NaiveDate) -> Option<usize> {
        let want = text.trim().to_lowercase();
        let want_day = (self.kind(c) == Kind::Date).then(|| day(text, today)).flatten();
        self.body().find(|&r| match want_day {
            Some(d) => self.day(r, c, today) == Some(d),
            None => self.cell(r, c).trim().to_lowercase() == want,
        })
    }

    // ----- Markdown ---------------------------------------------------------

    /// As a Markdown pipe table (the first row is the header), with the
    /// kinds of its columns on a comment line under it and its name on a
    /// second one. What is worked out (a pace, a formula) is written too, so
    /// that a reader sees it; it is not read back.
    pub fn to_markdown(&self, pad: &str) -> String {
        let bars = |text: &str| text.replace('|', "\\|").replace('\n', " ");
        let line = |r: usize| {
            let cells: Vec<String> = (0..self.columns()).map(|c| if r > 0 && self.kind(c).is_worked_out() { bars(&self.shown(r, c)) } else { bars(self.cell(r, c)) }).collect();
            format!("{pad}| {} |", cells.join(" | "))
        };
        let mut out: Vec<String> = Vec::new();
        for r in 0..self.rows.len() {
            out.push(line(r));
            if r == 0 {
                out.push(format!("{pad}|{}", " --- |".repeat(self.columns())));
            }
        }
        if self.is_typed() {
            let described: Vec<String> = (0..self.columns())
                .map(|c| {
                    let col = self.column(c);
                    let mut words = vec![col.kind.name().to_string()];
                    words.extend((!col.format.is_empty()).then(|| col.format.clone()));
                    words.extend(col.unit.clone());
                    words.extend(col.total.map(|t| t.name().to_string()));
                    match col.show {
                        Show::Number => {}
                        Show::Bar => words.push("bar".to_string()),
                        Show::Ring => words.push("ring".to_string()),
                    }
                    words.extend(col.goal.filter(|_| col.show != Show::Number).map(|g| format!("goal {g}")));
                    words.extend((col.signed && col.kind.is_amount()).then(|| "signed".to_string()));
                    // (Only when the list says more than the cells do.)
                    if col.kind.has_options() && col.options != self.plain_options(c) {
                        let list: Vec<String> = col.options.iter().map(|o| format!("{}:{}", o.name.replace(['|', ';', '{', '}', ':', '='], " ").replace("-->", "- ->"), o.color.name())).collect();
                        words.push(format!("{{{}}}", list.join("; ")));
                    }
                    if !col.formula.is_empty() {
                        words.push(format!("= {}", bars(&col.formula).replace("-->", "- ->")));
                    }
                    words.join(" ")
                })
                .collect();
            out.push(format!("{pad}<!-- columns: {} -->", described.join(" | ")));
        }
        let view = self.view_parts();
        if !self.name.is_empty() || !self.icon.is_empty() || self.beside || !view.is_empty() {
            let mut said = vec![bars(&self.name).replace("-->", "- ->")];
            said.extend((!self.icon.is_empty()).then(|| format!("icon {}", self.icon)));
            said.extend(self.beside.then(|| "beside".to_string()));
            said.extend(view.iter().map(|part| bars(part).replace("-->", "- ->")));
            out.push(format!("{pad}<!-- table: {} -->", said.join(" | ")));
        }
        out.join("\n")
    }

    /// Read a Markdown pipe table from the lines at the start of `lines`.
    /// Returns the table and how many lines it took, or `None` if those
    /// lines are no table (a table needs its header and the line of dashes).
    pub fn from_markdown(lines: &[&str]) -> Option<(Table, usize)> {
        let cells = |line: &str| -> Option<Vec<String>> {
            let line = line.trim();
            let inner = line.strip_prefix('|')?;
            Some(split_bars(inner.strip_suffix('|').unwrap_or(inner)))
        };
        let header = cells(lines.first()?)?;
        let rule = cells(lines.get(1)?)?;
        let is_rule = rule.len() == header.len() && rule.iter().all(|c| c.len() >= 3 && c.trim_matches(':').chars().all(|ch| ch == '-') && c.contains('-'));
        if !is_rule {
            return None;
        }
        let mut rows = vec![header];
        let mut taken = 2;
        for line in &lines[2..] {
            match cells(line) {
                Some(row) if rows.len() < MOST_ROWS => rows.push(row),
                _ => break,
            }
            taken += 1;
        }
        let mut table = Table::of(rows);
        let comment = |at: usize, word: &str| -> Option<String> { Some(lines.get(at)?.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim().strip_prefix(word)?.to_string()) };
        if let Some(described) = comment(taken, "columns:") {
            table.columns = split_bars(&described).iter().map(|part| read_column(part)).collect();
            taken += 1;
        }
        let mut view_parts = Vec::new();
        if let Some(said) = comment(taken, "table:") {
            for (i, part) in split_bars(&said).into_iter().enumerate() {
                match part.strip_prefix("icon ") {
                    _ if i == 0 => table.name = part,
                    Some(icon) => table.icon = icon.trim().to_string(),
                    None if part == "beside" => table.beside = true,
                    None => view_parts.push(part),
                }
            }
            taken += 1;
        }
        table.square();
        // What the line says of the view, once the columns are known.
        if !view_parts.is_empty() {
            for part in &view_parts {
                table.read_view_part(part);
            }
            table.change_view(|_| {});
        }
        // What is worked out is not kept in cells.
        let worked_out: Vec<usize> = (0..table.columns()).filter(|&c| table.kind(c).is_worked_out()).collect();
        for row in table.rows.iter_mut().skip(1) {
            for &c in &worked_out {
                row[c].clear();
            }
        }
        // Each cell is written the way its column writes, as if typed there.
        let today = crate::when::today();
        for r in 1..table.rows.len() {
            for c in 0..table.columns() {
                table.tidy(r, c, today);
            }
        }
        Some((table, taken))
    }

    /// This table was read from Markdown, which was written from `old` and
    /// then changed. Markdown cannot say all that a table is: take from
    /// `old` what it could not say (the view, the pages and the nesting of
    /// the rows, the template, how numbers show), by the headings of the
    /// columns and the names of the rows.
    pub fn adopt(&mut self, old: &Table) {
        let to: Vec<Option<usize>> = (0..old.columns()).map(|c| self.find_heading(&old.heading(c))).collect();
        let new_place = |c: usize| to.get(c).copied().flatten();
        if self.name.is_empty() {
            self.name = old.name.clone();
        }
        if self.icon.is_empty() {
            self.icon = old.icon.clone();
        }
        // A table with no line about its columns keeps the columns it had.
        let bare = self.columns.is_empty();
        for (c, place) in to.iter().enumerate() {
            let (Some(place), was) = (*place, old.column(c)) else { continue };
            if let Some(col) = self.column_mut(place) {
                if bare {
                    *col = was;
                } else if col.kind == was.kind {
                    // (What the Markdown says of the bar or ring wins.)
                    if col.show == Show::Number && col.goal.is_none() {
                        col.show = was.show;
                        col.goal = was.goal;
                    }
                    col.places = col.places.or(was.places);
                    if col.from.is_empty() {
                        col.from = was.from;
                    }
                }
            }
        }
        // A list of options that the Markdown did not spell out stays the
        // list the column had, with its colours and any new words after it.
        for (c, place) in to.iter().enumerate() {
            let (Some(place), was) = (*place, old.column(c)) else { continue };
            if !was.kind.has_options() || self.kind(place) != was.kind || self.column(place).options != self.plain_options(place) {
                continue;
            }
            let mut list = was.options;
            for o in self.column(place).options {
                if !list.iter().any(|l| l.name.eq_ignore_ascii_case(&o.name)) {
                    let color = Color::next(list.len() + place);
                    list.push(Choice { name: o.name, color });
                }
            }
            if let Some(col) = self.column_mut(place) {
                col.options = list;
            }
        }
        // A table line that says nothing of the view keeps the view it had.
        if self.view.is_plain() {
            self.view = old.view.clone();
            self.view.remap(new_place);
        }
        self.template = old.template.clone().map(|was| {
            let mut cells = vec![String::new(); self.columns()];
            for (c, text) in was.cells.iter().enumerate() {
                if let Some(place) = new_place(c) {
                    cells[place] = text.clone();
                }
            }
            Template { cells }
        });
        // Each row is the old row of the same name again (each old row
        // once): it goes by what that row went by, so its page finds it,
        // and is nested as it was.
        let mut left: Vec<usize> = (1..old.rows.len()).filter(|&r| old.info(r) != RowInfo::default()).collect();
        let names = new_place(0).unwrap_or(0);
        let mut unnamed = Vec::new();
        for r in 1..self.rows.len() {
            let name = self.cell(r, names).trim().to_lowercase();
            match left.iter().position(|&o| !name.is_empty() && old.title(o).to_lowercase() == name) {
                Some(i) => {
                    let was = old.info(left.remove(i));
                    if let Some(info) = self.info_mut(r) {
                        *info = was;
                    }
                }
                None => unnamed.push(r),
            }
        }
        // With as many rows as before, a row with another name is the row
        // that was at its place, with a new name.
        if self.rows.len() == old.rows.len() {
            for r in unnamed.into_iter().filter(|r| left.contains(r)) {
                let was = old.info(r);
                if let Some(info) = self.info_mut(r) {
                    *info = was;
                }
            }
        }
        self.square();
    }
}

/// The parts of a line between its bars, with `\|` for a bar in a part.
fn split_bars(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut part = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.peek() == Some(&'|') => {
                part.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut part).trim().to_string()),
            ch => part.push(ch),
        }
    }
    out.push(part.trim().to_string());
    out
}

/// One column of a `<!-- columns: … -->` line: its kind, then a format, a
/// unit or a total in any order ("number USD sum", "number km sum"), then
/// for a formula what it works out ("formula sum = Price * Qty").
fn read_column(words: &str) -> Column {
    let (words, formula) = match words.split_once('=') {
        Some((words, formula)) => (words, formula.trim()),
        None => (words, ""),
    };
    let mut col = Column::default();
    // A list of options: `{Long:green; Short:red}`.
    let (words, list) = match (words.find('{'), words.rfind('}')) {
        (Some(open), Some(close)) if open < close => (format!("{} {}", &words[..open], &words[close + 1..]), words[open + 1..close].to_string()),
        _ => (words.to_string(), String::new()),
    };
    for part in list.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, color) = part.rsplit_once(':').map_or((part, None), |(n, c)| (n.trim(), Color::parse(c)));
        if !name.is_empty() && col.option(name).is_none() {
            let color = color.unwrap_or(Color::next(col.options.len()));
            col.options.push(Choice { name: name.to_string(), color });
        }
    }
    let mut goal_next = false;
    for (i, word) in words.split_whitespace().enumerate() {
        if i == 0 {
            if let Some(kind) = Kind::parse(word) {
                col.kind = kind;
                continue;
            }
        }
        if std::mem::take(&mut goal_next) {
            if let Some(n) = read_number(word).filter(|n| n.rest.is_empty()) {
                col.goal = Some(n.value);
                continue;
            }
        }
        let shows = matches!(col.kind, Kind::Number | Kind::Formula);
        match word.to_lowercase().as_str() {
            "bar" | "bars" if shows => {
                col.show = Show::Bar;
                continue;
            }
            "ring" | "rings" if shows => {
                col.show = Show::Ring;
                continue;
            }
            "goal" if shows => {
                goal_next = true;
                continue;
            }
            "signed" => {
                col.signed = true;
                continue;
            }
            _ => {}
        }
        match (Total::parse(word), money::known_format(word)) {
            (Some(t), _) => col.total = Some(t),
            (None, Some(format)) if col.format.is_empty() && matches!(col.kind, Kind::Number | Kind::Formula) => col.format = format.to_string(),
            _ if col.unit.is_none() => col.unit = Some(word.to_string()),
            _ => {}
        }
    }
    if col.kind == Kind::Formula {
        col.formula = formula.to_string();
    }
    col.total = col.total.filter(|t| col.kind.totals().contains(t));
    if !col.kind.has_options() {
        col.options.clear();
    }
    col.signed &= col.kind.is_amount();
    col
}

// ----- reading cells -------------------------------------------------------

/// A number read from the start of a cell.
struct Read<'a> {
    value: f64,
    /// How many places after the point it was typed with, and how many of
    /// them it needs (those before its last zeros): "3.50" has two and
    /// needs one, "6,5" one and one, "1,500" none.
    typed: usize,
    needed: usize,
    /// What comes after it in the cell: " km" of "6.5 km".
    rest: &'a str,
}

/// The number at the start of a cell, after a minus and a money sign:
/// "6", "6.5 km", "6,5", "$3,500.00", "-€20". The thousands can have a
/// comma, a point or a space between them, and the last of a point and a
/// comma is the decimal point: "1,500", "1.234,56" and "1 234,56" are read
/// the way they were meant.
fn read_number(text: &str) -> Option<Read<'_>> {
    let (minus, text) = money::strip_sign(text);
    let three_digits = |s: &str| s.len() >= 3 && s.as_bytes()[..3].iter().all(u8::is_ascii_digit) && !s.as_bytes().get(3).is_some_and(u8::is_ascii_digit);
    let mut token = String::new();
    let mut end = 0;
    for (i, ch) in text.char_indices() {
        let after = &text[i + ch.len_utf8()..];
        if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            token.push(ch);
            end = i + ch.len_utf8();
        } else if !(matches!(ch, ' ' | '\u{a0}' | '\u{202f}') && !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) && three_digits(after)) {
            // (A space is part of the number only between the thousands: "1 500".)
            break;
        }
    }
    let rest = &text[end..];
    let token = token.trim_end_matches([',', '.']);
    let (points, commas) = (token.matches('.').count(), token.matches(',').count());
    // Each part between the signs of the thousands has three digits.
    let in_threes = |whole: &str, sign: char| whole.split(sign).enumerate().all(|(i, part)| if i == 0 { !part.is_empty() && part.len() <= 3 } else { part.len() == 3 });
    let decimal = match (points, commas) {
        (0, 0) => None,
        // Both: the one that comes last, and it comes once.
        (1, _) if commas > 0 && token.rfind('.') > token.rfind(',') => Some('.'),
        (_, 1) if points > 0 && token.rfind(',') > token.rfind('.') => Some(','),
        (_, _) if points > 0 && commas > 0 => return None,
        (1, 0) => Some('.'),
        // "1.234.567"; "1.2.3" is no number.
        (_, 0) => in_threes(token, '.').then_some(None)?,
        // One comma with three digits after it is between the thousands
        // ("1,500"), as are several commas; another is the decimal ("6,5").
        (0, 1) if token.len() - token.find(',').unwrap_or(0) - 1 != 3 => Some(','),
        _ => None,
    };
    let (whole, part) = match decimal {
        Some(sign) => token.rsplit_once(sign)?,
        None => (token, ""),
    };
    let digits: String = whole.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() && part.is_empty() {
        return None;
    }
    let value: f64 = format!("{digits}.{part}0").parse().ok()?;
    Some(Read { value: if minus { -value } else { value }, typed: part.len(), needed: part.trim_end_matches('0').len(), rest })
}

/// The number at the start of a cell, its unit left out: "6", "6.5 km",
/// "6,5", "1,500" (a thousand and five hundred), "$3,500.00", "-€20", "15%".
pub fn number(text: &str) -> Option<f64> {
    read_number(text).map(|read| read.value)
}

/// The number a cell is, when the cell is one number and no more: with a
/// money sign or a percent sign, before it or after it, or with `unit` (the
/// unit of its column) after it. "2 bags" and "3/10" start with a number,
/// and are words.
fn whole_number<'a>(text: &'a str, unit: &str) -> Option<Read<'a>> {
    read_number(text).filter(|read| {
        let rest = read.rest.trim();
        rest.is_empty() || rest == "%" || (!unit.is_empty() && rest.eq_ignore_ascii_case(unit)) || money::is_sign(rest)
    })
}

/// The words of a cell that holds several: "Home, Food" is two.
pub fn parts(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()
}

/// Where the words `part` are among the parts of a cell that holds
/// several: a name with a comma in it ("Smith, John") is kept as two parts
/// in a row.
fn part_at(parts: &[String], part: &str) -> Option<(usize, usize)> {
    let want = self::parts(part);
    let n = want.len();
    (n > 0 && n <= parts.len()).then(|| (0..=parts.len() - n).find(|&i| parts[i..i + n].iter().zip(&want).all(|(a, b)| a.to_lowercase() == b.to_lowercase()))).flatten().map(|i| (i, n))
}

/// A cell that holds several has these words among them.
pub fn holds_part(cell: &str, part: &str) -> bool {
    part_at(&parts(cell), part).is_some()
}

/// A cell that holds several, with `part` added to it, or taken out of it
/// if it was there.
pub fn toggle_part(cell: &str, part: &str) -> String {
    let mut all = parts(cell);
    match part_at(&all, part) {
        Some((i, n)) => {
            all.drain(i..i + n);
        }
        None => all.extend(parts(part)),
    }
    all.join(", ")
}

/// A length of time in seconds: "34:00", "1:02:30", "34 min", "1h 5m",
/// "90s"; a bare number is minutes.
pub fn seconds(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    if text.contains(':') {
        let parts: Vec<f64> = text.split(':').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
        return match parts.as_slice() {
            [m, s] => Some(m * 60.0 + s),
            [h, m, s] => Some(h * 3600.0 + m * 60.0 + s),
            _ => None,
        };
    }
    let mut total = 0.0;
    let mut found = false;
    let mut rest = text.as_str();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let n_end = rest.find(|ch: char| !(ch.is_ascii_digit() || ch == '.')).unwrap_or(rest.len());
        if n_end == 0 {
            return None;
        }
        let n: f64 = rest[..n_end].parse().ok()?;
        rest = rest[n_end..].trim_start();
        let u_end = rest.find(|ch: char| !ch.is_alphabetic()).unwrap_or(rest.len());
        let unit = &rest[..u_end];
        rest = &rest[u_end..];
        let scale = match unit {
            "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
            "" | "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            _ => return None,
        };
        total += n * scale;
        found = true;
    }
    found.then_some(total)
}

/// A time in seconds as "34:00" or "1:02:30".
pub fn clock(secs: f64) -> String {
    let s = secs.round().max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A ticked box: "x", "yes", "✓", "done", "true", "1".
pub fn checked(text: &str) -> bool {
    matches!(text.trim().to_lowercase().as_str(), "x" | "yes" | "y" | "true" | "1" | "✓" | "✔" | "done" | "[x]")
}

/// A day written in a cell: "2026-12-01", "Tue 1 Dec", "1 December",
/// "Dec 1, 2026", "today", "yesterday". With no year: the nearest such day
/// ("3 January" in December is next month).
pub fn day(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let text = text.trim().to_lowercase();
    match text.as_str() {
        "" => return None,
        "today" => return Some(today),
        "yesterday" => return Some(today - Duration::days(1)),
        "tomorrow" => return Some(today + Duration::days(1)),
        _ => {}
    }
    if let Ok(d) = NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        return Some(d);
    }
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let (mut month, mut dom, mut year) = (None, None, None);
    for word in text.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '/' || ch == '.').filter(|w| !w.is_empty()) {
        if let Some(m) = MONTHS.iter().position(|m| word.len() >= 3 && word.starts_with(m)) {
            month = Some(m as u32 + 1);
        } else if let Ok(n) = word.trim_end_matches(|ch: char| ch.is_alphabetic()).parse::<i32>() {
            match n {
                1..=31 if dom.is_none() => dom = Some(n as u32),
                1900..=2999 => year = Some(n),
                _ => {}
            }
        }
    }
    let (month, dom) = (month?, dom?);
    match year {
        Some(y) => NaiveDate::from_ymd_opt(y, month, dom),
        // The nearest such day; a day ahead counts as twice as far, since a
        // log is mostly of days gone by.
        None => (today.year() - 1..=today.year() + 1).filter_map(|y| NaiveDate::from_ymd_opt(y, month, dom)).min_by_key(|d| {
            let days = (*d - today).num_days();
            if days > 0 {
                days * 2
            } else {
                -days
            }
        }),
    }
}

/// The day a cell says, when the cell says a day and no more: each of its
/// words is the name of a month or of a day of the week, a number, or "of"
/// or "the". "Mark 3 done" has a month and a number in it (`day` reads it,
/// for a cell of a date column that says more than its day), and is no day:
/// such a cell is not written again as one, and does not make its column a
/// date column.
pub fn whole_day(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    const NAMES: [&str; 19] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december", "monday", "tuesday", "wednesday", "thursday", "friday",
        "saturday", "sunday",
    ];
    let low = text.trim().to_lowercase();
    let known = |word: &str| {
        let number = word.trim_end_matches(|ch: char| ch.is_ascii_alphabetic());
        let ending = &word[number.len()..];
        (word.len() >= 3 && NAMES.iter().any(|name| name.starts_with(word)))
            || matches!(word, "of" | "the" | "today" | "yesterday" | "tomorrow")
            || NaiveDate::parse_from_str(word, "%Y-%m-%d").is_ok()
            || (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && matches!(ending, "" | "st" | "nd" | "rd" | "th"))
    };
    low.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '/' || ch == '.').filter(|w| !w.is_empty()).all(known).then(|| day(text, today)).flatten()
}

/// A day as a date column writes it: "Tue 1 Dec", with the year when it is
/// not this year's.
pub fn format_day(d: NaiveDate, today: NaiveDate) -> String {
    if d.year() == today.year() {
        d.format("%a %-d %b").to_string()
    } else {
        d.format("%a %-d %b %Y").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Table {
        Table::of(rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect())
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// The run log of the mocks: a date, a distance, a time and a pace.
    fn log() -> Table {
        let mut t = table(&[
            &["Date", "Run", "km", "Time", "Pace", "Stretch"],
            &["Tue 10 Nov", "Easy", "3.0", "19:30", "", "x"],
            &["Thu 12 Nov", "Easy", "3.0", "19:12", "", ""],
            &["Sun 15 Nov", "Long", "3.5", "23:05", "", "x"],
        ]);
        t.set_kind(0, Kind::Date);
        t.set_kind(1, Kind::Choice);
        t.set_kind(2, Kind::Number);
        t.set_kind(3, Kind::Time);
        t.set_kind(4, Kind::Pace);
        t.set_kind(5, Kind::Checkbox);
        t
    }

    /// The budget of the mocks: what was spent, as plain text at first.
    fn spending() -> Table {
        table(&[&["Item", "Amount", "Category"], &["Rent", "2000", "Home"], &["Groceries", "150", "Food"], &["Dinner", "65", "Food"]])
    }

    #[test]
    fn a_new_table_and_one_that_cannot_be_read() {
        let new = Table::parse("");
        assert_eq!((new.rows.len(), new.columns()), (3, 3));
        assert_eq!(Table::parse("not json"), Table::default());
        // Rows of different widths are made square.
        let ragged = Table::parse(r#"{"rows":[["a"],["b","c","d"],[]]}"#);
        assert_eq!(ragged, table(&[&["a", "", ""], &["b", "c", "d"], &["", "", ""]]));
        assert_eq!(Table::parse(&ragged.to_data()), ragged);
        // A plain table keeps nothing but its rows in its data.
        assert_eq!(ragged.to_data(), r#"{"rows":[["a","",""],["b","c","d"],["","",""]]}"#);
    }

    #[test]
    fn rows_and_columns_come_and_go() {
        let mut t = table(&[&["Name", "Cost"], &["Rent", "2000"]]);
        assert_eq!(t.add_row(0), Some(1));
        t.set(1, 0, "Food\nand drink");
        assert_eq!(t, table(&[&["Name", "Cost"], &["Food and drink", ""], &["Rent", "2000"]]));
        t.add_column(0);
        assert_eq!(t.rows[0], vec!["Name", "", "Cost"]);
        t.remove_column(1);
        t.remove_row(1);
        assert_eq!(t, table(&[&["Name", "Cost"], &["Rent", "2000"]]));
        // The last row and the last column stay.
        t.remove_row(0);
        t.remove_row(0);
        t.remove_column(0);
        t.remove_column(0);
        assert_eq!(t, table(&[&["2000"]]));
        t.set(9, 9, "nowhere");
        assert_eq!(t.plain_text(), "2000");
    }

    #[test]
    fn columns_keep_their_kind_when_others_come_and_go() {
        let mut t = log();
        t.add_column(0);
        assert_eq!((t.kind(0), t.kind(1), t.kind(3)), (Kind::Date, Kind::Text, Kind::Number));
        t.remove_column(1);
        assert_eq!(t.kind(2), Kind::Number);
        assert_eq!(Table::parse(&t.to_data()), t);
        // A copy of a column has its cells, its kind and a name of its own.
        assert!(t.duplicate_column(2));
        assert_eq!((t.heading(3), t.kind(3), t.cell(3, 3), t.kind(4)), ("km 2".to_string(), Kind::Number, "3.5", Kind::Time));
        assert!(t.insert_column(0));
        assert_eq!((t.kind(0), t.kind(1)), (Kind::Text, Kind::Date));
    }

    #[test]
    fn markdown_there_and_back() {
        let t = table(&[&["Item", "Amount"], &["Salary", "$3,500"], &["A | B", ""]]);
        let md = t.to_markdown("");
        assert_eq!(md, "| Item | Amount |\n| --- | --- |\n| Salary | $3,500 |\n| A \\| B |  |");
        let lines: Vec<&str> = md.lines().chain(["", "after"]).collect();
        assert_eq!(Table::from_markdown(&lines), Some((t, 4)));
        // Alignment marks are fine; a missing line of dashes is no table.
        let aligned = ["| a | b |", "|:---|---:|", "| 1 | 2 |"];
        assert_eq!(Table::from_markdown(&aligned).map(|(t, n)| (t.rows.len(), n)), Some((2, 3)));
        assert_eq!(Table::from_markdown(&["| a | b |", "| 1 | 2 |"]), None);
        assert_eq!(Table::from_markdown(&["just text", "| --- |"]), None);
        assert_eq!(Table::from_markdown(&["| a |"]), None);
    }

    #[test]
    fn kinds_survive_markdown() {
        let mut t = log();
        t.set_total(2, Some(Total::Sum));
        t.set_unit(2, "km");
        t.set_total(5, Some(Total::Checked));
        let md = t.to_markdown("");
        assert!(md.ends_with("<!-- columns: date | choice | number km sum | time | pace | checkbox checked -->"), "{md}");
        // A pace is written for the reader, and not read back into a cell.
        assert!(md.contains("| 19:30 | 6:30 |"), "{md}");
        let lines: Vec<&str> = md.lines().chain(["after"]).collect();
        let (back, taken) = Table::from_markdown(&lines).unwrap();
        assert_eq!(back, t);
        assert_eq!(taken, lines.len() - 1);
        // Written by hand: short words, a unit, a total that does not fit is left out.
        let lines = ["| Day | Time | Name |", "| --- | --- | --- |", "| today | 30 min | a |", "<!--columns: date count|time avg mins| text sum-->"];
        let (t, taken) = Table::from_markdown(&lines).unwrap();
        assert_eq!(taken, 4);
        assert_eq!(t.column(0), Column { kind: Kind::Date, total: Some(Total::Count), ..Column::default() });
        assert_eq!(t.column(1), Column { kind: Kind::Time, unit: Some("mins".into()), total: Some(Total::Average), ..Column::default() });
        assert_eq!(t.column(2), Column::default());
    }

    #[test]
    fn a_name_a_format_and_a_formula_survive_markdown() {
        let mut t = table(&[&["Item", "Price", "Qty", "Cost"], &["Tea", "$4.50", "2", ""], &["A | B", "$1.00", "3", ""]]);
        t.name = "Shop | list".into();
        t.icon = "plus:green".into();
        t.beside = true;
        t.set_kind(1, Kind::Number);
        t.set_format(1, "USD");
        t.set_kind(2, Kind::Number);
        t.set_kind(3, Kind::Formula);
        t.set_formula(3, "Price * Qty");
        t.set_format(3, "usd");
        t.set_total(3, Some(Total::Sum));
        assert_eq!(t.shown(1, 3), "$9.00");
        assert_eq!(t.total(3), Some(("Sum", "$12.00".to_string())));
        let md = t.to_markdown("  ");
        assert!(md.contains("  | Tea | $4.50 | 2 | $9.00 |"), "{md}");
        assert!(md.contains("  <!-- columns: text | number USD | number | formula USD sum = Price * Qty -->"), "{md}");
        assert!(md.ends_with("  <!-- table: Shop \\| list | icon plus:green | beside -->"), "{md}");
        let lines: Vec<&str> = md.lines().collect();
        let (back, taken) = Table::from_markdown(&lines).unwrap();
        assert_eq!(taken, lines.len());
        assert!(back.beside);
        assert_eq!(back.to_data(), t.to_data());
        // The name is in the words that search reads.
        assert!(t.plain_text().starts_with("Shop | list\n"));
    }

    #[test]
    fn cells_are_read_by_their_kind() {
        assert_eq!(number("6"), Some(6.0));
        assert_eq!(number(" 6.5 km"), Some(6.5));
        assert_eq!(number("6,5"), Some(6.5));
        assert_eq!(number("1,500"), Some(1500.0));
        assert_eq!(number("km"), None);
        assert_eq!(number("$3,500.00"), Some(3500.0));
        assert_eq!(number("-$65.50"), Some(-65.5));
        assert_eq!(number("€ 20"), Some(20.0));
        assert_eq!(number("-4"), Some(-4.0));
        assert_eq!(number("15%"), Some(15.0));
        assert_eq!(number("$"), None);
        assert_eq!(number("Nokia 3310"), None);
        let decimals = |text: &str| read_number(text).map(|read| (read.typed, read.needed)).unwrap();
        assert_eq!((decimals("3.50"), decimals("6,5"), decimals("1,500"), decimals("$1,234.5"), decimals("7")), ((2, 1), (1, 1), (0, 0), (1, 1), (0, 0)));
        assert_eq!(parts(" Home, Food ,, "), vec!["Home", "Food"]);
        assert_eq!(seconds("34:00"), Some(2040.0));
        assert_eq!(seconds("1:02:30"), Some(3750.0));
        assert_eq!(seconds("34 minutes"), Some(2040.0));
        assert_eq!(seconds("1h 5m"), Some(3900.0));
        assert_eq!(seconds("34"), Some(2040.0));
        assert_eq!(seconds("soon"), None);
        assert_eq!(clock(2040.0), "34:00");
        assert_eq!(clock(15286.0), "4:14:46");
        assert!(checked("x") && checked("Yes") && !checked("") && !checked("no"));
        let today = d(2026, 12, 1);
        assert_eq!(day("Tue 1 Dec", today), Some(today));
        assert_eq!(day("10 Nov", today), Some(d(2026, 11, 10)));
        assert_eq!(day("Dec 1, 2025", today), Some(d(2025, 12, 1)));
        assert_eq!(day("2027-01-03", today), Some(d(2027, 1, 3)));
        assert_eq!(day("3 January", today), Some(d(2027, 1, 3)));
        // Without a year: the nearest such day, leaning to days gone by.
        assert_eq!(day("20 Jun", today), Some(d(2026, 6, 20)));
        assert_eq!(day("1 Mar", d(2026, 9, 1)), Some(d(2026, 3, 1)));
        assert_eq!(day("yesterday", today), Some(d(2026, 11, 30)));
        assert_eq!(day("soon", today), None);
        assert_eq!(format_day(today, today), "Tue 1 Dec");
        assert_eq!(format_day(d(2027, 1, 3), today), "Sun 3 Jan 2027");
    }

    #[test]
    fn pace_and_totals() {
        let mut t = log();
        assert_eq!(t.shown(1, 4), "6:30");
        assert_eq!(t.shown(3, 4), "6:36");
        assert_eq!(t.total(2), None);
        t.set_total(2, Some(Total::Sum));
        t.set_total(3, Some(Total::Sum));
        t.set_total(4, Some(Total::Average));
        t.set_total(5, Some(Total::Checked));
        t.set_total(0, Some(Total::Count));
        assert_eq!(t.total(2), Some(("Sum", "9.5".to_string())));
        assert_eq!(t.total(3), Some(("Sum", "1:01:47".to_string())));
        assert_eq!(t.total(4), Some(("Avg", "6:30".to_string())));
        assert_eq!(t.total(5), Some(("", "2 of 3".to_string())));
        assert_eq!(t.total(0), Some(("", "3 rows".to_string())));
        // The other totals.
        for (total, text) in [(Total::Median, ("Med", "3.0")), (Total::Range, ("Range", "0.5")), (Total::Highest, ("Max", "3.5")), (Total::Lowest, ("Min", "3.0")), (Total::Unique, ("", "2 different")), (Total::Empty, ("", "0 empty"))] {
            t.set_total(2, Some(total));
            assert_eq!(t.total(2), Some((text.0, text.1.to_string())), "{total:?}");
        }
        t.add_row(3);
        t.set(4, 1, "Rest");
        assert_eq!(t.total(2), Some(("", "1 empty".to_string())));
        t.set_total(2, Some(Total::Median));
        t.set(4, 2, "5");
        assert_eq!(t.total(2), Some(("Med", "3.2".to_string())));
        // A total that does not fit the kind is not kept.
        t.set_total(1, Some(Total::Sum));
        assert_eq!(t.column(1).total, None);
        // Turning a number column into text takes its total away.
        t.set_kind(2, Kind::Text);
        assert_eq!(t.column(2).total, None);
    }

    #[test]
    fn a_row_for_today_and_rows_by_name() {
        let today = d(2026, 12, 1);
        let mut t = log();
        let r = t.add_day_row(today, today).unwrap();
        assert_eq!(r, 4);
        assert_eq!(t.rows[4][0], "Tue 1 Dec");
        let unknown = t.put_named(r, &[("km".into(), "6 km".into()), ("time".into(), "34 minutes".into()), ("Pace".into(), "1:00".into()), ("Feel".into(), "Good".into())], today);
        assert_eq!(unknown, vec!["Feel".to_string()]);
        assert_eq!((t.rows[4][2].as_str(), t.rows[4][3].as_str(), t.rows[4][4].as_str()), ("6", "34:00", ""));
        assert_eq!(t.shown(4, 4), "5:40");
        assert_eq!(t.find_row(0, "today", today), Some(4));
        assert_eq!(t.find_row(0, "2026-11-12", today), Some(2));
        assert_eq!(t.find_row(1, "long", today), Some(3));
        assert_eq!(t.find_column("KM"), Some(2));
        assert_eq!(t.find_column("3"), Some(2));
        assert_eq!(t.find_column("Distance"), None);
        // An empty last row is used again.
        t.add_row(4);
        let again = t.add_day_row(today, today).unwrap();
        assert_eq!((again, t.rows.len()), (5, 6));
    }

    #[test]
    fn a_money_sign_in_a_cell_makes_the_column_money() {
        let today = d(2026, 11, 10);
        let mut t = spending();
        t.set(1, 1, "$2,000");
        t.settle(1, 1, today);
        // The column is US Dollar now, and every number in it is written so.
        assert_eq!(t.column(1), Column { kind: Kind::Number, format: "USD".into(), ..Column::default() });
        assert_eq!((t.cell(1, 1), t.cell(2, 1), t.cell(3, 1)), ("$2,000.00", "$150.00", "$65.00"));
        t.set_total(1, Some(Total::Sum));
        assert_eq!(t.total(1), Some(("Sum", "$2,215.00".to_string())));
        // A number typed later takes the sign; another sign does not change the column.
        t.add_row(3);
        t.set(4, 1, "12.5");
        t.settle(4, 1, today);
        t.set(3, 1, "€70");
        t.settle(3, 1, today);
        assert_eq!((t.cell(4, 1), t.cell(3, 1), t.column(1).format.as_str()), ("$12.50", "$70.00", "USD"));
        assert_eq!(t.total(1), Some(("Sum", "$2,232.50".to_string())));
        // A sign in a column of words changes nothing, and neither does one in the names of the rows.
        t.set(1, 2, "$5");
        t.settle(1, 2, today);
        t.set(1, 0, "$100");
        t.settle(1, 0, today);
        assert_eq!((t.kind(2), t.cell(1, 2), t.kind(0), t.cell(1, 0)), (Kind::Text, "$5", Kind::Text, "$100"));
        // Percent and other money: by their signs. A plain number column takes the sign too.
        let mut t = table(&[&["Name", "Share", "Cost"], &["a", "", "20"], &["b", "", "5,5"]]);
        t.set(1, 1, "15%");
        t.settle(1, 1, today);
        t.set_kind(2, Kind::Number);
        t.set(2, 2, "5,5 €");
        t.settle(2, 2, today);
        assert_eq!((t.column(1).format.as_str(), t.cell(1, 1), t.column(2).format.as_str(), t.cell(1, 2), t.cell(2, 2)), ("percent", "15%", "EUR", "€20.00", "€5.50"));
        // Back to plain numbers, and to another number of places.
        t.set_format(2, "");
        assert_eq!((t.cell(1, 2), t.cell(2, 2)), ("20.00", "5.50"));
        t.set_format(2, "JPY");
        t.set_places(2, Some(1));
        assert_eq!((t.cell(1, 2), t.cell(2, 2)), ("¥20.0", "¥5.5"));
        // A number typed with more places than the column has keeps them.
        t.set(1, 2, "12.345");
        t.settle(1, 2, today);
        t.set_places(2, Some(0));
        assert_eq!((t.cell(1, 2), t.cell(2, 2)), ("¥12.345", "¥5.5"));
        t.set_kind(2, Kind::Text);
        assert_eq!(t.column(2), Column::default());
    }

    #[test]
    fn numbers_are_read_the_way_they_were_meant() {
        // A space, a comma or a point between the thousands; the last of a
        // point and a comma is the decimal point.
        for (text, value) in [
            ("$1 500", 1500.0),
            ("1 234 567", 1234567.0),
            ("1 234,56 €", 1234.56),
            ("€1.234,56", 1234.56),
            ("1,234.56", 1234.56),
            ("1.234.567", 1234567.0),
            ("12,345,678", 12345678.0),
            ("1,500", 1500.0),
            ("6,5", 6.5),
            (".5", 0.5),
            ("5.", 5.0),
            ("2 bags", 2.0),
            ("3/10", 3.0),
            ("5 2026", 5.0),
            ("2 10", 2.0),
        ] {
            assert_eq!(number(text), Some(value), "{text}");
        }
        for text in ["1.2.3", "192.168.1.1", "2026.11.10", "1,2.3,4", ",", ".", "", "km"] {
            assert_eq!(number(text), None, "{text}");
        }
        // One number and no more, or a number at the start of words.
        for text in ["$3,500", "20 €", "20 eur", "15 %", "-$65.00", "6.5 km", "1 500", "5."] {
            assert!(whole_number(text, "km").is_some(), "{text}");
        }
        for text in ["2 bags", "3/10", "6.5 km easy", "1 50", "10k", "5 of 7"] {
            assert!(whole_number(text, "km").is_none(), "{text}");
        }
    }

    #[test]
    fn a_cell_is_written_again_only_when_it_is_one_number() {
        let today = d(2026, 11, 10);
        // A column of words with numbers in them stays words when a percent is typed beside them.
        let mut t = table(&[&["Task", "Done"], &["Read", "3/10"], &["Pack", "2 bags"], &["Run", ""]]);
        t.set(3, 1, "50%");
        t.settle(3, 1, today);
        assert_eq!((t.kind(1), t.column(1).format.as_str(), t.cell(1, 1), t.cell(2, 1), t.cell(3, 1)), (Kind::Text, "", "3/10", "2 bags", "50%"));
        // Money typed with a space between the thousands is the amount that was meant.
        let mut t = table(&[&["Item", "Amount"], &["Rent", ""], &["Food", "150"]]);
        t.set(1, 1, "$1 500");
        t.settle(1, 1, today);
        assert_eq!((t.column(1).format.as_str(), t.cell(1, 1), t.cell(2, 1)), ("USD", "$1,500.00", "$150.00"));
        // In a money column, words that start with a number stay as they were typed (and are read still).
        t.put(2, 1, "2 bags", today);
        assert_eq!((t.cell(2, 1), t.value(2, 1)), ("2 bags", Some(2.0)));
        t.put(2, 1, "€1.234,56", today);
        assert_eq!(t.cell(2, 1), "$1,234.56");
        t.put(2, 1, "1 250,5", today);
        assert_eq!((t.cell(2, 1), t.total(1)), ("$1,250.50", None));
        // A sign with words that are no amount does not make a money column.
        let mut t = table(&[&["Item", "Note"], &["Rent", ""]]);
        t.set(1, 1, "$1 50");
        t.settle(1, 1, today);
        assert_eq!((t.kind(1), t.cell(1, 1)), (Kind::Text, "$1 50"));
        // A locked table can ask for a cell to be written the way its column writes, and no more.
        let mut t = table(&[&["Item", "Amount"], &["Rent", "$12"]]);
        t.tidy(1, 1, today);
        t.tidy(0, 1, today);
        t.tidy(9, 9, today);
        assert_eq!((t.kind(1), t.cell(1, 1), t.cell(0, 1)), (Kind::Text, "$12", "Amount"));
    }

    #[test]
    fn words_with_a_month_in_them_are_no_day() {
        let today = d(2026, 11, 10);
        for text in ["Tue 1 Dec", "1 December", "Dec 1, 2026", "2026-12-01", "1st of December", "the 3rd of march", "sept 4", "Thurs 12 nov", "today"] {
            assert!(whole_day(text, today).is_some(), "{text}");
        }
        for text in ["Mark 3 done", "Decide on 2 options", "Marathon 10k", "May 3 be late", "3 dec (race)", "December", "12", ""] {
            assert!(whole_day(text, today).is_none(), "{text}");
        }
        // Such words do not make a date column, and are not written again as a day.
        let mut t = table(&[&["Task", "Note"], &["Plan", ""], &["Pack", ""]]);
        t.set(1, 1, "Mark 3 done");
        t.settle(1, 1, today);
        assert_eq!((t.kind(1), t.cell(1, 1)), (Kind::Text, "Mark 3 done"));
        // In a date column a cell that says more than its day keeps its words, and its day is read still.
        t.set(1, 1, "");
        t.set(2, 1, "12 nov");
        t.settle(2, 1, today);
        assert_eq!((t.kind(1), t.cell(2, 1)), (Kind::Date, "Thu 12 Nov"));
        t.set(1, 1, "3 dec (race)");
        t.settle(1, 1, today);
        assert_eq!((t.cell(1, 1), t.day(1, 1, today)), ("3 dec (race)", Some(d(2026, 12, 3))));
        t.put(1, 1, "Mark 3 done", today);
        assert_eq!(t.cell(1, 1), "Mark 3 done");
    }

    #[test]
    fn a_table_with_nothing_in_its_rows_has_no_totals() {
        let mut t = table(&[&["Date", "P&L", "Rules kept"], &["", "", ""]]);
        t.set_kind(0, Kind::Date);
        t.set_total(0, Some(Total::Count));
        t.set_kind(1, Kind::Number);
        t.set_total(1, Some(Total::Sum));
        t.set_kind(2, Kind::Checkbox);
        t.set_total(2, Some(Total::Checked));
        assert_eq!((t.total(0), t.total(1), t.total(2)), (None, None, None));
        t.set(1, 0, "Tue 10 Nov");
        assert_eq!((t.total(0), t.total(1), t.total(2)), (Some(("", "1 row".to_string())), None, Some(("", "0 of 1".to_string()))));
    }

    #[test]
    fn a_view_from_a_file_written_wrong_is_put_right() {
        let t = Table::parse(r#"{"rows":[["A","B","C"],["1","2","3"]],"view":{"frozen":18446744073709551615,"hidden":[0,7,2,2],"group":40}}"#);
        assert_eq!((t.view.frozen, t.view.hidden.clone(), t.view.group), (2, vec![2], None));
    }

    #[test]
    fn a_cell_of_several_takes_a_name_with_a_comma() {
        assert_eq!(toggle_part("Ana", "Smith, John"), "Ana, Smith, John");
        assert!(holds_part("Ana, smith, john", "Smith, John") && holds_part("Ana, Smith, John", "ana"));
        assert!(!holds_part("Ana, Smith", "Smith, John") && !holds_part("", "Ana") && !holds_part("Ana", ""));
        assert_eq!(toggle_part("Ana, Smith, John, Bo", "Smith, John"), "Ana, Bo");
        assert_eq!(toggle_part("Ana", "ana"), "");
        assert_eq!(toggle_part("", "Ana"), "Ana");
    }

    #[test]
    fn a_typed_day_makes_a_date_column_and_is_written_as_one() {
        let today = d(2026, 11, 10);
        let mut t = table(&[&["Item", "When"], &["Rent", "1 nov"], &["Gym", ""]]);
        t.set(2, 1, "2026-11-03");
        t.settle(2, 1, today);
        assert_eq!((t.kind(1), t.cell(1, 1), t.cell(2, 1)), (Kind::Date, "Sun 1 Nov", "Tue 3 Nov"));
        // "today" alone is a word; in a date column it is a day.
        let mut words = table(&[&["Item", "Note"], &["Rent", ""]]);
        words.set(1, 1, "today");
        words.settle(1, 1, today);
        assert_eq!(words.kind(1), Kind::Text);
        t.add_row(2);
        t.set(3, 1, "tomorrow");
        t.settle(3, 1, today);
        assert_eq!(t.cell(3, 1), "Wed 11 Nov");
    }

    #[test]
    fn formulas_are_worked_out_for_each_row() {
        let mut t = table(&[&["Item", "Price", "Qty", "Cost", "Big"], &["Tea", "4.5", "2", "", ""], &["Cup", "", "3", "", ""], &["Pot", "30", "1", "", ""]]);
        t.set_kind(1, Kind::Number);
        t.set_kind(2, Kind::Number);
        t.set_kind(3, Kind::Formula);
        t.set_formula(3, "Price * Qty");
        t.set_kind(4, Kind::Formula);
        t.set_formula(4, "if(Cost >= 20, \"big\", \"\")");
        assert_eq!((t.shown(1, 3), t.shown(2, 3), t.shown(3, 3)), ("9".to_string(), String::new(), "30".to_string()));
        assert_eq!((t.shown(1, 4), t.shown(3, 4)), (String::new(), "big".to_string()));
        assert_eq!((t.value(1, 3), t.value(2, 3)), (Some(9.0), None));
        t.set_total(3, Some(Total::Sum));
        assert_eq!(t.total(3), Some(("Sum", "39".to_string())));
        t.set(1, 1, "4.25");
        assert_eq!(t.shown(1, 3), "8.5");
        // What is wrong with a formula is said in a sentence; its cells are empty.
        assert_eq!(t.formula_error(3), None);
        t.set_formula(3, "Price * Weight");
        assert_eq!(t.formula_error(3), Some("There is no column \"Weight\".".to_string()));
        assert_eq!(t.shown(1, 3), "");
        t.set_formula(3, "Price *");
        assert!(t.formula_error(3).is_some());
        // A formula that reads itself, and two that read each other, are worth nothing (and end).
        t.set_formula(3, "Cost + 1");
        assert_eq!((t.shown(1, 3), t.formula_error(3)), (String::new(), Some("There is no column \"Cost\".".to_string())));
        t.set_formula(3, "Big + 1");
        t.set_formula(4, "Cost + 1");
        assert_eq!((t.shown(1, 3), t.shown(1, 4)), (String::new(), String::new()));
        // A new heading is found at once: the formulas are read again when a column changes.
        t.set_formula(3, "Price * Qty");
        t.remove_column(4);
        assert_eq!(t.shown(3, 3), "30");
    }

    #[test]
    fn rows_have_pages_and_can_be_nested() {
        let mut t = table(&[&["Item", "Amount"], &["Food", ""], &["Bread", "4"], &["Milk", "2"], &["Rent", "900"]]);
        t.set_kind(1, Kind::Number);
        // Nothing about the rows is kept until a row has something; rows
        // get names of their own all at once (when one is first opened).
        assert!(t.info.is_empty() && !t.has_ids() && t.row_id(4).is_none());
        assert!(t.name_rows() && !t.name_rows() && t.has_ids());
        let rent = t.row_id(4).unwrap().to_string();
        let ids: Vec<&str> = (1..5).map(|r| t.row_id(r).unwrap()).collect();
        assert!(ids.iter().all(|id| id.len() == 12) && (1..4).all(|i| !ids[..i].contains(&ids[i])), "{ids:?}");
        assert_eq!((t.row_with_id(&rent), t.row_with_id(""), t.row_with_id("no such row"), t.row_id(0)), (Some(4), None, None, None));
        // Bread and Milk under Food. The first row cannot be nested, and a row goes one step at a time.
        t.nest(1, true);
        t.nest(2, true);
        t.nest(2, true);
        t.nest(3, true);
        assert_eq!((0..5).map(|r| t.indent(r)).collect::<Vec<_>>(), [0, 0, 1, 1, 0]);
        assert!(t.has_children(1) && !t.has_children(2) && !t.has_children(4));
        assert_eq!(t.subtree_end(1), 4);
        // A row with rows nested in it shows their sum where it has nothing typed.
        assert_eq!((t.rolled_up(1, 1), t.rolled_up(2, 1), t.rolled_up(1, 0)), (Some("6".to_string()), None, None));
        t.set_total(1, Some(Total::Sum));
        assert_eq!(t.total(1), Some(("Sum", "906".to_string())));
        // A new row after a row with nested rows goes after them, at its level; a copy too.
        assert_eq!(t.add_row(1), Some(4));
        assert_eq!((t.indent(4), t.title(5), t.row_with_id(&rent)), (0, "Rent", Some(5)));
        // A new row and a copy go by names of their own.
        assert_eq!(t.duplicate_row(2), Some(3));
        assert_eq!((t.title(3), t.indent(3)), ("Bread", 1));
        assert!(t.row_id(3).is_some() && t.row_id(3) != t.row_id(2) && t.row_id(5).is_some_and(|id| id != rent));
        // Moving a row takes the rows nested in it along, with their pages.
        t.remove_row(5);
        t.remove_row(3);
        assert_eq!(t.move_row(4, 1), Some(1));
        assert_eq!((1..5).map(|r| (t.title(r), t.indent(r))).collect::<Vec<_>>(), [("Rent", 0), ("Food", 0), ("Bread", 1), ("Milk", 1)]);
        assert_eq!(t.row_with_id(&rent), Some(1));
        assert_eq!(t.move_row(2, 5), None);
        assert_eq!(t.move_row(2, 1), Some(1));
        assert_eq!((1..5).map(|r| t.title(r)).collect::<Vec<_>>(), ["Food", "Bread", "Milk", "Rent"]);
        // Taking a row out moves the rows nested in it out one step.
        t.remove_row(1);
        assert_eq!((1..4).map(|r| (t.title(r), t.indent(r))).collect::<Vec<_>>(), [("Bread", 0), ("Milk", 0), ("Rent", 0)]);
        assert_eq!(t.row_with_id(&rent), Some(3));
        assert_eq!(Table::parse(&t.to_data()), t);
        // The header is no row: when the first row is taken out, the row
        // that is the header now goes by nothing.
        let mut headless = t.clone();
        headless.remove_row(0);
        assert_eq!((headless.row_id(0), headless.row_with_id(&rent)), (None, Some(2)));
        // Nesting that makes no sense in a file is put right.
        let odd = Table::parse(r#"{"rows":[["a"],["b"],["c"]],"info":[{"indent":2},{"indent":3},{"indent":3}]}"#);
        assert_eq!((odd.indent(0), odd.indent(1), odd.indent(2)), (0, 0, 1));
    }

    #[test]
    fn a_moved_row_is_one_of_the_rows_it_lands_among() {
        let nested = || {
            let mut t = table(&[&["Item"], &["A"], &["A1"], &["A2"], &["B"], &["B1"]]);
            t.nest(2, true);
            t.nest(3, true);
            t.nest(5, true);
            t
        };
        let shape = |t: &Table| (1..t.rows.len()).map(|r| format!("{}{}", ".".repeat(t.indent(r) as usize), t.title(r))).collect::<Vec<_>>().join(" ");
        assert_eq!(shape(&nested()), "A .A1 .A2 B .B1");
        // Between two nested rows: nested with them, and its own nested row one deeper.
        let mut t = nested();
        assert_eq!(t.move_row(4, 3), Some(3));
        assert_eq!(shape(&t), "A .A1 .B ..B1 .A2");
        // After the last nested row, by its lower half: nested with it.
        let mut t = nested();
        assert_eq!(t.move_row_after(4, 3), Some(4));
        assert_eq!(shape(&t), "A .A1 .A2 .B ..B1");
        // Before a row that is not nested: not nested, and the rows nested in it come along.
        let mut t = nested();
        assert_eq!(t.move_row(2, 4), Some(3));
        assert_eq!(shape(&t), "A .A2 A1 B .B1");
        let mut t = nested();
        assert_eq!(t.move_row(4, 1), Some(1));
        assert_eq!(shape(&t), "B .B1 A .A1 .A2");
        // To the end.
        let mut t = nested();
        assert_eq!(t.move_row(2, 6), Some(5));
        assert_eq!(shape(&t), "A .A2 B .B1 A1");
        assert_whole(&t, "moved");
    }

    #[test]
    fn a_template_that_says_today_gives_the_day_the_row_is_made() {
        let mut t = table(&[&["Item", "Day", "Amount", "Paid"], &["Rent", "", "", ""]]);
        t.set_kind(1, Kind::Date);
        t.set_kind(2, Kind::Number);
        t.set_format(2, "USD");
        t.set_kind(3, Kind::Checkbox);
        t.template = Some(Template { cells: vec![String::new(), "today".into(), "12".into(), "yes".into()] });
        let r = t.add_row(1).unwrap();
        let today = crate::when::today();
        assert_eq!((t.cell(r, 0), t.cell(r, 1), t.cell(r, 2), t.cell(r, 3)), ("", format_day(today, today).as_str(), "$12.00", "x"));
    }

    #[test]
    fn a_row_with_a_new_name_is_the_row_at_its_place() {
        let mut old = table(&[&["Item", "Amount"], &["Rent", "2000"], &["Food", "150"], &["Fun", "65"]]);
        old.name_rows();
        // The AI gives one row another name and leaves the others.
        let md = old.to_markdown("").replace("Food", "Groceries");
        let lines: Vec<&str> = md.lines().collect();
        let (mut new, _) = Table::from_markdown(&lines).unwrap();
        new.adopt(&old);
        assert_eq!((1..4).map(|r| new.row_id(r)).collect::<Vec<_>>(), (1..4).map(|r| old.row_id(r)).collect::<Vec<_>>());
        // With a row more or fewer, a row with a new name is a new row.
        let md = old.to_markdown("").replace("| Food | 150 |", "| Groceries | 150 |\n| Cinema | 30 |");
        let lines: Vec<&str> = md.lines().collect();
        let (mut new, _) = Table::from_markdown(&lines).unwrap();
        new.adopt(&old);
        assert_eq!((new.row_id(1), new.row_id(2), new.row_id(3), new.row_id(4)), (old.row_id(1), None, None, old.row_id(3)));
    }

    #[test]
    fn a_new_row_starts_with_the_template() {
        let mut t = spending();
        t.template = Some(Template { cells: vec![String::new(), String::new(), "Food".into()] });
        let r = t.add_row(3).unwrap();
        assert_eq!(t.rows[r], vec!["", "", "Food"]);
        // The template follows the columns.
        t.insert_column(1);
        t.remove_column(0);
        assert_eq!(t.template.as_ref().unwrap().cells, vec!["", "", "Food"]);
        t.remove_column(2);
        assert_eq!(t.add_row(0).map(|r| t.rows[r].clone()), Some(vec![String::new(), String::new()]));
    }

    /// A table after any change: every row as wide as the header, one
    /// description for each row and column or none, a view and a template
    /// that name only what is there, nesting that makes sense; and nothing
    /// that reads the table fails.
    fn assert_whole(t: &Table, what: &str) {
        let width = t.columns();
        assert!((1..=MOST_COLUMNS).contains(&width) && (1..=MOST_ROWS).contains(&t.rows.len()), "{what}: {width} x {}", t.rows.len());
        assert!(t.rows.iter().all(|row| row.len() == width), "{what}: rows of other widths");
        assert!(t.info.is_empty() || t.info.len() == t.rows.len(), "{what}: {} infos for {} rows", t.info.len(), t.rows.len());
        assert!(t.columns.is_empty() || t.columns.len() == width, "{what}: {} columns described of {width}", t.columns.len());
        assert!(t.template.as_ref().is_none_or(|tp| tp.cells.len() == width), "{what}: the template has another width");
        let v = &t.view;
        assert!(v.sort.iter().all(|s| s.column < width) && v.filter.iter().chain(&v.paint).all(|rule| rule.column < width), "{what}: a sort or a rule names a column that is not there");
        assert!(v.group.is_none_or(|g| g < width) && v.hidden.iter().all(|&h| h != 0 && h < width) && v.frozen < width.max(2), "{what}: the view names a column that is not there: {v:?}");
        for r in 0..t.rows.len() {
            let most = if r <= 1 { 0 } else { t.indent(r - 1) + 1 };
            assert!(t.indent(r) <= most.min(MOST_NESTING), "{what}: row {r} is nested {} under {}", t.indent(r), most);
            // A row goes by a name no other row has, and is found by it; the header by none.
            assert!(t.row_id(r).is_none_or(|id| r > 0 && t.row_with_id(id) == Some(r)), "{what}: row {r} goes by {:?}, which finds {:?}", t.row_id(r), t.row_id(r).and_then(|id| t.row_with_id(id)));
        }
        // Once the rows have names, every row has one.
        assert!(!t.has_ids() || (1..t.rows.len()).all(|r| t.row_id(r).is_some()), "{what}: a row without a name in a table with names");
        let order = t.order();
        let mut once = order.clone();
        once.sort_unstable();
        once.dedup();
        assert!(once.len() == order.len() && order.iter().all(|&r| (1..t.rows.len()).contains(&r)), "{what}: the order {order:?} of {} rows", t.rows.len());
        for c in 0..width {
            let _ = (t.total(c), t.formula_error(c), t.full_at(c), t.groups(&order, c));
            for &r in &order {
                let _ = (t.shown(r, c), t.rolled_up(r, c), t.value(r, c), t.color(r));
            }
        }
        assert_eq!(&Table::parse(&t.to_data()), t, "{what}: not the same table when it is read again");
        let md = t.to_markdown("");
        let lines: Vec<&str> = md.lines().collect();
        let (back, taken) = Table::from_markdown(&lines).unwrap_or_else(|| panic!("{what}: its Markdown is no table:\n{md}"));
        assert_eq!((taken, back.rows.len(), back.columns()), (lines.len(), t.rows.len(), width), "{what}: its Markdown reads as another table:\n{md}");
    }

    #[test]
    fn a_table_stays_whole_through_any_changes() {
        let today = d(2026, 11, 10);
        // (The same "chance" each run: a failure can be found again.)
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |below: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % below.max(1) as u64) as usize
        };
        const WORDS: [&str; 10] = ["", "12.5", "$3", "x", "Food, Fun", "2026-11-03", "words | more", "-€4,5", "15%", "34:00"];
        const FORMATS: [&str; 5] = ["", "USD", "percent", "grouped", "JPY"];
        for round in 0..150 {
            let mut t = spending();
            t.name = "Spent".into();
            t.template = Some(Template { cells: vec![String::new(), String::new(), "Food".into()] });
            if round % 2 == 0 {
                t.name_rows();
            }
            t.change_view(|v| {
                v.sort_by(1, true);
                v.filter.push(Rule { column: 2, op: Op::Is, text: "Food".into(), color: String::new() });
                v.paint.push(Rule { column: 1, op: Op::Above, text: "10".into(), color: "red".into() });
                v.group = Some(2);
                v.toggle_hidden(2);
                v.frozen = 2;
            });
            assert_whole(&t, "at the start");
            for step in 0..60 {
                let (r, c, n) = (next(t.rows.len() + 2), next(t.columns() + 2), next(18));
                match n {
                    0 => drop(t.add_row(r)),
                    1 => t.remove_row(r),
                    2 => drop(t.duplicate_row(r)),
                    3 => drop(t.move_row(r, next(t.rows.len() + 2))),
                    4 => t.nest(r, true),
                    5 => t.nest(r, false),
                    6 => t.toggle_closed(r),
                    7 => drop(t.insert_column(c)),
                    8 => t.remove_column(c),
                    9 => drop(t.duplicate_column(c)),
                    10 => t.set_kind(c, Kind::ALL[next(Kind::ALL.len())]),
                    11 => t.set_format(c, FORMATS[next(FORMATS.len())]),
                    12 => {
                        t.set(r, c, WORDS[next(WORDS.len())]);
                        t.settle(r, c, today);
                    }
                    13 => drop(t.name_rows()),
                    14 => t.set_total(c, Some(Total::ALL[next(Total::ALL.len())])),
                    15 => t.set_formula(c, ["Amount * 2", "Item", "Amount +", "[Amount 2] / Amount", ""][next(5)]),
                    16 => drop(t.add_day_row(today, today)),
                    _ => t.change_view(|v| {
                        v.sort_by(c, r % 2 == 0);
                        v.toggle_hidden(c);
                        v.frozen = r;
                        v.group = (r % 3 > 0).then_some(c);
                        v.filter.push(Rule { column: c, op: [Op::Has, Op::Above, Op::Empty, Op::Ticked][r % 4], text: WORDS[r % WORDS.len()].into(), color: String::new() });
                        v.filter.truncate(3);
                    }),
                }
                assert_whole(&t, &format!("round {round}, step {step}, change {n} at row {r}, column {c}"));
            }
        }
    }

    #[test]
    fn a_table_written_again_keeps_what_markdown_cannot_say() {
        let mut old = spending();
        old.name = "Expenses".into();
        old.set_kind(1, Kind::Number);
        old.set_format(1, "USD");
        old.set_show(1, Show::Bar, Some(2000.0));
        old.set_kind(2, Kind::Choice);
        old.name_rows();
        let groceries = old.row_id(2).unwrap().to_string();
        old.nest(3, true);
        old.view.sort.push(view::Sort { column: 1, down: true });
        old.view.group = Some(2);
        old.view.hidden = vec![2];
        old.template = Some(Template { cells: vec![String::new(), String::new(), "Food".into()] });
        // The AI writes the page again: a column in front, a row gone, a row added, the name left out.
        let md = "| Paid | Item | Amount | Category |\n| --- | --- | --- | --- |\n| x | Groceries | $150.00 | Food |\n| | Dinner | $70.00 | Food |\n| | Cinema | $12.00 | Fun |\n<!-- columns: checkbox | text | number USD | choice -->";
        let lines: Vec<&str> = md.lines().collect();
        let (mut new, _) = Table::from_markdown(&lines).unwrap();
        new.adopt(&old);
        assert_eq!(new.name, "Expenses");
        assert_eq!((new.column(2).show, new.column(2).goal, new.column(2).format.as_str()), (Show::Bar, Some(2000.0), "USD"));
        assert_eq!(new.view.sort, vec![view::Sort { column: 2, down: true }]);
        assert_eq!((new.view.group, new.view.hidden.clone()), (Some(3), vec![3]));
        assert_eq!(new.template, Some(Template { cells: vec![String::new(), String::new(), String::new(), "Food".into()] }));
        // Groceries and Dinner are the rows they were (their pages find them); Cinema is new.
        assert_eq!((new.row_with_id(&groceries), new.row_id(2), new.indent(2), new.row_id(3)), (Some(1), old.row_id(3), 1, None));
        // With no line about the columns, the columns stay what they were.
        let bare: Vec<&str> = md.lines().take(5).collect();
        let (mut new, _) = Table::from_markdown(&bare).unwrap();
        new.adopt(&old);
        assert_eq!((new.kind(0), new.kind(2), new.column(2).format.as_str(), new.kind(3)), (Kind::Text, Kind::Number, "USD", Kind::Choice));
    }

    #[test]
    fn a_status_column_starts_with_three_words_and_keeps_its_own() {
        let mut t = Table::of(vec![vec!["Task".into(), "State".into()], vec!["Write".into(), "".into()], vec!["Ship".into(), "Waiting".into()]]);
        t.set_kind(1, Kind::Status);
        let names: Vec<String> = t.column(1).options.iter().map(|o| o.name.clone()).collect();
        assert_eq!(names, ["Not started", "In progress", "Done", "Waiting"]);
        assert_eq!(t.column(1).color_of("done"), Color::Green);
        let md = t.to_markdown("");
        let lines: Vec<&str> = md.lines().collect();
        let (back, _) = Table::from_markdown(&lines).unwrap();
        assert_eq!((back.kind(1), back.column(1).options.len()), (Kind::Status, 4), "{md}");
        // Tags that become a status keep their first word.
        let mut tags = Table::of(vec![vec!["Task".into(), "Tags".into()], vec!["Write".into(), "a, b".into()]]);
        tags.set_kind(1, Kind::Multi);
        tags.set_kind(1, Kind::Status);
        assert_eq!(tags.cell(1, 1), "a");
    }

    #[test]
    fn a_signed_column_survives_markdown_and_only_for_amounts() {
        let lines = ["| Item | P&L | Note |", "| --- | --- | --- |", "| A | -5 | x |", "<!-- columns: text | number USD sum signed | text signed -->"];
        let (t, _) = Table::from_markdown(&lines).unwrap();
        assert!(t.column(1).signed);
        assert!(!t.column(2).signed, "words have no sign");
        let md = t.to_markdown("");
        assert!(md.contains("number USD sum signed"), "{md}");
    }
}
