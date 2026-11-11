//! The window's top row (laid out like Aven's: room for the traffic lights,
//! the sidebar toggle, back and forward, then the tabs) and the bar at the top
//! of the page card (breadcrumbs and page actions).

use super::icons::{Icon, PageIcon};
use crate::state::{ancestors, AppState, Popover, Route};
use dioxus::prelude::*;

/// Icon and title for what a route shows.
pub fn label(route: &Route, pages: &std::collections::HashMap<String, crate::model::Page>) -> (Option<String>, String) {
    match route {
        Route::Home => (Some("home:gray".to_string()), "Home".to_string()),
        Route::Calendar => (Some("calendar:red".to_string()), "Calendar".to_string()),
        Route::Graph => (Some("graph:gray".to_string()), "Graph".to_string()),
        Route::Page(id) => pages
            .get(id)
            .map(|p| (p.icon.clone(), p.display_title().to_string()))
            .unwrap_or_else(|| (None, "Untitled".into())),
    }
}

#[derive(Clone, PartialEq)]
struct Shown {
    index: usize,
    id: String,
    icon: Option<String>,
    title: String,
    /// One part per page, in reading order, when the tab holds several:
    /// (pane, icon, title).
    segs: Vec<(String, Option<String>, String)>,
    /// The pane in focus.
    focus: String,
    active: bool,
}

#[component]
pub fn TopRow() -> Element {
    let state = use_context::<AppState>();
    let ui = state.ui.read();
    let pages = state.pages.read();
    let shown: Vec<Shown> = ui
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (icon, title) = label(t.route(), &pages);
            let segs = if t.is_split() {
                t.ordered()
                    .into_iter()
                    .map(|p| {
                        let (icon, title) = label(p.route(), &pages);
                        (p.id.clone(), icon, title)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Shown { index: i, id: t.id.clone(), icon, title, segs, focus: t.focus.clone(), active: i == ui.active }
        })
        .collect();
    let menu = match &ui.popover {
        Some(Popover::TabMenu { index }) => Some(*index),
        _ => None,
    };
    let pinned = ui.sidebar_open;
    let tab = ui.tab().clone();
    let space = ui.space.clone();
    drop(pages);
    drop(ui);
    let single = shown.len() == 1;

    rsx! {
        header { class: "toprow drag-region",
            div { class: if pinned { "nav-group pinned" } else { "nav-group" },
                button {
                    class: if pinned { "bar-btn on" } else { "bar-btn" },
                    title: if pinned { "Hide sidebar (⌘\\)" } else { "Keep sidebar open (⌘\\)" },
                    "aria-pressed": "{pinned}",
                    onclick: move |_| state.toggle_sidebar(),
                    Icon { name: "sidebar", size: 16 }
                }
                span { class: "bar-divider" }
                button {
                    class: "bar-btn push",
                    title: "Back (⌘[)",
                    disabled: !tab.can_back(),
                    onclick: move |_| state.back(),
                    Icon { name: "chevron-left", size: 16 }
                }
                button {
                    class: "bar-btn",
                    title: "Forward (⌘])",
                    disabled: !tab.can_forward(),
                    onclick: move |_| state.forward(),
                    Icon { name: "chevron-right", size: 16 }
                }
            }
            div { class: if single { "tabs single" } else { "tabs track" }, "data-space": "{space}",
                span { class: "tab-glider" }
                for t in shown {
                    div {
                        key: "{t.id}",
                        class: format!("tab{}{}", if t.active { " active" } else { "" }, if t.segs.is_empty() { "" } else { " split" }),
                        style: format!("--segs: {}", t.segs.len().max(1)),
                        title: if t.segs.is_empty() { t.title.clone() } else { t.segs.iter().map(|s| s.2.as_str()).collect::<Vec<_>>().join(" | ") },
                        draggable: "true",
                        "data-tab": "{t.id}",
                        onclick: move |_| state.select_tab(t.index),
                        onmousedown: move |e| {
                            // Middle button: no autoscroll; it closes the tab on release.
                            if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                e.prevent_default();
                            }
                        },
                        onauxclick: move |_| state.close_tab(t.index),
                        oncontextmenu: move |e| {
                            e.prevent_default();
                            state.set_popover(Some(Popover::TabMenu { index: t.index }));
                        },
                        if t.segs.is_empty() {
                            span { class: "tab-icon", PageIcon { icon: t.icon.clone(), size: 14 } }
                            span { class: "tab-title", "{t.title}" }
                        } else {
                            // One part per page: click it to work there, drag
                            // it along the bar to move the tab, or pull it
                            // out to give that page a tab (or a place) of its own.
                            for (pane, icon, title) in t.segs.clone() {
                                span {
                                    key: "{pane}",
                                    class: if t.active && t.focus == pane { "tab-seg on" } else { "tab-seg" },
                                    draggable: "true",
                                    "data-pane": "{pane}",
                                    onclick: {
                                        let index = t.index;
                                        let pane = pane.clone();
                                        move |e: MouseEvent| {
                                            e.stop_propagation();
                                            state.select_tab(index);
                                            state.focus_pane(&pane);
                                        }
                                    },
                                    span { class: "tab-icon", PageIcon { icon, size: 14 } }
                                    span { class: "tab-title", "{title}" }
                                }
                            }
                        }
                        button {
                            class: "tab-close",
                            title: "Close tab (⌘W)",
                            draggable: "false",
                            onclick: move |e| {
                                e.stop_propagation();
                                state.close_tab(t.index);
                            },
                            Icon { name: "close", size: 12 }
                        }
                        if menu == Some(t.index) {
                            TabMenu { index: t.index, split: !t.segs.is_empty() }
                        }
                    }
                }
                button {
                    class: "bar-btn new-tab",
                    title: "New tab (⌘T)",
                    onclick: move |_| state.new_tab(),
                    Icon { name: "plus", size: 16 }
                }
            }
        }
    }
}

#[component]
fn TabMenu(index: usize, split: bool) -> Element {
    let state = use_context::<AppState>();
    let others = state.ui.read().tabs.len() > 1;
    rsx! {
        div {
            class: "popover tab-menu",
            onclick: move |e| e.stop_propagation(),
            div { class: "menu-item compact", onclick: move |_| state.close_tab(index),
                span { "Close Tab" }
                span { class: "menu-shortcut", "⌘W" }
            }
            if others {
                div { class: "menu-item compact", onclick: move |_| state.close_other_tabs(index),
                    span { "Close Other Tabs" }
                }
            }
            if split {
                div { class: "menu-item compact", onclick: move |_| state.separate_tab(index),
                    span { "Separate Pages" }
                }
            }
            div {
                class: "menu-item compact",
                onclick: move |_| {
                    state.set_popover(None);
                    let route = state.ui.peek().tabs.get(index).map(|t| t.route().clone());
                    if let Some(route) = route {
                        super::open_window(route);
                        state.close_tab(index);
                    }
                },
                span { "Move to New Window" }
            }
        }
    }
}

/// Breadcrumbs and page actions at the top of a pane. When the tab shows
/// several pages, the bar also closes its pane, and dragging it moves the
/// pane (onto another pane's edge, or out to the tab bar).
#[component]
pub fn PageBar(route: Route, pane: String, multi: bool) -> Element {
    let state = use_context::<AppState>();
    let ui = state.ui.read();
    let menu_open = ui.popover == Some(Popover::PageMenu { pane: pane.clone() });
    // The pane that shows rows of tables opened at the side.
    let is_peek = ui.tab().peek.as_deref() == Some(pane.as_str());
    drop(ui);

    let page_id = match &route {
        Route::Page(id) => Some(id.clone()),
        Route::Home | Route::Calendar | Route::Graph => None,
    };
    let pages = state.pages.read();
    let mut crumbs: Vec<(String, Option<String>, String)> = Vec::new();
    let mut favorite = false;
    if let Some(id) = &page_id {
        for a in ancestors(&pages, id) {
            crumbs.push((a.id.clone(), a.icon.clone(), a.display_title().to_string()));
        }
        if let Some(p) = pages.get(id) {
            crumbs.push((p.id.clone(), p.icon.clone(), p.display_title().to_string()));
            favorite = p.favorite;
        }
    }
    drop(pages);
    // For the page menu: is a page being read aloud, and is this a meeting's notes?
    let reading = *state.reading.read();
    let meeting_page = menu_open && page_id.as_ref().is_some_and(|id| state.is_meeting_page(id));
    let meeting = meeting_page && crate::ai::has_ai();
    let listening_here = state.listening.read().as_ref().map(|l| l.page.clone()) == page_id && page_id.is_some();
    let can_listen = meeting_page && crate::listen::available() && state.listening.read().is_none();

    let words = if menu_open {
        page_id.as_ref().map(|id| {
            state
                .blocks
                .read()
                .get(id)
                .map(|bs| bs.iter().map(|b| b.plain_text().split_whitespace().count()).sum::<usize>())
                .unwrap_or(0)
        })
    } else {
        None
    };
    // A row's page sits in its table: the crumb before it is the table.
    if let Some(row) = page_id.as_deref().and_then(|id| state.row_ref(id)).filter(|row| !row.table.name.is_empty()) {
        if let Some(at) = crumbs.len().checked_sub(2) {
            crumbs[at].1 = (!row.table.icon.is_empty()).then(|| row.table.icon.clone());
            crumbs[at].2 = row.table.name.clone();
        }
    }
    let crumb_count = crumbs.len();

    rsx! {
        div {
            class: "pagebar",
            draggable: if multi { "true" } else { "false" },
            "data-pane-drag": "{pane}",
            if is_peek {
                button {
                    class: "icon-btn peek-btn",
                    title: "Close the side page",
                    "aria-label": "Close the side page",
                    "data-peek": "close",
                    onclick: {
                        let pane = pane.clone();
                        move |_| state.close_pane(Some(&pane))
                    },
                    Icon { name: "chevrons-right", size: 16 }
                }
                if let Some(id) = page_id.clone() {
                    button {
                        class: "icon-btn peek-btn",
                        title: "Open in a tab of its own",
                        "aria-label": "Open in a tab of its own",
                        "data-peek": "full",
                        onclick: {
                            let pane = pane.clone();
                            move |_| state.peek_to_tab(&pane, &id)
                        },
                        Icon { name: "expand", size: 15 }
                    }
                }
            }
            div { class: "crumbs",
                if route == Route::Home {
                    span { class: "crumb current", Icon { name: "home", size: 14 } span { "Home" } }
                }
                if route == Route::Calendar {
                    span { class: "crumb current", Icon { name: "calendar", size: 14 } span { "Calendar" } }
                }
                if route == Route::Graph {
                    span { class: "crumb current", Icon { name: "graph", size: 14 } span { "Graph" } }
                }
                for (i, (id, icon, title)) in crumbs.into_iter().enumerate() {
                    if i > 0 {
                        span { class: "crumb-sep", "/" }
                    }
                    span {
                        class: if i + 1 == crumb_count { "crumb current" } else { "crumb" },
                        onclick: move |_| state.navigate(Route::Page(id.clone())),
                        PageIcon { icon, size: 14 }
                        span { "{title}" }
                    }
                }
            }
            if let Some(id) = page_id {
                div { class: "top-actions",
                    button {
                        class: if favorite { "icon-btn star on" } else { "icon-btn star" },
                        title: if favorite { "Remove from Favorites" } else { "Add to Favorites" },
                        onclick: {
                            let id = id;
                            move |_| state.toggle_favorite(&id)
                        },
                        Icon { name: if favorite { "star-filled" } else { "star" }, size: 16 }
                    }
                    button {
                        class: "icon-btn",
                        title: "Page options",
                        onclick: {
                            let pane = pane.clone();
                            move |_| state.set_popover((!menu_open).then(|| Popover::PageMenu { pane: pane.clone() }))
                        },
                        Icon { name: "more-horizontal", size: 16 }
                    }
                    if menu_open {
                        div { class: "popover page-menu", onclick: move |e| e.stop_propagation(),
                            div {
                                class: "menu-item compact",
                                onclick: move |_| {
                                    state.set_popover(None);
                                    state.command("copy_link");
                                },
                                span { class: "menu-icon", Icon { name: "link", size: 16 } }
                                span { "Copy link" }
                                span { class: "menu-shortcut", "⌘L" }
                            }
                            // While a page is being read aloud, the way to stop it.
                            if reading {
                                div {
                                    class: "menu-item compact",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| {
                                            state.set_popover(None);
                                            state.toggle_reading(&id);
                                        }
                                    },
                                    span { class: "menu-icon", Icon { name: "speaker", size: 16 } }
                                    span { "Stop reading" }
                                }
                            }
                            if can_listen {
                                div {
                                    class: "menu-item compact",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| {
                                            state.set_popover(None);
                                            state.start_listening(&id);
                                        }
                                    },
                                    span { class: "menu-icon", Icon { name: "mic", size: 16 } }
                                    span { "Listen to this meeting" }
                                }
                            }
                            if listening_here {
                                div {
                                    class: "menu-item compact",
                                    onclick: move |_| {
                                        state.set_popover(None);
                                        state.stop_listening();
                                    },
                                    span { class: "menu-icon", Icon { name: "mic", size: 16 } }
                                    span { "Stop listening" }
                                }
                            }
                            if meeting && !listening_here {
                                div {
                                    class: "menu-item compact",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| {
                                            state.set_popover(None);
                                            state.ai_recap(&id);
                                        }
                                    },
                                    span { class: "menu-icon", Icon { name: "mic", size: 16 } }
                                    span { "Write a recap" }
                                }
                            }
                            if multi {
                                div {
                                    class: "menu-item compact",
                                    onclick: {
                                        let pane = pane.clone();
                                        move |_| state.fold_pane(&pane, true)
                                    },
                                    span { class: "menu-icon", Icon { name: "sidebar", size: 16 } }
                                    span { "Minimize pane" }
                                }
                            }
                            // Which page this one is kept in: asked at the top of the page.
                            div {
                                class: "menu-item compact",
                                id: "move-page",
                                onclick: {
                                    let id = id.clone();
                                    move |_| {
                                        state.set_popover(None);
                                        state.ask_place_now(&id);
                                    }
                                },
                                span { class: "menu-icon", Icon { name: "folder", size: 16 } }
                                span { "Move to…" }
                            }
                            div {
                                class: "menu-item compact",
                                onclick: {
                                    let id = id.clone();
                                    move |_| state.open_history(&id)
                                },
                                span { class: "menu-icon", Icon { name: "history", size: 16 } }
                                span { "Page history" }
                            }
                            div {
                                class: "menu-item compact danger",
                                onclick: {
                                    let id = id.clone();
                                    move |_| state.trash_page(&id)
                                },
                                span { class: "menu-icon", Icon { name: "trash", size: 16 } }
                                span { "Move to Trash" }
                            }
                            if let Some(n) = words {
                                div { class: "menu-footer", "Word count: {n}" }
                            }
                        }
                    }
                }
            }
            if multi {
                button {
                    class: "icon-btn",
                    title: "Close pane",
                    draggable: "false",
                    onclick: {
                        let pane = pane.clone();
                        move |e: MouseEvent| {
                            e.stop_propagation();
                            state.close_pane(Some(&pane));
                        }
                    },
                    Icon { name: "close", size: 16 }
                }
            }
        }
    }
}
