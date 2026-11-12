//! A table in a page: its name, its cells by the kind of their column, the
//! totals at its foot, what shows of its filters and sorts, and the values
//! of a row at the top of the row's page. Its menus are in table_menus.rs
//! and its other layouts (board, list, gallery, calendar) in table_views.rs.

use super::icons::{Icon, PageIcon};
use super::table_choices::{tag_class, ChoicePicker, Tag};
use super::table_editors::{DatePicker, WorkedOut};
use super::table_menus::{CellMenu, ChipMenu, ColumnMenu, ColumnsMenu, IconMenu, KindsMenu, PaintMenu, RowMenu, TableMenu};
use super::table_views::{Board, Calendar, Gallery, List};
use crate::state::{AppState, Open, Popover};
use crate::table::{checked, format_day, parts, Column, Kind, Layout, Show, Table};
use dioxus::prelude::*;

/// The order of the rows of the table that has the caret, kept while the
/// caret is in it: a row must not jump to its new place (a sort) or go out
/// of view (a filter) under the hand that types in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub block: String,
    /// How many rows the table had: with more or fewer, the order is stale.
    pub rows: usize,
    pub order: Vec<usize>,
}

pub static HELD: GlobalSignal<Option<Held>> = Signal::global(|| None);

/// The caret came into this table (`table` is it as it is now), or left it.
/// Only a table with a sort or a filter has an order to keep.
pub fn hold(id: &str, table: Option<&Table>) {
    let held = table.filter(|t| !t.view.sort.is_empty() || !t.view.filter.is_empty()).map(|t| Held { block: id.to_string(), rows: t.rows.len(), order: t.order() });
    let now = HELD.peek().clone();
    let mine = now.as_ref().is_some_and(|h| h.block == id);
    if (held.is_some() || mine) && now != held {
        *HELD.write() = held;
    }
}

/// The rows of a table that are picked (by a click on their handle), or its
/// columns (by ⌘ or ⇧ and a click on their headings): one or the other.
/// They stay picked when a menu closes, so that more can be added; a click
/// anywhere else lets them go (see the page script).
#[derive(Clone, Debug, PartialEq)]
pub struct Picked {
    pub block: String,
    /// In the order they were picked: the last one is where a range starts.
    pub rows: Vec<usize>,
    pub columns: Vec<usize>,
    /// How many rows and columns the table had: with more or fewer (the AI
    /// wrote the page again, another window changed the table), the numbers
    /// may be those of other rows and columns now, and nothing is picked.
    pub shape: (usize, usize),
}

pub static PICKED: GlobalSignal<Option<Picked>> = Signal::global(|| None);

/// How a click picks: that one alone; with ⇧, also all from the one picked
/// last to it; with ⌘, that one more, or no longer if it was picked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Alone,
    Range,
    Toggle,
}

/// The rows of a table in the order the screen shows them now (the order
/// that is kept while the caret is in the table, if one is).
pub fn shown_order(id: &str, table: &Table) -> Vec<usize> {
    HELD.peek().as_ref().filter(|h| h.block == id && h.rows == table.rows.len()).map(|h| h.order.clone()).unwrap_or_else(|| table.order())
}

/// What a click does to what is picked. `order` is what the screen shows:
/// a range takes what shows between its two ends, and no more.
fn pick_in(mut picked: Vec<usize>, item: usize, how: Pick, order: &[usize]) -> Vec<usize> {
    let place = |i: usize| order.iter().position(|&o| o == i);
    match how {
        Pick::Alone => picked = vec![item],
        Pick::Toggle => match picked.iter().position(|&p| p == item) {
            Some(at) => {
                picked.remove(at);
            }
            None => picked.push(item),
        },
        Pick::Range => {
            let between = picked.last().and_then(|&last| Some((place(last)?, place(item)?)));
            if let Some((a, b)) = between {
                let more: Vec<usize> = order[a.min(b)..=a.max(b)].iter().filter(|i| !picked.contains(i)).copied().collect();
                picked.extend(more);
            }
            // (The one clicked is the last: the next range starts from it.)
            picked.retain(|&p| p != item);
            picked.push(item);
        }
    }
    picked
}

fn shape_of(table: &Table) -> (usize, usize) {
    (table.rows.len(), table.columns())
}

fn set_picked(id: &str, table: &Table, rows: Vec<usize>, columns: Vec<usize>) {
    let now = (!rows.is_empty() || !columns.is_empty()).then(|| Picked { block: id.to_string(), rows, columns, shape: shape_of(table) });
    if *PICKED.peek() != now {
        *PICKED.write() = now;
    }
}

/// What of a table is picked, if the table is still as it was when it was
/// picked: its rows, and its columns.
fn picked_of(picked: &Option<Picked>, id: &str, table: &Table) -> (Vec<usize>, Vec<usize>) {
    picked.as_ref().filter(|p| p.block == id && p.shape == shape_of(table)).map(|p| (p.rows.clone(), p.columns.clone())).unwrap_or_default()
}

/// The rows of a table that are picked now, and its picked columns (one or
/// the other; `table` is the table as it is now).
pub fn picked(id: &str, table: &Table) -> (Vec<usize>, Vec<usize>) {
    picked_of(&PICKED.peek(), id, table)
}

/// Pick a row of a table (see [`Pick`]), as the screen shows its rows.
/// Picked columns are let go. Returns the rows picked now.
pub fn pick_row(id: &str, table: &Table, row: usize, how: Pick) -> Vec<usize> {
    let rows = pick_in(picked(id, table).0, row, how, &shown_order(id, table));
    set_picked(id, table, rows.clone(), Vec::new());
    rows
}

/// Pick a column of a table (see [`Pick`]), of those that show. Picked
/// rows are let go. Returns the columns picked now.
pub fn pick_column(id: &str, table: &Table, column: usize, how: Pick) -> Vec<usize> {
    let columns = pick_in(picked(id, table).1, column, how, &table.shown_columns());
    set_picked(id, table, Vec::new(), columns.clone());
    columns
}

/// Pick all the rows that show (the box in the row of the headings), or
/// none when all of them are picked already. Returns the rows picked now.
pub fn pick_all(id: &str, table: &Table) -> Vec<usize> {
    let order = shown_order(id, table);
    let rows = if all_of(&picked(id, table).0, &order) { Vec::new() } else { order };
    set_picked(id, table, rows.clone(), Vec::new());
    rows
}

/// Every row that shows is picked (and one shows).
fn all_of(picked: &[usize], order: &[usize]) -> bool {
    !order.is_empty() && order.iter().all(|r| picked.contains(r))
}

/// Let the picked rows or columns of a table go.
pub fn unpick(id: &str) {
    if PICKED.peek().as_ref().is_some_and(|p| p.block == id) {
        *PICKED.write() = None;
    }
}

/// The table changed by something other than typing in a cell. If a row
/// came or went, the numbers of the rows are other numbers now: an order
/// that is kept is worked out again. Picked rows and columns are let go: one
/// that came, went or moved gives their numbers to others.
pub fn changed(id: &str, table: &Table) {
    if HELD.peek().as_ref().is_some_and(|h| h.block == id && h.rows != table.rows.len()) {
        hold(id, Some(table));
    }
    unpick(id);
}

/// The table a popover belongs to.
fn table_of(popover: &Popover) -> Option<&str> {
    match popover {
        Popover::TableColumn { block_id, .. }
        | Popover::TableMenu { block_id }
        | Popover::TableKinds { block_id }
        | Popover::TableIcon { block_id }
        | Popover::TableRow { block_id, .. }
        | Popover::TableChip { block_id, .. }
        | Popover::TablePaint { block_id }
        | Popover::TableCell { block_id, .. } => Some(block_id),
        _ => None,
    }
}

/// A table. In its plain layout the first row is its header; Tab goes to the
/// next cell and Return to the cell below (a new row at the end). The handle
/// at the left of the row under the pointer adds a row, moves the row and
/// opens its menu (see the page script).
#[component]
pub fn TableView(page_id: String, id: String, data: String) -> Element {
    let state = use_context::<AppState>();
    let table = Table::parse(&data);
    let open = state.ui.read().popover.clone().filter(|p| table_of(p) == Some(id.as_str()));
    let (picked, picked_cols) = picked_of(&PICKED.read(), &id, &table);
    let order = HELD.read().as_ref().filter(|h| h.block == id && h.rows == table.rows.len()).map(|h| h.order.clone()).unwrap_or_else(|| table.order());
    {
        let id = id.clone();
        use_drop(move || {
            hold(&id, None);
            unpick(&id);
        });
    }
    let layout = table.view.layout;
    let today = crate::when::today();
    let has_date = table.first_of(Kind::Date).is_some();
    let named = !table.name.is_empty() || !table.icon.is_empty();
    let (p, b) = (page_id.clone(), id.clone());
    let body = match layout {
        Layout::Table => {
            // The rows whose page shows in a pane now, to mark them (looked
            // at here, where every change of the panes is seen).
            let mut open_rows: Vec<String> = if table.has_ids() { state.row_pages(&id).into_iter().filter(|(_, page)| state.shows_page(page)).map(|(row, _)| row).collect() } else { Vec::new() };
            open_rows.sort();
            rsx! { Grid { page_id: page_id.clone(), id: id.clone(), table: table.clone(), order, picked: picked.clone(), picked_cols, open_rows, open: open.clone(), named } }
        }
        Layout::Board => rsx! { Board { page_id: page_id.clone(), id: id.clone(), table: table.clone(), order } },
        Layout::List => rsx! { List { page_id: page_id.clone(), id: id.clone(), table: table.clone(), order } },
        Layout::Gallery => rsx! { Gallery { page_id: page_id.clone(), id: id.clone(), table: table.clone(), order } },
        Layout::Calendar => rsx! { Calendar { page_id: page_id.clone(), id: id.clone(), table: table.clone(), order } },
    };
    rsx! {
        div { class: "table-wrap layout-{layout.name()}", "data-table-wrap": "{id}", "data-page": "{page_id}",
            if named || layout != Layout::Table {
                TableHead { page_id: page_id.clone(), id: id.clone(), table: table.clone(), open: open.clone() }
            }
            Chips { page_id: page_id.clone(), id: id.clone(), table: table.clone(), open: open.clone() }
            {body}
            // The menus of the table itself hang from its top right corner.
            div { class: if named || layout != Layout::Table { "table-menus named" } else { "table-menus" },
                if matches!(open, Some(Popover::TableMenu { .. })) {
                    TableMenu { page_id: page_id.clone(), id: id.clone(), table: table.clone() }
                }
                if matches!(open, Some(Popover::TablePaint { .. })) {
                    PaintMenu { page_id: page_id.clone(), id: id.clone(), table: table.clone() }
                }
                if matches!(open, Some(Popover::TableIcon { .. })) {
                    IconMenu { page_id: page_id.clone(), id: id.clone(), icon: table.icon.clone() }
                }
            }
            if layout != Layout::Table {
                button { class: "table-today", "data-new-row": "1", onclick: move |_| state.new_card(&p, &b, &[]),
                    Icon { name: "plus", size: 14 }
                    span { "New row" }
                }
            } else if has_date {
                button { class: "table-today", "data-new-row": "1", title: "Add a row with the date of {format_day(today, today)}", onclick: move |_| state.add_today_row(&p, &b),
                    Icon { name: "plus", size: 14 }
                    span { "New row" }
                }
            } else {
                button { class: "table-today", "data-new-row": "1", onclick: move |_| state.new_table_row(&p, &b, &[]),
                    Icon { name: "plus", size: 14 }
                    span { "New row" }
                }
            }
        }
    }
}

/// The line above a table: its icon, its name, and the button of its menu.
#[component]
fn TableHead(page_id: String, id: String, table: Table, open: Option<Popover>) -> Element {
    let state = use_context::<AppState>();
    let locked = table.view.locked;
    let icon_open = matches!(open, Some(Popover::TableIcon { .. }));
    let (p_name, b_name) = (page_id.clone(), id.clone());
    let b_icon = id.clone();
    rsx! {
        div { class: "table-head",
            if !table.icon.is_empty() {
                button {
                    class: "table-icon",
                    title: "Change the icon",
                    "aria-label": "Icon of the table",
                    disabled: locked,
                    onclick: move |e| {
                        e.stop_propagation();
                        state.set_popover((!icon_open).then(|| Popover::TableIcon { block_id: b_icon.clone() }));
                    },
                    PageIcon { icon: Some(table.icon.clone()), size: 17 }
                }
            }
            input {
                class: "table-name",
                r#type: "text",
                placeholder: "Name",
                spellcheck: false,
                readonly: locked,
                "aria-label": "Name of the table",
                value: "{table.name}",
                oninput: move |e: FormEvent| state.set_table_name(&p_name, &b_name, &e.value()),
            }
            if let Some(c) = table.board_column().filter(|_| table.view.layout == Layout::Board) {
                span { class: "table-by", "by {table.heading(c)}" }
            }
            span { class: "table-head-gap" }
            MenuButton { id: id.clone(), open: matches!(open, Some(Popover::TableMenu { .. })) }
        }
    }
}

/// The button that opens the menu of a table.
#[component]
fn MenuButton(id: String, open: bool) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        button {
            class: if open { "table-more open" } else { "table-more" },
            title: "Table options",
            "aria-label": "Table options",
            "aria-haspopup": "menu",
            "aria-expanded": if open { "true" } else { "false" },
            onclick: move |e| {
                e.stop_propagation();
                state.set_popover((!open).then(|| Popover::TableMenu { block_id: id.clone() }));
            },
            Icon { name: "more-horizontal", size: 16 }
        }
    }
}

/// What the view of a table does, each as a small chip that opens to change
/// it: the filters, the sorts, the grouping.
#[component]
fn Chips(page_id: String, id: String, table: Table, open: Option<Popover>) -> Element {
    let state = use_context::<AppState>();
    let view = &table.view;
    // (A board says what it is by in the line of its name.)
    let grouped = view.group.filter(|_| view.layout == Layout::Table);
    if view.filter.is_empty() && view.sort.is_empty() && grouped.is_none() {
        return rsx! {};
    }
    let chip_open = match &open {
        Some(Popover::TableChip { sort, index, .. }) => Some((*sort, *index)),
        _ => None,
    };
    let filters: Vec<(usize, String)> = view.filter.iter().map(|rule| table.rule_words(rule)).enumerate().collect();
    let sorts: Vec<(usize, bool, String)> = view.sort.iter().enumerate().map(|(i, s)| (i, s.down, table.sort_words(s))).collect();
    let group_name = grouped.map(|c| table.heading(c));
    let (p_group, b_group) = (page_id.clone(), id.clone());
    rsx! {
        div { class: "table-chips",
            for (i, words) in filters {
                span { key: "f{i}", class: "chip-box",
                    button {
                        class: if chip_open == Some((false, i)) { "table-chip open" } else { "table-chip" },
                        "data-chip": "filter",
                        onclick: {
                            let id = id.clone();
                            move |e: MouseEvent| {
                                e.stop_propagation();
                                state.set_popover((chip_open != Some((false, i))).then(|| Popover::TableChip { block_id: id.clone(), sort: false, index: i }));
                            }
                        },
                        Icon { name: "filter", size: 13 }
                        span { "{words}" }
                    }
                    button {
                        class: "chip-x on-chip",
                        title: "Remove this filter",
                        "aria-label": "Remove this filter",
                        "data-chip-x": "filter",
                        onclick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            move |e: MouseEvent| {
                                e.stop_propagation();
                                state.set_popover(None);
                                state.change_table_view(&p, &b, |v| {
                                    if i < v.filter.len() {
                                        v.filter.remove(i);
                                    }
                                });
                            }
                        },
                        Icon { name: "close", size: 11 }
                    }
                    if chip_open == Some((false, i)) {
                        ChipMenu { page_id: page_id.clone(), id: id.clone(), table: table.clone(), sort: false, index: i }
                    }
                }
            }
            for (i, down, words) in sorts {
                span { key: "s{i}", class: "chip-box",
                    button {
                        class: if chip_open == Some((true, i)) { "table-chip open" } else { "table-chip" },
                        "data-chip": "sort",
                        onclick: {
                            let id = id.clone();
                            move |e: MouseEvent| {
                                e.stop_propagation();
                                state.set_popover((chip_open != Some((true, i))).then(|| Popover::TableChip { block_id: id.clone(), sort: true, index: i }));
                            }
                        },
                        Icon { name: if down { "sort-down" } else { "sort-up" }, size: 13 }
                        span { "{words}" }
                    }
                    button {
                        class: "chip-x on-chip",
                        title: "Remove this sort",
                        "aria-label": "Remove this sort",
                        "data-chip-x": "sort",
                        onclick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            move |e: MouseEvent| {
                                e.stop_propagation();
                                state.set_popover(None);
                                state.change_table_view(&p, &b, |v| {
                                    if i < v.sort.len() {
                                        v.sort.remove(i);
                                    }
                                });
                            }
                        },
                        Icon { name: "close", size: 11 }
                    }
                    if chip_open == Some((true, i)) {
                        ChipMenu { page_id: page_id.clone(), id: id.clone(), table: table.clone(), sort: true, index: i }
                    }
                }
            }
            if let Some(name) = group_name {
                span { class: "chip-box",
                    span { class: "table-chip", "data-chip": "group",
                        Icon { name: "group", size: 13 }
                        span { "By {name}" }
                        button {
                            class: "chip-x",
                            title: "Stop grouping",
                            "aria-label": "Stop grouping by {name}",
                            onclick: move |_| state.change_table_view(&p_group, &b_group, |v| v.group = None),
                            Icon { name: "close", size: 11 }
                        }
                    }
                }
            }
        }
    }
}

/// One line of the plain layout: the heading of a group, or a row.
enum Line {
    Group(String, usize),
    Row(usize),
}

/// The least width of a column with a kind, in points: room for the
/// longest word of its heading beside its sign (a heading wraps between
/// its words only), and for what its kind holds.
fn column_width(table: &Table, c: usize) -> usize {
    let longest = table.heading(c).split_whitespace().map(|word| word.chars().count()).max().unwrap_or(0);
    let for_kind = match table.kind(c) {
        Kind::Text | Kind::Multi | Kind::Link | Kind::Email | Kind::Relation => 150,
        Kind::Checkbox => 66,
        Kind::Number | Kind::Formula => 110,
        Kind::Time | Kind::Pace => 96,
        _ => 118,
    };
    (longest * 9 + 40).max(for_kind).max(if c == 0 { 150 } else { 0 }).min(280)
}

/// The plain layout: a header, the rows, the totals; at its right the
/// buttons for a new column and (with no line above the table) for its menu;
/// at its left the handle of the row under the pointer. The menus of its
/// cells, rows and columns are drawn beside the box that scrolls the table
/// sideways, not in it: a frozen cell is a layer of its own in that box, and
/// WebKit cuts what is in such a layer at the edges of the box, a menu fixed
/// to the window too. The page script hangs each menu from its cell.
#[component]
fn Grid(page_id: String, id: String, table: Table, order: Vec<usize>, picked: Vec<usize>, picked_cols: Vec<usize>, open_rows: Vec<String>, open: Option<Popover>, named: bool) -> Element {
    let state = use_context::<AppState>();
    let cols = table.shown_columns();
    let typed = table.is_typed();
    let locked = table.view.locked;
    // The first columns of the table are frozen (`view.frozen` of them): of
    // the columns that show, those. (Counted among the columns that show, a
    // frozen column that was hidden froze the next one in its place.)
    let frozen = cols.iter().filter(|&&c| c < table.view.frozen).count();
    let menu_col = match &open {
        Some(Popover::TableColumn { column, .. }) => Some(*column),
        _ => None,
    };
    let cell_menu = match &open {
        Some(Popover::TableCell { row, column, on_page: false, .. }) => Some((*row, *column)),
        _ => None,
    };
    // The menu of the picked rows hangs under the row picked last.
    let menu_rows: Vec<usize> = match &open {
        Some(Popover::TableRow { rows, .. }) => rows.clone(),
        _ => Vec::new(),
    };
    let row_menu = menu_rows.last().copied();
    // The cell with an open list, and which list it is.
    let cell_list = cell_menu.filter(|&(r, c)| r < table.rows.len() && c < table.columns()).map(|(r, c)| (r, c, table.column(c), table.cell(r, c).to_string(), table.heading(c), table.worked_out_from(c)));
    let cols_picked = |c: usize| picked_cols.contains(&c);
    let kinds_open = matches!(open, Some(Popover::TableKinds { .. }));
    let menu_open = matches!(open, Some(Popover::TableMenu { .. }));
    let any_menu = menu_col.is_some() || cell_menu.is_some() || row_menu.is_some();
    // The rows, under the headings of their groups when the table is grouped.
    let lines: Vec<Line> = match table.view.group {
        Some(g) => {
            let kind = table.kind(g);
            let heading = table.heading(g);
            table
                .groups(&order, g)
                .into_iter()
                .filter(|(_, rows)| !rows.is_empty())
                .flat_map(|(value, rows)| std::iter::once(Line::Group(group_label(kind, &heading, &value), rows.len())).chain(rows.into_iter().map(Line::Row)))
                .collect()
        }
        None => order.iter().copied().map(Line::Row).collect(),
    };
    // What a full bar or ring is, once for each column that shows one.
    let fulls: Vec<f64> = (0..table.columns()).map(|c| if table.column(c).show == Show::Number { 0.0 } else { table.full_at(c) }).collect();
    let totals: Vec<(usize, bool, Option<(&'static str, String)>)> = cols.iter().map(|&c| (c, table.kind(c).is_amount(), table.total(c))).collect();
    // Room for every column: with many, the table scrolls sideways in
    // its box before a column gets too thin to read. A column with a kind
    // has the width that its kind and the longest word of its heading need.
    let widths: Vec<usize> = cols.iter().map(|&c| if typed { column_width(&table, c) } else { 96 }).collect();
    let room: usize = widths.iter().sum();
    // (The first column, the names of the rows, takes what is left.)
    let wide = |n: usize| if typed && n > 0 { format!("width: {}px;", widths[n]) } else { String::new() };
    // (A table with kinds has a strip at its left for the boxes that pick
    // its rows, in the padding of the box that scrolls it: WebKit keeps a
    // frozen column at the edge inside that padding, right of the strip.)
    let sticky = |n: usize| if n < frozen { format!("left: var(--f{n}, 0px);") } else { String::new() };
    // Whether all the rows that show are picked, some, or none: the box of
    // the row of the headings says it.
    let all_picked = all_of(&picked, &order);
    let head_check = if all_picked { "all" } else if picked.is_empty() { "none" } else { "some" };
    // (Each time the table is drawn, also with no frozen column: what is
    // cut at its left edge while it is scrolled sideways is made again.)
    document::eval(&format!("window.__n2 && window.__n2.freezeColumns && window.__n2.freezeColumns({})", serde_json::to_string(&id).unwrap_or_default()));
    let b_kinds = id.clone();
    let block_class = format!("table-block{}{}{}{}", if typed { " typed" } else { "" }, if any_menu { " menu-open" } else { "" }, if locked { " locked" } else { "" }, if table.view.sort.is_empty() { "" } else { " sorted" });
    rsx! {
        div { class: "table-frame",
            div { class: "{block_class}", "data-table": "{id}", "data-page": "{page_id}", "data-frozen": "{frozen}",
                table { style: "min-width: {room}px;",
                    tbody {
                        tr { class: "head-row", "data-r": "0",
                            for (n, c) in cols.iter().copied().enumerate() {
                                td {
                                    key: "{c}",
                                    class: format!("head{}{}{}", if menu_col == Some(c) { " menu-open" } else { "" }, if n < frozen { " frozen" } else { "" }, if cols_picked(c) { " col-picked" } else { "" }),
                                    style: "{wide(n)}{sticky(n)}",
                                    "data-col": "{c}",
                                    if typed && n == 0 {
                                        button {
                                            class: "head-check {head_check}",
                                            role: "checkbox",
                                            tabindex: "-1",
                                            title: if all_picked { "Pick no rows" } else { "Pick all rows" },
                                            "aria-label": "Pick all rows",
                                            "aria-checked": match head_check { "all" => "true", "some" => "mixed", _ => "false" },
                                            "data-check": "all",
                                            match head_check {
                                                "all" => rsx! { Icon { name: "check", size: 11 } },
                                                "some" => rsx! { Icon { name: "minus", size: 11 } },
                                                _ => rsx! {},
                                            }
                                        }
                                    }
                                    HeadCell {
                                        page_id: page_id.clone(),
                                        id: id.clone(),
                                        c,
                                        text: table.cell(0, c).to_string(),
                                        heading: table.heading(c),
                                        column: table.column(c),
                                        typed,
                                        locked,
                                        menu_open: menu_col == Some(c),
                                    }
                                }
                            }
                        }
                        for (n, line) in lines.into_iter().enumerate() {
                            match line {
                                Line::Group(label, count) => rsx! {
                                    tr { key: "g-{n}", class: "group-row",
                                        td { colspan: "{cols.len()}",
                                            span { class: table.view.group.map_or("tag".to_string(), |g| tag_class(&table.column(g), &label)), "{label}" }
                                            span { class: "group-count", "{count}" }
                                        }
                                    }
                                },
                                Line::Row(r) => {
                                    let info = table.info(r);
                                    let indent = table.shown_indent(r) as usize;
                                    let kids = table.has_children(r) && table.view.filter.is_empty();
                                    let is_open = !info.id.is_empty() && open_rows.contains(&info.id);
                                    let color = table.color(r).unwrap_or_default().to_string();
                                    let class = format!("{}{}{}", if picked.contains(&r) { "picked " } else { "" }, if is_open { "open " } else { "" }, if color.is_empty() { "" } else { "painted" });
                                    let style = if color.is_empty() { String::new() } else { format!("--row-tint: var(--icon-{color}-soft);") };
                                    let (p_fold, b_fold) = (page_id.clone(), id.clone());
                                    let (p_open, b_open) = (page_id.clone(), id.clone());
                                    rsx! {
                                        tr { key: "{r}", class: "{class}", style: "{style}", "data-r": "{r}",
                                            for (n, c) in cols.iter().copied().enumerate() {
                                                td {
                                                    key: "{c}",
                                                    class: cell_class(table.kind(c), n == 0, n < frozen, cols_picked(c)),
                                                    style: "{sticky(n)}",
                                                    "data-col": "{c}",
                                                    if n == 0 && typed {
                                                        button {
                                                            class: "row-check",
                                                            role: "checkbox",
                                                            tabindex: "-1",
                                                            title: "Pick this row (⇧ click: all rows up to it)",
                                                            "aria-label": "Pick row {r}",
                                                            "aria-checked": if picked.contains(&r) { "true" } else { "false" },
                                                            "data-check": "{r}",
                                                            if picked.contains(&r) {
                                                                Icon { name: "check", size: 11 }
                                                            }
                                                        }
                                                    }
                                                    if n == 0 {
                                                        div { class: "first-cell", style: "padding-left: {indent * 18}px;",
                                                            if kids {
                                                                button {
                                                                    class: if info.closed { "row-fold closed" } else { "row-fold" },
                                                                    title: if info.closed { "Show the rows inside" } else { "Hide the rows inside" },
                                                                    "aria-expanded": if info.closed { "false" } else { "true" },
                                                                    onclick: {
                                                                        let (p, b) = (p_fold.clone(), b_fold.clone());
                                                                        move |_| state.table_op(&p, &b, "fold-row", r, 0)
                                                                    },
                                                                    Icon { name: "triangle-right", size: 10 }
                                                                }
                                                            }
                                                            CellBody {
                                                                page_id: page_id.clone(),
                                                                id: id.clone(),
                                                                r,
                                                                c,
                                                                text: table.cell(r, c).to_string(),
                                                                shown: if table.kind(c).is_worked_out() { table.shown(r, c) } else { String::new() },
                                                                heading: table.heading(c),
                                                                column: table.column(c),
                                                                hint: String::new(),
                                                                share: None,
                                                                menu_open: cell_menu == Some((r, c)),
                                                                own_menu: false,
                                                            }
                                                            button {
                                                                class: "row-open",
                                                                title: if table.view.full { "Open as a page (⌘ click: in a new tab)" } else { "Open at the side (⌘ click: in a new tab)" },
                                                                "data-open-row": "{r}",
                                                                onclick: {
                                                                    let (p, b) = (p_open.clone(), b_open.clone());
                                                                    move |e: MouseEvent| {
                                                                        e.stop_propagation();
                                                                        state.open_row(&p, &b, r, if e.modifiers().meta() { Open::Tab } else { Open::Auto });
                                                                    }
                                                                },
                                                                Icon { name: "side", size: 13 }
                                                                span { "Open" }
                                                            }
                                                        }
                                                    } else {
                                                        CellBody {
                                                            page_id: page_id.clone(),
                                                            id: id.clone(),
                                                            r,
                                                            c,
                                                            text: table.cell(r, c).to_string(),
                                                            shown: if table.kind(c).is_worked_out() { table.shown(r, c) } else { String::new() },
                                                            heading: table.heading(c),
                                                            column: table.column(c),
                                                            hint: table.rolled_up(r, c).unwrap_or_default(),
                                                            share: share_of(&table, r, c, fulls[c]),
                                                            menu_open: cell_menu == Some((r, c)),
                                                            own_menu: false,
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if totals.iter().any(|(_, _, total)| total.is_some()) {
                        tfoot { class: "table-totals", "aria-label": "Totals",
                            tr {
                                for (n, (c, amount, total)) in totals.into_iter().enumerate() {
                                    td {
                                        key: "{c}",
                                        class: format!("{}{}{}", if amount { "num" } else { "" }, if n < frozen { " frozen" } else { "" }, if cols_picked(c) { " col-picked" } else { "" }),
                                        style: "{sticky(n)}",
                                        if let Some((word, value)) = total {
                                            if !word.is_empty() {
                                                span { class: "total-word", "{word} " }
                                            }
                                            "{value}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // The bar that moves a wide table sideways. The page script sizes
            // it and moves the table with it (`syncBar` in bridge.js): the
            // engine's own bar moved the table apart from the script, and
            // the words of the cells showed under the frozen column.
            div { class: "table-bar", "aria-hidden": "true",
                div { class: "table-thumb" }
            }
            // The menus that hang from a cell, a row or a heading (each a
            // new element when it hangs from another one: the page script
            // places what comes into the page).
            div { class: "table-hung",
                for c in menu_col {
                    div { key: "column-{c}", class: "hung", "data-hang": "column", "data-c": "{c}",
                        if picked_cols.len() > 1 && cols_picked(c) {
                            ColumnsMenu { page_id: page_id.clone(), id: id.clone(), table: table.clone(), columns: picked_cols.clone() }
                        } else {
                            ColumnMenu { page_id: page_id.clone(), id: id.clone(), c, table: table.clone() }
                        }
                    }
                }
                for r in row_menu {
                    div { key: "row-{r}", class: "hung", "data-hang": "row", "data-r": "{r}",
                        RowMenu { page_id: page_id.clone(), id: id.clone(), table: table.clone(), rows: menu_rows.clone() }
                    }
                }
                for (r, c, column, text, heading, from) in cell_list {
                    div { key: "cell-{r}-{c}", class: "hung", "data-hang": "cell", "data-r": "{r}", "data-c": "{c}",
                        CellList { page_id: page_id.clone(), id: id.clone(), r, c, column, text, heading, from, in_table: true }
                    }
                }
            }
            // What is picked, and what to do with it.
            if !picked.is_empty() || !picked_cols.is_empty() {
                PickedBar { page_id: page_id.clone(), id: id.clone(), rows: picked.clone(), columns: picked_cols.clone(), locked, last_column: picked_cols.len() >= table.columns() }
            }
            // The handle of the row under the pointer (the page script puts it there).
            div { class: "row-handle", "data-row-handle": "{id}", "aria-hidden": "true",
                button { class: "g-btn", "data-row-op": "add-below", title: "Add a row below", tabindex: "-1", Icon { name: "plus", size: 15 } }
                button { class: "g-btn handle", "data-row-op": "row-menu", draggable: "true", title: "Drag to move · Click for options · ⌘ or ⇧ click to pick more rows", tabindex: "-1", Icon { name: "grip", size: 15 } }
            }
            div { class: if kinds_open || (menu_open && !named) { "table-side open" } else { "table-side" },
                if !locked {
                    button {
                        class: if kinds_open { "table-more open" } else { "table-more" },
                        title: "Add a column",
                        "aria-label": "Add a column",
                        "aria-haspopup": "menu",
                        "aria-expanded": if kinds_open { "true" } else { "false" },
                        "data-add-column": "1",
                        onclick: move |e| {
                            e.stop_propagation();
                            state.set_popover((!kinds_open).then(|| Popover::TableKinds { block_id: b_kinds.clone() }));
                        },
                        Icon { name: "plus", size: 15 }
                    }
                }
                if !named {
                    MenuButton { id: id.clone(), open: menu_open }
                }
                if kinds_open {
                    KindsMenu { page_id: page_id.clone(), id: id.clone() }
                }
            }
        }
    }
}

/// What the heading of a group says: the value, or that there is none.
pub fn group_label(kind: Kind, heading: &str, value: &str) -> String {
    match (kind, value.is_empty()) {
        (Kind::Checkbox, _) => if checked(value) { "Ticked" } else { "Not ticked" }.to_string(),
        (_, true) => format!("No {heading}"),
        (_, false) => value.to_string(),
    }
}

fn cell_class(kind: Kind, first: bool, frozen: bool, picked: bool) -> String {
    let by_kind = match kind {
        Kind::Checkbox => "check-cell",
        kind if kind.is_amount() => "num",
        _ => "",
    };
    format!("{by_kind}{}{}{}", if first { " first" } else { "" }, if frozen { " frozen" } else { "" }, if picked { " col-picked" } else { "" })
}

/// What opens from a cell, by the kind of its column: the options of a
/// choice cell, the rows a relation cell can name, the month of a date
/// cell, or what a worked-out cell is worked out from (`from`). The other
/// kinds are typed in, in the cell.
#[component]
pub fn CellList(page_id: String, id: String, r: usize, c: usize, column: Column, text: String, heading: String, from: String, in_table: bool) -> Element {
    match column.kind {
        k if k.has_options() => rsx! { ChoicePicker { page_id, id, r, c, column, text } },
        Kind::Relation => rsx! { CellMenu { page_id, id, r, c, from: column.from.clone(), text } },
        Kind::Date => rsx! { DatePicker { page_id, id, r, c, text } },
        k if k.is_worked_out() => rsx! { WorkedOut { id, c, heading, from, formula: k == Kind::Formula, in_table } },
        _ => rsx! {},
    }
}

/// What is picked in a table, and the way to take it out: a small bar over
/// the top left corner of the table, while rows or columns are picked. (It
/// is in a rail as high as the table, and stays at the top of the page while
/// a long table is scrolled under it.)
#[component]
fn PickedBar(page_id: String, id: String, rows: Vec<usize>, columns: Vec<usize>, locked: bool, last_column: bool) -> Element {
    let state = use_context::<AppState>();
    let words = match (rows.len(), columns.len()) {
        (1, _) => "1 row selected".to_string(),
        (0, 1) => "1 column selected".to_string(),
        (0, n) => format!("{n} columns selected"),
        (n, _) => format!("{n} rows selected"),
    };
    // (A locked table keeps its columns, and a table keeps one column.)
    let can_delete = !rows.is_empty() || (!locked && !last_column);
    let b_clear = id.clone();
    rsx! {
        div { class: "picked-rail",
            div { class: "picked-bar", role: "toolbar", "aria-label": "Selection",
                span { class: "table-picked", "{words}" }
                if can_delete {
                    button {
                        class: "picked-do",
                        title: "Delete (the Delete key does the same)",
                        "data-picked": "delete",
                        onclick: move |e| {
                            e.stop_propagation();
                            state.delete_picked_in_table(&page_id, &id);
                        },
                        Icon { name: "trash", size: 13 }
                        span { "Delete" }
                    }
                }
                button {
                    class: "picked-x",
                    title: "Clear the selection",
                    "aria-label": "Clear the selection",
                    "data-picked": "clear",
                    onclick: move |e| {
                        e.stop_propagation();
                        unpick(&b_clear);
                    },
                    Icon { name: "close", size: 12 }
                }
            }
        }
    }
}

/// How full the bar or ring of a cell is (none for a column that shows
/// plain numbers, or a cell with no amount).
fn share_of(table: &Table, r: usize, c: usize, full: f64) -> Option<f64> {
    if full <= 0.0 {
        return None;
    }
    table.value(r, c).map(|v| (v / full).clamp(0.0, 1.0))
}

/// The heading of a column: the icon of its kind, its name, and the button
/// of its menu.
#[component]
fn HeadCell(page_id: String, id: String, c: usize, text: String, heading: String, column: Column, typed: bool, locked: bool, menu_open: bool) -> Element {
    let state = use_context::<AppState>();
    let kind = column.kind;
    let menu_block = id.clone();
    rsx! {
        div { class: "th",
            if typed {
                span { class: "th-kind", title: "{kind.label()}", Icon { name: kind.icon(), size: 13 } }
            }
            textarea {
                class: "cell",
                rows: "1",
                spellcheck: false,
                readonly: locked,
                "data-r": "0",
                "data-c": "{c}",
                "aria-label": "Heading of column {c + 1}",
                value: "{text}",
                oninput: move |e: FormEvent| state.set_table_cell(&page_id, &id, 0, c, &e.value()),
            }
            button {
                class: if menu_open { "th-menu open" } else { "th-menu" },
                title: "Kind, total and order of this column",
                "aria-label": "Menu of {heading}",
                "aria-haspopup": "menu",
                "aria-expanded": if menu_open { "true" } else { "false" },
                onclick: move |e| {
                    e.stop_propagation();
                    state.set_popover((!menu_open).then(|| Popover::TableColumn { block_id: menu_block.clone(), column: c }));
                },
                Icon { name: "chevron-down", size: 12 }
            }
        }
    }
}

/// What a cell holds, by the kind of its column: a box to tick, something
/// worked out, tags, a link, pictures, or words to type. The same in a table
/// and in the list of values at the top of a row's page.
#[component]
pub fn CellBody(
    page_id: String,
    id: String,
    r: usize,
    c: usize,
    text: String,
    shown: String,
    heading: String,
    column: Column,
    hint: String,
    #[props(!optional)] share: Option<f64>,
    menu_open: bool,
    /// Its list is drawn here, in the cell (at the top of a row's page). A
    /// table draws the lists of its cells itself: see [`Grid`].
    own_menu: bool,
    /// What a worked-out cell is worked out from (see `Table::worked_out_from`).
    #[props(default)]
    from: String,
) -> Element {
    let state = use_context::<AppState>();
    let kind = column.kind;
    let label = format!("Row {}, {}", r + 1, heading);
    let today = crate::when::today();
    // What a cell of its kind looks like, shown in an empty cell that has
    // the caret ("$0.00", "0:00"), and a cell that its kind cannot read
    // (grey, with why in its tip: totals and sorts leave it out).
    let filled = !text.trim().is_empty();
    let odd = match kind {
        Kind::Number if filled && crate::table::number(&text).is_none() => Some("Not a number: the total leaves it out"),
        Kind::Number if filled && !crate::table::number_is_whole(&text) => Some("More than one number: only the first one counts"),
        Kind::Number => None,
        Kind::Time => (filled && crate::table::seconds(&text).is_none()).then_some("Not a length of time: write 34:00, 1:02:30 or 1h 5m"),
        Kind::Email => (filled && !text.contains('@')).then_some("Not an email address"),
        Kind::Phone => (filled && phone_url(&text).is_none()).then_some("Not a phone number"),
        _ => None,
    };
    let kind_hint = hint.is_empty() && matches!(kind, Kind::Number | Kind::Time);
    let hint = match kind {
        Kind::Number if kind_hint => crate::table::money::write(&column.format, 0.0, column.places.map(usize::from)),
        Kind::Time if kind_hint => "0:00".to_string(),
        _ => hint,
    };
    let odd_class = format!("{}{}", if odd.is_some() { " odd" } else { "" }, if kind_hint { " hinted" } else { "" });
    // A gain green and a loss red, in a column that asks for it.
    let amount = if kind.is_worked_out() { crate::table::number(&shown) } else { crate::table::number(&text).or_else(|| crate::table::number(&shown)) };
    let sign = match amount {
        Some(n) if column.signed && n > 0.0 => " pos",
        Some(n) if column.signed && n < 0.0 => " neg",
        _ => "",
    };
    let (p_type, b_type) = (page_id.clone(), id.clone());
    let (p_left, b_left) = (page_id.clone(), id.clone());
    let typing = move |e: FormEvent| state.set_table_cell(&p_type, &b_type, r, c, &e.value());
    let left = move |_: FormEvent| state.settle_table_cell(&p_left, &b_left, r, c);
    // Words to type, as most kinds are.
    let field = |class: &'static str| {
        rsx! {
            textarea {
                class: "{class}{sign}{odd_class}",
                rows: "1",
                spellcheck: kind == Kind::Text,
                "data-r": "{r}",
                "data-c": "{c}",
                "aria-label": "{label}",
                title: odd.unwrap_or_default(),
                placeholder: "{hint}",
                value: "{text}",
                oninput: typing.clone(),
                onchange: left.clone(),
            }
        }
    };
    let meter = share.map(|share| match column.show {
        Show::Bar => rsx! {
            span { class: "meter-bar", role: "img", "aria-label": "{(share * 100.0).round()} percent", span { style: "width: {share * 100.0}%;" } }
        },
        _ => rsx! {
            svg { class: "meter-ring", role: "img", "aria-label": "{(share * 100.0).round()} percent", width: "16", height: "16", view_box: "0 0 16 16",
                circle { class: "ring-rest", cx: "8", cy: "8", r: "6" }
                circle { class: "ring-done", cx: "8", cy: "8", r: "6", stroke_dasharray: "{share * 37.7} 37.7", transform: "rotate(-90 8 8)" }
            }
        },
    });
    match kind {
        Kind::Checkbox => {
            let on = checked(&text);
            rsx! {
                button {
                    class: if on { "cell-check on" } else { "cell-check" },
                    role: "checkbox",
                    "aria-checked": if on { "true" } else { "false" },
                    "aria-label": "{label}",
                    onclick: move |_| state.toggle_table_check(&page_id, &id, r, c),
                    if on {
                        Icon { name: "check", size: 11 }
                    }
                }
            }
        }
        Kind::Pace | Kind::Formula => {
            let block = id.clone();
            rsx! {
                div { class: "cell-meter",
                    textarea {
                        class: if menu_open { "cell computed open{sign}" } else { "cell computed{sign}" },
                        rows: "1",
                        readonly: true,
                        tabindex: "-1",
                        "data-r": "{r}",
                        "data-c": "{c}",
                        "aria-label": "{label}, worked out",
                        value: "{shown}",
                        onclick: move |e| {
                            e.stop_propagation();
                            state.set_popover((!menu_open).then(|| Popover::TableCell { block_id: block.clone(), row: r, column: c, on_page: own_menu }));
                        },
                    }
                    {meter}
                    if menu_open && own_menu {
                        WorkedOut { id: id.clone(), c, heading: heading.clone(), from: from.clone(), formula: kind == Kind::Formula, in_table: false }
                    }
                }
            }
        }
        Kind::Date => {
            let block = id.clone();
            // (Its day, time and end; a bell says it has a reminder.)
            let read = crate::table::date::When::read(&text, today, true);
            let shown = match read {
                Some(w) => w.shown(today),
                None => text.trim().to_string(),
            };
            let bell = read.is_some_and(|w| w.remind.is_some());
            let odd = !shown.is_empty() && crate::table::date::When::read(&text, today, false).is_none();
            rsx! {
                button {
                    class: format!("cell pick date{}{}", if menu_open { " open" } else { "" }, if odd { " odd" } else { "" }),
                    value: "{text}",
                    "data-r": "{r}",
                    "data-c": "{c}",
                    "aria-label": "{label}",
                    "aria-haspopup": "dialog",
                    title: if odd { "Not a day: the calendar and the sorts leave it out" } else { "" },
                    onclick: move |e| {
                        e.stop_propagation();
                        state.set_popover((!menu_open).then(|| Popover::TableCell { block_id: block.clone(), row: r, column: c, on_page: own_menu }));
                    },
                    if shown.is_empty() {
                        span { class: "cell-empty", "{hint}" }
                    } else {
                        span { class: "date-words", "{shown}" }
                        if bell {
                            span { class: "date-bell", title: "With a reminder", Icon { name: "bell", size: 12 } }
                        }
                    }
                }
                if menu_open && own_menu {
                    DatePicker { page_id: page_id.clone(), id: id.clone(), r, c, text: text.clone() }
                }
            }
        }
        Kind::Number if meter.is_some() => rsx! {
            div { class: "cell-meter",
                {field("cell")}
                {meter}
            }
        },
        k if k.has_options() => {
            let names = column.names_in(&text);
            let block = id.clone();
            rsx! {
                button {
                    class: if menu_open { "cell pick open" } else { "cell pick" },
                    value: "{text}",
                    "data-r": "{r}",
                    "data-c": "{c}",
                    "aria-label": "{label}",
                    "aria-haspopup": "listbox",
                    onclick: move |e| {
                        e.stop_propagation();
                        state.set_popover((!menu_open).then(|| Popover::TableCell { block_id: block.clone(), row: r, column: c, on_page: own_menu }));
                    },
                    for name in names {
                        Tag { key: "{name}", name: name.clone(), color: column.color_of(&name), dot: k == Kind::Status }
                    }
                }
                if menu_open && own_menu {
                    ChoicePicker { page_id: page_id.clone(), id: id.clone(), r, c, column: column.clone(), text: text.clone() }
                }
            }
        }
        // An address: a click puts the caret in it, ⌘ and a click (or the
        // small button at its end) opens it (see the page script).
        Kind::Link | Kind::Email | Kind::Phone => {
            let address = text.trim().to_string();
            let url = match kind {
                Kind::Email => (address.contains('@')).then(|| format!("mailto:{address}")),
                Kind::Phone => phone_url(&address),
                _ if address.contains("://") => Some(address.clone()),
                _ if address.is_empty() => None,
                _ => Some(format!("https://{address}")),
            };
            let (go_word, go_icon) = match kind {
                Kind::Email => ("Write to", "mail"),
                Kind::Phone => ("Call", "phone"),
                _ => ("Open", "arrow-up-right"),
            };
            rsx! {
                div { class: "cell-link",
                    {field("cell link")}
                    if let Some(url) = url {
                        button { class: "cell-go", "data-go": "1", title: "{go_word} (⌘ click)", "aria-label": "{go_word} {address}", tabindex: "-1", onclick: move |_| state.open_url(&url),
                            Icon { name: go_icon, size: 12 }
                        }
                    }
                }
            }
        }
        Kind::File => {
            let names: Vec<String> = parts(&text).into_iter().filter(|name| crate::files::valid_name(name)).collect();
            let (p_add, b_add) = (page_id.clone(), id.clone());
            rsx! {
                // A screenshot goes in with ⌘V while the cell is chosen, or
                // dropped on it (see `cellPictures` in bridge.js).
                div {
                    class: "cell-pics",
                    "data-pic-cell": "",
                    "data-page": "{page_id}",
                    "data-block": "{id}",
                    "data-r": "{r}",
                    "data-c": "{c}",
                    tabindex: "0",
                    title: "Paste or drop a picture here",
                    for name in names {
                        span { key: "{name}", class: "cell-pic",
                            img { src: "{crate::files::url(&name)}", alt: "Picture", draggable: "false" }
                            button {
                                class: "pic-x",
                                title: "Take this picture out",
                                "aria-label": "Take this picture out",
                                onclick: {
                                    let (p, b) = (page_id.clone(), id.clone());
                                    move |_| state.toggle_cell_part(&p, &b, r, c, &name)
                                },
                                Icon { name: "close", size: 9 }
                            }
                        }
                    }
                    button { class: "cell-add", title: "Add a picture", "aria-label": "Add a picture to {label}", onclick: move |_| state.pick_cell_picture(&p_add, &b_add, r, c),
                        Icon { name: "plus", size: 12 }
                    }
                }
            }
        }
        Kind::Relation => {
            let from = column.from.clone();
            let menu_block = id.clone();
            rsx! {
                div { class: "cell-pics",
                    if from.is_empty() {
                        span { class: "cell-note", "Choose its table in the column's menu" }
                    }
                    for (i, name) in parts(&text).into_iter().enumerate() {
                        button {
                            key: "{i}",
                            class: "tag as-link",
                            title: "Open this row",
                            onclick: {
                                let from = from.clone();
                                move |_| state.open_related(&from, &name)
                            },
                            "{name}"
                        }
                    }
                    if !from.is_empty() {
                        button {
                            class: if menu_open { "cell-add open" } else { "cell-add" },
                            title: "Choose rows",
                            "aria-label": "Choose rows for {label}",
                            "aria-haspopup": "menu",
                            onclick: move |e| {
                                e.stop_propagation();
                                state.set_popover((!menu_open).then(|| Popover::TableCell { block_id: menu_block.clone(), row: r, column: c, on_page: own_menu }));
                            },
                            Icon { name: "plus", size: 12 }
                        }
                    }
                    if menu_open && own_menu {
                        CellMenu { page_id: page_id.clone(), id: id.clone(), r, c, from: from.clone(), text: text.clone() }
                    }
                }
            }
        }
        _ => field("cell"),
    }
}

/// The address that calls a phone number: its digits, with a + before
/// them if it has one ("+1 (555) 010-2000" is tel:+15550102000). None for
/// words that are not a phone number.
pub fn phone_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || !text.chars().all(|ch| ch.is_ascii_digit() || " +-().".contains(ch)) {
        return None;
    }
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    (digits.len() >= 3).then(|| format!("tel:{}{digits}", if text.starts_with('+') { "+" } else { "" }))
}

/// The values of a row, at the top of the row's page: one line for each
/// column after the first (the first is the page's title), each to change in
/// place, and a line to add a column. For the page of a template: what a new
/// row starts with.
#[component]
pub fn RowValues(page_id: String) -> Element {
    let state = use_context::<AppState>();
    // (Read here so that this is drawn again when a table or a page changes.)
    let _ = (state.blocks.read().len(), state.pages.read().len());
    let Some(row) = state.row_ref(&page_id) else { return rsx! {} };
    let cell_menu = match state.ui.read().popover.clone() {
        Some(Popover::TableCell { block_id, row: at, column, on_page: true }) if block_id == row.block => Some((at, column)),
        _ => None,
    };
    let table = row.table.clone();
    let (page, block) = (row.page.clone(), row.block.clone());
    let locked = table.view.locked;
    let (p_add, b_add) = (page.clone(), block.clone());
    match row.row {
        Some(r) => {
            let fulls: Vec<f64> = (0..table.columns()).map(|c| if table.column(c).show == Show::Number { 0.0 } else { table.full_at(c) }).collect();
            rsx! {
                div { class: "row-values", "data-row-values": "{block}",
                    for c in 1..table.columns() {
                        div { key: "{c}", class: "row-value",
                            span { class: "value-kind", Icon { name: table.kind(c).icon(), size: 15 } }
                            input {
                                class: "value-name",
                                r#type: "text",
                                placeholder: "Column {c + 1}",
                                spellcheck: false,
                                readonly: locked,
                                "aria-label": "Name of column {c + 1}",
                                value: "{table.cell(0, c)}",
                                oninput: {
                                    let (p, b) = (page.clone(), block.clone());
                                    move |e: FormEvent| state.set_table_cell(&p, &b, 0, c, &e.value())
                                },
                            }
                            div { class: if table.kind(c) == Kind::Checkbox { "value-cell check-cell" } else { "value-cell" },
                                CellBody {
                                    page_id: page.clone(),
                                    id: block.clone(),
                                    r,
                                    c,
                                    text: table.cell(r, c).to_string(),
                                    shown: if table.kind(c).is_worked_out() { table.shown(r, c) } else { String::new() },
                                    heading: table.heading(c),
                                    column: table.column(c),
                                    hint: table.rolled_up(r, c).unwrap_or_else(|| "Empty".to_string()),
                                    share: share_of(&table, r, c, fulls[c]),
                                    menu_open: cell_menu == Some((r, c)),
                                    own_menu: true,
                                    from: table.worked_out_from(c),
                                }
                            }
                        }
                    }
                    if !locked {
                        button { class: "row-value add", "data-add-value": "1", onclick: move |_| state.add_table_column(&p_add, &b_add, Kind::Text),
                            span { class: "value-kind", Icon { name: "plus", size: 15 } }
                            span { "Add a column" }
                        }
                    }
                }
            }
        }
        None => {
            let cells = table.template.as_ref().map(|t| t.cells.clone()).unwrap_or_default();
            rsx! {
                div { class: "row-values", "data-row-values": "{block}",
                    p { class: "values-note", "A new row of this table starts with what you put here, and its page with the lines below." }
                    for c in (0..table.columns()).filter(|&c| !table.kind(c).is_worked_out()) {
                        div { key: "{c}", class: "row-value",
                            span { class: "value-kind", Icon { name: table.kind(c).icon(), size: 15 } }
                            span { class: "value-name", "{table.heading(c)}" }
                            div { class: "value-cell",
                                input {
                                    class: "cell",
                                    r#type: "text",
                                    placeholder: "Empty",
                                    "aria-label": "{table.heading(c)} of a new row",
                                    value: "{cells.get(c).cloned().unwrap_or_default()}",
                                    oninput: {
                                        let (p, b) = (page.clone(), block.clone());
                                        move |e: FormEvent| state.set_template_cell(&p, &b, c, &e.value())
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_of_the_headings_picks_all_rows_that_show_or_none() {
        let order = [3, 1, 2];
        assert!(all_of(&[1, 2, 3], &order));
        assert!(all_of(&[2, 3, 1, 4], &order));
        assert!(!all_of(&[1, 2], &order));
        // A table with no row that shows has nothing to pick.
        assert!(!all_of(&[], &[]));
    }

    #[test]
    fn a_phone_number_is_called_by_its_digits() {
        assert_eq!(phone_url("+1 (555) 010-2000").as_deref(), Some("tel:+15550102000"));
        assert_eq!(phone_url(" 020 7946 0958 ").as_deref(), Some("tel:02079460958"));
        // Words, a short number and a link are no phone numbers.
        assert_eq!(phone_url("call Sam"), None);
        assert_eq!(phone_url("12"), None);
        assert_eq!(phone_url("tel:123; rm"), None);
        assert_eq!(phone_url(""), None);
    }

    #[test]
    fn a_click_picks_one_and_shift_or_command_pick_more() {
        // The rows as the screen shows them (a sort): 4 is first, 1 is last.
        let order = [4, 2, 3, 5, 1];
        let alone = pick_in(vec![2, 3], 5, Pick::Alone, &order);
        assert_eq!(alone, [5]);
        // ⌘: one more, and the same again lets it go.
        let more = pick_in(alone, 2, Pick::Toggle, &order);
        assert_eq!(more, [5, 2]);
        assert_eq!(pick_in(more.clone(), 5, Pick::Toggle, &order), [2]);
        // ⇧: all that show from the one picked last to the one clicked.
        let range = pick_in(more, 1, Pick::Range, &order);
        assert_eq!(range, [5, 2, 3, 1]);
        // The next range starts from the one clicked last, upwards too.
        assert_eq!(pick_in(vec![3], 4, Pick::Range, &order), [3, 2, 4]);
        // With nothing picked, ⇧ picks the one clicked.
        assert_eq!(pick_in(Vec::new(), 3, Pick::Range, &order), [3]);
        // One that does not show (a filter) is no end of a range: it is picked alone.
        assert_eq!(pick_in(vec![2], 9, Pick::Range, &order), [2, 9]);
    }
}
