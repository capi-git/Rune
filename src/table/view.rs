//! How a table is shown: which rows (filters), in what order (sorts), in
//! which layout, with which columns, in which colours. None of this changes
//! the rows as they are kept.

use super::{checked, day, number, parts, seconds, Kind, Table};
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// The shape the rows are shown in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Table,
    /// A column of cards for each value of one column.
    Board,
    /// One line for each row.
    List,
    /// A card for each row.
    Gallery,
    /// A month, with each row on its day.
    Calendar,
}

impl Layout {
    pub const ALL: [Layout; 5] = [Layout::Table, Layout::Board, Layout::List, Layout::Gallery, Layout::Calendar];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Table => "table",
            Layout::Board => "board",
            Layout::List => "list",
            Layout::Gallery => "gallery",
            Layout::Calendar => "calendar",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Layout::Table => "Table",
            Layout::Board => "Board",
            Layout::List => "List",
            Layout::Gallery => "Gallery",
            Layout::Calendar => "Calendar",
        }
    }

    /// Name of its icon (see `ui/icons.rs`).
    pub fn icon(self) -> &'static str {
        match self {
            Layout::Table => "table",
            Layout::Board => "board",
            Layout::List => "list-bullet",
            Layout::Gallery => "gallery",
            Layout::Calendar => "calendar",
        }
    }

    fn is_table(&self) -> bool {
        *self == Layout::Table
    }
}

/// Rows in the order of one column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub column: usize,
    /// From high to low (Z to A, newest first).
    #[serde(default, skip_serializing_if = "is_false")]
    pub down: bool,
}

/// What a rule asks of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Is,
    IsNot,
    Has,
    HasNot,
    Above,
    Below,
    AtLeast,
    AtMost,
    Empty,
    Filled,
    Ticked,
    NotTicked,
}

impl Op {
    pub fn name(self) -> &'static str {
        match self {
            Op::Is => "is",
            Op::IsNot => "isnot",
            Op::Has => "has",
            Op::HasNot => "hasnot",
            Op::Above => "above",
            Op::Below => "below",
            Op::AtLeast => "atleast",
            Op::AtMost => "atmost",
            Op::Empty => "empty",
            Op::Filled => "filled",
            Op::Ticked => "ticked",
            Op::NotTicked => "notticked",
        }
    }

    /// It compares the cell with some words (else it needs none).
    pub fn needs_text(self) -> bool {
        !matches!(self, Op::Empty | Op::Filled | Op::Ticked | Op::NotTicked)
    }
}

/// How a rule's test is written in Markdown (the first of each is how it
/// is written; the others are read too).
const OP_WORDS: &[(&str, Op)] = &[
    ("is", Op::Is),
    ("=", Op::Is),
    ("is not", Op::IsNot),
    ("!=", Op::IsNot),
    ("has", Op::Has),
    ("contains", Op::Has),
    ("has not", Op::HasNot),
    ("does not contain", Op::HasNot),
    ("above", Op::Above),
    (">", Op::Above),
    ("below", Op::Below),
    ("<", Op::Below),
    ("at least", Op::AtLeast),
    (">=", Op::AtLeast),
    ("at most", Op::AtMost),
    ("<=", Op::AtMost),
    ("is empty", Op::Empty),
    ("is filled", Op::Filled),
    ("is not empty", Op::Filled),
    ("is ticked", Op::Ticked),
    ("is checked", Op::Ticked),
    ("is not ticked", Op::NotTicked),
    ("is not checked", Op::NotTicked),
];

fn op_words(op: Op) -> &'static str {
    OP_WORDS.iter().find(|(_, o)| *o == op).map_or("is", |(w, _)| w)
}

/// One thing asked of the rows: a filter (rows that do not pass are not
/// shown), or with a colour a paint rule (rows that pass get the colour).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub column: usize,
    pub op: Op,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// A colour of `ui/icons.rs` ("" for a filter).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    #[serde(default, skip_serializing_if = "Layout::is_table")]
    pub layout: Layout,
    /// The first decides; the next ones order the rows it leaves equal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<Sort>,
    /// A row shows when it passes every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filter: Vec<Rule>,
    /// The column whose values the rows are grouped by (in a board: the
    /// column of cards for each value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<usize>,
    /// Columns that are not shown. The first column always is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<usize>,
    /// How many columns at the left stay in place when the table is
    /// scrolled sideways.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen: usize,
    /// The first rule a row passes gives the row its colour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paint: Vec<Rule>,
    /// Nothing in the table can be changed by a slip of the hand.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// A row opens as a full page (else at the side).
    #[serde(default, skip_serializing_if = "is_false")]
    pub full: bool,
}

impl View {
    pub fn is_plain(&self) -> bool {
        *self == View::default()
    }

    /// Columns came, went or moved: `to` gives the new number of a column,
    /// or `None` for a column that is gone. What named a gone column goes.
    pub fn remap(&mut self, to: impl Fn(usize) -> Option<usize>) {
        self.sort = std::mem::take(&mut self.sort).into_iter().filter_map(|s| to(s.column).map(|column| Sort { column, ..s })).collect();
        for rules in [&mut self.filter, &mut self.paint] {
            *rules = std::mem::take(rules).into_iter().filter_map(|r| to(r.column).map(|column| Rule { column, ..r })).collect();
        }
        self.group = self.group.and_then(&to);
        self.hidden = self.hidden.iter().filter_map(|&c| to(c)).collect();
        // (No more columns than a table can have: a number from a file that
        // was written wrong must not be counted up to.)
        self.frozen = (0..self.frozen.min(super::MOST_COLUMNS)).filter_map(&to).map(|c| c + 1).max().unwrap_or(0);
    }

    /// Nothing that names a column a table of this width does not have, no
    /// column sorted or hidden twice, and the first column in view.
    pub(super) fn tidy(&mut self, width: usize) {
        if self.is_plain() {
            return;
        }
        self.remap(|c| (c < width).then_some(c));
        let mut seen = Vec::new();
        self.sort.retain(|s| !seen.contains(&s.column) && (seen.push(s.column), true).1);
        self.hidden.sort_unstable();
        self.hidden.dedup();
        self.hidden.retain(|&c| c != 0);
        self.frozen = self.frozen.min(width.saturating_sub(1));
    }

    /// Order the rows by column `c` (in place of what ordered them by that
    /// column before; other sorts stay, after it).
    pub fn sort_by(&mut self, c: usize, down: bool) {
        self.sort.retain(|s| s.column != c);
        self.sort.insert(0, Sort { column: c, down });
    }

    /// Show a hidden column, or hide one that shows.
    pub fn toggle_hidden(&mut self, c: usize) {
        match self.hidden.iter().position(|&h| h == c) {
            Some(i) => {
                self.hidden.remove(i);
            }
            None => self.hidden.push(c),
        }
    }
}

/// What a row is ordered by. Cells with nothing in them come last.
#[derive(Clone, Debug, PartialEq)]
enum Key {
    None,
    Amount(f64),
    Words(String),
}

impl Key {
    fn order(&self, other: &Key, down: bool) -> Ordering {
        let turned = |o: Ordering| if down { o.reverse() } else { o };
        match (self, other) {
            (Key::None, Key::None) => Ordering::Equal,
            (Key::None, _) => Ordering::Greater,
            (_, Key::None) => Ordering::Less,
            (Key::Amount(a), Key::Amount(b)) => turned(a.total_cmp(b)),
            (Key::Words(a), Key::Words(b)) => turned(a.cmp(b)),
            (Key::Amount(_), Key::Words(_)) => turned(Ordering::Less),
            (Key::Words(_), Key::Amount(_)) => turned(Ordering::Greater),
        }
    }
}

impl Table {
    /// Change the view, then put it right for this table.
    pub fn change_view(&mut self, change: impl FnOnce(&mut View)) {
        change(&mut self.view);
        self.view.tidy(self.columns());
    }

    /// The columns that show, in order.
    pub fn shown_columns(&self) -> Vec<usize> {
        (0..self.columns()).filter(|c| !self.view.hidden.contains(c)).collect()
    }

    /// What can be asked of the cells of column `c`.
    pub fn ops(&self, c: usize) -> &'static [Op] {
        match self.kind(c) {
            Kind::Number | Kind::Time | Kind::Pace | Kind::Formula | Kind::Rollup => &[Op::Is, Op::IsNot, Op::Above, Op::Below, Op::AtLeast, Op::AtMost, Op::Empty, Op::Filled],
            Kind::Date => &[Op::Is, Op::Below, Op::Above, Op::Empty, Op::Filled],
            Kind::Checkbox => &[Op::Ticked, Op::NotTicked],
            Kind::Choice | Kind::Status => &[Op::Is, Op::IsNot, Op::Empty, Op::Filled],
            Kind::Multi => &[Op::Has, Op::HasNot, Op::Empty, Op::Filled],
            _ => &[Op::Has, Op::HasNot, Op::Is, Op::IsNot, Op::Empty, Op::Filled],
        }
    }

    /// How an op reads for column `c`: "is before" for a day, "is less
    /// than" for an amount.
    pub fn op_label(&self, c: usize, op: Op) -> &'static str {
        let a_day = self.kind(c) == Kind::Date;
        match op {
            Op::Is => "is",
            Op::IsNot => "is not",
            Op::Has => "contains",
            Op::HasNot => "does not contain",
            Op::Above if a_day => "is after",
            Op::Above => "is more than",
            Op::Below if a_day => "is before",
            Op::Below => "is less than",
            Op::AtLeast => "is at least",
            Op::AtMost => "is at most",
            Op::Empty => "is empty",
            Op::Filled => "is not empty",
            Op::Ticked => "is ticked",
            Op::NotTicked => "is not ticked",
        }
    }

    /// A rule in words: "Category is Food".
    pub fn rule_words(&self, rule: &Rule) -> String {
        let said = format!("{} {}", self.heading(rule.column), self.op_label(rule.column, rule.op));
        match rule.text.trim() {
            text if rule.op.needs_text() && !text.is_empty() => format!("{said} {text}"),
            _ if rule.op.needs_text() => format!("{said} …"),
            _ => said,
        }
    }

    /// A sort in words: "Amount, high to low".
    pub fn sort_words(&self, sort: &Sort) -> String {
        format!("{}, {}", self.heading(sort.column), self.direction_words(sort.column, sort.down))
    }

    /// The two ways column `c` can be ordered, in words.
    pub fn direction_words(&self, c: usize, down: bool) -> &'static str {
        match (self.kind(c), down) {
            (Kind::Date, false) => "oldest first",
            (Kind::Date, true) => "newest first",
            (Kind::Checkbox, false) => "not ticked first",
            (Kind::Checkbox, true) => "ticked first",
            (Kind::Choice | Kind::Status, false) => "first to last",
            (Kind::Choice | Kind::Status, true) => "last to first",
            (kind, false) if kind.is_amount() => "low to high",
            (kind, true) if kind.is_amount() => "high to low",
            (_, false) => "A to Z",
            (_, true) => "Z to A",
        }
    }

    /// Whether a row passes a rule. A rule that still waits for its words
    /// lets every row pass.
    pub fn passes(&self, r: usize, rule: &Rule) -> bool {
        let kind = self.kind(rule.column);
        let shown = self.words(r, rule.column);
        let (text, want) = (shown.trim(), rule.text.trim());
        if rule.op.needs_text() && want.is_empty() {
            return true;
        }
        // Amounts and days are compared as what they are, not as letters.
        let measured = kind.is_amount() || kind == Kind::Date;
        let amount = |t: &str| match kind {
            Kind::Date => day(t, crate::when::today()).map(|d| d.num_days_from_ce() as f64),
            Kind::Time | Kind::Pace => seconds(t),
            _ => number(t),
        };
        let compare = |ok: fn(f64, f64) -> bool| matches!((amount(text), amount(want)), (Some(a), Some(b)) if ok(a, b));
        let same = || match kind {
            Kind::Multi => parts(text).iter().any(|p| p.to_lowercase() == want.to_lowercase()),
            // (A formula can give words, "Win" or "Loss": those are
            // compared as words.)
            _ if measured && (amount(text).is_some() || amount(want).is_some()) => compare(|a, b| a == b),
            _ => text.to_lowercase() == want.to_lowercase(),
        };
        let has = || text.to_lowercase().contains(&want.to_lowercase());
        match rule.op {
            Op::Empty => text.is_empty(),
            Op::Filled => !text.is_empty(),
            Op::Ticked => checked(text),
            Op::NotTicked => !checked(text),
            Op::Is => same(),
            Op::IsNot => !same(),
            Op::Has => has(),
            Op::HasNot => !has(),
            Op::Above => compare(|a, b| a > b),
            Op::Below => compare(|a, b| a < b),
            Op::AtLeast => compare(|a, b| a >= b),
            Op::AtMost => compare(|a, b| a <= b),
        }
    }

    fn passes_filters(&self, r: usize) -> bool {
        self.view.filter.iter().all(|rule| self.passes(r, rule))
    }

    /// The rows that count for the totals: those with something in them
    /// that pass every filter.
    pub fn counted(&self) -> Vec<usize> {
        self.body().filter(|&r| self.passes_filters(r)).collect()
    }

    fn key(&self, r: usize, c: usize) -> Key {
        let kind = self.kind(c);
        let shown = self.words(r, c);
        let text = shown.trim();
        let words = || Key::Words(text.to_lowercase());
        match kind {
            Kind::Checkbox => Key::Amount(if checked(text) { 1.0 } else { 0.0 }),
            _ if text.is_empty() => Key::None,
            // (By day, then by time: a day with no time comes first that day.)
            Kind::Date => super::date::When::read(text, crate::when::today(), false)
                .map(|w| Key::Amount(w.start.num_days_from_ce() as f64 + w.time.map_or(0.0, |t| (t.num_seconds_from_midnight() as f64 + 1.0) / 86_401.0)))
                .unwrap_or_else(words),
            // (In the order of its options, as Notion sorts them.)
            Kind::Choice | Kind::Status => self.option_place(c, text).map(|at| Key::Amount(at as f64)).unwrap_or_else(words),
            _ if kind.is_amount() => self.value(r, c).map(Key::Amount).unwrap_or_else(words),
            _ => words(),
        }
    }

    /// The rows to show, in order: those that pass the filters (a row with
    /// nothing in it yet always does, so that it can be filled in), ordered
    /// by the sorts, with rows that have nothing in them last. Without a
    /// filter, nested rows stay under their row (each level is ordered by
    /// itself), and the rows nested in a closed row are left out.
    pub fn order(&self) -> Vec<usize> {
        let sorts = &self.view.sort;
        let keys: Vec<Vec<Key>> = if sorts.is_empty() { Vec::new() } else { (0..self.rows.len()).map(|r| sorts.iter().map(|s| self.key(r, s.column)).collect()).collect() };
        let by_sorts = |a: &usize, b: &usize| {
            let blank = self.is_blank(*a).cmp(&self.is_blank(*b));
            sorts.iter().enumerate().fold(blank, |so_far, (i, s)| so_far.then_with(|| keys[*a][i].order(&keys[*b][i], s.down)))
        };
        if !self.view.filter.is_empty() || self.info.iter().all(|i| i.indent == 0) {
            let mut rows: Vec<usize> = (1..self.rows.len()).filter(|&r| self.is_blank(r) || self.passes_filters(r)).collect();
            if !sorts.is_empty() {
                rows.sort_by(by_sorts);
            }
            return rows;
        }
        let mut out = Vec::with_capacity(self.rows.len());
        self.walk(1, self.rows.len(), &by_sorts, &mut out);
        out
    }

    /// The rows of one level between `from` and `to`, each followed by the
    /// rows nested in it.
    fn walk(&self, from: usize, to: usize, by_sorts: &dyn Fn(&usize, &usize) -> Ordering, out: &mut Vec<usize>) {
        let mut level = Vec::new();
        let mut r = from;
        while r < to {
            level.push(r);
            r = self.subtree_end(r);
        }
        if !self.view.sort.is_empty() {
            level.sort_by(by_sorts);
        }
        for r in level {
            out.push(r);
            if !self.info(r).closed {
                self.walk(r + 1, self.subtree_end(r).min(to), by_sorts, out);
            }
        }
    }

    /// How deep a row shows nested (not at all while a filter is on: then
    /// the rows are a plain list).
    pub fn shown_indent(&self, r: usize) -> u8 {
        if self.view.filter.is_empty() { self.indent(r) } else { 0 }
    }

    /// `rows` in groups by what column `c` says, each with its value. Every
    /// value the column has makes a group, in the order of the rows as they
    /// are kept: a group stays in its place when the rows are sorted, and
    /// stays (empty) when a filter takes its rows. The rows with nothing in
    /// the column are the last group, with "" for its value.
    pub fn groups(&self, rows: &[usize], c: usize) -> Vec<(String, Vec<usize>)> {
        // (A box is ticked or not, however its cell says it: "x" and "yes"
        // made two groups named "Ticked", and "no" a second "Not ticked".)
        let ticks = self.kind(c) == Kind::Checkbox;
        let value = |r: usize| if ticks { if checked(self.cell(r, c)) { "x" } else { "" }.to_string() } else { self.words(r, c).trim().to_string() };
        // A choice column has a group for each option, in their order.
        let mut out: Vec<(String, Vec<usize>)> = if matches!(self.kind(c), Kind::Choice | Kind::Status) { self.column(c).options.iter().map(|o| (o.name.clone(), Vec::new())).collect() } else { Vec::new() };
        for r in self.body() {
            let v = value(r);
            if !v.is_empty() && !out.iter().any(|(g, _)| g.to_lowercase() == v.to_lowercase()) {
                out.push((v, Vec::new()));
            }
        }
        let mut none = Vec::new();
        for &r in rows {
            let v = value(r).to_lowercase();
            match out.iter_mut().find(|(g, _)| g.to_lowercase() == v) {
                Some((_, list)) => list.push(r),
                None => none.push(r),
            }
        }
        if !none.is_empty() {
            out.push((String::new(), none));
        }
        out
    }

    /// The column a board makes its columns of cards from: the group
    /// column, or else the first choice column.
    pub fn board_column(&self) -> Option<usize> {
        self.view.group.or_else(|| self.first_of(Kind::Choice)).or_else(|| self.first_of(Kind::Status))
    }

    /// The view as parts of the `<!-- table: … -->` line, columns by their
    /// headings: "board by Status", "sort Date down", "frozen 1",
    /// "hide Notes", "filter Result is Win", "color P&L below 0 red",
    /// "locked", "full". Read back by `read_view_part`.
    pub(super) fn view_parts(&self) -> Vec<String> {
        let v = &self.view;
        let h = |c: usize| self.heading(c);
        let mut out = Vec::new();
        match (v.layout, v.group) {
            (Layout::Table, Some(g)) => out.push(format!("group {}", h(g))),
            (Layout::Table, None) => {}
            (layout, Some(g)) => out.push(format!("{} by {}", layout.name(), h(g))),
            (layout, None) => out.push(layout.name().to_string()),
        }
        out.extend(v.sort.iter().map(|s| format!("sort {}{}", h(s.column), if s.down { " down" } else { "" })));
        out.extend((v.frozen > 0).then(|| format!("frozen {}", v.frozen)));
        out.extend(v.hidden.iter().map(|&c| format!("hide {}", h(c))));
        let rule = |r: &Rule| {
            let mut words = vec![h(r.column), op_words(r.op).to_string()];
            words.extend((r.op.needs_text() && !r.text.trim().is_empty()).then(|| r.text.trim().to_string()));
            words.join(" ")
        };
        out.extend(v.filter.iter().map(|r| format!("filter {}", rule(r))));
        out.extend(v.paint.iter().filter(|r| !r.color.is_empty()).map(|r| format!("color {} {}", rule(r), r.color)));
        out.extend(v.locked.then(|| "locked".to_string()));
        out.extend(v.full.then(|| "full".to_string()));
        out
    }

    /// Read one part of the `<!-- table: … -->` line that says something of
    /// the view (see `view_parts`). False when the part says nothing known.
    pub(super) fn read_view_part(&mut self, part: &str) -> bool {
        let (word, rest) = part.trim().split_once(char::is_whitespace).map_or((part.trim(), ""), |(w, r)| (w, r.trim()));
        let word = word.to_lowercase();
        let layout = Layout::ALL.into_iter().find(|l| l.name() == word);
        match word.as_str() {
            "locked" if rest.is_empty() => self.view.locked = true,
            "full" if rest.is_empty() => self.view.full = true,
            "frozen" | "freeze" => match rest.parse::<usize>() {
                Ok(n) => self.view.frozen = n,
                Err(_) => return false,
            },
            "group" | "by" => match self.heading_at_start(rest.strip_prefix("by ").unwrap_or(rest)) {
                Some((c, "")) => self.view.group = Some(c),
                _ => return false,
            },
            "sort" => match self.heading_at_start(rest) {
                Some((c, after)) => {
                    let down = matches!(after.to_lowercase().as_str(), "down" | "desc" | "descending" | "newest first" | "high to low" | "z to a" | "last to first");
                    self.view.sort.push(Sort { column: c, down });
                }
                None => return false,
            },
            "hide" => {
                let mut any = false;
                for name in rest.split(',') {
                    if let Some((c, "")) = self.heading_at_start(name.trim()) {
                        self.view.hidden.push(c);
                        any = true;
                    }
                }
                return any;
            }
            "filter" => match self.read_rule(rest, false) {
                Some(rule) => self.view.filter.push(rule),
                None => return false,
            },
            "color" | "colour" | "paint" => match self.read_rule(rest, true) {
                Some(rule) => self.view.paint.push(rule),
                None => return false,
            },
            _ => match layout {
                Some(layout) => {
                    if let Some(after) = Some(rest).filter(|r| !r.is_empty()) {
                        match self.heading_at_start(after.strip_prefix("by ").unwrap_or(after)) {
                            Some((c, "")) => self.view.group = Some(c),
                            _ => return false,
                        }
                    }
                    self.view.layout = layout;
                }
                None => return false,
            },
        }
        true
    }

    /// The column whose heading starts `text` (the longest such heading),
    /// and the words after it.
    fn heading_at_start<'a>(&self, text: &'a str) -> Option<(usize, &'a str)> {
        let low = text.to_lowercase();
        (0..self.columns())
            .filter_map(|c| {
                let h = self.heading(c).to_lowercase();
                let after = low.strip_prefix(&h)?;
                (after.is_empty() || after.starts_with(char::is_whitespace)).then_some((c, h.len()))
            })
            .max_by_key(|&(_, len)| len)
            // (Lower case can change the length of a few letters: cut at a
            // place that is a letter's start in the text itself.)
            .and_then(|(c, len)| text.get(len..).map(|after| (c, after.trim())))
    }

    /// "Result is Win", "P&L below 0", and for a paint rule a colour at the
    /// end: "Result is Loss red".
    fn read_rule(&self, text: &str, painted: bool) -> Option<Rule> {
        let (c, rest) = self.heading_at_start(text)?;
        let (rest, color) = if painted {
            let (rest, last) = rest.rsplit_once(char::is_whitespace).unwrap_or(("", rest));
            let color = super::Color::parse(last).filter(|c| *c != super::Color::Default)?;
            (rest.trim(), color.name().to_string())
        } else {
            (rest, String::new())
        };
        let low = rest.to_lowercase();
        let (op, text) = OP_WORDS
            .iter()
            .filter_map(|&(words, op)| {
                let after = low.strip_prefix(words)?;
                (after.is_empty() || after.starts_with(char::is_whitespace) || !words.ends_with(char::is_alphabetic)).then(|| (op, words.len()))
            })
            .max_by_key(|&(_, len)| len)
            .map(|(op, len)| (op, rest.get(len..).unwrap_or("").trim().to_string()))?;
        (self.ops(c).contains(&op) && (!op.needs_text() || !text.is_empty())).then_some(Rule { column: c, op, text, color })
    }

    /// The colour of a row: that of the first paint rule it passes.
    pub fn color(&self, r: usize) -> Option<&str> {
        if self.is_blank(r) {
            return None;
        }
        self.view.paint.iter().find(|p| !p.color.is_empty() && (!p.op.needs_text() || !p.text.trim().is_empty()) && self.passes(r, p)).map(|p| p.color.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Total, Table};
    use super::*;

    /// The spending of the mocks.
    fn spending() -> Table {
        let rows: &[&[&str]] = &[
            &["Item", "Amount", "Category", "Date", "Paid"],
            &["Rent", "$2,000.00", "Home", "2026-11-01", "x"],
            &["Internet", "$120.00", "Home", "2026-11-03", "x"],
            &["Groceries", "$150.00", "Food", "2026-11-05", "x"],
            &["Dinner with Rachel", "$65.00", "Food", "2026-11-07", "x"],
            &["Coffee beans", "$56.40", "food", "2026-11-08", "x"],
            &["Cinema", "$32.00", "Fun", "2026-11-08", "x"],
            &["Concert tickets", "$63.00", "Fun", "2026-11-13", ""],
            &["Train to Portland", "$48.00", "Travel", "2026-11-14", ""],
        ];
        let mut t = Table::of(rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect());
        t.set_kind(1, Kind::Number);
        t.set_format(1, "USD");
        t.set_kind(2, Kind::Choice);
        t.set_kind(3, Kind::Date);
        t.set_kind(4, Kind::Checkbox);
        t
    }

    fn names(t: &Table, rows: &[usize]) -> Vec<String> {
        rows.iter().map(|&r| t.title(r).to_string()).collect()
    }

    fn rule(column: usize, op: Op, text: &str) -> Rule {
        Rule { column, op, text: text.into(), color: String::new() }
    }

    #[test]
    fn a_plain_view_is_not_kept_and_rows_show_as_they_are() {
        let t = spending();
        assert!(!t.to_data().contains("view"));
        assert_eq!(t.order(), (1..9).collect::<Vec<_>>());
        assert_eq!(t.counted().len(), 8);
        assert_eq!(t.shown_columns(), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn rows_are_ordered_by_the_sorts() {
        let mut t = spending();
        t.add_row(8);
        t.change_view(|v| v.sort_by(1, true));
        // High to low, and the row with nothing in it last.
        assert_eq!(names(&t, &t.order()), ["Rent", "Groceries", "Internet", "Dinner with Rachel", "Concert tickets", "Coffee beans", "Train to Portland", "Cinema", ""]);
        t.change_view(|v| v.sort_by(1, false));
        assert_eq!(names(&t, &t.order())[..2], ["Cinema", "Train to Portland"]);
        assert_eq!(*t.order().last().unwrap(), 9);
        assert_eq!(t.view.sort.len(), 1);
        // By the words of a column, then by the day inside equal words.
        t.change_view(|v| {
            v.sort_by(3, true);
            v.sort_by(2, false);
        });
        assert_eq!(names(&t, &t.order())[..5], ["Internet", "Rent", "Coffee beans", "Dinner with Rachel", "Groceries"]);
        assert_eq!((t.sort_words(&t.view.sort[0]), t.sort_words(&t.view.sort[1])), ("Category, first to last".to_string(), "Date, newest first".to_string()));
        // Ticked boxes first.
        t.change_view(|v| {
            v.sort.clear();
            v.sort_by(4, false);
        });
        assert_eq!(names(&t, &t.order())[..2], ["Concert tickets", "Train to Portland"]);
        // The rows as they are kept did not move.
        assert_eq!(t.title(1), "Rent");
    }

    #[test]
    fn filters_leave_the_rows_that_pass_and_the_totals_count_only_those() {
        let mut t = spending();
        t.set_total(0, Some(Total::Count));
        t.set_total(1, Some(Total::Sum));
        t.set_total(4, Some(Total::Checked));
        t.change_view(|v| v.filter.push(rule(2, Op::Is, "food")));
        assert_eq!(names(&t, &t.order()), ["Groceries", "Dinner with Rachel", "Coffee beans"]);
        assert_eq!((t.total(0), t.total(1), t.total(4)), (Some(("", "3 of 8 rows".to_string())), Some(("Sum", "$271.40".to_string())), Some(("", "3 of 3".to_string()))));
        assert_eq!(t.rule_words(&t.view.filter[0]), "Category is food");
        // A second filter: both must pass. A new row with nothing in it shows, to be filled in.
        t.change_view(|v| v.filter.push(rule(1, Op::AtLeast, "65")));
        t.add_row(8);
        assert_eq!(names(&t, &t.order()), ["Groceries", "Dinner with Rachel", ""]);
        assert_eq!(t.total(1), Some(("Sum", "$215.00".to_string())));
        // A filter that still waits for its words hides nothing.
        t.change_view(|v| v.filter = vec![rule(1, Op::Above, " ")]);
        assert_eq!(t.order().len(), 9);
        assert_eq!(t.rule_words(&t.view.filter[0]), "Amount is more than …");
    }

    #[test]
    fn each_kind_of_column_is_asked_in_its_own_way() {
        let t = spending();
        let passing = |r: Rule| names(&t, &(1..9).filter(|&row| t.passes(row, &r)).collect::<Vec<_>>());
        assert_eq!(passing(rule(1, Op::Above, "$120")), ["Rent", "Groceries"]);
        assert_eq!(passing(rule(1, Op::AtMost, "48")), ["Cinema", "Train to Portland"]);
        assert_eq!(passing(rule(1, Op::Is, "65")), ["Dinner with Rachel"]);
        assert_eq!(passing(rule(1, Op::IsNot, "65")).len(), 7);
        assert_eq!(passing(rule(1, Op::Below, "words")), [] as [&str; 0]);
        assert_eq!(passing(rule(3, Op::Below, "3 Nov 2026")), ["Rent"]);
        assert_eq!(passing(rule(3, Op::Above, "2026-11-08")), ["Concert tickets", "Train to Portland"]);
        assert_eq!(passing(rule(3, Op::Is, "Nov 8, 2026")), ["Coffee beans", "Cinema"]);
        assert_eq!(passing(rule(4, Op::NotTicked, "")), ["Concert tickets", "Train to Portland"]);
        assert_eq!(passing(rule(4, Op::Ticked, "")).len(), 6);
        assert_eq!(passing(rule(0, Op::Has, "IN")), ["Internet", "Dinner with Rachel", "Cinema", "Train to Portland"]);
        assert_eq!(passing(rule(0, Op::HasNot, "e")), ["Train to Portland"]);
        assert_eq!(passing(rule(2, Op::IsNot, "Food")).len(), 5);
        assert_eq!(passing(rule(2, Op::Empty, "")).len(), 0);
        assert_eq!(passing(rule(2, Op::Filled, "")).len(), 8);
        assert_eq!((t.op_label(3, Op::Below), t.op_label(1, Op::Below), t.rule_words(&rule(4, Op::Ticked, ""))), ("is before", "is less than", "Paid is ticked".to_string()));
        assert!(t.ops(4) == [Op::Ticked, Op::NotTicked] && t.ops(1).contains(&Op::AtLeast) && !t.ops(0).contains(&Op::Above));
        // Several choices: one of them is enough.
        let mut tags = Table::of(vec![vec!["Name".into(), "Tags".into()], vec!["a".into(), "Home, Food".into()], vec!["b".into(), "Fun".into()]]);
        tags.set_kind(1, Kind::Multi);
        assert!(tags.passes(1, &rule(1, Op::Is, "food")) && !tags.passes(2, &rule(1, Op::Is, "food")) && tags.passes(1, &rule(1, Op::Has, "hom")));
    }

    #[test]
    fn groups_keep_their_place_and_a_row_takes_the_first_colour_it_passes() {
        let mut t = spending();
        let groups = t.groups(&t.order(), 2);
        assert_eq!(groups.iter().map(|(g, rows)| (g.as_str(), rows.len())).collect::<Vec<_>>(), [("Home", 2), ("Food", 3), ("Fun", 2), ("Travel", 1)]);
        // Sorted and filtered: the groups stay where they were, one of them empty now; rows with no value are last.
        t.set(8, 2, "");
        t.change_view(|v| {
            v.sort_by(1, false);
            v.filter.push(rule(1, Op::Below, "100"));
        });
        let groups = t.groups(&t.order(), 2);
        assert_eq!(groups.iter().map(|(g, rows)| (g.as_str(), names(&t, rows))).collect::<Vec<_>>(), [("Home", vec![]), ("Food", vec!["Coffee beans".to_string(), "Dinner with Rachel".to_string()]), ("Fun", vec!["Cinema".to_string(), "Concert tickets".to_string()]), ("Travel", vec![]), ("", vec!["Train to Portland".to_string()])]);
        assert_eq!(t.board_column(), Some(2));
        t.change_view(|v| v.group = Some(4));
        assert_eq!(t.board_column(), Some(4));
        // Colours.
        t.change_view(|v| {
            v.paint.push(Rule { color: "red".into(), ..rule(4, Op::NotTicked, "") });
            v.paint.push(Rule { color: "green".into(), ..rule(1, Op::Below, "60") });
            v.paint.push(Rule { color: "blue".into(), ..rule(2, Op::Is, "") });
        });
        assert_eq!((t.color(7), t.color(8), t.color(5), t.color(1)), (Some("red"), Some("red"), Some("green"), None));
    }

    #[test]
    fn nested_rows_stay_under_their_row() {
        let mut t = Table::of([["Item", "Amount"], ["Food", "9"], ["Bread", "4"], ["Milk", "2"], ["Rent", "900"], ["Heat", "50"], ["Fun", "30"]].iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect());
        t.set_kind(1, Kind::Number);
        t.nest(2, true);
        t.nest(3, true);
        t.nest(5, true);
        t.change_view(|v| v.sort_by(1, false));
        // Each level by itself: Food (9) before Fun (30) before Rent (900), Milk before Bread.
        assert_eq!(names(&t, &t.order()), ["Food", "Milk", "Bread", "Fun", "Rent", "Heat"]);
        assert_eq!(t.order().iter().map(|&r| t.shown_indent(r)).collect::<Vec<_>>(), [0, 1, 1, 0, 0, 1]);
        t.toggle_closed(1);
        assert_eq!(names(&t, &t.order()), ["Food", "Fun", "Rent", "Heat"]);
        // With a filter the rows are a plain list, closed or not.
        t.change_view(|v| v.filter.push(rule(1, Op::Below, "40")));
        assert_eq!(names(&t, &t.order()), ["Milk", "Bread", "Food", "Fun"]);
        assert!(t.order().iter().all(|&r| t.shown_indent(r) == 0));
    }

    #[test]
    fn the_view_follows_the_columns() {
        let mut t = spending();
        t.change_view(|v| {
            v.sort_by(1, true);
            v.filter.push(rule(2, Op::Is, "Food"));
            v.paint.push(Rule { color: "red".into(), ..rule(4, Op::NotTicked, "") });
            v.group = Some(2);
            v.toggle_hidden(3);
            v.toggle_hidden(0);
            v.frozen = 2;
            v.layout = Layout::Board;
        });
        // (The first column cannot be hidden.)
        assert_eq!(t.view.hidden, [3]);
        assert_eq!(Table::parse(&t.to_data()), t);
        t.insert_column(1);
        assert_eq!((t.view.sort[0].column, t.view.filter[0].column, t.view.paint[0].column, t.view.group, t.view.hidden.clone(), t.view.frozen), (2, 3, 5, Some(3), vec![4], 3));
        t.remove_column(1);
        t.remove_column(2);
        // The filter and the group named the column that went.
        assert_eq!((t.view.sort[0].column, t.view.filter.len(), t.view.paint[0].column, t.view.group, t.view.hidden.clone(), t.view.frozen), (1, 0, 3, None, vec![2], 2));
        assert_eq!(t.shown_columns(), [0, 1, 3]);
        t.change_view(|v| v.toggle_hidden(2));
        assert_eq!(t.shown_columns(), [0, 1, 2, 3]);
        // A file that names columns that are not there.
        let odd = Table::parse(r#"{"rows":[["a","b"]],"view":{"sort":[{"column":7},{"column":1},{"column":1,"down":true}],"hidden":[0,1,1,9],"frozen":5,"group":4}}"#);
        assert_eq!((odd.view.sort.len(), odd.view.hidden.clone(), odd.view.frozen, odd.view.group), (1, vec![1], 1, None));
    }

    #[test]
    fn a_link_to_a_page_in_a_cell_counts_as_the_name_of_the_page() {
        let md = [
            "| Task | Notes | Says |",
            "| --- | --- | --- |",
            "| One | [Zeta](rune://page/ab12) |  |",
            "| Two | apple |  |",
            "| Three | see [Plan](rune://page/cd34) |  |",
            "<!-- columns: text | text | formula = concat(Notes, \"!\") -->",
        ];
        let (mut t, _) = Table::from_markdown(&md).unwrap();
        // The words of a cell: the names the links were written with.
        assert_eq!((t.words(1, 1), t.words(2, 1), t.words(3, 1)), ("Zeta".to_string(), "apple".to_string(), "see Plan".to_string()));
        // A sort by the names (the address sorted before every letter).
        t.view.sort_by(1, false);
        assert_eq!(t.order(), [2, 3, 1]);
        // A filter finds the name, and not the letters of the address.
        let rule = |op: Op, text: &str| Rule { column: 1, op, text: text.into(), color: String::new() };
        assert!(t.passes(1, &rule(Op::Is, "zeta")) && !t.passes(1, &rule(Op::Has, "rune")) && t.passes(3, &rule(Op::Has, "plan")));
        // The heading of a group, a card and a formula say the name.
        assert_eq!(t.groups(&[1, 2, 3], 1).into_iter().map(|(name, _)| name).collect::<Vec<_>>(), ["Zeta", "apple", "see Plan"]);
        assert_eq!(t.seen(1, 1, chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()), "Zeta");
        assert_eq!(t.shown(1, 2), "Zeta!");
        // Search reads the names; the cell itself holds the link as it was written.
        assert!(t.plain_text().contains("One Zeta") && !t.plain_text().contains("rune://"));
        assert_eq!(t.cell(1, 1), "[Zeta](rune://page/ab12)");
        // The page has another name now (the app gives the names): that
        // name counts. A page in the Trash or gone has no name there, and
        // goes by the one its link was written with.
        t.names.insert("ab12".into(), "Alpha".into());
        assert_eq!((t.words(1, 1), t.words(3, 1)), ("Alpha".to_string(), "see Plan".to_string()));
        assert_eq!(t.order(), [1, 2, 3]);
        assert!(t.passes(1, &rule(Op::Is, "Alpha")) && !t.passes(1, &rule(Op::Is, "Zeta")));
        assert_eq!(t.shown(1, 2), "Alpha!");
        assert_eq!(t.linked_pages(), ["ab12", "cd34"]);
    }

    #[test]
    fn the_view_is_written_in_markdown() {
        let md = [
            "| Date | Ticker | Side | P&L | Progress | Result | Reviewed | Notes |",
            "| --- | --- | --- | --- | --- | --- | --- | --- |",
            "| 2026-11-10 | AAPL | Long | 120 | 40 | Win | x | Clean |",
            "| 2026-11-11 | TSLA | Short | -80 | 75 | Loss |  |  |",
            "<!-- columns: date | text | choice {Long:green; Short:red} | number USD sum | number percent ring goal 100 | choice {Win:green; Loss:red} | checkbox | text -->",
            "<!-- table: Trades | icon target:green | board by Result | sort Date down | sort Ticker | frozen 1 | hide Notes | filter Reviewed is not ticked | color P&L below 0 red | color Result is Win green | locked -->",
        ];
        let (t, taken) = Table::from_markdown(&md).unwrap();
        assert_eq!(taken, 6);
        assert_eq!((t.name.as_str(), t.icon.as_str()), ("Trades", "target:green"));
        let v = &t.view;
        assert_eq!((v.layout, v.group, v.frozen, v.hidden.clone(), v.locked), (Layout::Board, Some(5), 1, vec![7], true));
        assert_eq!(v.sort, vec![Sort { column: 0, down: true }, Sort { column: 1, down: false }]);
        assert_eq!(v.filter, vec![Rule { column: 6, op: Op::NotTicked, text: String::new(), color: String::new() }]);
        assert_eq!(v.paint[0], Rule { column: 3, op: Op::Below, text: "0".into(), color: "red".into() });
        assert_eq!(v.paint[1], Rule { column: 5, op: Op::Is, text: "Win".into(), color: "green".into() });
        assert_eq!((t.column(4).show, t.column(4).goal), (super::super::Show::Ring, Some(100.0)));
        // Written again, it says the same, and is read the same.
        let again = t.to_markdown("");
        assert!(again.contains("number percent ring goal 100"), "{again}");
        assert!(again.ends_with("<!-- table: Trades | icon target:green | board by Result | sort Date down | sort Ticker | frozen 1 | hide Notes | filter Reviewed is not ticked | color P&L below 0 red | color Result is Win green | locked -->"), "{again}");
        let lines: Vec<&str> = again.lines().collect();
        let (back, _) = Table::from_markdown(&lines).unwrap();
        assert_eq!(back.view, t.view);
        assert_eq!(back.column(4), t.column(4));
    }

    #[test]
    fn view_words_that_make_no_sense_are_left_out() {
        let md = [
            "| Name | Amount |",
            "| --- | --- |",
            "| Rent | 5 |",
            "<!-- columns: text | number -->",
            "<!-- table: Bills | sort Nothing | frozen many | color Amount above 3 | filter Amount has | gallery by Nobody | list | group Name -->",
        ];
        let (t, _) = Table::from_markdown(&md).unwrap();
        // Only the list and the group are understood (a paint rule needs a colour).
        assert_eq!((t.view.layout, t.view.group, t.view.sort.len(), t.view.paint.len(), t.view.filter.len(), t.view.frozen), (Layout::List, Some(0), 0, 0, 0, 0));
        // A table line with no view words keeps the view the table had.
        let mut old = t.clone();
        old.view.frozen = 1;
        let (mut plain, _) = Table::from_markdown(&md[..4]).unwrap();
        plain.adopt(&old);
        assert_eq!((plain.view.layout, plain.view.frozen), (Layout::List, 1));
        // One that says a view has the last word.
        let (mut said, _) = Table::from_markdown(&md).unwrap();
        said.adopt(&old);
        assert_eq!(said.view.frozen, 0);
    }
}
