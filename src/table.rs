//! Tables: rows of cells, with the first row as the header. A table is one
//! block; its cells are kept in the block's `data` (JSON), and it reads and
//! writes Markdown's pipe tables.
//!
//! A column can have a kind (a date, a number, a time, a pace, a checkbox, a
//! choice), a unit and a total that shows at the foot of the table. Cells
//! stay text: the kind says how to read them. A pace is not typed in; it is
//! worked out from the first number column and the first time column of its
//! row (a distance and a time). In Markdown the kinds are on a comment line
//! right under the table, so they survive a page that is written again:
//!
//! ```text
//! | Date | km | Time |
//! | --- | --- | --- |
//! | Tue 1 Dec | 6 | 34:00 |
//! <!-- columns: date count | number km sum | time sum -->
//! ```

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// The most rows and columns a table may have.
pub const MOST_ROWS: usize = 200;
pub const MOST_COLUMNS: usize = 12;

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
}

impl Kind {
    pub const ALL: [Kind; 7] = [Kind::Text, Kind::Number, Kind::Date, Kind::Time, Kind::Pace, Kind::Checkbox, Kind::Choice];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Number => "number",
            Kind::Date => "date",
            Kind::Time => "time",
            Kind::Pace => "pace",
            Kind::Checkbox => "checkbox",
            Kind::Choice => "choice",
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
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(s.trim()))
    }

    /// The totals that mean something for this kind of column.
    pub fn totals(self) -> &'static [Total] {
        match self {
            Kind::Number | Kind::Time => &[Total::Sum, Total::Average, Total::Highest, Total::Lowest, Total::Count],
            Kind::Pace => &[Total::Average, Total::Highest, Total::Lowest],
            Kind::Checkbox => &[Total::Checked],
            Kind::Text | Kind::Date | Kind::Choice => &[Total::Count],
        }
    }

    /// Its cells are amounts, which charts can draw and totals add up.
    pub fn is_amount(self) -> bool {
        matches!(self, Kind::Number | Kind::Time | Kind::Pace)
    }
}

/// What shows at the foot of a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Total {
    Count,
    Sum,
    Average,
    Highest,
    Lowest,
    /// How many boxes are ticked, of how many rows.
    Checked,
}

impl Total {
    pub const ALL: [Total; 6] = [Total::Count, Total::Sum, Total::Average, Total::Highest, Total::Lowest, Total::Checked];

    pub fn name(self) -> &'static str {
        match self {
            Total::Count => "count",
            Total::Sum => "sum",
            Total::Average => "average",
            Total::Highest => "highest",
            Total::Lowest => "lowest",
            Total::Checked => "checked",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Total::Count => "Count",
            Total::Sum => "Sum",
            Total::Average => "Average",
            Total::Highest => "Highest",
            Total::Lowest => "Lowest",
            Total::Checked => "Ticked",
        }
    }

    /// The short word written before the total at the foot.
    pub fn short(self) -> &'static str {
        match self {
            Total::Count => "",
            Total::Sum => "Sum",
            Total::Average => "Avg",
            Total::Highest => "Max",
            Total::Lowest => "Min",
            Total::Checked => "",
        }
    }

    pub fn parse(s: &str) -> Option<Total> {
        let s = s.trim();
        Total::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(s)).or(match s.to_ascii_lowercase().as_str() {
            "avg" | "mean" => Some(Total::Average),
            "max" | "most" => Some(Total::Highest),
            "min" | "least" => Some(Total::Lowest),
            "total" => Some(Total::Sum),
            _ => None,
        })
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Every row has the same number of cells; the first row is the header.
    pub rows: Vec<Vec<String>>,
    /// One for each column, or none at all when every column is plain text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
}

impl Default for Table {
    /// A new table: a header and two rows, three columns wide.
    fn default() -> Table {
        Table { rows: vec![vec![String::new(); 3]; 3], columns: Vec::new() }
    }
}

impl Table {
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
    /// keep one column description for each column (or none).
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
    }

    /// Put text in a cell. Line ends become spaces: a cell is one line.
    pub fn set(&mut self, row: usize, column: usize, text: &str) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(column)) {
            *cell = text.replace(['\n', '\r'], " ");
        }
    }

    /// A new empty row after `row` (at the end if there is no such row).
    pub fn add_row(&mut self, row: usize) {
        if self.rows.len() < MOST_ROWS {
            let at = (row + 1).min(self.rows.len());
            self.rows.insert(at, vec![String::new(); self.columns()]);
        }
    }

    /// A new empty column after `column`.
    pub fn add_column(&mut self, column: usize) {
        if self.columns() < MOST_COLUMNS {
            for row in &mut self.rows {
                let at = (column + 1).min(row.len());
                row.insert(at, String::new());
            }
            if !self.columns.is_empty() {
                let at = (column + 1).min(self.columns.len());
                self.columns.insert(at, Column::default());
            }
        }
    }

    /// Take a row out. The last row stays.
    pub fn remove_row(&mut self, row: usize) {
        if self.rows.len() > 1 && row < self.rows.len() {
            self.rows.remove(row);
        }
    }

    /// Take a column out. The last column stays.
    pub fn remove_column(&mut self, column: usize) {
        if self.columns() > 1 && column < self.columns() {
            for row in &mut self.rows {
                row.remove(column);
            }
            if column < self.columns.len() {
                self.columns.remove(column);
            }
            self.square();
        }
    }

    /// The words of the table, for search and for the AI.
    pub fn plain_text(&self) -> String {
        self.rows.iter().map(|row| row.join(" ")).collect::<Vec<_>>().join("\n")
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

    /// Read column `c` as `kind`. A total that means nothing for the new
    /// kind goes.
    pub fn set_kind(&mut self, c: usize, kind: Kind) {
        if let Some(col) = self.column_mut(c) {
            col.kind = kind;
            if col.total.is_some_and(|t| !kind.totals().contains(&t)) {
                col.total = None;
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
        (1..self.rows.len()).filter(|&r| self.rows[r].iter().any(|c| !c.trim().is_empty()))
    }

    fn cell(&self, r: usize, c: usize) -> &str {
        self.rows.get(r).and_then(|row| row.get(c)).map(String::as_str).unwrap_or("")
    }

    /// The amount in a cell: a number, a time in seconds, a pace in seconds
    /// for each unit, 1 or 0 for a checkbox.
    pub fn value(&self, r: usize, c: usize) -> Option<f64> {
        match self.kind(c) {
            Kind::Time => seconds(self.cell(r, c)),
            Kind::Pace => self.pace(r),
            Kind::Checkbox => Some(if checked(self.cell(r, c)) { 1.0 } else { 0.0 }),
            Kind::Number | Kind::Text | Kind::Date | Kind::Choice => number(self.cell(r, c)),
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

    /// What a cell shows: its text, or the pace worked out for it.
    pub fn shown(&self, r: usize, c: usize) -> String {
        match self.kind(c) {
            Kind::Pace if r > 0 => self.pace(r).map(clock).unwrap_or_default(),
            _ => self.cell(r, c).to_string(),
        }
    }

    /// How many places after the point the numbers of a column use (0 to 2).
    pub fn places(&self, c: usize) -> usize {
        self.body()
            .filter_map(|r| {
                let cell = self.cell(r, c);
                number(cell).map(|_| numeric_part(cell).split(['.', ',']).nth(1).map(str::len).unwrap_or(0))
            })
            .max()
            .unwrap_or(0)
            .min(2)
    }

    /// An amount of column `c` written as the column writes it.
    pub fn format(&self, c: usize, v: f64) -> String {
        match self.kind(c) {
            Kind::Time | Kind::Pace => clock(v),
            _ => format!("{:.*}", self.places(c), v),
        }
    }

    /// What shows at the foot of column `c`: the short word and the amount.
    pub fn total(&self, c: usize) -> Option<(&'static str, String)> {
        let total = self.column(c).total?;
        let rows: Vec<usize> = self.body().collect();
        let values: Vec<f64> = rows.iter().filter_map(|&r| self.value(r, c)).collect();
        let text = match total {
            Total::Count => {
                let n = rows.iter().filter(|&&r| !self.shown(r, c).trim().is_empty()).count();
                format!("{n} {}", if n == 1 { "row" } else { "rows" })
            }
            Total::Checked => {
                let n = rows.iter().filter(|&&r| checked(self.cell(r, c))).count();
                format!("{n} of {}", rows.len())
            }
            Total::Sum if !values.is_empty() => self.format(c, values.iter().sum()),
            Total::Average if !values.is_empty() => self.format(c, values.iter().sum::<f64>() / values.len() as f64),
            Total::Highest => self.format(c, values.iter().copied().reduce(f64::max)?),
            Total::Lowest => self.format(c, values.iter().copied().reduce(f64::min)?),
            Total::Sum | Total::Average => return None,
        };
        Some((total.short(), text))
    }

    pub fn has_totals(&self) -> bool {
        self.columns.iter().any(|c| c.total.is_some())
    }

    // ----- rows by the names of their columns ------------------------------

    /// Write `text` in a cell, in the form its column reads best: a day as
    /// "Tue 1 Dec", a time as "34:00", a tick as "x".
    pub fn put(&mut self, r: usize, c: usize, text: &str, today: NaiveDate) {
        let text = text.trim();
        let tidy = match self.kind(c) {
            Kind::Date => day(text, today).map(|d| format_day(d, today)),
            Kind::Time => seconds(text).map(clock),
            Kind::Number => number(text).map(|_| numeric_part(text).replace(',', ".")),
            Kind::Checkbox => Some(if checked(text) { "x".to_string() } else { String::new() }),
            Kind::Text | Kind::Pace | Kind::Choice => None,
        };
        let text = tidy.unwrap_or_else(|| text.to_string());
        self.set(r, c, &text);
    }

    /// Fill a row from (column, text) pairs. Names no column has are
    /// returned, and nothing is written for them.
    pub fn put_named(&mut self, r: usize, values: &[(String, String)], today: NaiveDate) -> Vec<String> {
        let mut unknown = Vec::new();
        for (name, text) in values {
            match self.find_column(name) {
                Some(c) if self.kind(c) != Kind::Pace => self.put(r, c, text, today),
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
        let empty_last = self.rows.len() > 1 && self.rows.last().is_some_and(|r| r.iter().all(|c| c.trim().is_empty()));
        if !empty_last {
            if self.rows.len() >= MOST_ROWS {
                return None;
            }
            self.rows.push(vec![String::new(); self.columns()]);
        }
        let r = self.rows.len() - 1;
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
    /// kinds of its columns on a comment line under it.
    pub fn to_markdown(&self, pad: &str) -> String {
        let cell = |text: &String| text.replace('|', "\\|").replace('\n', " ");
        let line = |row: &Vec<String>| format!("{pad}| {} |", row.iter().map(cell).collect::<Vec<_>>().join(" | "));
        let mut out: Vec<String> = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            out.push(line(row));
            if i == 0 {
                out.push(format!("{pad}|{}", " --- |".repeat(self.columns())));
            }
        }
        if self.is_typed() {
            let described: Vec<String> = (0..self.columns())
                .map(|c| {
                    let col = self.column(c);
                    let mut words = vec![col.kind.name().to_string()];
                    words.extend(col.unit.clone());
                    words.extend(col.total.map(|t| t.name().to_string()));
                    words.join(" ")
                })
                .collect();
            out.push(format!("{pad}<!-- columns: {} -->", described.join(" | ")));
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
            let inner = inner.strip_suffix('|').unwrap_or(inner);
            let mut out = Vec::new();
            let mut cell = String::new();
            let mut chars = inner.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '\\' if chars.peek() == Some(&'|') => {
                        cell.push('|');
                        chars.next();
                    }
                    '|' => out.push(std::mem::take(&mut cell).trim().to_string()),
                    c => cell.push(c),
                }
            }
            out.push(cell.trim().to_string());
            Some(out)
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
        let mut columns = Vec::new();
        if let Some(described) = lines.get(taken).and_then(|l| l.trim().strip_prefix("<!--")).and_then(|l| l.strip_suffix("-->")).and_then(|l| l.trim().strip_prefix("columns:")) {
            columns = described.split('|').map(read_column).collect();
            taken += 1;
        }
        let mut table = Table { rows, columns };
        table.square();
        Some((table, taken))
    }
}

/// One column of a `<!-- columns: … -->` line: its kind, then a unit or a
/// total in any order ("number km sum").
fn read_column(words: &str) -> Column {
    let mut col = Column::default();
    for (i, word) in words.split_whitespace().enumerate() {
        if i == 0 {
            if let Some(kind) = Kind::parse(word) {
                col.kind = kind;
                continue;
            }
        }
        match Total::parse(word) {
            Some(t) => col.total = Some(t),
            None if col.unit.is_none() => col.unit = Some(word.to_string()),
            None => {}
        }
    }
    col.total = col.total.filter(|t| col.kind.totals().contains(t));
    col
}

// ----- reading cells -------------------------------------------------------

/// The number at the start of a cell, its unit left out: "6", "6.5 km",
/// "6,5", "1,500" (a thousand and five hundred).
pub fn number(text: &str) -> Option<f64> {
    let part = numeric_part(text);
    if part.is_empty() || part == "-" || part == "+" {
        return None;
    }
    let commas = part.matches(',').count();
    let plain = if part.contains('.') || commas > 1 || (commas == 1 && part.split(',').nth(1).is_some_and(|after| after.len() == 3)) {
        part.replace(',', "")
    } else {
        part.replace(',', ".")
    };
    plain.parse().ok()
}

/// The digits (and sign, point, comma) at the start of a cell.
fn numeric_part(text: &str) -> &str {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|&(i, ch)| !(ch.is_ascii_digit() || ch == '.' || ch == ',' || (i == 0 && (ch == '-' || ch == '+'))))
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    text[..end].trim_end_matches([',', '.'])
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
        Table { rows: rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect(), columns: Vec::new() }
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

    #[test]
    fn a_new_table_and_one_that_cannot_be_read() {
        let new = Table::parse("");
        assert_eq!((new.rows.len(), new.columns()), (3, 3));
        assert_eq!(Table::parse("not json"), Table::default());
        // Rows of different widths are made square.
        let ragged = Table::parse(r#"{"rows":[["a"],["b","c","d"],[]]}"#);
        assert_eq!(ragged, table(&[&["a", "", ""], &["b", "c", "d"], &["", "", ""]]));
        assert_eq!(Table::parse(&ragged.to_data()), ragged);
        // A plain table keeps no column descriptions in its data.
        assert!(!ragged.to_data().contains("columns"));
    }

    #[test]
    fn rows_and_columns_come_and_go() {
        let mut t = table(&[&["Name", "Cost"], &["Rent", "2000"]]);
        t.add_row(0);
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
        let lines: Vec<&str> = md.lines().chain(["after"]).collect();
        let (back, taken) = Table::from_markdown(&lines).unwrap();
        assert_eq!(back, t);
        assert_eq!(taken, lines.len() - 1);
        // Written by hand: short words, a unit, a total that does not fit is left out.
        let lines = ["| Day | Time | Name |", "| --- | --- | --- |", "| today | 30 min | a |", "<!--columns: date count|time avg mins| text sum-->"];
        let (t, taken) = Table::from_markdown(&lines).unwrap();
        assert_eq!(taken, 4);
        assert_eq!(t.column(0), Column { kind: Kind::Date, unit: None, total: Some(Total::Count) });
        assert_eq!(t.column(1), Column { kind: Kind::Time, unit: Some("mins".into()), total: Some(Total::Average) });
        assert_eq!(t.column(2), Column::default());
    }

    #[test]
    fn cells_are_read_by_their_kind() {
        assert_eq!(number("6"), Some(6.0));
        assert_eq!(number(" 6.5 km"), Some(6.5));
        assert_eq!(number("6,5"), Some(6.5));
        assert_eq!(number("1,500"), Some(1500.0));
        assert_eq!(number("km"), None);
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
        // A total that does not fit the kind is not kept.
        t.set_total(1, Some(Total::Sum));
        assert_eq!(t.column(1).total, None);
        // Turning a number column into text takes its sum away.
        t.set_kind(2, Kind::Text);
        assert_eq!(t.column(2).total, None);
        assert!(t.has_totals());
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
}
