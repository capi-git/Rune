//! The sidebar, laid out like Aven's: workspace row, search field, a row of
//! tiles, then Favorites and Pages. Pinned, it sits on the window's glass; when
//! unpinned it slides out as a floating card while the pointer rests on the
//! window's left edge (see the peek handling in assets/bridge.js).

use super::icons::{Icon, PageIcon};
use crate::model::{Page, Space};
use crate::state::{children_of, roots_in, AppState, Modal, Popover, Route};
use dioxus::prelude::*;

#[component]
pub fn Sidebar() -> Element {
    let state = use_context::<AppState>();
    let ui = state.ui.read();
    let space_id = ui.space.clone();
    let on_home = *ui.tab().route() == Route::Home;
    let on_calendar = *ui.tab().route() == Route::Calendar;
    let on_graph = *ui.tab().route() == Route::Graph;
    let pinned = ui.sidebar_open;
    let space_menu = ui.popover == Some(Popover::SpaceMenu);
    drop(ui);
    let pages = state.pages.read();
    let roots = roots_in(&pages, &space_id);
    let mut favorites: Vec<Page> =
        pages.values().filter(|p| p.favorite && !p.trashed && p.space_id == space_id).cloned().collect();
    favorites.sort_by_key(|a| a.display_title().to_lowercase());
    drop(pages);
    let spaces = state.spaces.read().clone();
    let space = spaces.iter().find(|s| s.id == space_id).cloned().unwrap_or_else(|| Space::new("My Space", "blue", 1.0));
    let here = space_id.clone();
    // The list of a space comes in from the side its dot is on.
    let list_class = match *state.space_slide.read() {
        1 => "side-scroll from-next",
        -1 => "side-scroll from-prev",
        _ => "side-scroll",
    };

    rsx! {
        aside {
            class: if pinned { "sidebar" } else { "sidebar floating" },
            "aria-label": "Sidebar",
            div { class: "ws-row",
                button {
                    class: if space_menu { "ws-name current" } else { "ws-name" },
                    title: "Switch space",
                    onclick: move |_| state.set_popover((!space_menu).then_some(Popover::SpaceMenu)),
                    SpaceBadge { space: space.clone() }
                    for name in std::iter::once(space.display_name().to_string()) {
                        span { key: "{name}", class: "ws-label", "{name}" }
                    }
                    Icon { name: "chevron-down", size: 12, class: "ws-chevron" }
                }
                button {
                    class: "row-btn",
                    title: "New page (⌘N)",
                    onclick: move |_| state.new_page(None),
                    Icon { name: "plus", size: 16 }
                }
                if space_menu {
                    SpaceMenu { current: space_id }
                }
            }
            button { class: "search-field", onclick: move |_| state.set_modal(Some(Modal::QuickFind)),
                Icon { name: "search", size: 16 }
                span { "Search" }
                span { class: "search-hint", "⌘K" }
            }
            div { class: "tiles",
                button {
                    class: if on_home { "tile current" } else { "tile" },
                    title: "Home",
                    onclick: move |_| state.navigate(Route::Home),
                    Icon { name: "home", size: 16 }
                }
                button {
                    class: if on_calendar { "tile current" } else { "tile" },
                    title: "Calendar (⇧⌘C)",
                    onclick: move |_| state.navigate(Route::Calendar),
                    Icon { name: "calendar", size: 16 }
                }
                button {
                    class: if on_graph { "tile current" } else { "tile" },
                    title: "Graph: how your pages connect",
                    "aria-label": "Graph: how your pages connect",
                    onclick: move |_| state.navigate(Route::Graph),
                    Icon { name: "graph", size: 16 }
                }
            }
            // One list for each space (a new one when the space changes, so that it comes in).
            for sid in std::iter::once(here.clone()) {
                div { key: "{sid}", class: "{list_class}", "data-space": "{sid}",
                    if !favorites.is_empty() {
                        div { class: "section-label", span { "Favorites" } }
                        for p in favorites.clone() {
                            PageTreeItem { key: "fav-{p.id}", id: p.id, depth: 0, favorites: true }
                        }
                    }
                    div { class: "section-label",
                        span { "Pages" }
                        button {
                            class: "section-btn",
                            title: "Add a page",
                            onclick: move |_| state.new_page(None),
                            Icon { name: "plus", size: 12 }
                        }
                    }
                    for p in roots.clone() {
                        PageTreeItem { key: "{p.id}", id: p.id, depth: 0, favorites: false }
                    }
                    div { class: "tree-item muted", onclick: move |_| state.new_page(None),
                        span { class: "tree-icon", Icon { name: "plus", size: 16 } }
                        span { class: "tree-title", "Add a page" }
                    }
                }
            }
            if let Some(l) = state.listening.read().clone() {
                button {
                    class: "update-row listening",
                    id: "listening-row",
                    title: "Listening to a meeting. Click to go to its page.",
                    onclick: move |_| state.open_page(&l.page),
                    span { class: "listen-dot" }
                    span { class: "update-text", "Listening" }
                    span { class: "update-act", "{l.seconds / 60}:{l.seconds % 60:02}" }
                }
            }
            if let Some(update) = state.update.read().clone() {
                button {
                    class: "update-row",
                    id: "update-ready",
                    title: "A newer build ({crate::update::build_label(update.build)}) is ready. Click to install it and start again.",
                    onclick: move |_| state.install_update(),
                    span { class: "update-dot" }
                    span { class: "update-text", "Update ready" }
                    span { class: "update-act", "Restart" }
                }
            }
            // The foot: Settings, and a dot for each space. A click on a dot
            // goes to its space; so do two fingers moved sideways on the
            // sidebar (see bridge.js).
            div { class: "side-foot",
                button {
                    class: "row-btn",
                    id: "side-settings",
                    title: "Settings (⌘,)",
                    "aria-label": "Settings",
                    onclick: move |_| state.set_modal(Some(Modal::Settings)),
                    Icon { name: "settings", size: 16 }
                }
                div { class: "space-dots", role: "tablist", "aria-label": "Spaces",
                    for sp in spaces {
                        button {
                            key: "{sp.id}",
                            class: if sp.id == here { "space-dot current" } else { "space-dot" },
                            role: "tab",
                            "aria-selected": if sp.id == here { "true" } else { "false" },
                            "aria-label": "{sp.display_name()}",
                            title: "{sp.display_name()}",
                            "data-space": "{sp.id}",
                            style: "--c: var(--icon-{sp.color});",
                            onclick: {
                                let id = sp.id.clone();
                                move |_| state.switch_space(&id)
                            },
                        }
                    }
                }
                span { class: "side-foot-end" }
            }
            if pinned {
                div {
                    class: "resize-handle",
                    "data-resize": "sidebar",
                    title: "Drag to resize · Double-click to reset",
                }
            }
        }
    }
}

/// A space's badge: its first letter on its colour.
#[component]
pub fn SpaceBadge(space: Space) -> Element {
    rsx! {
        span { class: "space-badge", style: "--c: var(--icon-{space.color}); --c-soft: var(--icon-{space.color}-soft);", "{space.initial()}" }
    }
}

/// Switch between spaces or make a new one.
#[component]
fn SpaceMenu(current: String) -> Element {
    let state = use_context::<AppState>();
    let spaces = state.spaces.read().clone();
    let mut adding = use_signal(|| false);
    let mut name = use_signal(String::new);
    // A space being dragged to another place in the list, and the row (and
    // its half) it would land at.
    let mut dragged = use_signal(|| None::<String>);
    let mut over = use_signal(|| None::<(String, bool)>);
    let many = spaces.len() > 1;
    rsx! {
        div { class: "popover space-menu", onclick: move |e| e.stop_propagation(),
            div { class: "menu-section", "Spaces" }
            for sp in spaces {
                div {
                    key: "{sp.id}",
                    class: match over().filter(|(at, _)| *at == sp.id && dragged().is_some_and(|d| d != sp.id)) {
                        Some((_, true)) => "menu-item compact space-row drop-after",
                        Some((_, false)) => "menu-item compact space-row drop-before",
                        None if dragged().as_deref() == Some(sp.id.as_str()) => "menu-item compact space-row dragging",
                        None => "menu-item compact space-row",
                    },
                    "data-space": "{sp.id}",
                    title: if many { "Drag to change the order" } else { "" },
                    draggable: if many { "true" } else { "false" },
                    ondragstart: {
                        let id = sp.id.clone();
                        move |_| dragged.set(Some(id.clone()))
                    },
                    ondragover: {
                        let id = sp.id.clone();
                        move |e: DragEvent| {
                            e.prevent_default();
                            // The lower half of a row is after it.
                            let after = e.element_coordinates().y > 15.0;
                            if over() != Some((id.clone(), after)) {
                                over.set(Some((id.clone(), after)));
                            }
                        }
                    },
                    ondrop: {
                        let id = sp.id.clone();
                        move |e: DragEvent| {
                            e.prevent_default();
                            if let (Some(moved), Some((at, after))) = (dragged(), over()) {
                                if at == id {
                                    state.move_space(&moved, &at, after);
                                }
                            }
                            dragged.set(None);
                            over.set(None);
                        }
                    },
                    ondragend: move |_| {
                        dragged.set(None);
                        over.set(None);
                    },
                    onclick: {
                        let id = sp.id;
                        move |_| state.switch_space(&id)
                    },
                    if many {
                        span { class: "space-grip", "aria-hidden": "true", Icon { name: "grip", size: 12 } }
                    }
                    SpaceBadge { space: sp.clone() }
                    span { class: "grow", "{sp.display_name()}" }
                    if sp.id == current {
                        span { class: "menu-shortcut", Icon { name: "check", size: 14 } }
                    }
                }
            }
            div { class: "menu-sep" }
            if *adding.read() {
                div { class: "space-new",
                    input {
                        class: "text-input",
                        placeholder: "Name the new space",
                        onmounted: move |e: MountedEvent| async move {
                            let _ = e.set_focus(true).await;
                        },
                        oninput: move |e: FormEvent| name.set(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Enter {
                                state.create_space(name.read().clone());
                            } else if e.key() == Key::Escape {
                                e.stop_propagation();
                                adding.set(false);
                            }
                        },
                    }
                    button { class: "primary-btn", onclick: move |_| state.create_space(name.read().clone()), "Create" }
                }
            } else {
                div { class: "menu-item compact", onclick: move |_| adding.set(true),
                    span { class: "menu-icon", Icon { name: "plus", size: 16 } }
                    span { "New space" }
                }
            }
            div { class: "menu-item compact", onclick: move |_| state.set_modal(Some(Modal::Trash)),
                span { class: "menu-icon", Icon { name: "trash", size: 16 } }
                span { "Trash" }
            }
        }
    }
}

#[component]
fn PageTreeItem(id: String, depth: usize, favorites: bool) -> Element {
    let state = use_context::<AppState>();
    let Some(page) = state.pages.read().get(&id).cloned() else { return rsx! {} };

    let key = if favorites { format!("fav:{id}") } else { id.clone() };
    let ui = state.ui.read();
    let expanded = ui.expanded.contains(&key);
    let active = matches!(ui.tab().route(), Route::Page(p) if *p == id);
    let menu_open = ui.popover == Some(Popover::SidebarMenu { page_id: key.clone() });
    drop(ui);
    let kids = if expanded { children_of(&state.pages.read(), Some(&id)) } else { Vec::new() };

    let pad = 4 + depth * 14;
    let class = format!(
        "tree-item{}{}",
        if active { " active" } else { "" },
        if menu_open { " menu-open" } else { "" }
    );
    let title = page.display_title().to_string();
    let fav_label = if page.favorite { "Remove from Favorites" } else { "Add to Favorites" };

    let (nav_id, exp_key, menu_key, add_id) = (id.clone(), key.clone(), key, id.clone());
    let (fav_id, dup_id, tab_id, trash_id) = (id.clone(), id.clone(), id.clone(), id.clone());

    rsx! {
        div {
            div {
                class: "{class}",
                style: "padding-left: {pad}px",
                draggable: "true",
                "data-page": "{id}",
                "data-fav": if favorites { "1" } else { "0" },
                onclick: move |e| {
                    // ⌥-click opens the page beside the current one.
                    if e.modifiers().alt() {
                        state.open_split(Route::Page(nav_id.clone()));
                    } else {
                        state.open_page(&nav_id);
                    }
                },
                // The middle button opens the page in a new tab. (Every button
                // that is not the left one comes here: a right-click opened
                // one more tab each time.)
                onauxclick: move |e| {
                    if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                        state.open_in_new_tab(Route::Page(tab_id.clone()));
                    }
                },
                button {
                    class: if expanded { "chevron open" } else { "chevron" },
                    title: if expanded { "Collapse" } else { "Expand" },
                    onclick: move |e| {
                        e.stop_propagation();
                        state.toggle_expanded(&exp_key);
                    },
                    Icon { name: "chevron-right", size: 12 }
                }
                span { class: "tree-icon", PageIcon { icon: page.icon.clone(), size: 16 } }
                span { class: "tree-title", "{title}" }
                div { class: "tree-actions",
                    button {
                        class: "row-btn small",
                        title: "Delete, duplicate, and more…",
                        onclick: move |e| {
                            e.stop_propagation();
                            let next = (!menu_open).then(|| Popover::SidebarMenu { page_id: menu_key.clone() });
                            state.set_popover(next);
                        },
                        Icon { name: "more-horizontal", size: 14 }
                    }
                    button {
                        class: "row-btn small",
                        title: "Add a page inside",
                        onclick: move |e| {
                            e.stop_propagation();
                            state.new_page(Some(add_id.clone()));
                        },
                        Icon { name: "plus", size: 14 }
                    }
                }
                if menu_open {
                    div { class: "popover sidebar-menu", onclick: move |e| e.stop_propagation(),
                        div { class: "menu-item compact", onclick: move |_| state.toggle_favorite(&fav_id),
                            span { class: "menu-icon", Icon { name: if page.favorite { "star-filled" } else { "star" }, size: 16 } }
                            span { "{fav_label}" }
                        }
                        div { class: "menu-item compact", onclick: move |_| state.duplicate_page(&dup_id),
                            span { class: "menu-icon", Icon { name: "duplicate", size: 16 } }
                            span { "Duplicate" }
                        }
                        div {
                            class: "menu-item compact",
                            onclick: {
                                let id = id.clone();
                                move |_| state.open_split(Route::Page(id.clone()))
                            },
                            span { class: "menu-icon", Icon { name: "sidebar", size: 16 } }
                            span { "Open in split view" }
                            span { class: "menu-shortcut", "⌥ click" }
                        }
                        div { class: "menu-item compact danger", onclick: move |_| state.trash_page(&trash_id),
                            span { class: "menu-icon", Icon { name: "trash", size: 16 } }
                            span { "Move to Trash" }
                        }
                    }
                }
            }
            if expanded {
                div { class: "tree-children",
                    if kids.is_empty() {
                        div { class: "tree-empty", style: "padding-left: {pad + 44}px", "No pages inside" }
                    }
                    for k in kids {
                        PageTreeItem { key: "{k.id}", id: k.id, depth: depth + 1, favorites }
                    }
                }
            }
        }
    }
}
