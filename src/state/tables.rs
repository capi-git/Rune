//! Tables and charts: what a page does when a table changes (a column's
//! kind, format or total; a row added, moved, nested or opened as a page;
//! the view: filters, sorts, layout), or a chart is changed; and where a
//! chart finds its table.

use super::{descendants, remember, AppState, Pane, Popover, Route};
use crate::chart::Chart;
use crate::layout::Edge;
use crate::model::{index_of, new_id, Block, BlockKind, Id, Page};
use crate::table::{Color, Kind, Rolled, Rule, Show, Table, Total, View};
use crate::ui::tables::Pick;
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use std::collections::HashMap;

/// The table a chart draws, and the page it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSource {
    pub page: Id,
    pub title: String,
    pub table: Table,
    /// It is on another page than the chart.
    pub elsewhere: bool,
}

/// How a row opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    /// As its table says: at the side, or as a full page.
    Auto,
    Side,
    Full,
    Tab,
}

/// What the page of a table's template goes by, where the page of a row
/// has what its row goes by (see `Page::row_of`).
const TEMPLATE: &str = "template";

/// Pages that say they are a row of a table whose table or row is not there
/// any more (the table was deleted, say), found when the app starts: Undo
/// cannot bring the row back then. The first list is those with something
/// written in them: they become ordinary pages inside the page of the
/// table, so that they are in reach. The second is those with nothing in
/// them: they go. A page in the Trash, and one whose table cannot be looked
/// at, is left as it is.
pub fn lost_row_pages(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>) -> (Vec<Id>, Vec<Id>) {
    let (mut keep, mut gone) = (Vec::new(), Vec::new());
    for p in pages.values().filter(|p| !p.trashed) {
        let Some(key) = p.row_of.as_deref() else { continue };
        let Some(lines) = p.parent_id.as_ref().and_then(|parent| blocks.get(parent)) else { continue };
        // A locked page that is not open is one sealed line: its tables are
        // inside the seal and cannot be looked at. (Taken as "no table",
        // every start cut the pages of its rows off and deleted the empty ones.)
        if crate::lock::sealed(lines).is_some() {
            continue;
        }
        let found = key.split_once('/').is_some_and(|(block, row)| {
            lines.iter().find(|b| b.id == block && b.kind == BlockKind::Table).is_some_and(|b| row == TEMPLATE || Table::parse(&b.data).row_with_id(row).is_some())
        });
        if found {
            continue;
        }
        if blocks.get(&p.id).is_some_and(|bs| is_written(bs)) { keep.push(p.id.clone()) } else { gone.push(p.id.clone()) }
    }
    (keep, gone)
}

/// The rows that went with a change of a page, for each table whose rows
/// go by names: the table's block, and what each lost row went by. With the
/// table itself gone, that is all its rows, and its template.
pub fn lost_rows(before: &[Block], after: &[Block]) -> Vec<(Id, Vec<String>)> {
    let mut out = Vec::new();
    // (A page that is one sealed line now has its tables inside the seal:
    // no row went.)
    if crate::lock::sealed(after).is_some() {
        return out;
    }
    for was in before.iter().filter(|b| b.kind == BlockKind::Table) {
        let now = after.iter().find(|b| b.id == was.id && b.kind == BlockKind::Table);
        // (Most changes leave a table as it was, and most tables have no rows with names.)
        if now.is_some_and(|now| now.data == was.data || !was.data.contains("\"id\":")) {
            continue;
        }
        let old = Table::parse(&was.data);
        let kept = now.map(|b| Table::parse(&b.data));
        let mut gone: Vec<String> = (1..old.rows.len()).filter_map(|r| old.row_id(r)).filter(|id| kept.as_ref().is_none_or(|t| t.row_with_id(id).is_none())).map(str::to_string).collect();
        if kept.is_none() {
            gone.push(TEMPLATE.to_string());
        }
        if !gone.is_empty() {
            out.push((was.id.clone(), gone));
        }
    }
    out
}

/// Something is written in these lines of a page.
fn is_written(lines: &[Block]) -> bool {
    lines.iter().any(|b| b.kind != BlockKind::Text || !b.text.trim().is_empty())
}

/// The pages of the rows of the tables of the page `parent` that have
/// something written in them, by name. They are not in the sidebar (see
/// `children_of`); an export takes them with their page.
pub fn written_row_pages(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, parent: &str) -> Vec<Page> {
    let mut out: Vec<Page> = pages
        .values()
        .filter(|p| !p.trashed && p.parent_id.as_deref() == Some(parent))
        .filter(|p| p.row_of.as_deref().is_some_and(|key| !key.ends_with(&format!("/{TEMPLATE}"))))
        .filter(|p| blocks.get(&p.id).is_some_and(|bs| is_written(bs)))
        .cloned()
        .collect();
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.created_at.cmp(&b.created_at)));
    out
}

/// The pages of the rows of a table whose name is not the first cell of
/// their row any more, each with the name to take. (A letter typed in a
/// first cell names the page at once; this is for every other change: a
/// cell written the way its column writes, Undo, a row written by the AI.)
pub fn stale_row_titles(pages: &HashMap<Id, Page>, block: &str, table: &Table) -> Vec<(Id, String)> {
    if !table.has_ids() {
        return Vec::new();
    }
    let prefix = format!("{block}/");
    pages
        .values()
        .filter_map(|p| {
            let r = table.row_with_id(p.row_of.as_deref()?.strip_prefix(&prefix)?)?;
            let name = table.cell(r, 0);
            (p.title != name).then(|| (p.id.clone(), name.to_string()))
        })
        .collect()
}

/// The first table block of a page's lines.
fn first_table_block(lines: &[Block]) -> Option<&Block> {
    lines.iter().find(|b| b.kind == BlockKind::Table)
}

/// The first table of a page's lines, where they can be looked at: none
/// for a locked page, shut or open (see `lock::hides`).
fn first_table(page: &str, lines: &[Block]) -> Option<Table> {
    (!crate::lock::hides(lines, page)).then(|| first_table_block(lines).map(|b| Table::parse(&b.data))).flatten()
}

/// The page of a row of the table `block` (`row`: what the row goes by, or
/// `TEMPLATE`): the one that is not in the Trash (the oldest, should there
/// be two), else one in the Trash. With its id and whether it is in the Trash.
fn page_of(pages: &HashMap<Id, Page>, block: &str, row: &str) -> Option<(Id, bool)> {
    let key = format!("{block}/{row}");
    pages.values().filter(|p| p.row_of.as_deref() == Some(key.as_str())).min_by_key(|p| (p.trashed, p.created_at, p.id.clone())).map(|p| (p.id.clone(), p.trashed))
}

/// The page of a table's template that is not in the Trash, if it has one.
fn template_page<'a>(pages: &'a HashMap<Id, Page>, block: &str) -> Option<&'a Page> {
    page_of(pages, block, TEMPLATE).filter(|(_, trashed)| !trashed).and_then(|(id, _)| pages.get(&id))
}

/// How deep rollups reach: the rollups of a table on the page of a row are
/// worked out (a year of months, each a sum of its days), one more level
/// is not, and a column that takes from it shows nothing.
const DEEPEST: u8 = 2;

/// Give a table what its rollup columns take from the first table on the
/// page of each row (`Table::rolled`), and, for Markdown, the table each
/// row's page starts with (`Table::page`, from the template's page; only
/// for a table with a rollup column). A table with no rollup column is
/// left as it is, and nothing is looked at for it.
pub fn roll_up(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, block: &str, table: &mut Table) {
    if table.rollups().is_empty() {
        return;
    }
    table.rolled = rolled(pages, blocks, block, table, 1);
    if table.page.is_none() && table.template.is_some() {
        table.page = template_page(pages, block).and_then(|p| first_table(&p.id, blocks.get(&p.id)?)).map(|mut t| {
            t.rows.truncate(1);
            Box::new(t)
        });
    }
}

/// What the rollup columns of `table` take from the pages of its rows. A row whose page is in
/// the Trash or locked shows nothing. A column with no format of its own
/// shows that of the column it takes from: in the template's table, else
/// the one most pages have (of two as many, that of the page made first).
/// A page that writes its amounts in another kind of money than that is
/// left out, and counted in `Rolled::other`.
pub fn rolled(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, block: &str, table: &Table, depth: u8) -> Rolled {
    let mut out = Rolled::default();
    let rollups = table.rollups();
    if rollups.is_empty() {
        return out;
    }
    let prefix = format!("{block}/");
    let mut rows: Vec<(&str, &Page)> = pages
        .values()
        .filter(|p| !p.trashed)
        .filter_map(|p| Some((p.row_of.as_deref()?.strip_prefix(&prefix)?, p)))
        .filter(|(row, _)| *row != TEMPLATE && table.row_with_id(row).is_some())
        .collect();
    rows.sort_by(|a, b| a.1.created_at.cmp(&b.1.created_at).then(a.1.id.cmp(&b.1.id)));
    let worked = |page: &Page| -> Option<(Table, bool)> {
        let lines = blocks.get(&page.id)?;
        let b = first_table_block(lines).filter(|_| !crate::lock::hides(lines, &page.id))?;
        let mut t = Table::parse(&b.data);
        let deep = !t.rollups().is_empty();
        if deep && depth < DEEPEST {
            t.rolled = rolled(pages, blocks, &b.id, &t, depth + 1);
        }
        Some((t, deep && depth >= DEEPEST))
    };
    let theirs: Vec<(&str, Table, bool)> = rows.iter().filter_map(|(row, p)| worked(p).map(|(t, too_deep)| (*row, t, too_deep))).collect();
    let template = template_page(pages, block).and_then(|p| first_table(&p.id, blocks.get(&p.id)?));
    for c in rollups {
        let col = table.column(c);
        let source = |t: &Table| t.find_heading(&col.of).or_else(|| t.find_column(&col.of));
        let format = if !col.format.is_empty() {
            col.format.clone()
        } else {
            let said = template.as_ref().and_then(|t| source(t).map(|o| t.shown_format(o)));
            said.unwrap_or_else(|| {
                let mut seen: Vec<(String, usize)> = Vec::new();
                for (_, t, _) in &theirs {
                    if let Some(f) = source(t).map(|o| t.shown_format(o)).filter(|f| !f.is_empty()) {
                        match seen.iter_mut().find(|(g, _)| *g == f) {
                            Some((_, n)) => *n += 1,
                            None => seen.push((f, 1)),
                        }
                    }
                }
                // (The most; of two as many, the one seen first.)
                seen.iter().rev().max_by_key(|(_, n)| *n).map(|(f, _)| f.clone()).unwrap_or_default()
            })
        };
        if col.format.is_empty() && !format.is_empty() && col.way != Total::Count {
            out.formats.insert(c, format.clone());
        }
        let money = crate::table::money::currency(&format).map(|m| m.code);
        for (row, t, too_deep) in &theirs {
            let Some(o) = source(t) else { continue };
            // (Worked out from rollups deeper than they reach: nothing, not 0.)
            let unknown = (*too_deep && matches!(t.kind(o), Kind::Rollup | Kind::Formula))
                || t.rolled.too_deep.contains(&o)
                || (t.kind(o) == Kind::Formula && !t.rolled.too_deep.is_empty());
            if unknown {
                out.too_deep.insert(c);
                continue;
            }
            // (Euros are not added to dollars.)
            let theirs_money = crate::table::money::currency(&t.shown_format(o)).map(|m| m.code);
            if col.way != Total::Count && money.is_some() && theirs_money.is_some() && theirs_money != money {
                *out.other.entry(c).or_default() += 1;
                continue;
            }
            if let Some(v) = crate::table::rollup_of(&col, t) {
                out.values.insert((row.to_string(), c), v);
            }
        }
    }
    out
}

/// What `rolled` reads for the table `block`, as it is now: each page it
/// would look at (the pages of the rows, the template's, and a level down
/// the pages of their rows) with the data of its first table, and whether
/// it is in the Trash or locked. A table on the screen works its rollups
/// out again only when this changes. Nothing is parsed here.
pub fn rollup_sources(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, block: &str) -> Vec<(Id, String)> {
    let mut by_table: HashMap<&str, Vec<&Page>> = HashMap::new();
    for p in pages.values() {
        if let Some((table, _)) = p.row_of.as_deref().and_then(|key| key.split_once('/')) {
            by_table.entry(table).or_default().push(p);
        }
    }
    let mut out: Vec<(Id, String)> = Vec::new();
    let mut tables: Vec<String> = vec![block.to_string()];
    for _ in 0..DEEPEST {
        let mut next = Vec::new();
        for table in &tables {
            for p in by_table.get(table.as_str()).into_iter().flatten() {
                let lines = blocks.get(&p.id).map(Vec::as_slice).unwrap_or_default();
                let hidden = crate::lock::hides(lines, &p.id);
                let first = first_table_block(lines).filter(|_| !hidden && !p.trashed);
                out.push((format!("{}{}{}{}", p.id, p.row_of.as_deref().unwrap_or(""), if p.trashed { "/trashed" } else { "" }, if hidden { "/locked" } else { "" }), first.map(|b| b.data.clone()).unwrap_or_default()));
                next.extend(first.map(|b| b.id.clone()));
            }
        }
        tables = next;
    }
    out.sort();
    out
}

/// The table that each row's page of `table` starts with, when the table
/// says none and has rollup columns: an "Item" column, and each column the
/// rollups take from (an amount in the format of its rollup, with the same
/// total at its foot). `None` for a table with no rollup column.
pub fn table_for_rollups(table: &Table) -> Option<Table> {
    let mut heads = vec!["Item".to_string()];
    let mut columns = vec![crate::table::Column::default()];
    for c in table.rollups() {
        let col = table.column(c);
        let of = col.of.trim();
        if of.is_empty() || heads.iter().any(|h| h.eq_ignore_ascii_case(of)) || heads.len() >= crate::table::MOST_COLUMNS {
            continue;
        }
        heads.push(of.to_string());
        columns.push(if col.way == Total::Count {
            crate::table::Column { total: Some(Total::Count), ..Default::default() }
        } else {
            crate::table::Column { kind: Kind::Number, format: table.shown_format(c), places: col.places, total: Some(col.way), ..Default::default() }
        });
    }
    if heads.len() < 2 {
        return None;
    }
    let width = heads.len();
    let mut out = Table::of(vec![heads, vec![String::new(); width]]);
    out.columns = columns;
    Some(Table::parse(&out.to_data()))
}

/// The tables of `lines` (a page's, as kept) that the Markdown the page was
/// written from said what the page of each row starts with (`said`, see
/// `markdown::said_pages`): each said table with the block that is that
/// table, by its name and its headings, each block once, in their order.
pub fn said_for_blocks(lines: &[Block], said: Vec<Table>) -> Vec<(Id, Table)> {
    let heads = |t: &Table| (0..t.columns()).map(|c| t.heading(c).to_lowercase()).collect::<Vec<_>>();
    let tables: Vec<(Id, Table)> = lines.iter().filter(|b| b.kind == BlockKind::Table).map(|b| (b.id.clone(), Table::parse(&b.data))).collect();
    let mut used: Vec<Id> = Vec::new();
    let mut out = Vec::new();
    for table in said {
        // (By its name and headings; else by its headings: a rewrite keeps
        // the name a table had when the Markdown gave it none.)
        let same_name = |t: &Table| t.name.trim().eq_ignore_ascii_case(table.name.trim());
        let free = || tables.iter().filter(|(id, t)| !used.contains(id) && heads(t) == heads(&table));
        let found = free().find(|(_, t)| same_name(t)).or_else(|| free().next()).map(|(id, _)| id.clone());
        if let Some(id) = found {
            used.push(id.clone());
            out.push((id, table));
        }
    }
    out
}

/// The template that the Markdown of the table `block` (on the page
/// `page`) said each row's page starts with (`said.page`, the `page:` part
/// of its table line). It only makes a template: for a table with no
/// template's page that is not in the Trash, it returns the table with its
/// template set, and a new template's page with that table. A table that
/// has one gets `None`: its template's page is never changed this way (it
/// is changed as a page: `read_page` names it).
pub fn row_template(pages: &HashMap<Id, Page>, blocks: &HashMap<Id, Vec<Block>>, page: &str, block: &str, said: &Table) -> Option<(Table, Page, Vec<Block>)> {
    let starts = said.page.as_deref()?;
    if template_page(pages, block).is_some() {
        return None;
    }
    let mut table = blocks.get(page)?.iter().find(|b| b.id == block && b.kind == BlockKind::Table).map(|b| Table::parse(&b.data))?;
    table.template.get_or_insert_with(Default::default);
    let mut p = Page::new(Some(page.to_string()), 0.0);
    p.space_id = pages.get(page).map(|p| p.space_id.clone()).unwrap_or_default();
    p.row_of = Some(format!("{block}/{TEMPLATE}"));
    p.title = "Template".to_string();
    let mut b = Block::new(BlockKind::Table, starts.plain_text());
    b.data = starts.to_data();
    Some((table, p, vec![b]))
}

/// The page of the template of the table `block`, that is not in the Trash.
pub fn template_page_id(pages: &HashMap<Id, Page>, block: &str) -> Option<Id> {
    template_page(pages, block).map(|p| p.id.clone())
}

/// The table a page is a row of.
#[derive(Clone, Debug, PartialEq)]
pub struct RowRef {
    /// The page the table is on, and the table's block.
    pub page: Id,
    pub block: Id,
    /// The row; none for the page of the table's template.
    pub row: Option<usize>,
    pub table: Table,
}

impl AppState {
    /// Put the caret in a cell, once the page shows it.
    fn focus_cell(&self, id: &str, r: usize, c: usize) {
        document::eval(&format!(
            "requestAnimationFrame(()=>requestAnimationFrame(()=>window.__rune && window.__rune.focusCell({}, {r}, {c})))",
            serde_json::to_string(id).unwrap_or_default()
        ));
    }

    /// The table of a block, as it is now.
    pub fn table_at(&self, page: &str, id: &str) -> Option<Table> {
        let mut table = self.table_alone(page, id)?;
        if !table.rollups().is_empty() {
            table.rolled = rolled(&self.pages.peek(), &self.blocks.peek(), id, &table, 1);
        }
        Some(table)
    }

    /// The same, with nothing taken from the pages of its rows.
    fn table_alone(&self, page: &str, id: &str) -> Option<Table> {
        let blocks = self.blocks.peek();
        let b = blocks.get(page)?.iter().find(|b| b.id == id && b.kind == BlockKind::Table)?;
        let mut table = Table::parse(&b.data);
        self.name_links(&mut table, false);
        Some(table)
    }

    /// The tables of `page` that `markdown` (what the page was just written
    /// from: the AI's reply) said what the page of each row starts with:
    /// each that has no template yet gets one (see `row_template`). Nothing
    /// is read when the Markdown has no `page:` part.
    pub(super) fn make_row_templates(mut self, page: &str, markdown: &str) {
        if !markdown.to_lowercase().contains("page:") {
            return;
        }
        let lines = self.blocks.peek().get(page).cloned().unwrap_or_default();
        for (block, said) in said_for_blocks(&lines, crate::markdown::said_pages(markdown)) {
            let Some((table, holder, lines)) = row_template(&self.pages.peek(), &self.blocks.peek(), page, &block, &said) else { continue };
            self.note_in_table(page, &block, |t| t.template = table.template.clone());
            let pid = holder.id.clone();
            self.pages.write().insert(pid.clone(), holder);
            self.blocks.write().insert(pid.clone(), lines);
            self.save_page(&pid);
            self.save_blocks(&pid);
        }
    }

    /// The headings of the table that the pages of the rows of a table
    /// start with (its template's), or else of the first page of a row
    /// that has a table: what a rollup column can take from.
    pub fn columns_on_row_pages(&self, block: &str) -> Vec<String> {
        let (pages, blocks) = (self.pages.peek(), self.blocks.peek());
        let prefix = format!("{block}/");
        let of_page = |p: &Page| blocks.get(&p.id).and_then(|lines| first_table(&p.id, lines));
        let table = template_page(&pages, block).and_then(of_page).or_else(|| {
            let mut rows: Vec<&Page> = pages.values().filter(|p| !p.trashed && p.row_of.as_deref().is_some_and(|k| k.starts_with(&prefix))).collect();
            rows.sort_by_key(|p| p.created_at);
            rows.into_iter().find_map(of_page)
        });
        table.map(|t| (0..t.columns()).map(|c| t.heading(c)).collect()).unwrap_or_default()
    }

    /// Give a table the names that the pages linked in its cells have now
    /// (see `Table::words`): its sort, its filters and its groups read a
    /// link as that name. A page in the Trash, or gone, gets none. With
    /// `watch` (for the screen), the table is drawn again when a page gets
    /// another name. A table with no link is left as it is, and watches
    /// nothing.
    pub fn name_links(&self, table: &mut Table, watch: bool) {
        let linked = table.linked_pages();
        if linked.is_empty() {
            return;
        }
        let pages = if watch { self.pages.read() } else { self.pages.peek() };
        table.names = linked.into_iter().filter_map(|id| pages.get(&id).filter(|p| !p.trashed).map(|p| p.display_title().to_string()).map(|name| (id, name))).collect();
    }

    /// Change what a table is made of: its columns, their kinds, its name.
    /// A locked table stays as it is.
    fn edit_columns(self, page: &str, id: &str, change: impl FnOnce(&mut Table)) {
        self.edit_table(page, id, |t| {
            if !t.view.locked {
                change(t);
            }
        });
    }

    /// Change a table with no step for Undo (the rows get the names their
    /// pages find them by: see `row_page`).
    fn note_in_table(self, page: &str, id: &str, change: impl FnOnce(&mut Table)) {
        let mut bs = self.blocks.peek().get(page).cloned().unwrap_or_default();
        if let Some(i) = index_of(&bs, id).filter(|&i| bs[i].kind == BlockKind::Table) {
            let mut table = Table::parse(&bs[i].data);
            change(&mut table);
            bs[i].data = table.to_data();
            self.set_blocks(page, bs, true);
        }
    }

    // ----- columns ---------------------------------------------------------

    pub fn set_column_kind(self, page: &str, id: &str, column: usize, kind: Kind) {
        self.edit_columns(page, id, |t| t.set_kind(column, kind));
    }

    pub fn set_column_total(self, page: &str, id: &str, column: usize, total: Option<Total>) {
        self.edit_columns(page, id, |t| t.set_total(column, total));
    }

    pub fn set_column_unit(self, page: &str, id: &str, column: usize, unit: &str) {
        self.edit_columns(page, id, |t| t.set_unit(column, unit));
    }

    /// How the numbers of a column are written ("" as typed, "USD"…).
    pub fn set_column_format(self, page: &str, id: &str, column: usize, format: &str) {
        self.edit_columns(page, id, |t| t.set_format(column, format));
    }

    pub fn set_column_places(self, page: &str, id: &str, column: usize, places: Option<u8>) {
        self.edit_columns(page, id, |t| t.set_places(column, places));
    }

    /// Show the numbers of a column plain, with a bar or with a ring.
    /// Show the amounts of a column green above zero and red below it.
    pub fn set_column_signed(self, page: &str, id: &str, column: usize, on: bool) {
        self.edit_columns(page, id, |t| {
            if t.kind(column).is_amount() {
                t.columns[column].signed = on;
            }
        });
    }

    pub fn set_column_show(self, page: &str, id: &str, column: usize, show: Show) {
        self.edit_columns(page, id, |t| {
            let goal = t.column(column).goal;
            t.set_show(column, show, goal);
        });
    }

    /// The amount at which a bar is full (nothing: the highest of the column).
    pub fn set_column_goal(self, page: &str, id: &str, column: usize, text: &str) {
        self.edit_columns(page, id, |t| {
            let show = t.column(column).show;
            t.set_show(column, show, crate::table::number(text));
        });
    }

    pub fn set_column_formula(self, page: &str, id: &str, column: usize, formula: &str) {
        self.edit_columns(page, id, |t| t.set_formula(column, formula));
    }

    /// Which column of the table on each row's page a rollup column takes
    /// its amounts from, and the way it takes them together.
    pub fn set_column_rollup(self, page: &str, id: &str, column: usize, of: &str, way: Total) {
        self.edit_columns(page, id, |t| t.set_rollup(column, of, way));
    }

    /// The table whose rows a relation column names.
    pub fn set_column_from(self, page: &str, id: &str, column: usize, table: &str) {
        self.edit_columns(page, id, |t| t.set_from(column, table));
    }

    /// One of the commands of a column's menu: "sort-up", "sort-down",
    /// "filter", "group", "hide", "freeze", "unfreeze", "left", "right",
    /// "duplicate", "delete".
    pub fn column_op(self, page: &str, id: &str, c: usize, op: &str) {
        let mut then = None;
        let was = (op == "delete").then(|| self.before_removal(page, id)).flatten();
        match op {
            "sort-up" | "sort-down" => self.change_table_view(page, id, |v| v.sort_by(c, op == "sort-down")),
            "filter" => return self.add_table_filter(page, id, c),
            "group" => self.change_table_view(page, id, |v| v.group = if v.group == Some(c) { None } else { Some(c) }),
            "hide" => self.change_table_view(page, id, |v| v.toggle_hidden(c)),
            "freeze" => self.change_table_view(page, id, |v| v.frozen = c + 1),
            "unfreeze" => self.change_table_view(page, id, |v| v.frozen = 0),
            _ => self.edit_columns(page, id, |t| match op {
                "left" => then = t.insert_column(c).then_some(c),
                "right" => then = t.insert_column(c + 1).then_some(c + 1),
                "duplicate" => then = t.duplicate_column(c).then_some(c + 1),
                "delete" => t.remove_column(c),
                _ => {}
            }),
        }
        self.set_popover(None);
        // The caret goes to the heading of a new column, to name it.
        if let Some(c) = then {
            self.focus_cell(id, 0, c);
        }
        self.say_removed(page, id, was);
    }

    /// A new column of a kind, at the right end, with the caret in its heading.
    pub fn add_table_column(self, page: &str, id: &str, kind: Kind) {
        let mut at = None;
        self.edit_columns(page, id, |t| {
            let c = t.columns();
            if t.insert_column(c) {
                t.set_kind(c, kind);
                at = Some(c);
            }
        });
        self.set_popover(None);
        match at {
            Some(c) => self.focus_cell(id, 0, c),
            None => self.toast("A table holds at most 12 columns"),
        }
    }

    // ----- the table --------------------------------------------------------

    pub fn set_table_name(self, page: &str, id: &str, name: &str) {
        let name = name.replace(['\n', '\r'], " ");
        self.edit_columns(page, id, |t| t.name = name.trim().to_string());
    }

    /// The icon beside a table's name ("name:color"), or none.
    pub fn set_table_icon(self, page: &str, id: &str, icon: &str) {
        self.edit_columns(page, id, |t| t.icon = icon.to_string());
    }

    /// Whether a table can stand beside the block above it: that block is a
    /// table too, and no more than two already stand in that line.
    pub fn can_stand_beside(&self, page: &str, id: &str) -> bool {
        let blocks = self.blocks.peek();
        let Some(bs) = blocks.get(page) else { return false };
        let Some(i) = index_of(bs, id).filter(|&i| i > 0) else { return false };
        let in_line = bs[..i].iter().rev().take_while(|b| b.kind == BlockKind::Table && b.place.beside).count();
        bs[i - 1].kind == BlockKind::Table && bs[i - 1].indent == bs[i].indent && in_line < 2
    }

    /// Put a table beside the table above it, or back on a line of its own.
    pub fn set_table_beside(self, page: &str, id: &str, beside: bool) {
        if beside && !self.can_stand_beside(page, id) {
            return;
        }
        self.edit_blocks(page, |bs| {
            if let Some(i) = index_of(bs, id).filter(|&i| bs[i].kind == BlockKind::Table) {
                bs[i].place.beside = beside;
            }
        });
        self.set_popover(None);
    }

    /// Change how a table is shown (its filters, sorts, layout…).
    pub fn change_table_view(self, page: &str, id: &str, change: impl FnOnce(&mut View)) {
        self.edit_table(page, id, |t| t.change_view(change));
    }

    /// A new filter on a column, open to say what it asks.
    pub fn add_table_filter(self, page: &str, id: &str, c: usize) {
        let mut index = 0;
        self.edit_table(page, id, |t| {
            let op = t.ops(c)[0];
            t.change_view(|v| v.filter.push(Rule { column: c, op, text: String::new(), color: String::new() }));
            index = t.view.filter.len().saturating_sub(1);
        });
        self.set_popover(Some(Popover::TableChip { block_id: id.to_string(), sort: false, index }));
    }

    /// A new colour rule, on the first column that is not the names.
    pub fn add_table_paint(self, page: &str, id: &str) {
        self.edit_table(page, id, |t| {
            let c = if t.columns() > 1 { 1 } else { 0 };
            let op = t.ops(c)[0];
            let used: Vec<String> = t.view.paint.iter().map(|p| p.color.clone()).collect();
            let color = ["red", "orange", "yellow", "green", "blue", "purple", "pink", "brown", "gray"].into_iter().find(|c| !used.iter().any(|u| u == c)).unwrap_or("gray");
            t.change_view(|v| v.paint.push(Rule { column: c, op, text: String::new(), color: color.to_string() }));
        });
    }

    // ----- cells and rows ----------------------------------------------------

    /// A cell was typed in and left: its column may learn what it holds
    /// from it ("$3,500" makes the column US Dollar), and the cell is
    /// written the way its column writes.
    pub fn settle_table_cell(self, page: &str, id: &str, row: usize, column: usize) {
        let today = crate::when::today();
        // (The columns of a locked table stay what they are.)
        self.edit_table(page, id, |t| if t.view.locked { t.tidy(row, column, today) } else { t.settle(row, column, today) });
    }

    /// Give a cell other words, written the way its column writes.
    pub fn put_table_cell(self, page: &str, id: &str, row: usize, column: usize, text: &str) {
        let today = crate::when::today();
        self.edit_table(page, id, |t| {
            if row >= 1 && row < t.rows.len() && !t.kind(column).is_worked_out() {
                t.put(row, column, text, today);
            }
        });
    }

    /// Tick or clear a box of a checkbox column.
    pub fn toggle_table_check(self, page: &str, id: &str, row: usize, column: usize) {
        self.edit_table(page, id, |t| {
            let on = t.rows.get(row).and_then(|r| r.get(column)).is_some_and(|c| crate::table::checked(c));
            t.set(row, column, if on { "" } else { "x" });
        });
    }

    /// Add or take away one of the words of a cell that holds several (a
    /// row of another table named in a relation cell).
    pub fn toggle_cell_part(self, page: &str, id: &str, row: usize, column: usize, part: &str) {
        self.edit_table(page, id, |t| {
            let text = crate::table::toggle_part(t.cell(row, column), part);
            t.set(row, column, &text);
        });
    }

    /// Choose an option in a cell, making it first if it is new. (A
    /// locked table keeps its list of options, not its cells.)
    pub fn pick_table_option(self, page: &str, id: &str, row: usize, column: usize, name: &str, make: bool) {
        self.edit_table(page, id, |t| {
            let made = if make && !t.view.locked { t.add_option(column, name) } else { None };
            if !make || made.is_some() {
                t.toggle_name(row, column, made.as_deref().unwrap_or(name));
            }
        });
    }

    /// The list of a cell closes, and the caret goes back to the cell.
    pub fn close_table_cell(self, id: &str, row: usize, column: usize) {
        self.set_popover(None);
        self.focus_cell(id, row, column);
    }

    /// Change one value of a row from its menu ("Edit property"): a box
    /// is ticked or cleared, a picture is chosen, a cell with a list or a
    /// month opens it, and any other cell takes the caret.
    pub fn edit_table_cell(self, page: &str, id: &str, row: usize, column: usize) {
        let Some(table) = self.table_at(page, id).filter(|t| row >= 1 && row < t.rows.len() && column < t.columns()) else { return };
        match table.kind(column) {
            Kind::Checkbox => {
                self.set_popover(None);
                self.toggle_table_check(page, id, row, column);
            }
            Kind::File => {
                self.set_popover(None);
                self.pick_cell_picture(page, id, row, column);
            }
            k if k.has_options() || k.is_worked_out() || matches!(k, Kind::Date | Kind::Relation) => {
                self.set_popover(Some(Popover::TableCell { block_id: id.to_string(), row, column, on_page: false }));
            }
            _ => self.close_table_cell(id, row, column),
        }
    }

    /// Give an option another name. Returns the name it has now.
    pub fn rename_table_option(self, page: &str, id: &str, column: usize, old: &str, new: &str) -> String {
        let mut kept = old.to_string();
        self.edit_columns(page, id, |t| kept = t.rename_option(column, old, new));
        kept
    }

    pub fn color_table_option(self, page: &str, id: &str, column: usize, name: &str, color: Color) {
        self.edit_columns(page, id, |t| t.color_option(column, name, color));
    }

    /// Put an option before another in its list (last with none).
    pub fn move_table_option(self, page: &str, id: &str, column: usize, name: &str, before: Option<&str>) {
        self.edit_columns(page, id, |t| t.move_option(column, name, before));
    }

    pub fn remove_table_option(self, page: &str, id: &str, column: usize, name: &str) {
        self.edit_columns(page, id, |t| t.remove_option(column, name));
    }

    /// A new row at the end (an empty last row is used again), with the
    /// caret in its first cell. It passes the filters that say what a cell
    /// is, so that it stays in view; `fill` puts words in cells (the column
    /// of a board it was added in).
    pub fn new_table_row(self, page: &str, id: &str, fill: &[(usize, String)]) {
        let today = crate::when::today();
        let mut at = None;
        self.edit_table(page, id, |t| {
            let last = t.rows.len() - 1;
            let r = if last >= 1 && t.is_blank(last) { Some(last) } else { t.add_row(t.rows.len()) };
            let Some(r) = r else { return };
            t.fill_for_filters(r, today);
            for (c, text) in fill {
                t.put(r, *c, text, today);
            }
            at = Some(r);
        });
        match at {
            Some(r) => self.focus_cell(id, r, 0),
            None => self.toast("A table holds at most 200 rows"),
        }
    }

    /// A row at the end with today in its date column, and the caret in its
    /// first cell to type in.
    pub fn add_today_row(self, page: &str, id: &str) {
        self.add_day_row(page, id, crate::when::today());
    }

    /// A row at the end for a day (a day of a table shown as a calendar).
    pub fn add_day_row(self, page: &str, id: &str, day: chrono::NaiveDate) {
        let today = crate::when::today();
        let mut at = None;
        self.edit_table(page, id, |t| {
            at = t.add_day_row(day, today).map(|r| {
                // (It passes the filters, so that it stays in view.)
                t.fill_for_filters(r, today);
                let c = (0..t.columns()).find(|&c| !matches!(t.kind(c), Kind::Date | Kind::Pace | Kind::Formula | Kind::Checkbox)).unwrap_or(0);
                (r, c)
            });
        });
        if let Some((r, c)) = at {
            self.focus_cell(id, r, c);
        }
    }

    /// What the page script asks for a table: a row or a column added or
    /// taken out at the cell with the caret, a row opened, moved or folded,
    /// the menu of a row.
    pub fn table_op(self, page: &str, id: &str, op: &str, row: usize, column: usize) {
        match op {
            // The caret came into the table, or left it (see `ui::tables::HELD`).
            "hold" => return crate::ui::tables::hold(id, self.table_at(page, id).as_ref()),
            "release" => return crate::ui::tables::hold(id, None),
            // A click away from the picked rows or columns.
            "unpick" => return crate::ui::tables::unpick(id),
            // ⇧ (`column` is 1) or ⌘ (2) and a click on the handle of a row.
            "pick-row" => return self.pick_row(page, id, row, if column == 1 { Pick::Range } else { Pick::Toggle }),
            // The same on the heading of a column (`row` says which key).
            "pick-column" => return self.pick_column(page, id, column, if row == 1 { Pick::Range } else { Pick::Toggle }),
            // The box in the row of the headings: all rows, or none.
            "pick-all" => {
                let Some(table) = self.table_at(page, id) else { return };
                self.set_popover(None);
                crate::ui::tables::pick_all(id, &table);
                return;
            }
            // The Delete key, with rows or columns picked.
            "delete-picked" => return self.delete_picked_in_table(page, id),
            // ⌘D, with rows picked: a copy of each under it.
            "duplicate-picked" => {
                let Some(table) = self.table_at(page, id) else { return };
                let rows = crate::ui::tables::picked(id, &table).0;
                if !rows.is_empty() {
                    self.rows_op(page, id, &rows, "duplicate");
                }
                return;
            }
            "open-row" => return self.open_row(page, id, row, if column == 1 { Open::Tab } else { Open::Auto }),
            "row-menu" => return self.row_menu(page, id, row),
            "fold-row" => return self.edit_table(page, id, |t| t.toggle_closed(row)),
            // To before the row `column`, or to after it and the rows nested in it.
            "move-row" | "move-after" => {
                return self.edit_table(page, id, |t| {
                    if op == "move-after" {
                        t.move_row_after(row, column);
                    } else {
                        t.move_row(row, column);
                    }
                })
            }
            _ => {}
        }
        let mut focus = None;
        let today = crate::when::today();
        let was = matches!(op, "remove-row" | "remove-column").then(|| self.before_removal(page, id)).flatten();
        self.edit_table(page, id, |t| {
            if t.view.locked && matches!(op, "add-column" | "remove-column") {
                return;
            }
            // (A new row gets what the filters ask of a cell, as the "New
            // row" line gives it: else the row is gone as soon as it is made.)
            let fresh = |t: &mut crate::table::Table, at: usize, c: usize| {
                t.add_row(at).map(|r| {
                    t.fill_for_filters(r, today);
                    // (The new row is put after `at`: `at` is still that row.)
                    t.fill_for_group(r, at);
                    (r, c)
                })
            };
            focus = match op {
                "add-row" => fresh(t, row, column),
                "add-below" => fresh(t, row, 0),
                "add-column" => {
                    t.add_column(column);
                    Some((row, column + 1))
                }
                "remove-row" => {
                    t.remove_row(row);
                    Some((row.saturating_sub(1), column))
                }
                "remove-column" => {
                    t.remove_column(column);
                    Some((row, column.saturating_sub(1)))
                }
                _ => None,
            };
        });
        // The caret goes to the new row or column, or stays near what went.
        if let Some((r, c)) = focus {
            self.focus_cell(id, r, c);
        }
        self.say_removed(page, id, was);
    }

    /// How many rows and columns a table has, and the step that Undo would
    /// take back on its page, before rows or columns are taken out.
    fn before_removal(&self, page: &str, id: &str) -> Option<(usize, usize, Option<u64>)> {
        let table = self.table_at(page, id)?;
        Some((table.rows.len(), table.columns(), self.history.peek().next_undo(page)))
    }

    /// Say what was taken out of a table (`was`: see `before_removal`),
    /// with an "Undo" that takes back that one step. (In words: the Delete
    /// key can take out what is out of sight.)
    fn say_removed(self, page: &str, id: &str, was: Option<(usize, usize, Option<u64>)>) {
        let (Some((rows, columns, step)), Some(now)) = (was, self.table_at(page, id)) else { return };
        let said = match (rows.saturating_sub(now.rows.len()), columns.saturating_sub(now.columns())) {
            (0, 0) => return,
            (1, 0) => "1 row deleted".to_string(),
            (n, 0) => format!("{n} rows deleted"),
            (_, 1) => "1 column deleted".to_string(),
            (_, n) => format!("{n} columns deleted"),
        };
        self.toast_step(said, page, step);
    }

    /// Open the menu of a row. A row that is one of several picked rows
    /// opens the menu of them all; any other row is picked alone first.
    pub fn row_menu(self, page: &str, id: &str, row: usize) {
        let Some(table) = self.table_at(page, id).filter(|t| row >= 1 && row < t.rows.len()) else { return };
        let mut rows = crate::ui::tables::picked(id, &table).0;
        if rows.len() > 1 && rows.contains(&row) {
            // (The menu hangs under the last of its rows: the one clicked.)
            rows.retain(|&r| r != row);
            rows.push(row);
        } else {
            rows = crate::ui::tables::pick_row(id, &table, row, Pick::Alone);
        }
        self.set_popover(Some(Popover::TableRow { block_id: id.to_string(), rows }));
    }

    /// Pick one more row, or the rows up to it (those that show between
    /// them: a row that a filter hides is not picked), with no menu.
    pub fn pick_row(self, page: &str, id: &str, row: usize, how: Pick) {
        let Some(table) = self.table_at(page, id).filter(|t| row >= 1 && row < t.rows.len()) else { return };
        self.set_popover(None);
        crate::ui::tables::pick_row(id, &table, row, how);
    }

    /// Pick one more column, or the columns up to it (those that show).
    /// A locked table keeps its columns: there is nothing to pick them for.
    pub fn pick_column(self, page: &str, id: &str, column: usize, how: Pick) {
        let Some(table) = self.table_at(page, id).filter(|t| column < t.columns() && !t.view.locked) else { return };
        self.set_popover(None);
        crate::ui::tables::pick_column(id, &table, column, how);
    }

    /// Take out what is picked in a table: its rows, or its columns. One
    /// step for Undo, however many they are.
    pub fn delete_picked_in_table(self, page: &str, id: &str) {
        let Some(table) = self.table_at(page, id) else { return };
        let (rows, columns) = crate::ui::tables::picked(id, &table);
        // (Each says what it took out: see `say_removed`.)
        if !rows.is_empty() {
            self.rows_op(page, id, &rows, "delete");
        } else if !columns.is_empty() {
            self.columns_op(page, id, &columns, "delete");
        }
    }

    /// One of the commands for several picked columns: "delete", "hide".
    /// The table keeps one column, and its first column always shows.
    pub fn columns_op(self, page: &str, id: &str, columns: &[usize], op: &str) {
        match op {
            "hide" => self.change_table_view(page, id, |v| {
                for &c in columns.iter().filter(|&&c| c > 0) {
                    if !v.hidden.contains(&c) {
                        v.toggle_hidden(c);
                    }
                }
            }),
            "delete" => {
                let was = self.before_removal(page, id);
                self.edit_columns(page, id, |t| t.remove_columns(&columns));
                self.say_removed(page, id, was);
            }
            _ => {}
        }
        crate::ui::tables::unpick(id);
        self.set_popover(None);
    }

    /// One of the commands of the menu of rows, for all the picked rows:
    /// "duplicate", "delete", "nest" (under the row above), "unnest".
    pub fn rows_op(self, page: &str, id: &str, rows: &[usize], op: &str) {
        let mut rows = rows.to_vec();
        rows.sort_unstable();
        rows.dedup();
        let was = (op == "delete").then(|| self.before_removal(page, id)).flatten();
        self.edit_table(page, id, |t| {
            rows.retain(|&r| r >= 1 && r < t.rows.len());
            match op {
                "delete" => t.remove_rows(&rows),
                // (From the last row up, so that the numbers of the rows still to do stay true.)
                "duplicate" => {
                    for &r in rows.iter().rev() {
                        t.duplicate_row(r);
                    }
                }
                "nest" | "unnest" => {
                    for &r in &rows {
                        t.nest(r, op == "nest");
                    }
                }
                _ => {}
            }
        });
        crate::ui::tables::unpick(id);
        self.set_popover(None);
        self.say_removed(page, id, was);
    }

    // ----- rows as pages -------------------------------------------------------
    //
    // The page of a row says which row it is (`Page::row_of`): the block of
    // the table, a bar, and what the row goes by (`RowInfo::id`), or
    // "template" for the page of the table's template. The table says
    // nothing about the pages: Undo can put an older table back, and the
    // pages of its rows still find their rows.

    /// The pages of the rows of a table, by what each row goes by (those in
    /// the Trash too).
    pub fn row_pages(&self, block: &str) -> HashMap<String, Id> {
        let prefix = format!("{block}/");
        self.pages.peek().values().filter_map(|p| p.row_of.as_deref()?.strip_prefix(&prefix).map(|row| (row.to_string(), p.id.clone()))).collect()
    }

    /// The page of one row (`row` is what the row goes by), if it has one.
    fn page_of_row(&self, block: &str, row: &str) -> Option<Id> {
        page_of(&self.pages.peek(), block, row).map(|(id, _)| id)
    }

    /// The table a page is a row of (or the template of).
    pub fn row_ref(&self, page_id: &str) -> Option<RowRef> {
        let (key, parent) = {
            let pages = self.pages.peek();
            let p = pages.get(page_id)?;
            (p.row_of.clone()?, p.parent_id.clone()?)
        };
        let (block, row) = key.split_once('/')?;
        // (With nothing from the pages of the rows: `RowValues` works that
        // out, as the table does, only when one of them changes.)
        let table = self.table_alone(&parent, block)?;
        let row = match row {
            TEMPLATE => None,
            id => Some(table.row_with_id(id)?),
        };
        Some(RowRef { page: parent, block: block.to_string(), row, table })
    }

    /// The page of a row was given a name: its row takes it.
    pub(super) fn name_row_cell(self, page_id: &str) {
        let Some(RowRef { page, block, row: Some(r), table }) = self.row_ref(page_id) else { return };
        let title = self.pages.peek().get(page_id).map(|p| p.title.clone()).unwrap_or_default();
        if table.cell(r, 0) != title {
            self.set_table_cell(&page, &block, r, 0, &title);
        }
    }

    /// The first cell of a row was typed in (`row` is what the row goes
    /// by): the row's page, if it has one, takes the name.
    pub(super) fn name_row_page(self, block: &str, row: &str, title: &str) {
        if let Some(page_id) = self.page_of_row(block, row) {
            self.title_row_page(&page_id, title);
        }
    }

    /// Give the page of a row the name of its row.
    fn title_row_page(self, page_id: &str, title: &str) {
        if self.pages.peek().get(page_id).is_none_or(|p| p.title == title) {
            return;
        }
        self.rename_page(page_id, title.to_string());
        // A title that is on the screen and not being typed in shows it.
        document::eval(&format!(
            "(function(){{const t=document.getElementById({});if(t&&t!==document.activeElement)t.value={};}})()",
            serde_json::to_string(&format!("title-{page_id}")).unwrap_or_default(),
            serde_json::to_string(title).unwrap_or_default()
        ));
    }

    /// A table changed by more than a letter typed in a cell: the page of
    /// each of its rows takes the name its row has now (see `stale_row_titles`).
    pub(super) fn name_row_pages(self, block: &str, table: &Table) {
        let stale = stale_row_titles(&self.pages.peek(), block, table);
        for (page_id, title) in stale {
            self.title_row_page(&page_id, &title);
        }
    }

    /// The same for every table of a page, after the page was put back
    /// whole (Undo, Redo) or written again (by the AI).
    pub(super) fn name_row_pages_of(self, page: &str) {
        let tables: Vec<(Id, Table)> = self.blocks.peek().get(page).map(|bs| bs.iter().filter(|b| b.kind == BlockKind::Table).map(|b| (b.id.clone(), Table::parse(&b.data))).collect()).unwrap_or_default();
        for (id, table) in tables {
            self.name_row_pages(&id, &table);
        }
    }

    /// The page of a row (of the template with no row), made now if there
    /// is none yet. The page of a new row starts with the lines of the
    /// template's page.
    fn row_page(mut self, page: &str, id: &str, row: Option<usize>) -> Option<Id> {
        let mut table = self.table_at(page, id)?;
        // The first row of a table to be opened: every row gets a name to
        // go by. That is no step for Undo, and an older step would put a
        // table without the names back (the pages would lose their rows):
        // the steps of this page are let go.
        if row.is_some() && (1..table.rows.len()).any(|r| table.row_id(r).is_none()) {
            self.note_in_table(page, id, |t| {
                t.name_rows();
            });
            self.history.write().forget(page);
            table = self.table_at(page, id)?;
        }
        let key = match row {
            Some(r) => format!("{id}/{}", table.row_id(r)?),
            None => format!("{id}/{TEMPLATE}"),
        };
        let have = |key: &str| key.split_once('/').and_then(|(block, row)| page_of(&self.pages.peek(), block, row));
        if let Some((pid, trashed)) = have(&key) {
            // Back from the Trash (its row was taken out, and that undone).
            if trashed {
                self.set_trashed(&pid, false);
            }
            return Some(pid);
        }
        let mut new = Page::new(Some(page.to_string()), 0.0);
        new.space_id = self.pages.peek().get(page).map(|p| p.space_id.clone()).unwrap_or_default();
        new.row_of = Some(key);
        new.title = match row {
            Some(r) => table.title(r).to_string(),
            None => "Template".to_string(),
        };
        // (Never a template's page that is in the Trash.)
        let template = row.and(table.template.as_ref()).and_then(|_| template_page(&self.pages.peek(), id).map(|p| p.id.clone()));
        // With no template, the page of a row of a table with rollup
        // columns starts with a table of what they take from.
        let for_rollups = || {
            let t = row.and_then(|_| table_for_rollups(&table))?;
            let mut b = Block::new(BlockKind::Table, t.plain_text());
            b.data = t.to_data();
            Some(vec![b])
        };
        let lines: Vec<Block> = template
            .and_then(|t| self.blocks.peek().get(&t).cloned())
            .filter(|lines| !lines.is_empty())
            .map(|lines| lines.into_iter().map(|b| Block { id: new_id(), ..b }).collect())
            .or_else(for_rollups)
            .unwrap_or_else(|| vec![Block::text("")]);
        let pid = new.id.clone();
        self.pages.write().insert(pid.clone(), new);
        self.blocks.write().insert(pid.clone(), lines);
        self.save_page(&pid);
        self.save_blocks(&pid);
        Some(pid)
    }

    /// Open a row as a page: at the side, as a full page, or in a new tab.
    pub fn open_row(self, page: &str, id: &str, row: usize, how: Open) {
        let Some(table) = self.table_at(page, id).filter(|t| row >= 1 && row < t.rows.len()) else { return };
        let Some(pid) = self.row_page(page, id, Some(row)) else { return };
        let route = Route::Page(pid);
        match how {
            Open::Auto if table.view.full => self.navigate(route),
            Open::Full => self.navigate(route),
            Open::Tab => self.open_in_new_tab(route),
            Open::Auto | Open::Side => self.open_peek(route),
        }
    }

    /// Show a page at the side of the pane in focus. The pane at the side
    /// is used again for the next page opened this way.
    pub fn open_peek(mut self, route: Route) {
        let mut fresh = None;
        {
            let pages = self.pages.peek();
            let mut guard = self.ui.write();
            let ui = &mut *guard;
            ui.popover = None;
            remember(ui, &pages, &route);
            let tab = ui.tab_mut();
            let peek = tab.peek.clone().filter(|p| tab.pane(p).is_some());
            match (tab.pane_showing(&route), peek) {
                (Some(shown), _) => tab.focus = shown,
                (None, Some(peek)) => {
                    tab.folded.retain(|p| *p != peek);
                    if let Some(pane) = tab.panes.iter_mut().find(|p| p.id == peek) {
                        fresh = Some((peek.clone(), route.place_key()));
                        pane.push(route);
                    }
                    tab.focus = peek;
                }
                (None, None) => {
                    let pane = Pane::new(route);
                    tab.peek = Some(pane.id.clone());
                    tab.add_pane(pane, None, Edge::Right);
                }
            }
        }
        if let Some((pane, place)) = fresh {
            self.place_fresh(&pane, &place);
        }
        self.save_ui();
    }

    /// The page at the side goes to a tab of its own.
    pub fn peek_to_tab(self, pane: &str, page_id: &str) {
        self.close_pane(Some(pane));
        self.open_page(page_id);
    }

    /// Something asked of the page of a row, which is made first if the row
    /// has none: "copy-link", "favorite".
    pub fn row_page_do(self, page: &str, id: &str, row: usize, what: &str) {
        let Some(pid) = self.row_page(page, id, Some(row)) else { return };
        match what {
            "copy-link" => self.copy_to_clipboard(crate::inline::page_url(&pid), "Link copied"),
            "favorite" => self.toggle_favorite(&pid),
            _ => {}
        }
        self.set_popover(None);
    }

    /// A new row for a layout with no cells to type in (a board, a
    /// calendar): it opens at the side, to be named there.
    pub fn new_card(self, page: &str, id: &str, fill: &[(usize, String)]) {
        let today = crate::when::today();
        let mut at = None;
        self.edit_table(page, id, |t| {
            at = t.add_row(t.rows.len());
            if let Some(r) = at {
                t.fill_for_filters(r, today);
                for (c, text) in fill {
                    t.put(r, *c, text, today);
                }
            }
        });
        match at {
            Some(r) => self.open_row(page, id, r, Open::Side),
            None => self.toast("A table holds at most 200 rows"),
        }
    }

    /// The pages of rows that were taken out of a table (`rows`: what each
    /// row went by): a page with nothing written in it goes for good,
    /// another goes to the Trash. It still says which row it was, so that
    /// it is the page of its row again when Undo brings the row back and
    /// the row is opened.
    fn drop_row_pages(self, block: &str, rows: Vec<String>) {
        let pages = self.row_pages(block);
        for pid in rows.iter().filter_map(|row| pages.get(row)) {
            if self.pages.peek().get(pid).is_none_or(|p| p.trashed) {
                continue;
            }
            let written = self.blocks.peek().get(pid).is_some_and(|bs| is_written(bs));
            if written {
                self.set_trashed(pid, true);
            } else {
                self.delete_forever(pid);
            }
        }
    }

    /// A page changed from `before` to `after` (a row or a table deleted,
    /// the page written again, an older version put back, Undo): the pages
    /// of the rows that went with the change go too.
    pub(super) fn drop_lost_row_pages(self, before: &[Block], after: &[Block]) {
        for (block, rows) in lost_rows(before, after) {
            // (A table that was cut here and pasted in another page is the
            // same block there, see `bring_row_pages`. Undo and an older
            // version give a table that comes back here a new id then
            // (`tables_put_back`); should the one id be in two pages all
            // the same, the pages of its rows stay with the table that is
            // still there.)
            let elsewhere = !after.iter().any(|b| b.id == block) && self.blocks.peek().values().any(|bs| bs.iter().any(|b| b.id == block && b.kind == BlockKind::Table));
            if !elsewhere {
                self.drop_row_pages(&block, rows);
            }
        }
    }

    /// A table that was cut came to `page` by a paste, as the block it was:
    /// the pages of its rows come with it. The cut put those with something
    /// written in them in the Trash, in the page the table was cut from:
    /// they come into this page, and those of `restore` (the ones the cut
    /// put in the Trash) out of the Trash. (The page of a row that is not
    /// in the table any more stays where it is.)
    pub(super) fn bring_row_pages(mut self, page: &str, block: &str, restore: &[Id]) {
        let Some(table) = self.table_at(page, block) else { return };
        let space = self.pages.peek().get(page).map(|p| p.space_id.clone()).unwrap_or_default();
        for (row, pid) in self.row_pages(block) {
            let there = if row == TEMPLATE { table.template.is_some() } else { table.row_with_id(&row).is_some() };
            let inside = descendants(&self.pages.peek(), &pid);
            // (Never into a page that is inside the row's own page.)
            if !there || inside.iter().any(|p| p == page) {
                continue;
            }
            let trashed = {
                let mut pages = self.pages.write();
                for p in &inside {
                    if let Some(p) = pages.get_mut(p) {
                        p.space_id = space.clone();
                    }
                }
                let Some(p) = pages.get_mut(&pid) else { continue };
                p.parent_id = Some(page.to_string());
                p.trashed
            };
            if trashed && restore.contains(&pid) {
                self.set_trashed(&pid, false);
            } else {
                for p in &inside {
                    self.save_page(p);
                }
            }
        }
    }

    /// A page came back from the Trash by hand. If it was the page of a row
    /// that is not there any more, it is an ordinary page now (inside the
    /// page of the table), so that it is in the sidebar.
    pub(super) fn unrow_if_lost(mut self, page_id: &str) {
        // (The table of a locked page that is not open cannot be looked at:
        // the page stays the page of its row.)
        let parent_shut = {
            let parent = self.pages.peek().get(page_id).and_then(|p| p.parent_id.clone());
            parent.is_some_and(|parent| self.blocks.peek().get(&parent).is_some_and(|bs| crate::lock::sealed(bs).is_some()))
        };
        let lost = !parent_shut && self.pages.peek().get(page_id).is_some_and(|p| p.row_of.is_some()) && self.row_ref(page_id).is_none();
        if lost {
            if let Some(p) = self.pages.write().get_mut(page_id) {
                p.row_of = None;
            }
            self.save_page(page_id);
        }
    }

    // ----- the template of new rows --------------------------------------------

    /// Open the template of a table at the side: what a new row starts with.
    pub fn edit_row_template(self, page: &str, id: &str) {
        self.edit_table(page, id, |t| {
            t.template.get_or_insert_with(Default::default);
        });
        if let Some(pid) = self.row_page(page, id, None) {
            self.open_peek(Route::Page(pid));
        }
    }

    /// What new rows start with in one column.
    pub fn set_template_cell(self, page: &str, id: &str, column: usize, text: &str) {
        self.edit_table(page, id, |t| {
            let width = t.columns();
            let template = t.template.get_or_insert_with(Default::default);
            template.cells.resize(width, String::new());
            if let Some(cell) = template.cells.get_mut(column) {
                *cell = text.replace(['\n', '\r'], " ");
            }
        });
    }

    /// New rows start empty again. The template's page goes with it.
    pub fn remove_row_template(self, page: &str, id: &str) {
        self.edit_table(page, id, |t| t.template = None);
        self.drop_row_pages(id, vec![TEMPLATE.to_string()]);
        self.set_popover(None);
    }

    // ----- rows of another table, and pictures, in cells -----------------------

    /// Every table of the space of `page`, with a name to choose it by, for
    /// a relation column: its own name, or the title of its page.
    pub fn tables_to_link(&self, page: &str, but: &str) -> Vec<(Id, String)> {
        let blocks = self.blocks.peek();
        let pages = self.pages.peek();
        let space = pages.get(page).map(|p| p.space_id.clone());
        let mut out: Vec<(Id, String)> = pages
            .values()
            .filter(|p| !p.trashed && p.row_of.is_none() && Some(&p.space_id) == space.as_ref())
            .flat_map(|p| {
                let tables: Vec<&Block> = blocks.get(&p.id).map(|bs| bs.iter().filter(|b| b.kind == BlockKind::Table && b.id != but).collect()).unwrap_or_default();
                let many = tables.len() > 1;
                tables.into_iter().enumerate().map(move |(n, b)| {
                    let name = Table::parse(&b.data).name;
                    let name = match (name.is_empty(), many) {
                        (false, _) => name,
                        (true, false) => p.display_title().to_string(),
                        (true, true) => format!("{}, table {}", p.display_title(), n + 1),
                    };
                    (b.id.clone(), name)
                })
            })
            .collect();
        out.sort_by_key(|(_, name)| name.to_lowercase());
        out
    }

    /// The page a table is on, and the table.
    pub fn find_table(&self, block: &str) -> Option<(Id, Table)> {
        let blocks = self.blocks.peek();
        blocks.iter().find_map(|(page, bs)| bs.iter().find(|b| b.id == block && b.kind == BlockKind::Table).map(|b| (page.clone(), Table::parse(&b.data))))
    }

    /// Open the row of another table that a relation cell names.
    pub fn open_related(self, block: &str, name: &str) {
        let Some((page, table)) = self.find_table(block) else { return self.toast("That table is gone") };
        let want = name.trim().to_lowercase();
        let row = table.body().find(|&r| table.title(r).to_lowercase() == want);
        match row {
            Some(r) => self.open_row(&page, block, r, Open::Side),
            None => self.toast("That row is gone"),
        }
    }

    /// Choose a picture for a cell of a picture column.
    pub fn pick_cell_picture(self, page: &str, id: &str, row: usize, column: usize) {
        let (page, id) = (page.to_string(), id.to_string());
        spawn_forever(async move {
            let Some(file) = rfd::AsyncFileDialog::new().set_title("Choose a Picture").add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tiff", "bmp"]).pick_file().await else {
                return;
            };
            match crate::files::import(file.path()) {
                Ok(name) => self.toggle_cell_part(&page, &id, row, column, &name),
                Err(text) => self.notice(text),
            }
        });
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
        let mut source = Table::parse(&table.data);
        if !source.rollups().is_empty() {
            roll_up(&pages, &blocks, &table.id, &mut source);
        }
        Some(ChartSource {
            title: pages.get(&target).map(|p| p.display_title().to_string()).unwrap_or_default(),
            elsewhere: target != page,
            page: target,
            table: source,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_of_rows_that_lost_their_row_are_found_when_the_app_starts() {
        let mut table = Table::of(vec![vec!["Item".into()], vec!["Rent".into()], vec!["Food".into()]]);
        table.name_rows();
        let rent = table.row_id(1).unwrap().to_string();
        let mut block = Block::new(BlockKind::Table, "");
        block.data = table.to_data();
        let home = Page::new(None, 1.0);
        let row_page = |key: String, written: bool, blocks: &mut HashMap<Id, Vec<Block>>| {
            let mut p = Page::new(Some(home.id.clone()), 0.0);
            p.row_of = Some(key);
            blocks.insert(p.id.clone(), vec![Block::text(if written { "Paid on the 1st" } else { "" })]);
            p
        };
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        blocks.insert(home.id.clone(), vec![block.clone()]);
        let of_rent = row_page(format!("{}/{rent}", block.id), true, &mut blocks);
        let template = row_page(format!("{}/template", block.id), false, &mut blocks);
        let gone_row = row_page(format!("{}/no-such-row", block.id), true, &mut blocks);
        let gone_empty = row_page(format!("{}/no-such-row-2", block.id), false, &mut blocks);
        let gone_table = row_page("no-such-table/row".to_string(), true, &mut blocks);
        let mut in_trash = row_page(format!("{}/no-such-row-3", block.id), true, &mut blocks);
        in_trash.trashed = true;
        // A page whose table cannot be looked at (the lines of its page are not here) is left alone.
        let mut elsewhere = Page::new(Some("a page that is not loaded".into()), 0.0);
        elsewhere.row_of = Some("x/y".into());
        let pages: HashMap<Id, Page> = [&home, &of_rent, &template, &gone_row, &gone_empty, &gone_table, &in_trash, &elsewhere].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        let (mut keep, gone) = lost_row_pages(&pages, &blocks);
        keep.sort();
        let mut want = vec![gone_row.id.clone(), gone_table.id.clone()];
        want.sort();
        assert_eq!((keep, gone), (want, vec![gone_empty.id.clone()]));
        // An export takes the pages of rows that have something written in
        // them (not the template's page, not one in the Trash).
        let mut pages = pages;
        blocks.insert(template.id.clone(), vec![Block::text("Starts each row")]);
        pages.get_mut(&in_trash.id).unwrap().row_of = Some(format!("{}/{rent}", block.id));
        let out: Vec<Id> = written_row_pages(&pages, &blocks, &home.id).into_iter().map(|p| p.id).collect();
        let mut want = vec![of_rent.id.clone(), gone_row.id.clone(), gone_table.id.clone()];
        want.sort();
        let mut got = out.clone();
        got.sort();
        assert_eq!(got, want);
        assert!(written_row_pages(&pages, &blocks, "another page").is_empty());
    }

    /// At a start the lines of a locked page are one sealed block: the
    /// pages of the rows of its tables are not lost, written or empty.
    #[test]
    fn the_row_pages_of_a_locked_page_are_left_alone() {
        let parent = Page::new(None, 0.0);
        let mut written = Page::new(Some(parent.id.clone()), 0.0);
        written.row_of = Some("t1/r1".into());
        let mut empty = Page::new(Some(parent.id.clone()), 1.0);
        empty.row_of = Some("t1/r2".into());
        let mut template = Page::new(Some(parent.id.clone()), 2.0);
        template.row_of = Some(format!("t1/{TEMPLATE}"));
        let sealed = crate::lock::seal_with(&crate::lock::test_key("pw-pw-pw"), &parent.id, &[Block::text("a table is in here")]);
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        blocks.insert(parent.id.clone(), vec![sealed.clone()]);
        blocks.insert(written.id.clone(), vec![Block::text("What I wrote about this row")]);
        blocks.insert(empty.id.clone(), vec![Block::text("")]);
        blocks.insert(template.id.clone(), vec![Block::text("")]);
        let pages: HashMap<Id, Page> = [&parent, &written, &empty, &template].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        assert_eq!(lost_row_pages(&pages, &blocks), (vec![], vec![]));
        // The page is locked while its table is on it (the lines turn into
        // the sealed block), or a sealed version is put back: no row went.
        let mut table = Table::of(vec![vec!["Name".into()], vec!["Rent".into()]]);
        table.name_rows();
        let mut block = Block::new(BlockKind::Table, "");
        block.data = table.to_data();
        assert!(!lost_rows(std::slice::from_ref(&block), &[]).is_empty(), "a table that went takes its rows");
        assert!(lost_rows(&[block], &[sealed]).is_empty());
    }

    #[test]
    fn the_page_of_a_row_goes_by_the_first_cell_of_its_row() {
        let mut table = Table::of(vec![vec!["Day".into(), "km".into()], vec!["Thu 12 Nov".into(), "3".into()], vec!["Run".into(), "5".into()]]);
        let home = Page::new(None, 1.0);
        let page_of = |key: String, title: &str| {
            let mut p = Page::new(Some(home.id.clone()), 0.0);
            p.row_of = Some(key);
            p.title = title.to_string();
            p
        };
        // A table whose rows go by no names has no pages to name.
        let none: HashMap<Id, Page> = [page_of("b/x".into(), "x")].into_iter().map(|p| (p.id.clone(), p)).collect();
        assert!(stale_row_titles(&none, "b", &table).is_empty());
        table.name_rows();
        let (first, second) = (table.row_id(1).unwrap().to_string(), table.row_id(2).unwrap().to_string());
        let stale = page_of(format!("b/{first}"), "12 nov");
        let same = page_of(format!("b/{second}"), "Run");
        let template = page_of(format!("b/{TEMPLATE}"), "Template");
        let other_table = page_of(format!("c/{first}"), "12 nov");
        let gone = page_of("b/no-such-row".into(), "Old");
        let pages: HashMap<Id, Page> = [&home, &stale, &same, &template, &other_table, &gone].into_iter().map(|p| (p.id.clone(), p.clone())).collect();
        assert_eq!(stale_row_titles(&pages, "b", &table), vec![(stale.id.clone(), "Thu 12 Nov".to_string())]);
    }

    #[test]
    fn the_rows_that_went_with_a_change_are_found() {
        let block_of = |t: &Table| {
            let mut b = Block::new(BlockKind::Table, "");
            b.data = t.to_data();
            b
        };
        let mut named = Table::of(vec![vec!["Item".into()], vec!["Rent".into()], vec!["Food".into()], vec!["Fun".into()]]);
        named.name_rows();
        let ids: Vec<String> = (1..4).map(|r| named.row_id(r).unwrap().to_string()).collect();
        let plain = Table::of(vec![vec!["Name".into()], vec!["a".into()]]);
        let (a, b) = (block_of(&named), block_of(&plain));
        let before = vec![Block::text("Intro"), a.clone(), b.clone()];
        // Nothing went: the same page, a cell typed in, a row moved, a row added.
        assert!(lost_rows(&before, &before).is_empty());
        let mut changed = named.clone();
        changed.set(2, 0, "Groceries");
        changed.move_row(3, 1);
        changed.add_row(1);
        let with = |t: &Table| vec![Block::text("Intro"), Block { data: t.to_data(), ..a.clone() }, b.clone()];
        assert!(lost_rows(&before, &with(&changed)).is_empty());
        // A row taken out; a table with no named rows taken out or changed asks for nothing but its template.
        changed.remove_row(changed.row_with_id(&ids[1]).unwrap());
        assert_eq!(lost_rows(&before, &with(&changed)), vec![(a.id.clone(), vec![ids[1].clone()])]);
        assert_eq!(lost_rows(&before, &before[..2]), vec![(b.id.clone(), vec![TEMPLATE.to_string()])]);
        // The table taken out: all its rows, and its template.
        let mut all = ids.clone();
        all.push(TEMPLATE.to_string());
        assert_eq!(lost_rows(&before, &[before[0].clone(), b.clone()]), vec![(a.id.clone(), all)]);
        // An older form of the table (no names yet) put back takes the names its rows go by now.
        let mut older = Table::of(vec![vec!["Item".into()], vec!["Food".into()], vec!["Rent".into()], vec!["Cinema".into()]]);
        assert!(older.keep_row_names(&named));
        assert_eq!((older.row_id(1), older.row_id(2)), (named.row_id(2), named.row_id(1)));
        assert!(older.row_id(3).is_some_and(|id| !ids.iter().any(|i| i == id)));
        assert_eq!(lost_rows(&before, &with(&older)), vec![(a.id.clone(), vec![ids[2].clone()])]);
        assert!(!Table::of(vec![vec!["Item".into()], vec!["Rent".into()]]).keep_row_names(&plain));
    }

    /// A month's budget on a page, and the pages of two of its days.
    fn budget_with_days() -> (HashMap<Id, Page>, HashMap<Id, Vec<Block>>, Page, Id) {
        let md = "| Date | Budget | Spent |\n| --- | --- | --- |\n| 2026-11-01 | $60.00 |  |\n| 2026-11-02 | $60.00 |  |\n| 2026-11-03 | $60.00 |  |\n<!-- columns: date | number USD sum | rollup Amount sum sum -->";
        let lines: Vec<&str> = md.lines().collect();
        let (mut table, _) = Table::from_markdown(&lines).unwrap();
        table.name_rows();
        let mut block = Block::new(BlockKind::Table, "");
        block.data = table.to_data();
        let home = Page::new(None, 1.0);
        let mut pages: HashMap<Id, Page> = HashMap::new();
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        blocks.insert(home.id.clone(), vec![Block::text("November"), block.clone()]);
        pages.insert(home.id.clone(), home.clone());
        for (r, amounts) in [(1, vec!["$12.50", "$8.00"]), (2, vec![])] {
            let mut day = Page::new(Some(home.id.clone()), 0.0);
            day.row_of = Some(format!("{}/{}", block.id, table.row_id(r).unwrap()));
            let mut rows = vec![vec!["Item".to_string(), "Amount".to_string()]];
            rows.extend(amounts.iter().map(|a| vec!["Lunch".to_string(), a.to_string()]));
            let mut spends = Table::of(rows);
            spends.set_kind(1, Kind::Number);
            spends.set_format(1, "EUR");
            let mut b = Block::new(BlockKind::Table, "");
            b.data = spends.to_data();
            blocks.insert(day.id.clone(), vec![Block::text("What I bought"), b]);
            pages.insert(day.id.clone(), day);
        }
        (pages, blocks, home, block.id)
    }

    #[test]
    fn a_rollup_takes_its_amounts_from_the_pages_of_the_rows() {
        let (mut pages, blocks, home, block) = budget_with_days();
        let mut table = Table::parse(&blocks[&home.id][1].data);
        roll_up(&pages, &blocks, &block, &mut table);
        // Day 1 spent €20.50, day 2 has a page and nothing spent, day 3 has no page.
        assert_eq!((table.shown(1, 2), table.shown(2, 2), table.shown(3, 2)), ("€20.50".to_string(), "€0.00".to_string(), String::new()));
        assert_eq!(table.total(2), Some(("Sum", "€20.50".to_string())));
        // As Markdown (for the AI), the worked-out amounts are written.
        assert!(table.to_markdown("").contains("| $60.00 | €20.50 |"), "{}", table.to_markdown(""));
        // A page in the Trash counts for nothing.
        for p in pages.values_mut().filter(|p| p.row_of.is_some()) {
            p.trashed = true;
        }
        let mut table = Table::parse(&blocks[&home.id][1].data);
        roll_up(&pages, &blocks, &block, &mut table);
        assert_eq!(table.shown(1, 2), "");
        // A table with no rollup and no template is left alone.
        let mut plain = Table::of(vec![vec!["A".into()], vec!["x".into()]]);
        roll_up(&pages, &blocks, "b", &mut plain);
        assert_eq!(plain, Table::of(vec![vec!["A".into()], vec!["x".into()]]));
    }

    /// The Markdown of the budget with `page:` on its table line, as the AI writes it.
    fn said_budget(blocks: &HashMap<Id, Vec<Block>>, home: &Page, words: &str) -> String {
        format!("{}\n<!-- table: November | page: {words} -->", Table::parse(&blocks[&home.id][1].data).to_markdown(""))
    }

    #[test]
    fn page_makes_the_template_of_a_table_that_has_none_and_nothing_more() {
        let (mut pages, blocks, home, block) = budget_with_days();
        // With no template, a row's page starts with a table of what the rollups take from.
        let mut kept = Table::parse(&blocks[&home.id][1].data);
        kept.set_format(2, "USD");
        let made = table_for_rollups(&kept).unwrap();
        assert_eq!((made.heading(0), made.heading(1), made.kind(1), made.column(1).format.as_str(), made.column(1).total), ("Item".to_string(), "Amount".to_string(), Kind::Number, "USD", Some(Total::Sum)));
        assert!(table_for_rollups(&Table::of(vec![vec!["A".into()]])).is_none());
        // `page:` in the AI's Markdown: read from the words, never kept in the table's data.
        let md = said_budget(&blocks, &home, "Item | Amount USD sum | Category choice {Food; Fun}");
        let said = crate::markdown::said_pages(&md);
        assert_eq!(said.len(), 1, "{md}");
        let stored = crate::markdown::to_blocks(&md);
        assert!(!stored[0].data.contains("\"page\""), "{}", stored[0].data);
        let pairs = said_for_blocks(&blocks[&home.id], said);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, block);
        let (table, holder, lines) = row_template(&pages, &blocks, &home.id, &block, &pairs[0].1).unwrap();
        assert!(table.template.is_some() && table.page.is_none());
        assert_eq!((holder.row_of.clone(), holder.parent_id.clone(), holder.title.as_str()), (Some(format!("{block}/{TEMPLATE}")), Some(home.id.clone()), "Template"));
        let start = Table::parse(&lines[0].data);
        assert_eq!((0..start.columns()).map(|c| start.heading(c)).collect::<Vec<_>>(), vec!["Item", "Amount", "Category"]);
        assert_eq!((start.column(1).format.as_str(), start.kind(2)), ("USD", Kind::Choice));
        // A table that has one: `page:` changes nothing, whatever it says.
        pages.insert(holder.id.clone(), holder.clone());
        for words in ["Item | Amount USD sum | Category choice {Food; Fun}", "Other | Columns", "Item"] {
            let said = crate::markdown::said_pages(&said_budget(&blocks, &home, words));
            assert!(row_template(&pages, &blocks, &home.id, &block, &said[0]).is_none(), "{words}");
        }
        // The AI reads what it is, as `page:`, and the page that holds it is named by read_page.
        let mut blocks = blocks;
        blocks.insert(holder.id.clone(), lines);
        let mut read = Table::parse(&blocks[&home.id][1].data);
        read.template = Some(Default::default());
        roll_up(&pages, &blocks, &block, &mut read);
        assert!(read.to_markdown("").contains("page: Item text | Amount number USD sum | Category choice {Food:"), "{}", read.to_markdown(""));
        assert_eq!(template_page_id(&pages, &block), Some(holder.id.clone()));
        // A table with no rollup is not given `page:`.
        let mut plain = Table::of(vec![vec!["A".into()], vec!["x".into()]]);
        plain.template = Some(Default::default());
        roll_up(&pages, &blocks, &block, &mut plain);
        assert!(plain.page.is_none());
    }

    #[test]
    fn a_rollup_inside_the_template_page_says_keeps_what_it_takes_from() {
        // A year: a row for each month; each month's page starts with a table
        // of its days, whose Spent takes the sum of each day's Amount.
        let md = "| Month | Spent |\n| --- | --- |\n| November |  |\n<!-- columns: text | rollup Spent sum USD sum -->\n\
                  <!-- table: 2026 | page: Day date | Budget number USD sum | Spent rollup Amount sum USD sum | Left formula USD sum = Budget - Spent -->";
        let said = crate::markdown::said_pages(md);
        let starts = said[0].page.as_deref().unwrap();
        assert_eq!((starts.kind(2), starts.column(2).of.as_str(), starts.column(3).formula.as_str()), (Kind::Rollup, "Amount", "Budget - Spent"));
        let mut year = crate::markdown::to_blocks(md).remove(0);
        let mut t = Table::parse(&year.data);
        t.name_rows();
        year.data = t.to_data();
        let home = Page::new(None, 0.0);
        let pages: HashMap<Id, Page> = [(home.id.clone(), home.clone())].into_iter().collect();
        let blocks: HashMap<Id, Vec<Block>> = [(home.id.clone(), vec![year.clone()])].into_iter().collect();
        let (_, _, lines) = row_template(&pages, &blocks, &home.id, &year.id, &said[0]).unwrap();
        // Kept and read back: the month's table still takes the sum of Amount.
        let month = Table::parse(&lines[0].data);
        assert_eq!((month.kind(2), month.column(2).of.as_str(), month.column(2).way, month.column(2).format.as_str()), (Kind::Rollup, "Amount", Total::Sum, "USD"));
        assert_eq!(month.worked_out_from(2), "Sum of Amount in the table on the row's page.");
    }

    /// A page with a table, and a template page for it made by hand: a
    /// column with no heading, a choice column whose options came from
    /// typing (one with a colon), a Price with four places, rows, a name, a
    /// sort, and words under the table.
    fn hand_made_template(rollup: bool) -> (HashMap<Id, Page>, HashMap<Id, Vec<Block>>, Page, Id, Id) {
        let md = if rollup {
            "| Trade | Cost |\n| --- | --- |\n| AAPL |  |\n<!-- columns: text | rollup Price sum USD sum -->"
        } else {
            "| Trade | Cost |\n| --- | --- |\n| AAPL |  |\n<!-- columns: text | number USD sum -->"
        };
        let (mut table, _) = Table::from_markdown(&md.lines().collect::<Vec<_>>()).unwrap();
        table.name_rows();
        table.template = Some(Default::default());
        let mut block = Block::new(BlockKind::Table, "");
        block.data = Table::parse(&table.to_data()).to_data();
        let home = Page::new(None, 1.0);
        let mut template = Page::new(Some(home.id.clone()), 0.0);
        template.row_of = Some(format!("{}/{TEMPLATE}", block.id));
        let mut own = Table::of(vec![vec!["Step".into(), "Price".into(), "When".into(), String::new()], vec!["Entry".into(), "185.25".into(), String::new(), "a".into()], vec!["Stop".into(), "180".into(), String::new(), String::new()]]);
        own.set_kind(1, Kind::Number);
        own.set_format(1, "USD");
        own.set_places(1, Some(4));
        own.set_kind(2, Kind::Choice);
        // (Options made by typing in the cells, as a person does.)
        for (r, name) in [(1, "9:30"), (2, "Close; late")] {
            let made = own.add_option(2, name).unwrap();
            own.toggle_name(r, 2, &made);
        }
        own.name = "Plan".into();
        own.change_view(|v| v.sort_by(0, true));
        let mut tb = Block::new(BlockKind::Table, own.plain_text());
        tb.data = own.to_data();
        let mut pages: HashMap<Id, Page> = HashMap::new();
        let mut blocks: HashMap<Id, Vec<Block>> = HashMap::new();
        blocks.insert(home.id.clone(), vec![Block::text("Intro with a tpyo"), block.clone()]);
        blocks.insert(template.id.clone(), vec![tb, Block::text("Check the news first")]);
        pages.insert(home.id.clone(), home.clone());
        pages.insert(template.id.clone(), template.clone());
        (pages, blocks, home, block.id, template.id)
    }

    #[test]
    fn a_template_made_by_hand_stays_through_any_ai_rewrite() {
        for rollup in [false, true] {
            let (pages, blocks, home, block, template) = hand_made_template(rollup);
            let was = blocks[&template].clone();
            assert_eq!(Table::parse(&was[0].data).column(2).options.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), vec!["9:30", "Close; late"]);
            let mut lines = blocks[&home.id].clone();
            for round in 0..10 {
                // As the AI is given the page, written again with the typo put right…
                let mut t = Table::parse(&lines[1].data);
                roll_up(&pages, &blocks, &block, &mut t);
                let md = format!("Intro with a typo {round}\n\n{}", t.to_markdown(""));
                assert_eq!(md.contains("page:"), rollup, "{md}");
                // …and once with words of its own after `page:`.
                let md = match md.find("<!-- table:") {
                    _ if round % 2 == 0 => md,
                    Some(at) => {
                        let end = at + md[at..].find("-->").unwrap();
                        format!("{} | page: Item | Other text -->{}", md[..end].split(" | page:").next().unwrap().trim_end(), &md[end + 3..])
                    }
                    None => format!("{md}\n<!-- table: | page: Item | Other text -->"),
                };
                lines = crate::model::keep_unchanged(&lines, crate::markdown::to_blocks(&md));
                assert!(lines.iter().all(|b| !b.data.contains("\"page\"")));
                let mut now = blocks.clone();
                now.insert(home.id.clone(), lines.clone());
                for (b, said) in said_for_blocks(&lines, crate::markdown::said_pages(&md)) {
                    assert!(row_template(&pages, &now, &home.id, &b, &said).is_none(), "round {round}: {md}");
                }
            }
            assert_eq!(lines[1].id, blocks[&home.id][1].id);
            assert_eq!(Table::parse(&lines[1].data), Table::parse(&blocks[&home.id][1].data));
            assert_eq!(blocks[&template], was);
        }
    }

    #[test]
    fn a_locked_template_page_is_never_given_to_the_ai() {
        let (pages, mut blocks, home, block, template) = hand_made_template(true);
        let sealed = crate::lock::seal_with(&crate::lock::test_key("pw-pw-pw"), &template, &blocks[&template]);
        blocks.insert(template.clone(), vec![sealed]);
        let mut t = Table::parse(&blocks[&home.id][1].data);
        roll_up(&pages, &blocks, &block, &mut t);
        assert!(t.page.is_none());
        let said = crate::markdown::said_pages(&format!("{}\n<!-- table: | page: Item | Amount USD -->", t.to_markdown("")));
        assert!(row_template(&pages, &blocks, &home.id, &block, &said[0]).is_none());
    }

    #[test]
    fn a_locked_page_of_a_row_gives_nothing_to_its_rollup() {
        let (pages, blocks, home, block) = budget_with_days();
        let day = pages.values().find(|p| p.row_of.as_deref().is_some_and(|k| k.starts_with(&block)) && blocks[&p.id].len() == 2 && Table::parse(&blocks[&p.id][1].data).rows.len() == 3).unwrap().id.clone();
        let mut table = Table::parse(&blocks[&home.id][1].data);
        roll_up(&pages, &blocks, &block, &mut table);
        assert_eq!(table.shown(1, 2), "€20.50");
        // Open (its lines in plain in memory, its key held): nothing.
        crate::lock::hold(&day, crate::lock::test_key("pw-pw-pw"), 0);
        let mut table = Table::parse(&blocks[&home.id][1].data);
        roll_up(&pages, &blocks, &block, &mut table);
        crate::lock::forget(&day);
        assert_eq!(table.shown(1, 2), "");
        assert!(!table.to_markdown("").contains("20.50"));
        // Shut: nothing either.
        let mut shut = blocks.clone();
        shut.insert(day.clone(), vec![crate::lock::seal_with(&crate::lock::test_key("pw-pw-pw"), &day, &blocks[&day])]);
        let mut table = Table::parse(&blocks[&home.id][1].data);
        roll_up(&pages, &shut, &block, &mut table);
        assert_eq!(table.shown(1, 2), "");
    }

    /// A table on `parent` with a row for each of `rows` (its name and the
    /// lines of its page), and the column `rollup` taking from those pages.
    fn table_of_pages(pages: &mut HashMap<Id, Page>, blocks: &mut HashMap<Id, Vec<Block>>, parent: &Page, rollup: &str, rows: Vec<(&str, Vec<Block>)>) -> Block {
        let mut lines = vec![format!("| Name | Spent |"), "| --- | --- |".to_string()];
        lines.extend(rows.iter().map(|(name, _)| format!("| {name} |  |")));
        lines.push(format!("<!-- columns: text | {rollup} -->"));
        let (mut table, _) = Table::from_markdown(&lines.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
        table.name_rows();
        let mut b = Block::new(BlockKind::Table, "");
        b.data = table.to_data();
        for (r, (name, page_lines)) in rows.into_iter().enumerate() {
            let mut p = Page::new(Some(parent.id.clone()), 0.0);
            p.title = name.to_string();
            p.created_at += r as i64;
            p.row_of = Some(format!("{}/{}", b.id, table.row_id(r + 1).unwrap()));
            blocks.insert(p.id.clone(), page_lines);
            pages.insert(p.id.clone(), p);
        }
        b
    }

    /// The table block of a page of spending in `format`.
    fn spends_block(format: &str, amounts: &[&str]) -> Block {
        let mut rows = vec![vec!["Item".to_string(), "Spent".to_string()]];
        rows.extend(amounts.iter().map(|a| vec!["Thing".to_string(), a.to_string()]));
        let mut t = Table::of(rows);
        t.set_kind(1, Kind::Number);
        t.set_format(1, format);
        let mut b = Block::new(BlockKind::Table, "");
        b.data = t.to_data();
        b
    }

    #[test]
    fn a_rollup_of_a_rollup_is_worked_out_two_levels_deep_and_no_further() {
        let (mut pages, mut blocks) = (HashMap::new(), HashMap::new());
        let year = Page::new(None, 0.0);
        let november = Page::new(Some(year.id.clone()), 0.0);
        let days = table_of_pages(&mut pages, &mut blocks, &november, "rollup Spent sum", vec![("Day 1", vec![spends_block("USD", &["$12.50", "$8.00"])]), ("Day 2", vec![spends_block("USD", &["$5.00"])])]);
        let months = table_of_pages(&mut pages, &mut blocks, &year, "rollup Spent sum", vec![("November", vec![days])]);
        // (The month's page is the page of the year's first row.)
        let month_page = pages.values().find(|p| p.title == "November").unwrap().id.clone();
        let mut year_table = Table::parse(&months.data);
        roll_up(&pages, &blocks, &months.id, &mut year_table);
        assert_eq!(year_table.shown(1, 1), "$25.50", "the year takes the month's sum of its days");
        // A level more (a decade of years of months of days): nothing, not $0.
        let decade = Page::new(None, 0.0);
        let mut year_lines = vec![months.clone()];
        year_lines.truncate(1);
        let years = table_of_pages(&mut pages, &mut blocks, &decade, "rollup Spent sum", vec![("2026", year_lines)]);
        let mut decade_table = Table::parse(&years.data);
        roll_up(&pages, &blocks, &years.id, &mut decade_table);
        assert_eq!((decade_table.shown(1, 1), decade_table.value(1, 1)), (String::new(), None));
        let _ = month_page;
    }

    #[test]
    fn money_of_another_kind_is_not_added_and_the_format_does_not_change_from_one_start_to_the_next() {
        let (mut pages, mut blocks) = (HashMap::new(), HashMap::new());
        let trip = Page::new(None, 0.0);
        // The first page made is in dollars; most are in euros.
        let b = table_of_pages(&mut pages, &mut blocks, &trip, "rollup Spent sum sum", vec![
            ("Boston", vec![spends_block("USD", &["$100.00"])]),
            ("Lisbon", vec![spends_block("EUR", &["€20.00"])]),
            ("Porto", vec![spends_block("EUR", &["€30.00"])]),
        ]);
        for _ in 0..20 {
            // (A map is walked in another order at each start of the app.)
            let pages: HashMap<Id, Page> = pages.clone().into_iter().collect();
            let mut t = Table::parse(&b.data);
            roll_up(&pages, &blocks, &b.id, &mut t);
            assert_eq!((t.shown(1, 1), t.shown(2, 1), t.shown(3, 1)), (String::new(), "€20.00".to_string(), "€30.00".to_string()));
            assert_eq!(t.total(1), Some(("Sum", "€50.00".to_string())));
            assert!(t.worked_out_from(1).contains("One row shows nothing: its page has its amounts in another kind of money."), "{}", t.worked_out_from(1));
            // The format it shows is not written in its Markdown: a rewrite sees the same table.
            assert!(t.to_markdown("").contains("rollup Spent sum sum -->"), "{}", t.to_markdown(""));
        }
        // As many of each: the page made first decides.
        let (mut pages, mut blocks) = (HashMap::new(), HashMap::new());
        let b = table_of_pages(&mut pages, &mut blocks, &trip, "rollup Spent sum", vec![("Boston", vec![spends_block("USD", &["$1.00"])]), ("Lisbon", vec![spends_block("EUR", &["€2.00"])])]);
        let mut t = Table::parse(&b.data);
        roll_up(&pages, &blocks, &b.id, &mut t);
        assert_eq!((t.shown(1, 1), t.shown(2, 1)), ("$1.00".to_string(), String::new()));
    }

    #[test]
    fn a_table_on_the_screen_looks_again_only_when_a_page_it_takes_from_changes() {
        let (mut pages, mut blocks, home, block) = budget_with_days();
        let before = rollup_sources(&pages, &blocks, &block);
        assert_eq!(before.len(), 2);
        // A line typed on another page: the same.
        let other = Page::new(None, 0.0);
        blocks.insert(other.id.clone(), vec![Block::text("Something else")]);
        pages.insert(other.id.clone(), other);
        blocks.get_mut(&home.id).unwrap()[0].text = "November, again".into();
        assert_eq!(rollup_sources(&pages, &blocks, &block), before);
        // An amount on the page of a day: another.
        let day = pages.values().find(|p| p.row_of.as_deref().is_some_and(|k| k.starts_with(&block))).unwrap().id.clone();
        let mut t = Table::parse(&blocks[&day][1].data);
        let r = t.add_row(t.rows.len()).unwrap();
        t.set(r, 1, "€1.00");
        blocks.get_mut(&day).unwrap()[1].data = t.to_data();
        assert_ne!(rollup_sources(&pages, &blocks, &block), before);
    }

    #[test]
    fn a_template_page_in_the_trash_is_never_used() {
        let (mut pages, blocks, home, block, template) = hand_made_template(true);
        pages.get_mut(&template).unwrap().trashed = true;
        let md = format!("{}\n<!-- table: | page: Item | Amount USD -->", Table::parse(&blocks[&home.id][1].data).to_markdown(""));
        let said = crate::markdown::said_pages(&md);
        let (_, fresh, lines) = row_template(&pages, &blocks, &home.id, &block, &said[0]).unwrap();
        assert_ne!(fresh.id, template, "a new one, not the one in the Trash");
        // From then on the live one is the template's page, whatever the order of the map.
        pages.insert(fresh.id.clone(), fresh.clone());
        let mut blocks = blocks;
        blocks.insert(fresh.id.clone(), lines);
        for _ in 0..20 {
            let pages: HashMap<Id, Page> = pages.clone().into_iter().collect();
            assert_eq!(template_page(&pages, &block).map(|p| p.id.clone()), Some(fresh.id.clone()));
            assert_eq!(page_of(&pages, &block, TEMPLATE), Some((fresh.id.clone(), false)));
            assert!(row_template(&pages, &blocks, &home.id, &block, &said[0]).is_none(), "one is there: nothing is made again");
        }
    }
}
