//! The menus of a table: of a column, of the table, of its rows, of a chip
//! (a filter or a sort), its colour rules, its icon, the kinds of a new
//! column, and the rows a relation cell can name. Rune's own menus: plain
//! rows, the chosen one grey, a second list beside the first for a choice.

use super::icons::{format_icon, parse_icon, Icon, ICON_COLORS};
use crate::state::{AppState, Open, Popover};
use crate::table::money::{self, CURRENCIES};
use crate::table::{holds_part, Kind, Layout, Rule, Show, Table};
use dioxus::prelude::*;

/// How many kinds of money the list of formats shows at first.
const MAIN_CURRENCIES: usize = 4;

/// One line of a menu.
#[component]
fn MenuRow(
    #[props(default, into)] icon: String,
    label: String,
    #[props(default)] aside: String,
    #[props(default)] active: bool,
    /// It opens a second list.
    #[props(default)]
    more: bool,
    /// How a test finds it.
    #[props(default)]
    name: String,
    /// Its words are a tag of this colour.
    #[props(default, into)]
    tag: String,
    onpick: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: if active { "ai-menu-row active" } else { "ai-menu-row" },
            role: "menuitem",
            "data-item": "{name}",
            onclick: move |e| {
                e.stop_propagation();
                onpick.call(());
            },
            span { class: "ai-menu-value",
                if !icon.is_empty() {
                    Icon { name: icon.clone(), size: 14 }
                }
                if tag.is_empty() {
                    "{label}"
                } else {
                    span { class: "tag c-{tag}", span { class: "tag-words", "{label}" } }
                }
            }
            if more || !aside.is_empty() {
                span { class: "ai-menu-aside",
                    "{aside}"
                    if more {
                        Icon { name: "chevron-right", size: 12 }
                    }
                }
            }
        }
    }
}

/// The menu of a column: what it holds (its kind, format, total), how the
/// rows are ordered and filtered by it, and the column itself (hide, insert,
/// copy, delete). A locked table shows only what changes the view.
#[component]
pub fn ColumnMenu(page_id: String, id: String, c: usize, table: Table) -> Element {
    let state = use_context::<AppState>();
    let mut sub = use_signal(|| "");
    let column = table.column(c);
    let kind = column.kind;
    let locked = table.view.locked;
    let numbers = matches!(kind, Kind::Number | Kind::Formula);
    let sorted = table.view.sort.iter().find(|s| s.column == c).map(|s| s.down);
    let place = table.shown_columns().iter().position(|&shown| shown == c).unwrap_or(0);
    let frozen_here = table.view.frozen == place + 1;
    let grouped = table.view.group == Some(c);
    let total = column.total.map(|t| t.label()).unwrap_or("None");
    let from_name = (!column.from.is_empty()).then(|| state.tables_to_link(&page_id, &id).into_iter().find(|(block, _)| *block == column.from).map(|(_, name)| name).unwrap_or_else(|| "Gone".to_string())).unwrap_or_else(|| "Choose".to_string());
    let error = table.formula_error(c);
    let unit = column.unit.clone().unwrap_or_default();
    let goal = column.goal.map(|g| g.to_string()).unwrap_or_default();
    let places = column.places;
    let mut all_money = use_signal(|| false);
    let mut format_row = use_signal(|| "");
    let mut open_row = move |name: &'static str| format_row.set(if format_row() == name { "" } else { name });
    // (One closure for each command of the column.)
    let op = |name: &'static str| {
        let (p, b) = (page_id.clone(), id.clone());
        move |_| state.column_op(&p, &b, c, name)
    };
    let mut toggle = move |name: &'static str| sub.set(if sub() == name { "" } else { name });
    let (p_formula, b_formula) = (page_id.clone(), id.clone());
    let (p_unit, b_unit) = (page_id.clone(), id.clone());
    let (p_goal, b_goal) = (page_id.clone(), id.clone());
    let (p_freeze, b_freeze) = (page_id.clone(), id.clone());
    let tables = if sub() == "from" { state.tables_to_link(&page_id, &id) } else { Vec::new() };
    rsx! {
        div { class: "popover column-menu table-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            if !locked {
                MenuRow { icon: kind.icon(), label: "Type", aside: kind.label(), more: true, active: sub() == "type", name: "type", onpick: move |_| toggle("type") }
                if numbers {
                    MenuRow { icon: "money", label: "Format", aside: money::format_label(&column.format), more: true, active: sub() == "format", name: "format", onpick: move |_| toggle("format") }
                }
                if kind == Kind::Relation {
                    MenuRow { icon: "table", label: "Table", aside: from_name, more: true, active: sub() == "from", name: "from", onpick: move |_| toggle("from") }
                }
                MenuRow { icon: "sigma", label: "Total", aside: total, more: true, active: sub() == "total", name: "total", onpick: move |_| toggle("total") }
                if kind == Kind::Formula {
                    label { class: "column-field",
                        span { "Formula" }
                        input {
                            r#type: "text",
                            placeholder: "Price * Qty",
                            spellcheck: false,
                            "aria-label": "Formula",
                            "data-item": "formula",
                            value: "{column.formula}",
                            oninput: move |e: FormEvent| state.set_column_formula(&p_formula, &b_formula, c, &e.value()),
                        }
                    }
                    if let Some(error) = error {
                        div { class: "column-error", role: "status", "{error}" }
                    }
                }
                if kind == Kind::Number {
                    label { class: "column-field",
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
                div { class: "ai-menu-sep" }
            }
            MenuRow { icon: "sort-up", label: "Sort {table.direction_words(c, false)}", active: sorted == Some(false), name: "sort-up", onpick: op("sort-up") }
            MenuRow { icon: "sort-down", label: "Sort {table.direction_words(c, true)}", active: sorted == Some(true), name: "sort-down", onpick: op("sort-down") }
            MenuRow { icon: "filter", label: "Filter", name: "filter", onpick: op("filter") }
            MenuRow { icon: "group", label: if grouped { "Stop grouping" } else { "Group by this" }, name: "group", onpick: op("group") }
            div { class: "ai-menu-sep" }
            if c > 0 {
                MenuRow { icon: "eye-off", label: "Hide", name: "hide", onpick: op("hide") }
            }
            MenuRow {
                icon: "freeze",
                label: if frozen_here { "Unfreeze" } else { "Freeze up to here" },
                name: "freeze",
                onpick: move |_| {
                    state.change_table_view(&p_freeze, &b_freeze, |v| v.frozen = if frozen_here { 0 } else { place + 1 });
                    state.set_popover(None);
                },
            }
            if !locked {
                MenuRow { icon: "insert-left", label: "Insert left", name: "left", onpick: op("left") }
                MenuRow { icon: "insert-right", label: "Insert right", name: "right", onpick: op("right") }
                MenuRow { icon: "duplicate", label: "Duplicate", name: "duplicate", onpick: op("duplicate") }
                if table.columns() > 1 {
                    MenuRow { icon: "trash", label: "Delete", name: "delete", onpick: op("delete") }
                }
            }
            if sub() == "type" {
                div { class: "popover column-sub", role: "menu",
                    for k in Kind::ALL {
                        MenuRow {
                            key: "{k.name()}",
                            icon: k.icon(),
                            label: k.label(),
                            aside: if k == Kind::Pace { "from distance and time" } else { "" },
                            active: k == kind,
                            name: k.name(),
                            onpick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.set_column_kind(&p, &b, c, k)
                            },
                        }
                    }
                }
            }
            if sub() == "format" {
                div { class: "popover column-sub tall", role: "menu",
                    for (format, label, sample) in [("", "Number", "1500"), (money::GROUPED, "With separators", "1,500"), (money::PERCENT, "Percent", "15%")] {
                        MenuRow {
                            key: "{label}",
                            label,
                            aside: sample,
                            active: column.format == format,
                            name: if format.is_empty() { "plain" } else { format },
                            onpick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.set_column_format(&p, &b, c, format)
                            },
                        }
                    }
                    div { class: "ai-menu-sep" }
                    for cur in CURRENCIES.iter().filter(|cur| all_money() || CURRENCIES[..MAIN_CURRENCIES].iter().any(|m| m.code == cur.code) || cur.code == column.format) {
                        MenuRow {
                            key: "{cur.code}",
                            label: cur.name,
                            aside: money::write(cur.code, 1500.0, None),
                            active: column.format == cur.code,
                            name: cur.code,
                            onpick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.set_column_format(&p, &b, c, cur.code)
                            },
                        }
                    }
                    if !all_money() {
                        MenuRow { label: "Other currency", more: true, name: "more-money", onpick: move |_| all_money.set(true) }
                    }
                    div { class: "ai-menu-sep" }
                    MenuRow { label: "Decimal places", aside: places.map_or("Auto".to_string(), |n| n.to_string()), more: true, active: format_row() == "places", name: "places", onpick: move |_| open_row("places") }
                    if format_row() == "places" {
                    div { class: "pick-chips", role: "group", "aria-label": "Places after the point",
                        for (n, label) in [(None, "Auto"), (Some(0u8), "0"), (Some(1), "1"), (Some(2), "2"), (Some(3), "3")] {
                            button {
                                key: "{label}",
                                class: if places == n { "pick on" } else { "pick" },
                                "aria-pressed": if places == n { "true" } else { "false" },
                                "data-places": "{label}",
                                onclick: {
                                    let (p, b) = (page_id.clone(), id.clone());
                                    move |_| state.set_column_places(&p, &b, c, n)
                                },
                                "{label}"
                            }
                        }
                    }
                    }
                    MenuRow {
                        label: "Green and red by sign",
                        aside: if column.signed { "On".to_string() } else { "Off".to_string() },
                        active: column.signed,
                        name: "signed",
                        onpick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            let on = !column.signed;
                            move |_| state.set_column_signed(&p, &b, c, on)
                        },
                    }
                    MenuRow { label: "Show as", aside: column.show.label(), more: true, active: format_row() == "show", name: "show-as", onpick: move |_| open_row("show") }
                    if format_row() == "show" {
                    div { class: "pick-chips", role: "group", "aria-label": "Show as",
                        for show in Show::ALL {
                            button {
                                key: "{show.label()}",
                                class: if column.show == show { "pick on" } else { "pick" },
                                "aria-pressed": if column.show == show { "true" } else { "false" },
                                "data-show": "{show.label()}",
                                onclick: {
                                    let (p, b) = (page_id.clone(), id.clone());
                                    move |_| state.set_column_show(&p, &b, c, show)
                                },
                                "{show.label()}"
                            }
                        }
                    }
                    if column.show != Show::Number {
                        label { class: "column-field",
                            span { "Full at" }
                            input {
                                r#type: "text",
                                placeholder: "the highest",
                                "aria-label": "The amount at which it is full",
                                value: "{goal}",
                                onchange: move |e: FormEvent| state.set_column_goal(&p_goal, &b_goal, c, &e.value()),
                            }
                        }
                    }
                    }
                }
            }
            if sub() == "total" {
                div { class: "popover column-sub", role: "menu",
                    MenuRow {
                        label: "None",
                        active: column.total.is_none(),
                        name: "none",
                        onpick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            move |_| state.set_column_total(&p, &b, c, None)
                        },
                    }
                    for t in kind.totals().iter().copied() {
                        MenuRow {
                            key: "{t.name()}",
                            label: t.label(),
                            active: column.total == Some(t),
                            name: t.name(),
                            onpick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.set_column_total(&p, &b, c, Some(t))
                            },
                        }
                    }
                }
            }
            if sub() == "from" {
                div { class: "popover column-sub tall", role: "menu",
                    if tables.is_empty() {
                        div { class: "ai-menu-label", "There is no other table in this space yet." }
                    }
                    for (block, name) in tables {
                        MenuRow {
                            key: "{block}",
                            icon: "table",
                            label: name,
                            active: column.from == block,
                            onpick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.set_column_from(&p, &b, c, &block)
                            },
                        }
                    }
                }
            }
        }
    }
}

/// The kinds a new column can be, in two columns.
#[component]
pub fn KindsMenu(page_id: String, id: String) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "popover table-menu kinds-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            div { class: "ai-menu-label", "Kind of column" }
            div { class: "kinds-grid",
                for kind in Kind::ALL {
                    MenuRow {
                        key: "{kind.name()}",
                        icon: kind.icon(),
                        label: kind.label(),
                        name: kind.name(),
                        onpick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            move |_| state.add_table_column(&p, &b, kind)
                        },
                    }
                }
            }
        }
    }
}

/// The menu of a table: its name, its layout, which columns show, its
/// filters, sorts, grouping and colours, how rows open, the template of new
/// rows, its place beside another table, its lock; and the table itself.
#[component]
pub fn TableMenu(page_id: String, id: String, table: Table) -> Element {
    let state = use_context::<AppState>();
    let mut sub = use_signal(|| "");
    let view = table.view.clone();
    let locked = view.locked;
    let layout = view.layout;
    let columns: Vec<(usize, String, &'static str)> = (0..table.columns()).map(|c| (c, table.heading(c), table.kind(c).icon())).collect();
    let shown = table.shown_columns().len();
    let group = view.group.map(|c| table.heading(c)).unwrap_or_else(|| "None".to_string());
    let rules = view.paint.len();
    let beside = state.blocks.peek().get(&page_id).and_then(|bs| bs.iter().find(|b| b.id == id).map(|b| b.place.beside)).unwrap_or(false);
    let can_beside = !beside && state.can_stand_beside(&page_id, &id);
    let has_template = table.template.is_some();
    let mut toggle = move |name: &'static str| sub.set(if sub() == name { "" } else { name });
    // (One closure for each thing that changes the view and closes the menu.)
    let (p, b) = (page_id.clone(), id.clone());
    let (p_name, b_name) = (page_id.clone(), id.clone());
    let b_icon = id.clone();
    let (p_paint, b_paint) = (page_id.clone(), id.clone());
    let (p_template, b_template) = (page_id.clone(), id.clone());
    let (p_no_template, b_no_template) = (page_id.clone(), id.clone());
    let (p_beside, b_beside) = (page_id.clone(), id.clone());
    let (p_lock, b_lock) = (page_id.clone(), id.clone());
    let (p_copy, b_copy) = (page_id.clone(), id.clone());
    let (p_delete, b_delete) = (page_id.clone(), id.clone());
    rsx! {
        div { class: "popover table-menu own-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            if !locked {
                input {
                    class: "menu-name",
                    r#type: "text",
                    placeholder: "Name this table",
                    spellcheck: false,
                    "aria-label": "Name of the table",
                    "data-item": "name",
                    value: "{table.name}",
                    oninput: move |e: FormEvent| state.set_table_name(&p_name, &b_name, &e.value()),
                }
            }
            if !locked {
                MenuRow { icon: "smile", label: "Icon", name: "icon", onpick: move |_| state.set_popover(Some(Popover::TableIcon { block_id: b_icon.clone() })) }
            }
            MenuRow { icon: layout.icon(), label: "View", aside: layout.label(), more: true, active: sub() == "view", name: "view", onpick: move |_| toggle("view") }
            MenuRow { icon: "eye", label: "Columns", aside: "{shown} shown", more: true, active: sub() == "columns", name: "columns", onpick: move |_| toggle("columns") }
            MenuRow { icon: "filter", label: "Filter", more: true, active: sub() == "filter", name: "filter", onpick: move |_| toggle("filter") }
            MenuRow { icon: "sort", label: "Sort", more: true, active: sub() == "sort", name: "sort", onpick: move |_| toggle("sort") }
            MenuRow { icon: "group", label: "Group", aside: group, more: true, active: sub() == "group", name: "group", onpick: move |_| toggle("group") }
            MenuRow {
                icon: "palette",
                label: "Colours",
                aside: match rules {
                    0 => String::new(),
                    1 => "1 rule".to_string(),
                    n => format!("{n} rules"),
                },
                name: "paint",
                onpick: move |_| {
                    if rules == 0 {
                        state.add_table_paint(&p_paint, &b_paint);
                    }
                    state.set_popover(Some(Popover::TablePaint { block_id: b_paint.clone() }));
                },
            }
            div { class: "ai-menu-sep" }
            MenuRow { icon: "side", label: "Open rows", aside: if view.full { "As a page" } else { "At the side" }, more: true, active: sub() == "open", name: "open", onpick: move |_| toggle("open") }
            MenuRow { icon: "template", label: "Template for new rows", name: "template", onpick: move |_| state.edit_row_template(&p_template, &b_template) }
            if has_template {
                MenuRow { icon: "close", label: "Remove the template", name: "no-template", onpick: move |_| state.remove_row_template(&p_no_template, &b_no_template) }
            }
            if beside || can_beside {
                MenuRow {
                    icon: "side",
                    label: if beside { "On a line of its own" } else { "Beside the table above" },
                    name: "beside",
                    onpick: move |_| state.set_table_beside(&p_beside, &b_beside, !beside),
                }
            }
            MenuRow {
                icon: "lock",
                label: if locked { "Unlock" } else { "Lock" },
                aside: if locked { "" } else { "columns and name" },
                name: "lock",
                onpick: move |_| {
                    state.change_table_view(&p_lock, &b_lock, |v| v.locked = !locked);
                    state.set_popover(None);
                },
            }
            div { class: "ai-menu-sep" }
            MenuRow {
                icon: "duplicate",
                label: "Duplicate",
                name: "duplicate",
                onpick: move |_| {
                    state.duplicate_block(&p_copy, &b_copy);
                    state.set_popover(None);
                },
            }
            MenuRow { icon: "trash", label: "Delete", name: "delete", onpick: move |_| state.delete_block(&p_delete, &b_delete) }
            if sub() == "view" {
                div { class: "popover column-sub", role: "menu",
                    for l in Layout::ALL {
                        MenuRow {
                            key: "{l.name()}",
                            icon: l.icon(),
                            label: l.label(),
                            active: l == layout,
                            name: l.name(),
                            onpick: {
                                let (p, b) = (p.clone(), b.clone());
                                move |_| {
                                    state.change_table_view(&p, &b, |v| v.layout = l);
                                    state.set_popover(None);
                                }
                            },
                        }
                    }
                }
            }
            if sub() == "columns" {
                div { class: "popover column-sub tall", role: "menu",
                    for (c, heading, icon) in columns.clone() {
                        MenuRow {
                            key: "{c}",
                            icon,
                            label: heading,
                            aside: if c == 0 { "always" } else if view.hidden.contains(&c) { "hidden" } else { "shown" },
                            active: !view.hidden.contains(&c),
                            name: "column-{c}",
                            onpick: {
                                let (p, b) = (p.clone(), b.clone());
                                move |_| state.change_table_view(&p, &b, |v| v.toggle_hidden(c))
                            },
                        }
                    }
                }
            }
            if sub() == "filter" || sub() == "sort" {
                div { class: "popover column-sub tall", role: "menu",
                    for (c, heading, icon) in columns.clone() {
                        MenuRow {
                            key: "{c}",
                            icon,
                            label: heading,
                            name: "column-{c}",
                            onpick: {
                                let (p, b) = (p.clone(), b.clone());
                                let sort = sub() == "sort";
                                move |_| {
                                    if sort {
                                        state.change_table_view(&p, &b, |v| v.sort_by(c, false));
                                        state.set_popover(None);
                                    } else {
                                        state.add_table_filter(&p, &b, c);
                                    }
                                }
                            },
                        }
                    }
                }
            }
            if sub() == "group" {
                div { class: "popover column-sub tall", role: "menu",
                    MenuRow {
                        label: "None",
                        active: view.group.is_none(),
                        name: "none",
                        onpick: {
                            let (p, b) = (p.clone(), b.clone());
                            move |_| state.change_table_view(&p, &b, |v| v.group = None)
                        },
                    }
                    for (c, heading, icon) in columns.clone() {
                        MenuRow {
                            key: "{c}",
                            icon,
                            label: heading,
                            active: view.group == Some(c),
                            name: "column-{c}",
                            onpick: {
                                let (p, b) = (p.clone(), b.clone());
                                move |_| state.change_table_view(&p, &b, |v| v.group = Some(c))
                            },
                        }
                    }
                }
            }
            if sub() == "open" {
                div { class: "popover column-sub", role: "menu",
                    for (full, label, icon) in [(false, "At the side", "side"), (true, "As a full page", "expand")] {
                        MenuRow {
                            key: "{label}",
                            icon,
                            label,
                            active: view.full == full,
                            name: if full { "full" } else { "side" },
                            onpick: {
                                let (p, b) = (p.clone(), b.clone());
                                move |_| state.change_table_view(&p, &b, |v| v.full = full)
                            },
                        }
                    }
                }
            }
        }
    }
}

/// What a chip asks, open to change: for a filter, how the cell is compared
/// and with what; for a sort, its direction. And a line to take it away.
#[component]
pub fn ChipMenu(page_id: String, id: String, table: Table, sort: bool, index: usize) -> Element {
    let state = use_context::<AppState>();
    let (p, b) = (page_id.clone(), id.clone());
    let remove = move |_| {
        state.change_table_view(&p, &b, |v| {
            if sort && index < v.sort.len() {
                v.sort.remove(index);
            } else if !sort && index < v.filter.len() {
                v.filter.remove(index);
            }
        });
        state.set_popover(None);
    };
    if sort {
        let Some(now) = table.view.sort.get(index).cloned() else { return rsx! {} };
        return rsx! {
            div { class: "popover table-menu chip-menu", role: "menu", onclick: move |e| e.stop_propagation(),
                for down in [false, true] {
                    MenuRow {
                        key: "{down}",
                        icon: if down { "sort-down" } else { "sort-up" },
                        label: table.direction_words(now.column, down),
                        active: now.down == down,
                        name: if down { "down" } else { "up" },
                        onpick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            move |_| state.change_table_view(&p, &b, |v| {
                                if let Some(s) = v.sort.get_mut(index) {
                                    s.down = down;
                                }
                            })
                        },
                    }
                }
                div { class: "ai-menu-sep" }
                MenuRow { icon: "trash", label: "Remove", name: "remove", onpick: remove }
            }
        };
    }
    let Some(rule) = table.view.filter.get(index).cloned() else { return rsx! {} };
    let kind = table.kind(rule.column);
    // The values a choice column has, to pick one of.
    let choices: Vec<String> = if kind.has_options() && rule.op.needs_text() { table.column(rule.column).options.iter().map(|o| o.name.clone()).collect() } else { Vec::new() };
    let column_of_rule = table.column(rule.column);
    let (p_text, b_text) = (page_id.clone(), id.clone());
    rsx! {
        div { class: "popover table-menu chip-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            div { class: "ai-menu-label", "{table.heading(rule.column)}" }
            for op in table.ops(rule.column).iter().copied() {
                MenuRow {
                    key: "{op.name()}",
                    label: table.op_label(rule.column, op),
                    active: rule.op == op,
                    name: op.name(),
                    onpick: {
                        let (p, b) = (page_id.clone(), id.clone());
                        move |_| state.change_table_view(&p, &b, |v| {
                            if let Some(f) = v.filter.get_mut(index) {
                                f.op = op;
                            }
                        })
                    },
                }
            }
            if rule.op.needs_text() {
                div { class: "ai-menu-sep" }
                if choices.is_empty() {
                    label { class: "column-field",
                        input {
                            r#type: "text",
                            placeholder: match kind {
                                Kind::Date => "A day: 12 Nov",
                                k if k.is_amount() => "An amount",
                                _ => "Words",
                            },
                            autofocus: true,
                            "aria-label": "What the filter compares with",
                            "data-item": "text",
                            value: "{rule.text}",
                            oninput: move |e: FormEvent| {
                                let text = e.value();
                                state.change_table_view(&p_text, &b_text, |v| {
                                    if let Some(f) = v.filter.get_mut(index) {
                                        f.text = text;
                                    }
                                })
                            },
                        }
                    }
                }
                for value in choices {
                    MenuRow {
                        key: "{value}",
                        label: value.clone(),
                        tag: column_of_rule.color_of(&value).name(),
                        active: rule.text.trim().eq_ignore_ascii_case(&value),
                        name: "value",
                        onpick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            move |_| {
                                let value = value.clone();
                                state.change_table_view(&p, &b, |v| {
                                    if let Some(f) = v.filter.get_mut(index) {
                                        f.text = value;
                                    }
                                })
                            }
                        },
                    }
                }
            }
            div { class: "ai-menu-sep" }
            MenuRow { icon: "trash", label: "Remove", name: "remove", onpick: remove }
        }
    }
}

/// The colour rules of a table: the first rule a row passes gives the row
/// its colour. One rule is open at a time, to say its column, what it asks
/// and its colour.
#[component]
pub fn PaintMenu(page_id: String, id: String, table: Table) -> Element {
    let state = use_context::<AppState>();
    let mut at = use_signal(|| table.view.paint.len().saturating_sub(1));
    let rules: Vec<(usize, Rule)> = table.view.paint.iter().cloned().enumerate().collect();
    let columns: Vec<(usize, String)> = (0..table.columns()).map(|c| (c, table.heading(c))).collect();
    // Change one rule.
    let change = move |index: usize, edit: Box<dyn FnOnce(&mut Rule)>, page: &str, block: &str| {
        state.change_table_view(page, block, |v| {
            if let Some(rule) = v.paint.get_mut(index) {
                edit(rule);
            }
        })
    };
    let (p_new, b_new) = (page_id.clone(), id.clone());
    let count = rules.len();
    rsx! {
        div { class: "popover table-menu own-menu paint-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            div { class: "ai-menu-label", "A row takes the colour of the first rule it passes" }
            for (i, rule) in rules {
                div { key: "{i}", class: if at() == i { "paint-rule open" } else { "paint-rule" },
                    div { class: "paint-line",
                        button {
                            class: "paint-words",
                            "data-item": "rule-{i}",
                            onclick: move |_| at.set(i),
                            span { class: "paint-dot", style: "background: var(--icon-{rule.color});" }
                            span { "{table.rule_words(&rule)}" }
                        }
                        button {
                            class: "chip-x",
                            title: "Remove this rule",
                            "aria-label": "Remove the rule {table.rule_words(&rule)}",
                            onclick: {
                                let (p, b) = (page_id.clone(), id.clone());
                                move |_| state.change_table_view(&p, &b, |v| {
                                    if i < v.paint.len() {
                                        v.paint.remove(i);
                                    }
                                })
                            },
                            Icon { name: "close", size: 11 }
                        }
                    }
                    if at() == i {
                        div { class: "pick-chips", role: "group", "aria-label": "Column",
                            for (c, heading) in columns.clone() {
                                button {
                                    key: "{c}",
                                    class: if rule.column == c { "pick on" } else { "pick" },
                                    "aria-pressed": if rule.column == c { "true" } else { "false" },
                                    onclick: {
                                        let (p, b) = (page_id.clone(), id.clone());
                                        let first = table.ops(c)[0];
                                        let keeps = table.ops(c).contains(&rule.op);
                                        move |_| change(i, Box::new(move |r| {
                                            r.column = c;
                                            if !keeps {
                                                r.op = first;
                                            }
                                        }), &p, &b)
                                    },
                                    "{heading}"
                                }
                            }
                        }
                        div { class: "pick-chips", role: "group", "aria-label": "What it asks",
                            for op in table.ops(rule.column).iter().copied() {
                                button {
                                    key: "{op.name()}",
                                    class: if rule.op == op { "pick on" } else { "pick" },
                                    "aria-pressed": if rule.op == op { "true" } else { "false" },
                                    "data-op": "{op.name()}",
                                    onclick: {
                                        let (p, b) = (page_id.clone(), id.clone());
                                        move |_| change(i, Box::new(move |r| r.op = op), &p, &b)
                                    },
                                    "{table.op_label(rule.column, op)}"
                                }
                            }
                        }
                        if rule.op.needs_text() {
                            label { class: "column-field",
                                input {
                                    r#type: "text",
                                    placeholder: "Compared with",
                                    "aria-label": "What the rule compares with",
                                    "data-item": "paint-text",
                                    value: "{rule.text}",
                                    oninput: {
                                        let (p, b) = (page_id.clone(), id.clone());
                                        move |e: FormEvent| {
                                            let text = e.value();
                                            change(i, Box::new(move |r| r.text = text), &p, &b)
                                        }
                                    },
                                }
                            }
                        }
                        div { class: "paint-colors", role: "group", "aria-label": "Colour",
                            for (color, name) in ICON_COLORS.iter().copied() {
                                button {
                                    key: "{color}",
                                    class: if rule.color == color { "paint-dot big on" } else { "paint-dot big" },
                                    style: "background: var(--icon-{color});",
                                    title: "{name}",
                                    "aria-label": "{name}",
                                    "aria-pressed": if rule.color == color { "true" } else { "false" },
                                    "data-color": "{color}",
                                    onclick: {
                                        let (p, b) = (page_id.clone(), id.clone());
                                        move |_| change(i, Box::new(move |r| r.color = color.to_string()), &p, &b)
                                    },
                                }
                            }
                        }
                    }
                }
            }
            div { class: "ai-menu-sep" }
            MenuRow {
                icon: "plus",
                label: "New colour rule",
                name: "new-rule",
                onpick: move |_| {
                    state.add_table_paint(&p_new, &b_new);
                    at.set(count);
                },
            }
        }
    }
}

/// The menu of the picked rows: open one, nest them, copy them, take them out.
#[component]
pub fn RowMenu(page_id: String, id: String, table: Table, rows: Vec<usize>) -> Element {
    let state = use_context::<AppState>();
    let one = (rows.len() == 1).then(|| rows[0]);
    let nested = rows.iter().any(|&r| table.indent(r) > 0);
    let can_nest = rows.iter().any(|&r| r >= 2 && table.indent(r) <= table.indent(r - 1) && table.indent(r) < crate::table::MOST_NESTING)
        // ("The row above" is the row that is kept above: with a sort or a filter the screen shows another.)
        && table.view.filter.is_empty()
        && table.view.sort.is_empty();
    let favorite = one.and_then(|r| table.row_id(r)).and_then(|row| state.row_pages(&id).remove(row)).is_some_and(|page| state.pages.peek().get(&page).is_some_and(|p| p.favorite));
    let many = |op: &'static str| {
        let (p, b, rows) = (page_id.clone(), id.clone(), rows.clone());
        move |_| state.rows_op(&p, &b, &rows, op)
    };
    let open = |how: Open| {
        let (p, b) = (page_id.clone(), id.clone());
        move |_| {
            if let Some(r) = one {
                state.open_row(&p, &b, r, how);
            }
        }
    };
    let page_do = |what: &'static str| {
        let (p, b) = (page_id.clone(), id.clone());
        move |_| {
            if let Some(r) = one {
                state.row_page_do(&p, &b, r, what);
            }
        }
    };
    rsx! {
        div { class: "popover table-menu row-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            if one.is_some() {
                MenuRow { icon: "side", label: "Open at the side", name: "side", onpick: open(Open::Side) }
                MenuRow { icon: "expand", label: "Open as a full page", name: "full", onpick: open(Open::Full) }
                MenuRow { icon: "new-tab", label: "Open in a new tab", aside: "⌘ click", name: "tab", onpick: open(Open::Tab) }
                div { class: "ai-menu-sep" }
            }
            if can_nest {
                MenuRow { icon: "indent", label: "Nest under the row above", name: "nest", onpick: many("nest") }
            }
            if nested {
                MenuRow { icon: "outdent", label: "Move out", name: "unnest", onpick: many("unnest") }
            }
            MenuRow { icon: "duplicate", label: "Duplicate", name: "duplicate", onpick: many("duplicate") }
            if one.is_some() {
                MenuRow { icon: "link", label: "Copy link", name: "copy-link", onpick: page_do("copy-link") }
                MenuRow { icon: "star", label: if favorite { "Remove from Favorites" } else { "Add to Favorites" }, name: "favorite", onpick: page_do("favorite") }
            }
            div { class: "ai-menu-sep" }
            MenuRow {
                icon: "trash",
                label: match rows.len() {
                    1 => "Delete".to_string(),
                    n => format!("Delete {n} rows"),
                },
                name: "delete",
                onpick: many("delete"),
            }
        }
    }
}

/// The icons a table can have, in one of the colours, or none.
#[component]
pub fn IconMenu(page_id: String, id: String, icon: String) -> Element {
    let state = use_context::<AppState>();
    const ICONS: [&str; 20] = ["plus", "minus", "check", "star", "heart", "home", "calendar", "briefcase", "book", "flame", "gift", "coffee", "target", "trophy", "rocket", "leaf", "music", "plane", "wrench", "globe"];
    let (name, color) = if icon.is_empty() { (String::new(), "green".to_string()) } else { parse_icon(&icon) };
    let (p_none, b_none) = (page_id.clone(), id.clone());
    rsx! {
        div { class: "popover table-menu icon-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            div { class: "icon-grid", style: "color: var(--icon-{color});",
                for i in ICONS {
                    button {
                        key: "{i}",
                        class: if name == i { "icon-pick on" } else { "icon-pick" },
                        title: "{i}",
                        "aria-label": "{i}",
                        "data-icon": "{i}",
                        onclick: {
                            let (p, b, color) = (page_id.clone(), id.clone(), color.clone());
                            move |_| state.set_table_icon(&p, &b, &format_icon(i, &color))
                        },
                        Icon { name: i, size: 16 }
                    }
                }
            }
            div { class: "paint-colors", role: "group", "aria-label": "Colour",
                for (c, label) in ICON_COLORS.iter().copied() {
                    button {
                        key: "{c}",
                        class: if color == c { "paint-dot big on" } else { "paint-dot big" },
                        style: "background: var(--icon-{c});",
                        title: "{label}",
                        "aria-label": "{label}",
                        "data-color": "{c}",
                        onclick: {
                            let (p, b) = (page_id.clone(), id.clone());
                            let name = if name.is_empty() { "plus".to_string() } else { name.clone() };
                            move |_| state.set_table_icon(&p, &b, &format_icon(&name, c))
                        },
                    }
                }
            }
            div { class: "ai-menu-sep" }
            MenuRow {
                label: "No icon",
                name: "none",
                onpick: move |_| {
                    state.set_table_icon(&p_none, &b_none, "");
                    state.set_popover(None);
                },
            }
        }
    }
}

/// The rows of another table that a relation cell can name: a click adds
/// one to the cell, or takes it out.
#[component]
pub fn CellMenu(page_id: String, id: String, r: usize, c: usize, from: String, text: String) -> Element {
    let state = use_context::<AppState>();
    let rows: Vec<String> = state.find_table(&from).map(|(_, table)| table.body().map(|row| table.title(row).to_string()).filter(|name| !name.is_empty()).collect()).unwrap_or_default();
    rsx! {
        div { class: "popover table-menu cell-menu", role: "menu", onclick: move |e| e.stop_propagation(),
            if rows.is_empty() {
                div { class: "ai-menu-label", "That table has no rows with a name yet." }
            }
            for (i, name) in rows.into_iter().enumerate() {
                MenuRow {
                    key: "{i}",
                    label: name.clone(),
                    active: holds_part(&text, &name),
                    name: "row",
                    onpick: {
                        let (p, b) = (page_id.clone(), id.clone());
                        move |_| state.toggle_cell_part(&p, &b, r, c, &name)
                    },
                }
            }
        }
    }
}
