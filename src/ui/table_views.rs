//! The other layouts of a table: the same rows as a board (a column of
//! cards for each value of one column), a list, a gallery of cards, or a
//! month with each row on its day. A click on a row opens it as a page.

use super::table_choices::tag_class;
use super::icons::Icon;
use super::tables::group_label;
use crate::state::{AppState, Open};
use crate::table::{checked, parts, Kind, Table};
use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;
use std::collections::HashMap;

/// What a card or a line says of a row besides its name: what the first
/// columns that show have for it (not the names, not the column `but`, not
/// pictures; a ticked box says the name of its column).
fn card_facts(table: &Table, r: usize, group: usize) -> Vec<String> {
    table.shown_columns().into_iter().filter(|&c| c != 0 && c != group && !matches!(table.kind(c), Kind::Checkbox | Kind::File)).map(|c| table.shown(r, c).trim().to_string()).filter(|text| !text.is_empty()).take(2).collect()
}

/// What a row says besides its name, in a few short words.
fn facts(table: &Table, r: usize, but: Option<usize>, most: usize) -> Vec<String> {
    table
        .shown_columns()
        .into_iter()
        .filter(|&c| c != 0 && Some(c) != but)
        .filter_map(|c| {
            let text = match table.kind(c) {
                Kind::File => String::new(),
                Kind::Checkbox if checked(table.cell(r, c)) => table.heading(c),
                Kind::Checkbox => String::new(),
                _ => table.shown(r, c).trim().to_string(),
            };
            (!text.is_empty()).then_some(text)
        })
        .take(most)
        .collect()
}

/// The name of a row, as a card shows it.
fn name_of(table: &Table, r: usize) -> String {
    match table.title(r) {
        "" => "Untitled".to_string(),
        name => name.to_string(),
    }
}

/// The colour a rule gives a row, as a style.
fn tint(table: &Table, r: usize) -> String {
    table.color(r).map(|color| format!("--row-tint: var(--icon-{color}-soft);")).unwrap_or_default()
}

/// The totals of a table, in one line under a layout that has no columns
/// to put them under.
#[component]
fn Summary(table: Table) -> Element {
    let totals: Vec<String> = table
        .shown_columns()
        .into_iter()
        .filter(|&c| table.kind(c) != Kind::Checkbox)
        .filter_map(|c| {
            table.total(c).map(|(word, value)| match word {
                "" => value,
                word => format!("{word} {value}"),
            })
        })
        .collect();
    // How many rows there are comes first, as a count of the rows would.
    let rows = table.counted().iter().filter(|&&r| !table.is_blank(r)).count();
    let count = if rows == 1 { "1 row".to_string() } else { format!("{rows} rows") };
    let mut totals = totals;
    if rows > 0 && !totals.iter().any(|t| t.ends_with(" row") || t.ends_with(" rows")) {
        totals.insert(0, count);
    }
    if totals.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "table-summary", "aria-label": "Totals", "{totals.join(\"  ·  \")}" }
    }
}

/// A layout that cannot show the table yet says what it needs.
#[component]
fn Needs(page_id: String, id: String, words: String, kind: Kind) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "table-needs",
            span { "{words}" }
            button { class: "ghost-btn small", onclick: move |_| state.add_table_column(&page_id, &id, kind),
                Icon { name: "plus", size: 13 }
                span { "Add a {kind.label().to_lowercase()} column" }
            }
        }
    }
}

/// A column of cards for each value of one column (the group column, or
/// the first choice column). A card dragged to another column takes that
/// column's value (see the page script).
#[component]
pub fn Board(page_id: String, id: String, table: Table, order: Vec<usize>) -> Element {
    let state = use_context::<AppState>();
    let Some(g) = table.board_column() else {
        return rsx! { Needs { page_id, id, words: "A board makes a column of cards for each value of a choice column.", kind: Kind::Choice } };
    };
    let (kind, heading) = (table.kind(g), table.heading(g));
    let mut groups = table.groups(&order, g);
    if groups.is_empty() {
        groups.push((String::new(), Vec::new()));
    }
    let amount = (0..table.columns()).find(|&c| table.kind(c) == Kind::Number && c != g);
    rsx! {
        div { class: "table-board", "data-board": "{id}", "data-page": "{page_id}", "data-group": "{g}",
            for (value, rows) in groups {
                div { key: "{value}", class: "board-col", "data-value": "{value}",
                    div { class: "board-head",
                        span { class: "{tag_class(&table.column(g), &value)}", "{group_label(kind, &heading, &value)}" }
                        span { class: "board-count", "{rows.len()}" }
                        if let Some(c) = amount.filter(|_| !rows.is_empty()) {
                            span { class: "board-sum", "{table.format(c, rows.iter().filter_map(|&r| table.value(r, c)).sum())}" }
                        }
                    }
                    for r in rows {
                        button {
                            key: "{r}",
                            class: if table.color(r).is_some() { "board-card painted" } else { "board-card" },
                            style: "{tint(&table, r)}",
                            draggable: "true",
                            "data-card": "{r}",
                            onclick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |e: MouseEvent| state.open_row(&p, &b, r, if e.modifiers().meta() { Open::Tab } else { Open::Auto })
                            },
                            b { "{name_of(&table, r)}" }
                            span { class: "card-facts",
                                for (i, fact) in card_facts(&table, r, g).into_iter().enumerate() {
                                    span { key: "{i}", "{fact}" }
                                }
                            }
                        }
                    }
                    button {
                        class: "board-new",
                        "data-new-card": "1",
                        onclick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            // (A new card in the column of ticked boxes is ticked.)
                            let value = if kind == Kind::Checkbox && checked(&value) { "x".to_string() } else { value.clone() };
                            move |_| state.new_card(&p, &b, &[(g, value.clone())])
                        },
                        Icon { name: "plus", size: 13 }
                        span { "New" }
                    }
                }
            }
        }
        Summary { table: table.clone() }
    }
}

/// One line for each row: its name, and what its first columns say.
#[component]
pub fn List(page_id: String, id: String, table: Table, order: Vec<usize>) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "table-list",
            for r in order {
                button {
                    key: "{r}",
                    class: if table.color(r).is_some() { "list-row painted" } else { "list-row" },
                    style: "{tint(&table, r)} padding-left: {8 + table.shown_indent(r) as usize * 18}px;",
                    "data-card": "{r}",
                    onclick: {
                        let (p, b) = (page_id.clone(), id.clone());
                        move |e: MouseEvent| state.open_row(&p, &b, r, if e.modifiers().meta() { Open::Tab } else { Open::Auto })
                    },
                    Icon { name: "file", size: 15 }
                    span { class: "list-name", "{name_of(&table, r)}" }
                    for (i, fact) in facts(&table, r, None, 4).into_iter().enumerate() {
                        span { key: "{i}", class: "list-fact", "{fact}" }
                    }
                }
            }
        }
        Summary { table: table.clone() }
    }
}

/// A card for each row: its first picture if it has one, its name, and what
/// its first columns say.
#[component]
pub fn Gallery(page_id: String, id: String, table: Table, order: Vec<usize>) -> Element {
    let state = use_context::<AppState>();
    let pictures = table.first_of(Kind::File);
    rsx! {
        div { class: "table-gallery",
            for r in order {
                button {
                    key: "{r}",
                    class: if table.color(r).is_some() { "gallery-card painted" } else { "gallery-card" },
                    style: "{tint(&table, r)}",
                    "data-card": "{r}",
                    onclick: {
                        let (p, b) = (page_id.clone(), id.clone());
                        move |e: MouseEvent| state.open_row(&p, &b, r, if e.modifiers().meta() { Open::Tab } else { Open::Auto })
                    },
                    if let Some(name) = pictures.and_then(|c| parts(table.cell(r, c)).into_iter().find(|name| crate::files::valid_name(name))) {
                        img { class: "gallery-pic", src: "{crate::files::url(&name)}", alt: "", draggable: "false" }
                    }
                    b { "{name_of(&table, r)}" }
                    for (i, fact) in facts(&table, r, None, 3).into_iter().enumerate() {
                        span { key: "{i}", "{fact}" }
                    }
                }
            }
        }
        Summary { table: table.clone() }
    }
}

/// The first day of the month `shift` months from the month of `day`.
fn month_start(day: NaiveDate, shift: i32) -> NaiveDate {
    let months = day.year() * 12 + day.month0() as i32 + shift;
    NaiveDate::from_ymd_opt(months.div_euclid(12), months.rem_euclid(12) as u32 + 1, 1).unwrap_or(day)
}

/// A month, with each row on the day of its first date column. The arrows
/// go to the months beside it; the "+" of a day adds a row for that day.
#[component]
pub fn Calendar(page_id: String, id: String, table: Table, order: Vec<usize>) -> Element {
    let state = use_context::<AppState>();
    let mut shift = use_signal(|| 0i32);
    let Some(dc) = table.first_of(Kind::Date) else {
        return rsx! { Needs { page_id, id, words: "A calendar puts each row on the day of its date column.", kind: Kind::Date } };
    };
    let today = crate::when::today();
    let first = month_start(today, shift());
    // Six weeks from the Sunday on or before the first of the month.
    let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
    let mut on_day: HashMap<NaiveDate, Vec<usize>> = HashMap::new();
    for &r in &order {
        if let Some(day) = table.day(r, dc, today) {
            on_day.entry(day).or_default().push(r);
        }
    }
    let weeks = if (start + Duration::days(35)).month() == first.month() { 6 } else { 5 };
    let days: Vec<(NaiveDate, Vec<usize>)> = (0..weeks * 7).map(|i| start + Duration::days(i)).map(|day| (day, on_day.remove(&day).unwrap_or_default())).collect();
    rsx! {
        div { class: "table-month", "data-month": "{first.format(\"%Y-%m\")}",
            div { class: "month-bar",
                span { class: "month-name", "{first.format(\"%B %Y\")}" }
                if shift() != 0 {
                    button { class: "ghost-btn small", onclick: move |_| shift.set(0), "Today" }
                }
                button { class: "table-more", title: "The month before", "aria-label": "The month before", "data-month-step": "-1", onclick: move |_| shift -= 1, Icon { name: "chevron-left", size: 14 } }
                button { class: "table-more", title: "The month after", "aria-label": "The month after", "data-month-step": "1", onclick: move |_| shift += 1, Icon { name: "chevron-right", size: 14 } }
            }
            div { class: "month-grid",
                for name in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] {
                    span { key: "{name}", class: "month-weekday", "{name}" }
                }
                for (day, rows) in days {
                    div {
                        key: "{day}",
                        class: match (day.month() == first.month(), day == today) {
                            (true, true) => "month-day today",
                            (true, false) => "month-day",
                            (false, _) => "month-day other",
                        },
                        "data-day": "{day}",
                        span { class: "day-number", "{day.day()}" }
                        button {
                            class: "day-add",
                            title: "Add a row for this day",
                            "aria-label": "Add a row for {day.format(\"%-d %B\")}",
                            onclick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.new_card(&p, &b, &[(dc, day.format("%Y-%m-%d").to_string())])
                            },
                            Icon { name: "plus", size: 11 }
                        }
                        for r in rows {
                            button {
                                key: "{r}",
                                class: if table.color(r).is_some() { "day-row painted" } else { "day-row" },
                                style: "{tint(&table, r)}",
                                title: "{name_of(&table, r)}",
                                "data-card": "{r}",
                                onclick: {
                                    let (p, b) = (page_id.clone(), id.clone());
                                    move |e: MouseEvent| state.open_row(&p, &b, r, if e.modifiers().meta() { Open::Tab } else { Open::Auto })
                                },
                                "{name_of(&table, r)}"
                            }
                        }
                    }
                }
            }
        }
        Summary { table: table.clone() }
    }
}
