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

/// The letter a pinned tab shows in place of an icon: the first of the
/// page's name, for a page with no icon (the grey page sign made all such
/// tabs look the same). None for a page with an icon or with no name, and
/// for Home, the calendar and the graph, which have icons.
fn pin_letter(route: &Route, pages: &std::collections::HashMap<String, crate::model::Page>) -> Option<String> {
    let Route::Page(id) = route else { return None };
    let page = pages.get(id).filter(|p| p.icon.as_deref().is_none_or(|i| i.trim().is_empty()))?;
    page.title.trim().chars().next().map(|c| c.to_uppercase().to_string())
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
    /// Pinned: its icon only (of its first page, when it holds several).
    pinned: bool,
    /// What the pointer's rest on the tab says: the name of each page.
    names: String,
    /// Pinned, and its page has no icon: the letter it shows (see `pin_letter`).
    letter: Option<String>,
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
            let names = if segs.is_empty() { title.clone() } else { segs.iter().map(|s| s.2.as_str()).collect::<Vec<_>>().join(" | ") };
            // A pinned tab is drawn as one part, with the icon of its first page.
            let (icon, segs) = if t.pinned { (segs.first().map(|s| s.1.clone()).unwrap_or(icon), Vec::new()) } else { (icon, segs) };
            let letter = t.ordered().first().filter(|_| t.pinned).and_then(|first| pin_letter(first.route(), &pages));
            Shown { index: i, id: t.id.clone(), icon, title, segs, focus: t.focus.clone(), active: i == ui.active, pinned: t.pinned, names, letter }
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
                        class: format!("tab{}{}{}", if t.active { " active" } else { "" }, if t.segs.is_empty() { "" } else { " split" }, if t.pinned { " pinned" } else { "" }),
                        style: format!("--segs: {}", t.segs.len().max(1)),
                        title: "{t.names}",
                        "aria-label": if t.pinned { t.names.clone() },
                        draggable: "true",
                        "data-tab": "{t.id}",
                        onclick: move |_| state.select_tab(t.index),
                        onmousedown: move |e| {
                            // Middle button: no autoscroll; it closes the tab on release.
                            if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                e.prevent_default();
                            }
                        },
                        // (The middle button only: a right-click is for the menu
                        // of the tab, and closed the tab under it.)
                        onauxclick: move |e| {
                            if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                state.close_tab(t.index);
                            }
                        },
                        oncontextmenu: move |e| {
                            e.prevent_default();
                            state.set_popover(Some(Popover::TabMenu { index: t.index }));
                        },
                        if let Some(letter) = &t.letter {
                            span { class: "tab-icon", span { class: "tab-letter space-badge", "{letter}" } }
                        } else if t.pinned {
                            span { class: "tab-icon", PageIcon { icon: t.icon.clone(), size: 14 } }
                        } else if t.segs.is_empty() {
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
                                    // The middle button on one part closes
                                    // that page only (the press went on to
                                    // the tab and closed all its pages).
                                    onauxclick: {
                                        let index = t.index;
                                        let pane = pane.clone();
                                        move |e| {
                                            if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                                e.stop_propagation();
                                                state.close_pane_of(index, Some(&pane));
                                            }
                                        }
                                    },
                                    span { class: "tab-icon", PageIcon { icon, size: 14 } }
                                    span { class: "tab-title", "{title}" }
                                }
                            }
                        }
                        if !t.pinned {
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
                        }
                        if menu == Some(t.index) {
                            TabMenu { index: t.index }
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
fn TabMenu(index: usize) -> Element {
    let state = use_context::<AppState>();
    let ui = state.ui.read();
    let (pinned, split, copies) = ui.tabs.get(index).map(|t| (t.pinned, t.is_split(), t.can_duplicate())).unwrap_or_default();
    // (Tabs that "Close Other Tabs" would close: a pinned one stays.)
    let others = ui.tabs.iter().enumerate().any(|(i, t)| i != index && !t.pinned);
    drop(ui);
    rsx! {
        div {
            class: "popover tab-menu",
            onclick: move |e| e.stop_propagation(),
            div { class: "menu-item compact", onclick: move |_| state.pin_tab(index, !pinned),
                span { if pinned { "Unpin Tab" } else { "Pin Tab" } }
            }
            // (Not for the tab of Home, the calendar or the graph: there is one of each.)
            if copies {
                div { class: "menu-item compact", onclick: move |_| state.duplicate_tab(index),
                    span { "Duplicate Tab" }
                }
            }
            // (A pinned tab does not close: unpin it first.)
            if !pinned {
                div { class: "menu-item compact", onclick: move |_| state.close_tab(index),
                    span { "Close Tab" }
                    span { class: "menu-shortcut", "⌘W" }
                }
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
                    let tab = state.ui.peek().tabs.get(index).map(|t| (t.route().clone(), t.is_split()));
                    if let Some((route, split)) = tab {
                        super::open_window(route);
                        // A tab of several pages gives the page in front
                        // and keeps the others (the whole tab was closed,
                        // and the new window showed only that one page).
                        if split {
                            state.close_pane_of(index, None);
                        } else {
                            // (Moved, not closed: no message says "Tab closed".)
                            state.take_tab(index);
                        }
                    }
                },
                span { "Move to New Window" }
            }
        }
    }
}

/// While Control is held after Ctrl+Tab: the tabs of this window and space
/// in the order of use, the one reached marked. It shows only when Control
/// is still held a moment after the first Tab (bridge.js sets
/// `body[data-tab-walk]`), so a quick Ctrl+Tab does not flash it. A press
/// on a row brings that tab forward.
#[component]
pub fn TabWalk() -> Element {
    let state = use_context::<AppState>();
    let Some((list, at)) = state.tab_walk.read().clone() else { return rsx! {} };
    let ui = state.ui.read();
    let pages = state.pages.read();
    // (id, icon, letter, name) of each tab, as its tab in the row shows it.
    let rows: Vec<(String, Option<String>, Option<String>, String)> = list
        .iter()
        .filter_map(|id| ui.tabs.iter().find(|t| &t.id == id))
        .map(|t| {
            let ordered = t.ordered();
            let first = ordered.first().map(|p| p.route().clone()).unwrap_or(Route::Home);
            let (icon, _) = label(&first, &pages);
            let name = ordered.iter().map(|p| label(p.route(), &pages).1).collect::<Vec<_>>().join(" | ");
            (t.id.clone(), icon, pin_letter(&first, &pages), name)
        })
        .collect();
    drop(pages);
    drop(ui);
    // The row of the tab reached, by its id: a tab that closed meanwhile has
    // no row, and the places in the list are not those of the rows.
    let reached = list.get(at).cloned();
    rsx! {
        div { class: "tab-walk", role: "listbox", "aria-label": "Tabs in the order of use",
            for (id, icon, letter, name) in rows {
                div {
                    key: "{id}",
                    class: if Some(&id) == reached.as_ref() { "tab-walk-row on" } else { "tab-walk-row" },
                    role: "option",
                    "aria-selected": "{Some(&id) == reached.as_ref()}",
                    // (A press, not a click: with Control held a click is
                    // the Mac's right click, and no click comes.)
                    onmousedown: {
                        let id = id.clone();
                        move |e: MouseEvent| {
                            e.prevent_default();
                            state.walk_tabs_to(&id);
                        }
                    },
                    oncontextmenu: move |e| e.prevent_default(),
                    if let Some(letter) = letter {
                        span { class: "tab-icon", span { class: "tab-letter space-badge", "{letter}" } }
                    } else {
                        span { class: "tab-icon", PageIcon { icon, size: 14 } }
                    }
                    span { class: "tab-walk-name", "{name}" }
                }
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
    let lock_state = page_id.as_deref().map(|id| crate::lock::page_lock(&state, id)).unwrap_or_default();

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
                            // A locked page: lock it now or take its password
                            // away (it keeps no history); any other: lock it.
                            if lock_state.is_open() {
                                div {
                                    class: "menu-item compact",
                                    id: "lock-now",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| {
                                            state.set_popover(None);
                                            state.relock(&id);
                                        }
                                    },
                                    span { class: "menu-icon", Icon { name: "lock", size: 16 } }
                                    span { "Lock now" }
                                }
                                div {
                                    class: "menu-item compact",
                                    id: "remove-password",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| {
                                            state.set_popover(None);
                                            state.remove_password(&id);
                                        }
                                    },
                                    span { class: "menu-icon", Icon { name: "lock", size: 16 } }
                                    span { "Remove password" }
                                }
                            } else if !lock_state.is_locked() {
                                div {
                                    class: "menu-item compact",
                                    id: "lock-page",
                                    onclick: move |_| {
                                        state.set_popover(None);
                                        state.command("lock_page");
                                    },
                                    span { class: "menu-icon", Icon { name: "lock", size: 16 } }
                                    span { "Lock with password…" }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Page;

    #[test]
    fn a_pinned_tab_of_a_page_with_no_icon_shows_the_first_letter_of_its_name() {
        let mut pages = std::collections::HashMap::new();
        let mut add = |id: &str, title: &str, icon: Option<&str>| {
            let mut p = Page::new(None, 1.0);
            p.id = id.into();
            p.title = title.into();
            p.icon = icon.map(str::to_string);
            pages.insert(id.to_string(), p);
        };
        add("plain", "  quiet page", None);
        add("blank", "Écoute", Some("  "));
        add("icon", "Plan", Some("plane:blue"));
        add("bare", "   ", None);
        let letter = |id: &str| pin_letter(&Route::Page(id.into()), &pages);
        assert_eq!(letter("plain").as_deref(), Some("Q"));
        assert_eq!(letter("blank").as_deref(), Some("É"));
        // A page with an icon shows its icon; one with no name, the page sign.
        assert_eq!(letter("icon"), None);
        assert_eq!(letter("bare"), None);
        assert_eq!(letter("gone"), None);
        assert_eq!(pin_letter(&Route::Home, &pages), None);
    }
}
