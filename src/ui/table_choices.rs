//! The tags of a choice cell, and the list that opens from it: what is
//! chosen, a box to find or make an option, the options, and for each of
//! them its name and colour. Built the way Notion's select works.

use super::icons::Icon;
use crate::state::AppState;
use crate::table::{Choice, Color, Column, Kind};
use dioxus::prelude::*;

/// A word of a choice column.
#[component]
pub fn Tag(name: String, color: Color, #[props(default)] dot: bool) -> Element {
    rsx! {
        span { class: if dot { "tag c-{color.name()} dot" } else { "tag c-{color.name()}" },
            span { class: "tag-words", "{name}" }
        }
    }
}

/// The class of the tag of a value: its colour, where its column has one.
pub fn tag_class(column: &Column, value: &str) -> String {
    if column.kind.has_options() && !value.trim().is_empty() {
        let dot = if column.kind == Kind::Status { " dot" } else { "" };
        format!("tag c-{}{dot}", column.color_of(value).name())
    } else {
        "tag".to_string()
    }
}

/// The list of a choice cell.
#[component]
pub fn ChoicePicker(page_id: String, id: String, r: usize, c: usize, column: Column, text: String) -> Element {
    let state = use_context::<AppState>();
    let mut query = use_signal(String::new);
    let mut active = use_signal(|| 0usize);
    let mut editing = use_signal(|| None::<String>);
    let mut dragged = use_signal(|| None::<String>);
    let locked = state.table_at(&page_id, &id).is_some_and(|t| t.view.locked);
    let multi = column.kind == Kind::Multi;
    let chosen = column.names_in(&text);
    let q = query().trim().to_string();
    let low = q.to_lowercase();
    let found: Vec<Choice> = column.options.iter().filter(|o| low.is_empty() || o.name.to_lowercase().contains(&low)).cloned().collect();
    let can_make = !q.is_empty() && !locked && column.option(&q).is_none();
    let rows = found.len() + usize::from(can_make);
    let at = active().min(rows.saturating_sub(1));
    let next_color = Color::next(column.options.len() + c);

    if let Some(name) = editing() {
        if let Some(option) = column.option(&name).cloned() {
            return rsx! {
                OptionEditor { page_id: page_id.clone(), id: id.clone(), c, option, on_back: move |new: Option<String>| editing.set(new) }
            };
        }
    }

    // Choose a word (making it first if it is new). A single choice is then done.
    let pick = {
        let (p, b) = (page_id.clone(), id.clone());
        move |name: String, make: bool| {
            let (mut query, mut active) = (query, active);
            state.pick_table_option(&p, &b, r, c, &name, make);
            query.set(String::new());
            active.set(0);
            if !multi {
                state.close_table_cell(&b, r, c);
            }
        }
    };
    let remove = pick.clone();
    let keys = {
        let pick = pick.clone();
        let found = found.clone();
        let chosen = chosen.clone();
        let (q, b) = (q.clone(), id.clone());
        move |e: KeyboardEvent| match e.key() {
            Key::ArrowDown => {
                e.prevent_default();
                active.set(if rows == 0 { 0 } else { (at + 1) % rows });
            }
            Key::ArrowUp => {
                e.prevent_default();
                active.set(if rows == 0 { 0 } else { (at + rows - 1) % rows });
            }
            Key::Enter => {
                e.prevent_default();
                if let Some(o) = found.get(at) {
                    pick(o.name.clone(), false);
                } else if can_make {
                    pick(q.clone(), true);
                }
            }
            Key::Backspace if q.is_empty() => {
                if let Some(last) = chosen.last() {
                    pick(last.clone(), false);
                }
            }
            // (Escape is the page script's: it closes every menu.)
            Key::Tab => {
                e.prevent_default();
                state.close_table_cell(&b, r, c);
            }
            _ => {}
        }
    };
    let is_chosen = |name: &str| chosen.iter().any(|n| n.eq_ignore_ascii_case(name));
    rsx! {
        div { class: "popover table-menu choice-picker", role: "listbox", onclick: move |e| e.stop_propagation(),
            div { class: "pick-bar",
                for name in chosen.iter().cloned() {
                    span { key: "{name}", class: "tag c-{column.color_of(&name).name()}",
                        span { class: "tag-words", "{name}" }
                        button {
                            class: "tag-x",
                            "aria-label": "Take away {name}",
                            onclick: {
                                let remove = remove.clone();
                                let name = name.clone();
                                move |_| remove(name.clone(), false)
                            },
                            Icon { name: "close", size: 9 }
                        }
                    }
                }
                input {
                    class: "pick-input",
                    placeholder: "Search for an option...",
                    "aria-label": "Search or add an option",
                    value: "{query}",
                    spellcheck: false,
                    onmounted: move |e: MountedEvent| async move {
                        let _ = e.set_focus(true).await;
                    },
                    oninput: move |e: FormEvent| {
                        query.set(e.value());
                        active.set(0);
                    },
                    onkeydown: keys,
                }
            }
            div { class: "pick-hint",
                if rows == 0 && !q.is_empty() {
                    "No options found"
                } else if locked {
                    "Select an option"
                } else {
                    "Select an option or create one"
                }
            }
            div { class: "pick-list",
                for (i, option) in found.iter().cloned().enumerate() {
                    div {
                        key: "{option.name}",
                        class: if i == at { "pick-row active" } else { "pick-row" },
                        role: "option",
                        "aria-selected": if is_chosen(&option.name) { "true" } else { "false" },
                        "data-option": "{option.name}",
                        draggable: if locked || !q.is_empty() { "false" } else { "true" },
                        ondragstart: {
                            let name = option.name.clone();
                            move |_| dragged.set(Some(name.clone()))
                        },
                        ondragover: move |e| e.prevent_default(),
                        ondrop: {
                            let (p, b, name) = (page_id.clone(), id.clone(), option.name.clone());
                            move |e: DragEvent| {
                                e.prevent_default();
                                if let Some(moved) = dragged() {
                                    state.move_table_option(&p, &b, c, &moved, Some(&name));
                                }
                                dragged.set(None);
                            }
                        },
                        ondragend: move |_| dragged.set(None),
                        onmouseenter: move |_| active.set(i),
                        onclick: {
                            let pick = pick.clone();
                            let name = option.name.clone();
                            move |_| pick(name.clone(), false)
                        },
                        if !locked {
                            span { class: "pick-grip", "aria-hidden": "true", Icon { name: "grip", size: 12 } }
                        }
                        Tag { name: option.name.clone(), color: option.color, dot: column.kind == Kind::Status }
                        if is_chosen(&option.name) {
                            span { class: "pick-on", Icon { name: "check", size: 13 } }
                        }
                        if !locked {
                            button {
                                class: "pick-more",
                                title: "Name and colour",
                                "aria-label": "Name and colour of {option.name}",
                                onclick: {
                                    let name = option.name.clone();
                                    move |e: MouseEvent| {
                                        e.stop_propagation();
                                        editing.set(Some(name.clone()));
                                    }
                                },
                                Icon { name: "more-horizontal", size: 14 }
                            }
                        }
                    }
                }
                if can_make {
                    div {
                        class: if at == found.len() { "pick-row active make" } else { "pick-row make" },
                        role: "option",
                        "data-make": "1",
                        onmouseenter: {
                            let n = found.len();
                            move |_| active.set(n)
                        },
                        onclick: {
                            let pick = pick.clone();
                            let q = q.clone();
                            move |_| pick(q.clone(), true)
                        },
                        span { class: "pick-make-word", "Create" }
                        Tag { name: q.clone(), color: next_color }
                    }
                }
            }
        }
    }
}

/// The name, the colour and the delete button of one option.
#[component]
fn OptionEditor(page_id: String, id: String, c: usize, option: Choice, on_back: EventHandler<Option<String>>) -> Element {
    let state = use_context::<AppState>();
    let old = option.name.clone();
    let (p_name, b_name) = (page_id.clone(), id.clone());
    let (p_del, b_del, name_del) = (page_id.clone(), id.clone(), option.name.clone());
    rsx! {
        div { class: "popover table-menu choice-picker option-editor", role: "menu", onclick: move |e| e.stop_propagation(),
            button { class: "pop-back", onclick: move |_| on_back.call(None),
                Icon { name: "chevron-left", size: 13 }
                span { "Options" }
            }
            input {
                class: "pop-input",
                "aria-label": "Name of the option",
                value: "{option.name}",
                spellcheck: false,
                onchange: move |e: FormEvent| {
                    let kept = state.rename_table_option(&p_name, &b_name, c, &old, &e.value());
                    on_back.call(Some(kept));
                },
            }
            button {
                class: "ai-menu-row",
                role: "menuitem",
                "data-item": "delete-option",
                onclick: move |_| {
                    state.remove_table_option(&p_del, &b_del, c, &name_del);
                    on_back.call(None);
                },
                span { class: "ai-menu-value", Icon { name: "trash", size: 14 } "Delete" }
            }
            div { class: "ai-menu-sep" }
            div { class: "ai-menu-label", "Colors" }
            div { class: "color-list",
                for color in Color::ALL {
                    button {
                        key: "{color.name()}",
                        class: if option.color == color { "ai-menu-row active" } else { "ai-menu-row" },
                        role: "menuitemradio",
                        "data-color": "{color.name()}",
                        "aria-checked": if option.color == color { "true" } else { "false" },
                        onclick: {
                            let (p, b, name) = (page_id.clone(), id.clone(), option.name.clone());
                            move |_| state.color_table_option(&p, &b, c, &name, color)
                        },
                        span { class: "ai-menu-value",
                            span { class: "color-swatch c-{color.name()}" }
                            "{color.label()}"
                        }
                        if option.color == color {
                            Icon { name: "check", size: 13 }
                        }
                    }
                }
            }
        }
    }
}
