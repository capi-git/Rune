//! Typed tables and charts: what a page does when a column changes its
//! kind or total, a box in a table is ticked, a row for today is added, or
//! a chart is changed; and where a chart finds its table.

use super::AppState;
use crate::chart::Chart;
use crate::model::{index_of, BlockKind, Id, Page};
use crate::table::{Kind, Table, Total};
use dioxus::prelude::*;

/// The table a chart draws, and the page it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSource {
    pub page: Id,
    pub title: String,
    pub table: Table,
    /// It is on another page than the chart.
    pub elsewhere: bool,
}

impl AppState {
    pub fn set_column_kind(self, page: &str, id: &str, column: usize, kind: Kind) {
        self.edit_table(page, id, |t| t.set_kind(column, kind));
    }

    pub fn set_column_total(self, page: &str, id: &str, column: usize, total: Option<Total>) {
        self.edit_table(page, id, |t| t.set_total(column, total));
    }

    pub fn set_column_unit(self, page: &str, id: &str, column: usize, unit: &str) {
        self.edit_table(page, id, |t| t.set_unit(column, unit));
    }

    /// Tick or clear a box of a checkbox column.
    pub fn toggle_table_check(self, page: &str, id: &str, row: usize, column: usize) {
        self.edit_table(page, id, |t| {
            let on = t.rows.get(row).and_then(|r| r.get(column)).is_some_and(|c| crate::table::checked(c));
            t.set(row, column, if on { "" } else { "x" });
        });
    }

    /// A row at the end with today in its date column, and the caret in its
    /// first cell to type in.
    pub fn add_today_row(self, page: &str, id: &str) {
        let today = crate::when::today();
        let mut at = None;
        self.edit_table(page, id, |t| {
            at = t.add_day_row(today, today).map(|r| {
                let c = (0..t.columns()).find(|&c| !matches!(t.kind(c), Kind::Date | Kind::Pace | Kind::Checkbox)).unwrap_or(0);
                (r, c)
            });
        });
        if let Some((r, c)) = at {
            document::eval(&format!(
                "requestAnimationFrame(()=>requestAnimationFrame(()=>window.__n2 && window.__n2.focusCell({}, {r}, {c})))",
                serde_json::to_string(id).unwrap_or_default()
            ));
        }
    }

    /// Change what a chart draws. Its title is its text, for search.
    pub fn set_chart(self, page: &str, id: &str, chart: &Chart) {
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id).filter(|&i| bs[i].kind == BlockKind::Chart) {
                bs[i].data = chart.to_data();
                bs[i].text = chart.title.trim().to_string();
            }
        });
    }

    /// The table a chart draws: with `from` empty, the table nearest above
    /// it on its page (else the first one below it); otherwise the first
    /// table of the page with that id or title (this space first).
    pub fn chart_source(&self, page: &str, id: &str, from: &str) -> Option<ChartSource> {
        let blocks = self.blocks.read();
        let pages = self.pages.read();
        let from = from.trim();
        let target = if from.is_empty() || from == page {
            page.to_string()
        } else if pages.contains_key(from) {
            from.to_string()
        } else {
            let space = pages.get(page).map(|p| p.space_id.clone());
            let want = from.to_lowercase();
            let mut found: Vec<&Page> = pages.values().filter(|p| !p.trashed && p.title.trim().to_lowercase() == want).collect();
            found.sort_by_key(|p| (Some(&p.space_id) != space.as_ref(), std::cmp::Reverse(p.updated_at)));
            found.first()?.id.clone()
        };
        let bs = blocks.get(&target)?;
        let table = if target == page {
            let at = index_of(bs, id).unwrap_or(bs.len());
            bs[..at].iter().rev().find(|b| b.kind == BlockKind::Table).or_else(|| bs.iter().find(|b| b.kind == BlockKind::Table))?
        } else {
            bs.iter().find(|b| b.kind == BlockKind::Table)?
        };
        Some(ChartSource {
            title: pages.get(&target).map(|p| p.display_title().to_string()).unwrap_or_default(),
            elsewhere: target != page,
            page: target,
            table: Table::parse(&table.data),
        })
    }

    /// The pages of this page's space that hold a table, by title, for the
    /// "From" list of a chart.
    pub fn pages_with_tables(&self, page: &str) -> Vec<(Id, String)> {
        let blocks = self.blocks.peek();
        let pages = self.pages.peek();
        let space = pages.get(page).map(|p| p.space_id.clone());
        let mut out: Vec<(Id, String)> = pages
            .values()
            .filter(|p| !p.trashed && Some(&p.space_id) == space.as_ref() && p.id != page)
            .filter(|p| blocks.get(&p.id).is_some_and(|bs| bs.iter().any(|b| b.kind == BlockKind::Table)))
            .map(|p| (p.id.clone(), p.display_title().to_string()))
            .collect();
        out.sort_by_key(|(_, title)| title.to_lowercase());
        out
    }
}
