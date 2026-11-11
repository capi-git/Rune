//! A table in a page: its cells, the kind of each column (an icon in the
//! heading and a small menu to change it), the totals under it, and a
//! button that adds a row for today when it has a date column.

use super::icons::Icon;
use crate::state::{AppState, Popover};
use crate::table::{format_day, Column, Kind, Table};
use dioxus::prelude::*;

/// A table: the first row is its header. Tab goes to the next cell and
/// Return to the cell below (a new row at the end); the small buttons above
/// it, shown while a cell has the caret, add and take out the row or column
/// of that cell (see the page script).
#[component]
pub fn TableView(page_id: String, id: String, data: String) -> Element {
    let state = use_context::<AppState>();
    let table = Table::parse(&data);
    let typed = table.is_typed();
    let open_menu = match state.ui.read().popover.clone() {
        Some(Popover::TableColumn { block_id, column }) if block_id == id => Some(column),
        _ => None,
    };
    let today = crate::when::today();
    let has_date = table.first_of(Kind::Date).is_some();
    let columns = table.columns();
    let totals: Vec<(usize, bool, Option<(&'static str, String)>)> = (0..columns).map(|c| (c, table.kind(c).is_amount(), table.total(c))).collect();
    let (today_page, today_id) = (page_id.clone(), id.clone());
    rsx! {
        div { class: "table-wrap",
            div {
                class: match (typed, open_menu.is_some()) {
                    (true, true) => "table-block typed menu-open",
                    (true, false) => "table-block typed",
                    (false, true) => "table-block menu-open",
                    (false, false) => "table-block",
                },
                "data-table": "{id}",
                "data-page": "{page_id}",
                div { class: "table-tools",
                    button { class: "ghost-btn small", "data-table-op": "add-row", title: "Add a row below this one", "Add row" }
                    button { class: "ghost-btn small", "data-table-op": "add-column", title: "Add a column after this one", "Add column" }
                    button { class: "ghost-btn small", "data-table-op": "remove-row", title: "Take this row out", "Delete row" }
                    button { class: "ghost-btn small", "data-table-op": "remove-column", title: "Take this column out", "Delete column" }
                }
                table {
                    tbody {
                        for (r, row) in table.rows.iter().enumerate() {
                            tr { key: "{r}",
                                for (c, cell) in row.iter().enumerate() {
                                    Cell {
                                        key: "{c}",
                                        page_id: page_id.clone(),
                                        id: id.clone(),
                                        r,
                                        c,
                                        text: cell.clone(),
                                        shown: table.shown(r, c),
                                        heading: table.heading(c),
                                        column: table.column(c),
                                        typed,
                                        menu_open: r == 0 && open_menu == Some(c),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if table.has_totals() {
                table { class: "table-totals", "aria-label": "Totals",
                    tbody {
                        tr {
                            for (c, amount, total) in totals {
                                td { key: "{c}", class: if amount { "num" } else { "" },
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
            if has_date {
                button {
                    class: "table-today",
                    title: "Add a row with today's date",
                    onclick: move |_| state.add_today_row(&today_page, &today_id),
                    Icon { name: "plus", size: 14 }
                    span { "New row for today" }
                    span { class: "table-today-day", "{format_day(today, today)}" }
                }
            }
        }
    }
}

/// One cell. In the header: its words, the icon of its kind and the button
/// of its menu. Under it, by kind: a box to tick, a pace worked out, a tag
/// for a choice, or words to type.
#[component]
fn Cell(page_id: String, id: String, r: usize, c: usize, text: String, shown: String, heading: String, column: Column, typed: bool, menu_open: bool) -> Element {
    let state = use_context::<AppState>();
    let kind = column.kind;
    let (pid, bid) = (page_id.clone(), id.clone());
    let typing = move |e: FormEvent| state.set_table_cell(&pid, &bid, r, c, &e.value());
    if r == 0 {
        let menu_block = id.clone();
        return rsx! {
            td { class: if menu_open { "head menu-open" } else { "head" },
                div { class: "th",
                    if typed {
                        span { class: "th-kind", title: "{kind.label()}", Icon { name: kind.icon(), size: 13 } }
                    }
                    textarea {
                        class: "cell",
                        rows: "1",
                        spellcheck: false,
                        "data-r": "0",
                        "data-c": "{c}",
                        "aria-label": "Heading of column {c + 1}",
                        value: "{text}",
                        oninput: typing,
                    }
                    button {
                        class: if menu_open { "th-menu open" } else { "th-menu" },
                        title: "Kind and total of this column",
                        "aria-label": "Kind and total of {heading}",
                        "aria-haspopup": "menu",
                        "aria-expanded": if menu_open { "true" } else { "false" },
                        onclick: move |e| {
                            e.stop_propagation();
                            state.set_popover((!menu_open).then(|| Popover::TableColumn { block_id: menu_block.clone(), column: c }));
                        },
                        Icon { name: "chevron-down", size: 12 }
                    }
                }
                if menu_open {
                    ColumnMenu { page_id: page_id.clone(), id: id.clone(), c, column: column.clone() }
                }
            }
        };
    }
    let label = format!("Row {}, {}", r + 1, heading);
    match kind {
        Kind::Checkbox => {
            let on = crate::table::checked(&text);
            rsx! {
                td { class: "check-cell",
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
        }
        Kind::Pace => rsx! {
            td { class: "num",
                textarea {
                    class: "cell computed",
                    rows: "1",
                    readonly: true,
                    tabindex: "-1",
                    "data-r": "{r}",
                    "data-c": "{c}",
                    "aria-label": "{label}, worked out from the distance and the time",
                    value: "{shown}",
                }
            }
        },
        _ => rsx! {
            td { class: if kind.is_amount() { "num" } else { "" },
                textarea {
                    class: match (kind, text.trim().is_empty()) {
                        (Kind::Choice, false) => "cell choice",
                        _ => "cell",
                    },
                    rows: "1",
                    spellcheck: kind == Kind::Text,
                    "data-r": "{r}",
                    "data-c": "{c}",
                    "aria-label": "{label}",
                    value: "{text}",
                    oninput: typing,
                }
            }
        },
    }
}

/// The menu of a column: the kinds it can be, the total at its foot, and
/// (for numbers) its unit. Rune's own menu: plain rows, the chosen one grey.
#[component]
fn ColumnMenu(page_id: String, id: String, c: usize, column: Column) -> Element {
    let state = use_context::<AppState>();
    let (p_unit, b_unit) = (page_id.clone(), id.clone());
    let unit = column.unit.clone().unwrap_or_default();
    rsx! {
        div { class: "popover column-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            div { class: "ai-menu-label", "Type" }
            for kind in Kind::ALL {
                button {
                    key: "{kind.name()}",
                    class: if column.kind == kind { "ai-menu-row active" } else { "ai-menu-row" },
                    role: "menuitemradio",
                    "aria-checked": if column.kind == kind { "true" } else { "false" },
                    "data-kind": "{kind.name()}",
                    onclick: {
                        let (p, b) = (page_id.clone(), id.clone());
                        move |_| state.set_column_kind(&p, &b, c, kind)
                    },
                    span { class: "ai-menu-value", Icon { name: kind.icon(), size: 14 } "{kind.label()}" }
                    if kind == Kind::Pace {
                        span { class: "ai-menu-aside", "from distance and time" }
                    }
                }
            }
            div { class: "ai-menu-sep" }
            div { class: "ai-menu-label", "Total at the foot" }
            button {
                class: if column.total.is_none() { "ai-menu-row active" } else { "ai-menu-row" },
                role: "menuitemradio",
                "aria-checked": if column.total.is_none() { "true" } else { "false" },
                onclick: {
                    let (p, b) = (page_id.clone(), id.clone());
                    move |_| state.set_column_total(&p, &b, c, None)
                },
                span { class: "ai-menu-value", "None" }
            }
            for total in column.kind.totals().iter().copied() {
                button {
                    key: "{total.name()}",
                    class: if column.total == Some(total) { "ai-menu-row active" } else { "ai-menu-row" },
                    role: "menuitemradio",
                    "aria-checked": if column.total == Some(total) { "true" } else { "false" },
                    "data-total": "{total.name()}",
                    onclick: {
                        let (p, b) = (page_id.clone(), id.clone());
                        move |_| state.set_column_total(&p, &b, c, Some(total))
                    },
                    span { class: "ai-menu-value", "{total.label()}" }
                }
            }
            if column.kind == Kind::Number {
                div { class: "ai-menu-sep" }
                label { class: "column-unit",
                    span { "Unit" }
                    input {
                        r#type: "text",
                        placeholder: "km",
                        "aria-label": "Unit",
                        value: "{unit}",
                        onchange: move |e: FormEvent| state.set_column_unit(&p_unit, &b_unit, c, &e.value()),
                    }
                }
            }
        }
    }
}
