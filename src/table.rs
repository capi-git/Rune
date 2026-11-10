//! Simple tables: rows of cells of plain text, with the first row as the
//! header. A table is one block; its cells are kept in the block's `data`
//! (JSON), and it reads and writes Markdown's pipe tables.

use serde::{Deserialize, Serialize};

/// The most rows and columns a table may have.
pub const MOST_ROWS: usize = 200;
pub const MOST_COLUMNS: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Every row has the same number of cells; the first row is the header.
    pub rows: Vec<Vec<String>>,
}

impl Default for Table {
    /// A new table: a header and two rows, three columns wide.
    fn default() -> Table {
        Table { rows: vec![vec![String::new(); 3]; 3] }
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

    /// Make every row as wide as the widest, with at least one cell at all.
    fn square(&mut self) {
        self.rows.truncate(MOST_ROWS);
        let width = self.rows.iter().map(Vec::len).max().unwrap_or(0).clamp(1, MOST_COLUMNS);
        if self.rows.is_empty() {
            self.rows.push(Vec::new());
        }
        for row in &mut self.rows {
            row.resize(width, String::new());
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
        }
    }

    /// The words of the table, for search and for the AI.
    pub fn plain_text(&self) -> String {
        self.rows.iter().map(|row| row.join(" ")).collect::<Vec<_>>().join("\n")
    }

    /// As a Markdown pipe table (the first row is the header).
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
        let mut table = Table { rows };
        table.square();
        Some((table, taken))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Table {
        Table { rows: rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect() }
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
}
