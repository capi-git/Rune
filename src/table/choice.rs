//! The words a choice column or a column of several choices offers, each
//! with a colour: a list that belongs to the column, as Notion's options do.
//! A cell still holds the words themselves; every word in a cell is one of
//! the column's options.

use super::{Column, Kind, Table};
use serde::{Deserialize, Serialize};

/// The colour of a tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    #[default]
    Default,
    Gray,
    Brown,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Red,
}

impl Color {
    pub const ALL: [Color; 10] = [Color::Default, Color::Gray, Color::Brown, Color::Orange, Color::Yellow, Color::Green, Color::Blue, Color::Purple, Color::Pink, Color::Red];
    /// The colours new options take, one after the other.
    const NEXT: [Color; 9] = [Color::Blue, Color::Green, Color::Orange, Color::Purple, Color::Pink, Color::Red, Color::Yellow, Color::Brown, Color::Gray];

    /// The colour the option after `count` others takes. (Callers add the
    /// number of the column, so that columns do not all start alike.)
    pub fn next(count: usize) -> Color {
        Color::NEXT[count % Color::NEXT.len()]
    }

    pub fn name(self) -> &'static str {
        match self {
            Color::Default => "default",
            Color::Gray => "gray",
            Color::Brown => "brown",
            Color::Orange => "orange",
            Color::Yellow => "yellow",
            Color::Green => "green",
            Color::Blue => "blue",
            Color::Purple => "purple",
            Color::Pink => "pink",
            Color::Red => "red",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Color::Default => "Default",
            Color::Gray => "Gray",
            Color::Brown => "Brown",
            Color::Orange => "Orange",
            Color::Yellow => "Yellow",
            Color::Green => "Green",
            Color::Blue => "Blue",
            Color::Purple => "Purple",
            Color::Pink => "Pink",
            Color::Red => "Red",
        }
    }

    pub fn parse(s: &str) -> Option<Color> {
        let s = s.trim();
        Color::ALL.into_iter().find(|c| c.name().eq_ignore_ascii_case(s)).or(s.eq_ignore_ascii_case("grey").then_some(Color::Gray))
    }
}

/// One word a column offers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub name: String,
    #[serde(default)]
    pub color: Color,
}

/// The three words a status column starts with.
fn status_options() -> Vec<Choice> {
    [("Not started", Color::Gray), ("In progress", Color::Blue), ("Done", Color::Green)].map(|(name, color)| Choice { name: name.to_string(), color }).to_vec()
}

/// The words in a cell: for several choices those between the commas, else
/// the whole cell.
pub fn names_in(kind: Kind, cell: &str) -> Vec<String> {
    match kind {
        Kind::Multi => cell.split(',').map(str::trim).filter(|n| !n.is_empty()).map(String::from).collect(),
        _ => Some(cell.trim()).filter(|n| !n.is_empty()).map(String::from).into_iter().collect(),
    }
}

impl Column {
    /// The option with this name (any case).
    pub fn option(&self, name: &str) -> Option<&Choice> {
        let name = name.trim();
        self.options.iter().find(|o| o.name.eq_ignore_ascii_case(name))
    }

    /// The colour of a word of this column.
    pub fn color_of(&self, name: &str) -> Color {
        self.option(name).map(|o| o.color).unwrap_or_default()
    }

    pub fn names_in(&self, cell: &str) -> Vec<String> {
        names_in(self.kind, cell)
    }
}

impl Table {
    /// Every word in a cell of a column with options is one of them. A word
    /// that is not yet takes the next colour.
    pub(super) fn sync_options(&mut self) {
        for c in 0..self.columns.len() {
            let kind = self.columns[c].kind;
            if !kind.has_options() {
                continue;
            }
            if kind == Kind::Status && self.columns[c].options.is_empty() {
                self.columns[c].options = status_options();
            }
            for r in 1..self.rows.len() {
                for name in names_in(kind, &self.rows[r][c]) {
                    if self.columns[c].option(&name).is_none() {
                        let color = Color::next(self.columns[c].options.len() + c);
                        self.columns[c].options.push(Choice { name, color });
                    }
                }
            }
        }
    }

    /// The options a column would have if it knew nothing but its cells.
    /// When it has just these there is nothing about them to write down.
    pub(super) fn plain_options(&self, c: usize) -> Vec<Choice> {
        let kind = self.kind(c);
        let mut out: Vec<Choice> = if kind == Kind::Status { status_options() } else { Vec::new() };
        for r in 1..self.rows.len() {
            for name in names_in(kind, self.cell(r, c)) {
                if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&name)) {
                    let color = Color::next(out.len() + c);
                    out.push(Choice { name, color });
                }
            }
        }
        out
    }

    /// A name fit for an option: one line, and no comma in a column whose
    /// cells keep their words apart with commas.
    fn tidy_option(&self, c: usize, name: &str) -> String {
        let name = name.replace(['\n', '\r'], " ");
        let name = if self.kind(c) == Kind::Multi { name.replace(',', " ") } else { name };
        name.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A new option at the end of a column's list, in the next colour. A
    /// name it has already is left as it is. Returns the name as kept.
    pub fn add_option(&mut self, c: usize, name: &str) -> Option<String> {
        let name = self.tidy_option(c, name);
        if name.is_empty() {
            return None;
        }
        let col = self.column_mut(c)?;
        if let Some(o) = col.option(&name) {
            return Some(o.name.clone());
        }
        let color = Color::next(col.options.len() + c);
        col.options.push(Choice { name: name.clone(), color });
        Some(name)
    }

    /// Give an option another name, in the cells that have it too. A name
    /// that another option has is refused. Returns the name it has now.
    pub fn rename_option(&mut self, c: usize, old: &str, new: &str) -> String {
        let new = self.tidy_option(c, new);
        let taken = self.column(c).options.iter().any(|o| o.name.eq_ignore_ascii_case(&new) && !o.name.eq_ignore_ascii_case(old));
        if new.is_empty() || taken {
            return old.to_string();
        }
        if let Some(o) = self.column_mut(c).and_then(|col| col.options.iter_mut().find(|o| o.name.eq_ignore_ascii_case(old))) {
            o.name = new.clone();
        }
        self.map_names(c, |n| Some(if n.eq_ignore_ascii_case(old) { new.clone() } else { n.to_string() }));
        new
    }

    pub fn color_option(&mut self, c: usize, name: &str, color: Color) {
        if let Some(o) = self.column_mut(c).and_then(|col| col.options.iter_mut().find(|o| o.name.eq_ignore_ascii_case(name))) {
            o.color = color;
        }
    }

    /// Put an option at another place in its column's list: before the
    /// option `before`, or last with none.
    pub fn move_option(&mut self, c: usize, name: &str, before: Option<&str>) {
        let Some(col) = self.column_mut(c) else { return };
        let Some(from) = col.options.iter().position(|o| o.name.eq_ignore_ascii_case(name)) else { return };
        if before.is_some_and(|b| b.eq_ignore_ascii_case(name)) {
            return;
        }
        let option = col.options.remove(from);
        let to = before.and_then(|b| col.options.iter().position(|o| o.name.eq_ignore_ascii_case(b))).unwrap_or(col.options.len());
        col.options.insert(to, option);
    }

    /// Take an option away, and out of the cells that have it.
    pub fn remove_option(&mut self, c: usize, name: &str) {
        self.map_names(c, |n| (!n.eq_ignore_ascii_case(name)).then(|| n.to_string()));
        if let Some(col) = self.column_mut(c) {
            col.options.retain(|o| !o.name.eq_ignore_ascii_case(name));
        }
    }

    fn map_names(&mut self, c: usize, f: impl Fn(&str) -> Option<String>) {
        let kind = self.kind(c);
        for r in 1..self.rows.len() {
            let names: Vec<String> = names_in(kind, &self.rows[r][c]).iter().filter_map(|n| f(n)).collect();
            self.rows[r][c] = names.join(", ");
        }
    }

    /// Choose a word in a cell. A single choice takes it, or lets it go
    /// when it has it already; several choices add it or take it away.
    pub fn toggle_name(&mut self, r: usize, c: usize, name: &str) {
        if r == 0 || r >= self.rows.len() || c >= self.columns() {
            return;
        }
        let kind = self.kind(c);
        let mut names = names_in(kind, &self.rows[r][c]);
        let had = names.iter().any(|n| n.eq_ignore_ascii_case(name));
        if had {
            names.retain(|n| !n.eq_ignore_ascii_case(name));
        } else if kind == Kind::Multi {
            names.push(name.to_string());
        } else {
            names = vec![name.to_string()];
        }
        self.rows[r][c] = names.join(", ");
    }

    /// A word as its column has it ("long" is "Long" where that is an
    /// option), and for several choices each of them.
    pub(super) fn as_options(&self, c: usize, text: &str) -> String {
        let col = self.column(c);
        col.names_in(text).iter().map(|n| col.option(n).map(|o| o.name.clone()).unwrap_or_else(|| n.clone())).collect::<Vec<_>>().join(", ")
    }

    /// Where an option stands in its column's list (rows sort by it).
    pub(super) fn option_place(&self, c: usize, name: &str) -> Option<usize> {
        let name = name.trim();
        self.column(c).options.iter().position(|o| o.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Table {
        Table::of(rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect())
    }

    fn names(t: &Table, c: usize) -> Vec<String> {
        t.column(c).options.iter().map(|o| o.name.clone()).collect()
    }

    fn trades() -> Table {
        let mut t = table(&[&["Ticker", "Side", "Tags"], &["AAPL", "Long", "Swing, Earnings"], &["TSLA", "Short", "swing"], &["NVDA", "long", ""]]);
        t.set_kind(1, Kind::Choice);
        t.set_kind(2, Kind::Multi);
        t
    }

    #[test]
    fn options_come_from_the_cells_and_keep_their_colours() {
        let mut t = trades();
        assert_eq!((names(&t, 1), names(&t, 2)), (vec!["Long".to_string(), "Short".to_string()], vec!["Swing".to_string(), "Earnings".to_string()]));
        assert_eq!((t.column(1).color_of("LONG"), t.column(1).color_of("Short")), (Color::Green, Color::Orange));
        t.color_option(1, "Short", Color::Red);
        assert_eq!(Table::parse(&t.to_data()).column(1).color_of("short"), Color::Red);
        assert_eq!(Table::parse(&t.to_data()), t);
        // A word typed by another way in becomes an option.
        t.set(3, 1, "Hedge");
        assert_eq!(names(&t, 1), ["Long", "Short", "Hedge"]);
        // A plain column has none.
        t.set_kind(1, Kind::Text);
        assert!(t.column(1).options.is_empty());
    }

    #[test]
    fn a_cell_takes_and_lets_go_of_its_choices() {
        let mut t = trades();
        t.toggle_name(1, 1, "Short");
        assert_eq!(t.cell(1, 1), "Short");
        t.toggle_name(1, 1, "short");
        assert_eq!(t.cell(1, 1), "");
        t.toggle_name(2, 2, "Earnings");
        assert_eq!(t.cell(2, 2), "swing, Earnings");
        t.toggle_name(2, 2, "SWING");
        assert_eq!(t.cell(2, 2), "Earnings");
        t.toggle_name(0, 1, "Short");
        t.toggle_name(9, 1, "Short");
        assert_eq!(t.cell(0, 1), "Side");
    }

    #[test]
    fn an_option_is_made_renamed_and_taken_away_in_its_cells_too() {
        let mut t = trades();
        assert_eq!(t.add_option(1, "  Flat   out "), Some("Flat out".to_string()));
        assert_eq!(t.add_option(1, "long"), Some("Long".to_string()));
        assert_eq!(t.add_option(1, "  "), None);
        assert_eq!(names(&t, 1), ["Long", "Short", "Flat out"]);
        assert_eq!(t.add_option(2, "a, b"), Some("a b".to_string()));
        assert_eq!(t.rename_option(1, "Long", "Buy"), "Buy");
        assert_eq!((t.cell(1, 1), t.cell(3, 1), t.cell(2, 1)), ("Buy", "Buy", "Short"));
        // A name another option has is refused, and so is no name.
        assert_eq!(t.rename_option(1, "Buy", "short"), "Buy");
        assert_eq!(t.rename_option(1, "Buy", "  "), "Buy");
        t.remove_option(2, "Swing");
        assert_eq!((t.cell(1, 2), t.cell(2, 2)), ("Earnings", ""));
        assert!(t.column(2).option("swing").is_none());
    }

    #[test]
    fn an_option_moves_in_its_list() {
        let mut t = table(&[&["T", "S"], &["a", "One"], &["b", "Two"], &["c", "Three"]]);
        t.set_kind(1, Kind::Choice);
        t.move_option(1, "three", Some("One"));
        assert_eq!(names(&t, 1), ["Three", "One", "Two"]);
        t.move_option(1, "Three", None);
        assert_eq!(names(&t, 1), ["One", "Two", "Three"]);
        t.move_option(1, "One", Some("one"));
        t.move_option(1, "Nothing", Some("One"));
        t.move_option(1, "Two", Some("Nothing"));
        assert_eq!(names(&t, 1), ["One", "Three", "Two"]);
    }

    #[test]
    fn rows_sort_and_group_by_the_order_of_the_options() {
        let mut t = table(&[&["Task", "Status"], &["a", "Done"], &["b", "Todo"], &["c", "Doing"]]);
        t.set_kind(1, Kind::Choice);
        t.column_mut(1).unwrap().options = ["Todo", "Doing", "Done", "Blocked"].iter().enumerate().map(|(i, n)| Choice { name: n.to_string(), color: Color::next(i) }).collect();
        t.change_view(|v| v.sort_by(1, false));
        assert_eq!(t.order(), [2, 3, 1]);
        // A board has a column for each option, with rows or without.
        let groups = t.groups(&t.order(), 1);
        assert_eq!(groups.iter().map(|(g, rows)| (g.as_str(), rows.len())).collect::<Vec<_>>(), [("Todo", 1), ("Doing", 1), ("Done", 1), ("Blocked", 0)]);
    }

    #[test]
    fn a_column_of_several_made_a_single_choice_keeps_the_first() {
        let mut t = trades();
        t.set_kind(2, Kind::Choice);
        assert_eq!((t.cell(1, 2), t.cell(2, 2), t.cell(3, 2)), ("Swing", "swing", ""));
        assert_eq!(names(&t, 2), ["Swing", "Earnings"]);
    }

    #[test]
    fn colours_are_written_down_only_when_they_say_something() {
        let mut t = trades();
        let plain = t.to_markdown("");
        assert!(plain.contains("<!-- columns: text | choice | multi -->"), "{plain}");
        t.color_option(1, "Short", Color::Red);
        t.add_option(1, "Flat");
        let said = t.to_markdown("");
        assert!(said.contains("choice {Long:green; Short:red; Flat:purple}"), "{said}");
        let lines: Vec<&str> = said.lines().collect();
        let (back, _) = Table::from_markdown(&lines).unwrap();
        assert_eq!((back.column(1).color_of("Short"), names(&back, 1)), (Color::Red, vec!["Long".to_string(), "Short".to_string(), "Flat".to_string()]));
        // The AI leaves the list out: the column keeps its list and colours.
        let bare = said.replace(" {Long:green; Short:red; Flat:purple}", "").replace("| Short |", "| Hedge |");
        let lines: Vec<&str> = bare.lines().collect();
        let (mut new, _) = Table::from_markdown(&lines).unwrap();
        new.adopt(&t);
        assert_eq!(names(&new, 1), ["Long", "Short", "Flat", "Hedge"]);
        assert_eq!(new.column(1).color_of("Short"), Color::Red);
    }
}
