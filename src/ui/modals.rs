//! Quick find, settings and trash.

use crate::ai::{self, local::Installed, AiEvent, ChatMessage, Provider};
use super::icons::{Icon, PageIcon};
use crate::model::{Block, BlockKind, Page};
use crate::state::{ancestors, AppState, Look, Route, Theme, VoiceFor};
use crate::{backup, store, when};
use dioxus::prelude::*;
use futures_util::StreamExt;

#[derive(Clone, PartialEq)]
struct Hit {
    id: String,
    icon: String,
    title: String,
    snippet: Option<String>,
    path: String,
}

fn snippet_around(text: &str, query_lower: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let at = lower.find(query_lower)?;
    // Map the byte offset in the lowercased text back to a char index. A few
    // letters become two when they are lower-cased ("İ"), so the count can
    // run past the end of the text itself: keep it inside.
    let chars: Vec<char> = text.chars().collect();
    let char_at = lower[..at].chars().count().min(chars.len());
    let start = char_at.saturating_sub(30);
    let end = (char_at + query_lower.chars().count() + 50).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    s = s.replace('\n', " ");
    if start > 0 {
        s.insert(0, '…');
    }
    if end < chars.len() {
        s.push('…');
    }
    Some(s)
}

/// How well a page fits what was typed in search, and the line of it to
/// show. Every word must be in the page (its name or its text), in any
/// order. 0: the name starts with what was typed; 1: the name holds it;
/// 2: the name holds every word; 3: a line holds it as typed; 4: the words
/// are spread over the page. `typed`, and `name`, in small letters.
fn search_fit(typed: &str, name: &str, lines: &[String]) -> Option<(u8, Option<String>)> {
    let words: Vec<&str> = typed.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let lower: Vec<String> = lines.iter().map(|l| l.to_lowercase()).collect();
    if !words.iter().all(|w| name.contains(w) || lower.iter().any(|l| l.contains(w))) {
        return None;
    }
    // The line to show: one with the words as typed, or else the line with the most of them.
    let whole = lines.iter().find_map(|l| snippet_around(l, typed));
    let shown = whole.clone().or_else(|| {
        let count = |l: &String| words.iter().filter(|w| l.contains(**w)).count();
        let best = (0..lower.len()).max_by_key(|&i| (count(&lower[i]), std::cmp::Reverse(i))).filter(|&i| count(&lower[i]) > 0)?;
        let word = words.iter().find(|w| lower[best].contains(**w))?;
        snippet_around(&lines[best], word)
    });
    let rank = if name.starts_with(typed) {
        0
    } else if name.contains(typed) {
        1
    } else if words.iter().all(|w| name.contains(w)) {
        2
    } else if whole.is_some() {
        3
    } else {
        4
    };
    Some((rank, shown))
}

/// A command that search can run: the name of `AppState::command`, what it
/// says, its keys, and other words that find it.
#[derive(Clone, PartialEq)]
struct Cmd {
    id: &'static str,
    label: String,
    keys: &'static str,
    also: &'static str,
}

/// What search can run now: the commands of the menu bar that make sense
/// here (a page's own only with a page open, the AI's only with an AI).
fn commands_now(state: &AppState) -> Vec<Cmd> {
    let page = state.current_page();
    let locked = page.as_deref().map(|id| crate::lock::page_lock(state, id)).unwrap_or_default();
    let mut v: Vec<Cmd> = Vec::new();
    let mut add = |id: &'static str, label: &str, keys: &'static str, also: &'static str| v.push(Cmd { id, label: label.into(), keys, also });
    add("new_page", "New page", "⌘N", "add create");
    add("meeting_notes", "New meeting notes", "⌘⇧M", "call");
    add("new_window", "New window", "⌘⇧N", "");
    add("new_tab", "New tab", "⌘T", "");
    add("reopen_tab", "Reopen closed tab", "⌘⇧T", "");
    add("calendar", "Calendar", "⌘⇧C", "events plan");
    add("graph", "Graph of pages", "", "links map");
    add("toggle_sidebar", "Show or hide sidebar", "⌘\\", "");
    add("focus_mode", if state.ui.read().focus_mode { "Leave focus mode" } else { "Focus mode" }, "⌘.", "zen write distraction");
    let dark = match state.ui.read().theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => crate::ui::window_is_dark(),
    };
    add("dark_mode", if dark { "Light mode" } else { "Dark mode" }, "⌘⇧L", "dark light theme appearance");
    add("settings", "Settings", "⌘,", "preferences options");
    add("trash", "Trash", "", "deleted bin");
    add("import", "Import…", "", "notion markdown");
    add("export_all", "Export workspace…", "", "markdown backup");
    add("zoom_in", "Zoom in", "⌘=", "bigger larger");
    add("zoom_out", "Zoom out", "⌘-", "smaller");
    add("zoom_reset", "Actual size", "⌘0", "zoom");
    if state.spaces.read().len() > 1 {
        add("space_next", "Next space", "", "");
        add("space_prev", "Previous space", "", "");
    }
    let sync = crate::devsync::status();
    if sync.icloud || sync.wifi {
        add("sync_now", "Sync now", "", "iphone icloud");
    }
    if crate::ai::shown() {
        add("ai", "Ask AI", "⌘J", "assistant claude codex");
        add("ask_notes", "Ask your notes", "⌘⇧J", "assistant question");
    }
    if let Some(id) = page.as_deref() {
        if !locked.is_locked() || locked.is_open() {
            add("copy_link", "Copy link to page", "⌘L", "");
            add("copy_name", "Copy page name", "⌥⌘L", "title");
            add("copy_markdown", "Copy page as Markdown", "", "");
            add("export_page", "Export page as Markdown…", "", "");
            add("find", "Find in page", "⌘F", "search");
            add("read_aloud", "Read page aloud", "⌘⇧R", "speak");
            add("page_history", "Page history", "", "versions earlier restore");
        }
        let fav = state.pages.read().get(id).is_some_and(|p| p.favorite);
        add("favorite", if fav { "Remove from favourites" } else { "Add to favourites" }, "", "star pin");
        if locked.is_open() {
            add("lock_now", "Lock page now", "", "password close");
            add("unlock_page", "Remove page password", "", "unlock");
        } else if !locked.is_locked() {
            add("lock_page", "Lock page with a password…", "", "private secret encrypt");
        }
    }
    v
}

/// How well a command fits what was typed (small letters): every word must
/// start a word of its name or of its other words. 0: the name starts with
/// what was typed; 1: any other fit.
fn command_fit(typed: &str, cmd: &Cmd) -> Option<u8> {
    let label = cmd.label.to_lowercase();
    let words: Vec<&str> = label.split(|c: char| !c.is_alphanumeric()).chain(cmd.also.split(' ')).filter(|w| !w.is_empty()).collect();
    let typed_words: Vec<&str> = typed.split_whitespace().collect();
    if typed_words.is_empty() || !typed_words.iter().all(|t| words.iter().any(|w| w.starts_with(t))) {
        return None;
    }
    Some(if label.starts_with(typed) { 0 } else { 1 })
}

/// One line of search: a page, a command, or a new page by the typed name.
#[derive(Clone, PartialEq)]
enum Found {
    Page(Hit),
    Command(Cmd),
    Create { name: String, inside: Option<String> },
}

/// Whether a page is named by what was typed (both in small letters), by
/// the rule a command fits by: every typed word starts a word of the name.
/// "tab" names "Guitar tab book" and not "Database".
fn named_by(typed: &str, name: &str) -> bool {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut typed_words = typed.split_whitespace().peekable();
    typed_words.peek().is_some() && typed_words.all(|t| words.iter().any(|w| w.starts_with(t)))
}

/// The order of the lines of search, each list given best fit first; a
/// page comes with its fit and whether its name is named by what was typed
/// (`named_by`). A command leads only when its own name starts with what
/// was typed; otherwise the pages named by the typed words come before
/// it, and all other pages come after it. ("notes" with Return opened no
/// page named "Project notes": it made new meeting notes, because any word
/// of a command's name put it above that page. Letters inside a longer
/// word do not name a page: "tab" must not open "Database" in place of a
/// new tab.)
fn in_order(hits: Vec<(u8, bool, Hit)>, cmds: Vec<(u8, Cmd)>) -> Vec<Found> {
    let page = |h: (u8, bool, Hit)| Found::Page(h.2);
    let (first, hits): (Vec<_>, Vec<_>) = hits.into_iter().partition(|h| h.0 == 0);
    let (named, rest): (Vec<_>, Vec<_>) = hits.into_iter().partition(|h| h.0 <= 2 && h.1);
    let (own, other): (Vec<_>, Vec<_>) = cmds.into_iter().partition(|c| c.0 == 0);
    let command = |c: (u8, Cmd)| Found::Command(c.1);
    first.into_iter().map(page).chain(own.into_iter().map(command)).chain(named.into_iter().map(page)).chain(other.into_iter().map(command)).chain(rest.into_iter().map(page)).collect()
}

impl Found {
    fn key(&self) -> String {
        match self {
            Found::Page(h) => h.id.clone(),
            Found::Command(c) => format!("cmd-{}", c.id),
            Found::Create { .. } => "create".into(),
        }
    }
}

#[component]
pub fn QuickFind() -> Element {
    let state = use_context::<AppState>();
    let mut query = use_signal(String::new);
    let mut selected = use_signal(|| 0usize);

    let found = use_memo(move || {
    let q = query.read().trim().to_lowercase();
    let space = state.ui.read().space.clone();
    let pages = state.pages.read();
    let blocks = state.blocks.read();
    let path_of = |p: &Page| ancestors(&pages, &p.id).iter().map(|a| a.display_title().to_string()).collect::<Vec<_>>().join(" / ");
    // (how well it fits, when it was last changed, the hit)
    let mut hits: Vec<(u8, i64, Hit)> = Vec::new();
    if q.is_empty() {
        let recent = state.ui.read().recent.clone();
        let mut ordered: Vec<&Page> = recent.iter().filter_map(|id| pages.get(id)).filter(|p| !p.trashed && p.space_id == space).collect();
        let mut rest: Vec<&Page> = pages.values().filter(|p| !p.trashed && p.space_id == space && !recent.contains(&p.id)).collect();
        rest.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        ordered.extend(rest);
        for p in ordered.into_iter().take(30) {
            hits.push((0, 0, Hit { id: p.id.clone(), icon: p.display_icon().into(), title: p.display_title().into(), snippet: None, path: path_of(p) }));
        }
    } else {
        for p in pages.values().filter(|p| !p.trashed && p.space_id == space) {
            // (A locked page is found by its name only, also while it is open.)
            let lines: Vec<String> = blocks
                .get(&p.id)
                .filter(|bs| !crate::lock::hides(bs, &p.id))
                .map(|bs| bs.iter().map(|b| b.plain_text()).collect())
                .unwrap_or_default();
            if let Some((rank, snippet)) = search_fit(&q, &p.display_title().to_lowercase(), &lines) {
                hits.push((rank, p.updated_at, Hit { id: p.id.clone(), icon: p.display_icon().into(), title: p.display_title().into(), snippet, path: path_of(p) }));
            }
        }
        // The best fit first; among equal ones, the page changed last.
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.title.to_lowercase().cmp(&b.2.title.to_lowercase())));
        hits.truncate(30);
    }
    if q.is_empty() {
        drop(blocks);
        drop(pages);
        return (true, hits.into_iter().map(|h| Found::Page(h.2)).collect::<Vec<Found>>());
    }
    // Pages and the commands that fit (a few), see `in_order`, then a new
    // page by that name.
    let mut cmds: Vec<(u8, Cmd)> = commands_now(&state).into_iter().filter_map(|c| command_fit(&q, &c).map(|r| (r, c))).collect();
    cmds.sort_by_key(|(r, _)| *r);
    cmds.truncate(5);
    let typed = query.read().trim().to_string();
    let parts = crate::state::path_parts(&typed);
    let create = match parts.split_last() {
        Some((name, above)) => {
            let (_, missing) = crate::state::find_path(&pages, &space, &parts);
            (!missing.is_empty()).then(|| Found::Create { name: name.clone(), inside: above.last().cloned() })
        }
        None => None,
    };
    drop(blocks);
    drop(pages);
    let mut out = in_order(hits.into_iter().map(|h| (h.0, named_by(&q, &h.2.title.to_lowercase()), h.2)).collect(), cmds);
    out.extend(create);
    (false, out)
    });
    let (no_query, hits) = found();
    let count = hits.len();
    let sel = (*selected.read()).min(count.saturating_sub(1));
    let hits_for_keys = hits.clone();
    // Open a page, run a command or make the page.
    let choose = move |found: &Found, new_tab: bool| match found {
        Found::Page(hit) => {
            if new_tab {
                state.open_in_new_tab(Route::Page(hit.id.clone()))
            } else {
                state.open_page(&hit.id)
            }
        }
        Found::Command(cmd) => {
            state.set_modal(None);
            state.command(cmd.id);
        }
        Found::Create { .. } => {
            let typed = query.peek().trim().to_string();
            state.set_modal(None);
            state.new_page_at_path(&typed);
        }
    };

    rsx! {
        div { class: "modal-backdrop", onclick: move |_| state.set_modal(None),
            div { class: "modal", onclick: move |e| e.stop_propagation(),
                div { class: "qf-input-row",
                    span { class: "qf-glass", Icon { name: "search", size: 18 } }
                    input {
                        class: "qf-input",
                        placeholder: "Search pages or commands…",
                        onmounted: move |e: MountedEvent| async move {
                            let _ = e.set_focus(true).await;
                        },
                        oninput: move |e: FormEvent| {
                            query.set(e.value());
                            selected.set(0);
                        },
                        onkeydown: move |e: KeyboardEvent| {
                            match e.key() {
                                // (The chosen line stays in view: the list is
                                // longer than its box, and Return opened a
                                // page that could not be seen.)
                                Key::ArrowDown => {
                                    e.prevent_default();
                                    if count > 0 { selected.set((sel + 1) % count); }
                                    document::eval("requestAnimationFrame(() => requestAnimationFrame(() => { const el = document.querySelector('.qf-item.selected'); if (el) el.scrollIntoView({ block: 'nearest' }); }))");
                                }
                                Key::ArrowUp => {
                                    e.prevent_default();
                                    if count > 0 { selected.set((sel + count - 1) % count); }
                                    document::eval("requestAnimationFrame(() => requestAnimationFrame(() => { const el = document.querySelector('.qf-item.selected'); if (el) el.scrollIntoView({ block: 'nearest' }); }))");
                                }
                                Key::Enter => {
                                    e.prevent_default();
                                    if let Some(found) = hits_for_keys.get(sel) {
                                        choose(found, e.modifiers().meta());
                                    }
                                }
                                Key::Escape => state.set_modal(None),
                                _ => {}
                            }
                        },
                    }
                }
                div { class: "qf-results",
                    if hits.is_empty() {
                        div { class: "menu-empty", "No results" }
                    } else if no_query {
                        div { class: "menu-section", "Recent pages" }
                    }
                    for (i, found) in hits.into_iter().enumerate() {
                        div {
                            key: "{found.key()}",
                            class: if i == sel { "qf-item selected" } else { "qf-item" },
                            onmouseenter: move |_| selected.set(i),
                            onclick: {
                                let found = found.clone();
                                move |e: MouseEvent| choose(&found, e.modifiers().meta())
                            },
                            match &found {
                                Found::Page(hit) => rsx! {
                                    span { class: "qf-icon", PageIcon { icon: Some(hit.icon.clone()), size: 18 } }
                                    div { class: "qf-text",
                                        div { class: "qf-title", "{hit.title}" }
                                        if let Some(s) = &hit.snippet {
                                            div { class: "qf-snippet", "{s}" }
                                        }
                                    }
                                    if !hit.path.is_empty() {
                                        span { class: "qf-path", "{hit.path}" }
                                    }
                                },
                                Found::Command(cmd) => rsx! {
                                    span { class: "qf-icon qf-cmd-icon", Icon { name: "chevrons-right", size: 15 } }
                                    div { class: "qf-text",
                                        div { class: "qf-title qf-cmd", "{cmd.label}" }
                                    }
                                    if !cmd.keys.is_empty() {
                                        span { class: "qf-keys", "{cmd.keys}" }
                                    }
                                },
                                Found::Create { name, inside } => rsx! {
                                    span { class: "qf-icon qf-cmd-icon", Icon { name: "plus", size: 16 } }
                                    div { class: "qf-text",
                                        div { class: "qf-title qf-cmd",
                                            "New page "
                                            span { class: "qf-new-name", "“{name}”" }
                                            if let Some(inside) = inside {
                                                " in "
                                                span { class: "qf-new-name", "“{inside}”" }
                                            }
                                        }
                                    }
                                },
                            }
                        }
                    }
                }
                div { class: "qf-footer", "↑↓ select  ·  ↵ open  ·  ⌘↵ open in new tab  ·  esc close" }
            }
        }
    }
}

/// Tint swatches for the glass: (label, colour). "" is neutral grey.
const TINTS: &[(&str, &str)] = &[
    ("Neutral", ""),
    ("Blue", "#3a7dd2"),
    ("Purple", "#8858d4"),
    ("Pink", "#cf4f97"),
    ("Red", "#d94848"),
    ("Orange", "#e0742a"),
    ("Yellow", "#cf9418"),
    ("Green", "#3c9862"),
];

/// The parts of Settings, in the order they're listed.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Appearance,
    Space,
    Calendar,
    Sync,
    Ai,
    Notifications,
    Data,
}

impl Section {
    const ALL: [Section; 7] = [Section::Appearance, Section::Space, Section::Calendar, Section::Sync, Section::Ai, Section::Notifications, Section::Data];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Space => "Space",
            Section::Calendar => "Calendar",
            Section::Sync => "Sync",
            Section::Ai => "AI",
            Section::Notifications => "Notifications",
            Section::Data => "Data",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Appearance => "palette",
            Section::Space => "folder",
            Section::Calendar => "calendar",
            Section::Sync => "phone",
            Section::Ai => "sparkles",
            Section::Notifications => "bell",
            Section::Data => "inbox",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Section::Appearance => "The look of the window.",
            Section::Space => "The space you're in.",
            Section::Calendar => "Other calendars, and reminders.",
            Section::Sync => "Your notes on your iPhone too, with no server between.",
            Section::Ai => "Ask AI uses a tool on this Mac, with your own account. It sees only the page you ask about.",
            Section::Notifications => "Reminders, and notes for meetings.",
            Section::Data => "Your notes are in one file on this Mac.",
        }
    }
}

/// Settings, laid out like Aven's: the sections down the left, the chosen
/// section's settings on the right as rows on cards (what it is and what it
/// does on the left, its control on the right). Esc or "Back to notes" closes it.
#[component]
pub fn SettingsView() -> Element {
    let state = use_context::<AppState>();
    // Something can send the person to one part (the AI box, with no AI tool).
    let mut first = state.settings_first;
    let mut section = use_signal(|| {
        let wanted = first.peek().and_then(|name| Section::ALL.into_iter().find(|s| s.label() == name));
        wanted.unwrap_or(Section::Appearance)
    });
    use_effect(move || {
        if first.peek().is_some() {
            first.set(None);
        }
    });
    let current = section();
    rsx! {
        div { class: "settings-view", role: "dialog", "aria-label": "Settings",
            nav { class: "settings-nav",
                button { class: "settings-back", onclick: move |_| state.set_modal(None),
                    Icon { name: "chevron-left", size: 16 }
                    span { "Back to notes" }
                    span { class: "settings-key", "esc" }
                }
                div { class: "settings-nav-title", "Settings" }
                for s in Section::ALL {
                    button {
                        class: if current == s { "settings-nav-item on" } else { "settings-nav-item" },
                        "data-section": "{s.label()}",
                        onclick: move |_| section.set(s),
                        span { class: "settings-nav-icon", Icon { name: s.icon(), size: 15 } }
                        span { "{s.label()}" }
                    }
                }
            }
            div { class: "settings-card",
                div { class: "settings-scroll",
                    // Keyed by section, so each one settles in as it opens.
                    for s in [current] {
                        div { key: "{s.label()}", class: "settings-content",
                            header { class: "settings-page-header",
                                h1 { "{s.label()}" }
                                p { "{s.blurb()}" }
                            }
                            match s {
                                Section::Appearance => rsx! { AppearanceSettings {} },
                                Section::Space => rsx! { SpaceSettings {} },
                                Section::Calendar => rsx! { CalendarSettings {} },
                                Section::Sync => rsx! { SyncSettings {} },
                                Section::Ai => rsx! {
                                    AiSettings {}
                                    VoiceSettings {}
                                    ConnectorSettings {}
                                },
                                Section::Notifications => rsx! { NotificationSettings {} },
                                Section::Data => rsx! { DataSettings {} },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A titled card of settings rows.
#[component]
fn Group(#[props(default, into)] title: String, children: Element) -> Element {
    rsx! {
        section { class: "set-group",
            if !title.is_empty() {
                h2 { "{title}" }
            }
            div { class: "set-card", {children} }
        }
    }
}

/// One setting: what it is (and a line on what it does), then its control.
#[component]
fn Row(#[props(into)] label: String, #[props(default, into)] note: String, #[props(default)] tone: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "set-row",
            div { class: "set-copy",
                div { class: "set-label", "{label}" }
                if !note.is_empty() {
                    div { class: "set-note {tone}", "{note}" }
                }
            }
            div { class: "set-control", {children} }
        }
    }
}

#[component]
fn AppearanceSettings() -> Element {
    let state = use_context::<AppState>();
    let theme = state.ui.read().theme;
    let look = state.look.read().clone();
    let custom_tint = if look.tint.is_empty() || TINTS.iter().any(|(_, c)| *c == look.tint) {
        "#7a8a99".to_string()
    } else {
        look.tint.clone()
    };
    let is_custom = !look.tint.is_empty() && !TINTS.iter().any(|(_, c)| *c == look.tint);
    let glass_label = match look.glass {
        0 => "Solid".to_string(),
        g => format!("{g}%"),
    };
    let blur_max = crate::glass::BLUR_MAX;
    let blur_fill = u32::from(look.blur.min(blur_max)) * 100 / u32::from(blur_max);
    let blur_label = match look.blur {
        0 => "None".to_string(),
        _ => format!("{blur_fill}%"),
    };
    // A solid window shows nothing behind it, so there is nothing to blur.
    let solid = look.glass == 0;
    rsx! {
        Group {
            Row { label: "Theme", note: "⌘⇧L switches light and dark.",
                div { class: "segmented",
                    for (label, value) in [("System", Theme::System), ("Light", Theme::Light), ("Dark", Theme::Dark)] {
                        button {
                            class: if theme == value { "on" } else { "" },
                            onclick: move |_| state.set_theme(value),
                            "{label}"
                        }
                    }
                }
            }
            Row { label: "Accent", note: "The colour of links, checkboxes and switches.",
                div { class: "tints accents",
                    for (label, id, swatch) in crate::state::ACCENTS {
                        button {
                            class: if look.accent == id { "tint on" } else { "tint" },
                            title: "{label}",
                            "aria-label": "{label}",
                            "data-accent-pick": "{id}",
                            style: "--c: {swatch}",
                            onclick: {
                                let look = look.clone();
                                move |_| state.set_look(Look { accent: id.to_string(), ..look.clone() }, true)
                            },
                        }
                    }
                }
            }
        }
        Group { title: "Window glass",
            Row { label: "Transparency", note: "How much shows through the window.",
                div { class: "set-slider",
                    input {
                        class: "slider",
                        r#type: "range",
                        min: "0",
                        max: "100",
                        step: "1",
                        value: "{look.glass}",
                        style: "--fill: {look.glass}%",
                        id: "glass-amount",
                        "aria-label": "Transparency",
                        oninput: {
                            let look = look.clone();
                            move |e: FormEvent| {
                                let glass = e.value().parse::<u8>().unwrap_or(look.glass).min(100);
                                state.set_look(Look { glass, ..look.clone() }, false);
                            }
                        },
                        onchange: {
                            let look = look.clone();
                            move |e: FormEvent| {
                                let glass = e.value().parse::<u8>().unwrap_or(look.glass).min(100);
                                state.set_look(Look { glass, ..look.clone() }, true);
                            }
                        },
                    }
                    span { class: "set-value", "{glass_label}" }
                }
            }
            Row { label: "Blur", note: "How soft the desktop looks behind it.",
                div { class: "set-slider",
                    input {
                        class: "slider",
                        id: "glass-blur",
                        r#type: "range",
                        min: "0",
                        max: "{blur_max}",
                        step: "1",
                        value: "{look.blur}",
                        style: "--fill: {blur_fill}%",
                        disabled: solid,
                        "aria-label": "Blur",
                        oninput: {
                            let look = look.clone();
                            move |e: FormEvent| {
                                let blur = e.value().parse::<u8>().unwrap_or(look.blur).min(blur_max);
                                state.set_look(Look { blur, ..look.clone() }, false);
                            }
                        },
                        onchange: {
                            let look = look.clone();
                            move |e: FormEvent| {
                                let blur = e.value().parse::<u8>().unwrap_or(look.blur).min(blur_max);
                                state.set_look(Look { blur, ..look.clone() }, true);
                            }
                        },
                    }
                    span { class: "set-value", "{blur_label}" }
                }
            }
            Row { label: "Tint", note: "The colour of the glass.",
                div { class: "tints",
                    for (label, color) in TINTS {
                        button {
                            class: if look.tint == *color { "tint on" } else { "tint" },
                            title: "{label}",
                            style: if color.is_empty() { "".to_string() } else { format!("--c: {color}") },
                            onclick: {
                                let look = look.clone();
                                move |_| state.set_look(Look { tint: color.to_string(), ..look.clone() }, true)
                            },
                        }
                    }
                    label {
                        class: if is_custom { "tint custom on" } else { "tint custom" },
                        title: "Any colour",
                        style: "--c: {custom_tint}",
                        input {
                            r#type: "color",
                            value: "{custom_tint}",
                            oninput: {
                                let look = look.clone();
                                move |e: FormEvent| {
                                    let c = e.value();
                                    if crate::state::valid_hex(&c) {
                                        state.set_look(Look { tint: c, ..look.clone() }, false);
                                    }
                                }
                            },
                            onchange: {
                                let look = look;
                                move |e: FormEvent| {
                                    let c = e.value();
                                    if crate::state::valid_hex(&c) {
                                        state.set_look(Look { tint: c, ..look.clone() }, true);
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SpaceSettings() -> Element {
    let state = use_context::<AppState>();
    let space = state.space();
    let space_name = space.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let space_color = space.as_ref().map(|s| s.color.clone()).unwrap_or_else(|| "blue".into());
    let space_id = space.as_ref().map(|s| s.id.clone()).unwrap_or_default();
    let space_count = state.spaces.read().len();
    rsx! {
        Group { title: "This space",
            Row { label: "Name",
                // Keyed by space, so switching spaces shows the new name.
                for id in [space_id] {
                    input {
                        key: "{id}",
                        class: "text-input",
                        "aria-label": "Space name",
                        initial_value: "{space_name}",
                        oninput: move |e: FormEvent| state.rename_space(e.value()),
                    }
                }
            }
            Row { label: "Colour", note: "For its badge in the sidebar.",
                div { class: "tints",
                    for (color, label) in super::icons::ICON_COLORS {
                        button {
                            class: if space_color == *color { "tint on" } else { "tint" },
                            title: "{label}",
                            style: "--c: var(--icon-{color})",
                            onclick: move |_| state.set_space_color(color),
                        }
                    }
                }
            }
        }
        if space_count > 1 {
            Group {
                Row { label: "Delete this space", note: "Only when it has no pages left.",
                    button { class: "secondary-btn danger-text", onclick: move |_| state.delete_space(), "Delete" }
                }
            }
        }
    }
}

#[component]
fn AiSettings() -> Element {
    let app = use_context::<AppState>();
    let mut test = use_signal(|| None::<(bool, String)>);
    let mut testing = use_signal(|| false);
    let mut chosen = use_signal(|| store::setting(ai::PROVIDER_SETTING).and_then(|s| Provider::from_id(&s)));
    let mut lookups = use_signal(|| 0u32);
    // Look for installed AI tools off the UI thread; "Look again" re-runs it.
    let tools = use_resource(move || async move {
        let refresh = lookups() > 0;
        let found = ai::local::installed_async(refresh || ai::local::installed_cached().is_none()).await;
        // A tool that was connected a moment ago: the AI parts show at once.
        app.show_ai_state();
        found
    });
    let found: Option<Vec<Installed>> = tools.read().clone();
    let looking = found.is_none();
    let found = found.unwrap_or_default();
    let prefs = ai::Prefs { provider: *chosen.read(), ..ai::Prefs::load() };
    let active = ai::pick_provider(&prefs, &found);
    let automatic = chosen.read().is_none();
    let installed = |p: Provider| found.iter().any(|i| i.provider == p);
    let status = |p: Provider| match found.iter().find(|i| i.provider == p) {
        _ if looking => "Looking…".to_string(),
        Some(Installed { version: Some(v), .. }) => format!("Installed · {v}"),
        Some(_) => "Installed".to_string(),
        None => "Not installed".to_string(),
    };
    // (choice, name, line under it, ready to use)
    let mut choices: Vec<(Option<Provider>, String, String, bool)> = vec![(
        None,
        "Automatic".to_string(),
        match active {
            Some(p) if automatic => format!("Using {}: the first one installed.", p.name()),
            _ if looking => "Looking…".to_string(),
            _ if active.is_none() => ai::NO_AI.to_string(),
            _ => "Uses Claude Code if it's installed, otherwise Codex.".to_string(),
        },
        active.is_some(),
    )];
    for p in Provider::ALL {
        choices.push((Some(p), p.name().to_string(), status(p), installed(p)));
    }
    let claude_model = ai::normalize_model(Provider::ClaudeCode, &store::setting(Provider::ClaudeCode.model_setting()).unwrap_or_default());
    let codex_model = store::setting(Provider::Codex.model_setting()).unwrap_or_default();

    let run_test = move |_| {
        testing.set(true);
        test.set(None);
        let prefs = ai::Prefs::load();
        let label = ai::active_label(&prefs);
        spawn(async move {
            let messages = vec![ChatMessage::system("Reply in one short, friendly sentence."), ChatMessage::user("Hello")];
            let mut rx = ai::start(prefs, messages);
            let mut reply = String::new();
            let mut outcome = None;
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Step(..) => {}
                    AiEvent::Working { .. } => {}
                    AiEvent::Delta(t) => reply.push_str(&t),
                    AiEvent::Done => {
                        outcome = Some((true, format!("{label} replied: “{}”", reply.trim().chars().take(160).collect::<String>())));
                        break;
                    }
                    AiEvent::Error(e) => {
                        outcome = Some((false, e));
                        break;
                    }
                }
            }
            test.set(outcome.or(Some((false, "No reply.".into()))));
            testing.set(false);
        });
    };
    let (test_note, test_tone) = match test.read().clone() {
        Some((true, msg)) => (msg, "ok"),
        Some((false, msg)) => (msg, "err"),
        None => ("Sends a short hello and shows the reply.".to_string(), ""),
    };

    rsx! {
        section { class: "set-group",
            div { class: "set-group-head",
                h2 { "AI tool" }
                button { class: "link-btn", disabled: looking, onclick: move |_| lookups += 1,
                    if looking { "Looking…" } else { "Look again" }
                }
            }
            div { class: "set-card",
                for (choice, name, line, ready) in choices {
                    button {
                        key: "{name}",
                        class: if chosen() == choice { "set-row choice on" } else { "set-row choice" },
                        role: "radio",
                        "aria-checked": "{chosen() == choice}",
                        onclick: move |_| {
                            store::set_setting(ai::PROVIDER_SETTING, choice.map(|p| p.id()).unwrap_or(""));
                            chosen.set(choice);
                            app.show_ai_state();
                            test.set(None);
                        },
                        div { class: "set-copy",
                            div { class: "set-label", "{name}" }
                            div { class: if ready { "set-note ok" } else { "set-note" }, "{line}" }
                        }
                        div { class: "set-control", span { class: "radio" } }
                    }
                }
            }
        }
        match active {
            Some(provider) => rsx! {
                Group { title: "Model",
                    Row {
                        label: "Model",
                        note: format!("Which {} model answers.", provider.name()),
                        ModelPicker { provider, current: if provider == Provider::ClaudeCode { claude_model.clone() } else { codex_model.clone() } }
                    }
                    if !looking && !installed(provider) {
                        Row {
                            label: "Not installed",
                            tone: "err",
                            note: if provider == Provider::ClaudeCode {
                                "Get Claude Code at claude.com/claude-code, then press Look again."
                            } else {
                                "Install Codex with npm install -g @openai/codex, then press Look again."
                            },
                        }
                    }
                    Row {
                        label: "Signing in",
                        note: if provider == Provider::ClaudeCode {
                            "Not signed in yet? Run claude in Terminal once and sign in."
                        } else {
                            "Not signed in yet? Run codex login in Terminal once."
                        },
                    }
                }
                Group {
                    Row { label: "Try it", note: test_note, tone: test_tone,
                        button { class: "secondary-btn", disabled: testing(), onclick: run_test,
                            if testing() { "Testing…" } else { "Test" }
                        }
                    }
                }
            },
            None => rsx! {},
        }
    }
}

/// Updates: which build this is, whether a newer one is ready, and how
/// updates arrive (see update.rs).
#[component]
fn UpdateSettings() -> Element {
    use crate::update;
    let state = use_context::<AppState>();
    let mut auto = use_signal(update::automatic);
    let mut source = use_signal(|| update::source().display().to_string());
    let custom = source() != update::DEFAULT_SOURCE;
    // With no build folder on this Mac, newer versions come from the published releases.
    let released = update::from_releases();
    let mut online = use_signal(update::online);
    let installed = update::own_app().is_some() && update::own_build().is_some();
    let ready = state.update.read().clone();
    let status = match (&ready, installed) {
        (Some(u), _) => format!("{}. A newer build is ready: {}.", update::describe(), update::build_label(u.build)),
        (None, true) => format!("{}. Up to date.", update::describe()),
        (None, false) => format!("{}. This copy runs from the build folder, so it does not update itself.", update::describe()),
    };
    rsx! {
        Group { title: "Updates",
            Row { label: "Rune", note: status,
                if ready.is_some() {
                    button { class: "primary-btn", id: "update-now", onclick: move |_| state.install_update(), "Update and restart" }
                } else if installed {
                    button { class: "secondary-btn", id: "update-check", onclick: move |_| state.check_update(true), "Check now" }
                }
            }
            Row {
                label: "Update automatically",
                note: "Installs a newer build when the app opens.",
                button {
                    id: "update-auto",
                    class: if auto() { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{auto()}",
                    "aria-label": "Update automatically",
                    onclick: move |_| {
                        let on = !auto();
                        update::set_automatic(on);
                        auto.set(on);
                    },
                    span { class: "knob" }
                }
            }
            if released {
                Row {
                    label: "Look for new versions",
                    note: "Asks the Rune page on GitHub for the newest version one time a day. Nothing of yours is sent.",
                    button {
                        id: "update-online",
                        class: if online() { "switch on" } else { "switch" },
                        role: "switch",
                        "aria-checked": "{online()}",
                        "aria-label": "Look for new versions",
                        onclick: move |_| {
                            let on = !online();
                            update::set_online(on);
                            online.set(on);
                        },
                        span { class: "knob" }
                    }
                }
                Row { label: "Get a version by hand", note: "The page with every version of Rune.",
                    button { class: "secondary-btn", onclick: move |_| state.open_url(update::RELEASES_PAGE), "Open the page" }
                }
            }
            if !released {
            Row { label: "Where updates come from", note: source(), tone: "mono",
                if custom {
                    button {
                        class: "ghost-btn",
                        onclick: move |_| {
                            update::set_source(None);
                            source.set(update::DEFAULT_SOURCE.to_string());
                            state.check_update(false);
                        },
                        "Use the usual place"
                    }
                }
                button {
                    class: "secondary-btn",
                    onclick: move |_| async move {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Choose the Rune app that newer builds arrive as")
                            .add_filter("App", &["app"])
                            .pick_file()
                            .await;
                        if let Some(app) = picked {
                            update::set_source(Some(app.path()));
                            source.set(app.path().display().to_string());
                            state.check_update(true);
                        }
                    },
                    "Choose…"
                }
            }
            }
        }
    }
}

/// Voice: how the app hears you, and the shortcut that opens the AI from any app.
#[component]
fn VoiceSettings() -> Element {
    let mut anywhere = use_signal(|| store::setting("voice_anywhere").as_deref() != Some("off"));
    let mut engine = use_signal(|| if crate::listen::own_ears() { "builtin" } else { "dictation" });
    let can_listen = crate::listen::available();
    rsx! {
        Group { title: "Voice",
            Row {
                label: "Hear me with",
                note: "For the voice box and the microphones beside text boxes.",
                select {
                    id: "voice-engine",
                    class: "select",
                    "aria-label": "Hear me with",
                    onchange: move |e: FormEvent| {
                        let v = if e.value() == "builtin" { "builtin" } else { "dictation" };
                        store::set_setting("voice_engine", v);
                        engine.set(v);
                    },
                    option { value: "dictation", selected: engine() == "dictation", "The Mac's Dictation" }
                    option { value: "builtin", selected: engine() == "builtin", disabled: !can_listen, "Rune's listener" }
                }
            }
            Row {
                label: "AI from anywhere",
                note: "⌃⌥Space in any app opens Ask AI.",
                button {
                    id: "voice-anywhere",
                    class: if anywhere() { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{anywhere()}",
                    "aria-label": "AI from anywhere",
                    onclick: move |_| {
                        let on = !anywhere();
                        store::set_setting("voice_anywhere", if on { "on" } else { "off" });
                        anywhere.set(on);
                    },
                    span { class: "knob" }
                }
            }
        }
    }
}

/// Connect AI tools: what to give Claude Code, Codex, the Claude app, Cursor
/// and others so they can read and write this workspace (the MCP connector,
/// which is this app started with `--mcp`).
#[component]
fn ConnectorSettings() -> Element {
    use crate::mcp::connect;
    let state = use_context::<AppState>();
    let program = use_hook(connect::program);
    let (p1, p2, p3, p4) = (program.clone(), program.clone(), program.clone(), program.clone());
    rsx! {
        Group { title: "Connect AI tools",
            Row {
                label: "Rune connector",
                note: "Lets an AI tool read and write your pages on this Mac. Give each tool its lines one time.",
            }
            Row { label: "Claude Code", note: "Run this once in Terminal.",
                button {
                    class: "secondary-btn",
                    "data-copy": "claude-code",
                    onclick: move |_| state.copy_to_clipboard(connect::claude_code(&p1), "Copied. Paste it in Terminal"),
                    "Copy command"
                }
            }
            Row { label: "Codex", note: "Add these lines to ~/.codex/config.toml.",
                button {
                    class: "secondary-btn",
                    "data-copy": "codex",
                    onclick: move |_| state.copy_to_clipboard(connect::codex(&p2), "Copied the lines for Codex"),
                    "Copy lines"
                }
            }
            Row {
                label: "Claude app, Cursor and others",
                note: "For the MCP settings file of the tool.",
                button {
                    class: "secondary-btn",
                    "data-copy": "json",
                    onclick: move |_| state.copy_to_clipboard(connect::json(&p3), "Copied the JSON"),
                    "Copy JSON"
                }
            }
            Row { label: "The command itself", note: connect::command(&program), tone: "mono",
                button {
                    class: "ghost-btn",
                    "data-copy": "command",
                    onclick: move |_| state.copy_to_clipboard(connect::command(&p4), "Copied the command"),
                    "Copy"
                }
            }
        }
    }
}

/// Pick the model a tool uses. Anything chosen before that isn't in the list
/// stays available as a custom choice.
#[component]
fn ModelPicker(provider: Provider, current: String) -> Element {
    let mut choices = ai::model_choices(provider);
    if !current.is_empty() && !choices.iter().any(|(id, _)| *id == current) {
        choices.push((current.clone(), format!("{current} (custom)")));
    }
    rsx! {
        select {
            class: "select",
            "aria-label": "Model",
            onchange: move |e: FormEvent| store::set_setting(provider.model_setting(), e.value().trim()),
            for (id, label) in choices {
                option { value: "{id}", selected: id == current, "{label}" }
            }
        }
    }
}

/// The calendar: the calendars of this Mac (off until the person turns
/// them on), and the reminder a new event starts with.
/// Sync with the iPhone (src/devsync.rs): two switches, both off at first,
/// the code that connects an iPhone over Wi-Fi, and where things stand.
#[component]
fn SyncSettings() -> Element {
    let state = use_context::<AppState>();
    let mut status = use_signal(crate::devsync::status);
    let mut paired = use_signal(crate::devsync::paired);
    let mut offer = use_signal(|| None::<crate::devsync::wifi::Offer>);
    let mut busy = use_signal(|| false);
    // Where things stand changes in the background: look every two seconds.
    use_future(move || async move {
        loop {
            crate::ai::sleep_ms(2_000).await;
            let now = crate::devsync::status();
            if *status.peek() != now {
                status.set(now);
            }
            paired.set(crate::devsync::paired());
            if offer.peek().as_ref().is_some_and(|o| o.expires_ms < crate::model::now_ms()) {
                offer.set(None);
            }
        }
    });
    // The code dies with the screen that shows it.
    use_drop(crate::devsync::withdraw);
    let s = status();
    let mut switch = move |icloud: bool, wifi: bool| {
        busy.set(true);
        if let Err(e) = crate::devsync::set(icloud, wifi) {
            state.toast(format!("Sync did not start: {e}"));
        }
        if !wifi {
            offer.set(None);
        }
        status.set(crate::devsync::status());
        busy.set(false);
    };
    let icloud_note = if !s.icloud {
        "Works anywhere, also while this Mac sleeps."
    } else if s.icloud_ready {
        "On. Through your own iCloud Drive."
    } else {
        "Waiting for the iPhone: open Rune on it once, with iCloud Drive on."
    };
    let state_note = if let Some(fault) = &s.fault {
        format!("Sync stopped: {fault}")
    } else if s.needs_update {
        "Another device writes a newer form. Update Rune on this Mac.".to_string()
    } else if s.last_ms > 0 {
        let ago = crate::when::ago(s.last_ms).to_lowercase();
        if s.waiting { format!("Synced {ago}. Some files are still on their way.") } else { format!("Synced {ago}.") }
    } else {
        "Starting…".to_string()
    };
    let (icloud, wifi) = (s.icloud, s.wifi);
    let state_tone: &'static str = if s.fault.is_some() { "err" } else { "" };
    let icloud_tone: &'static str = if icloud && s.icloud_ready { "ok" } else { "" };
    rsx! {
        Group { title: "Ways to sync",
            Row { label: "Through iCloud Drive", note: icloud_note, tone: icloud_tone,
                button {
                    id: "sync-icloud-switch",
                    class: if icloud { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{icloud}",
                    "aria-label": "Sync through iCloud Drive",
                    disabled: busy(),
                    onclick: move |_| switch(!icloud, wifi),
                    span { class: "knob" }
                }
            }
            Row { label: "Over Wi-Fi", note: "Straight from this Mac, when the iPhone is on the same Wi-Fi and Rune is open on both.",
                button {
                    id: "sync-wifi-switch",
                    class: if wifi { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{wifi}",
                    "aria-label": "Sync over Wi-Fi",
                    disabled: busy(),
                    onclick: move |_| switch(icloud, !wifi),
                    span { class: "knob" }
                }
            }
        }
        if wifi {
            Group { title: "iPhones",
                if let Some(o) = offer() {
                    div { class: "sync-code",
                        div { class: "sync-qr", dangerous_inner_html: "{o.qr_svg}" }
                        div { class: "sync-code-copy",
                            div { class: "set-label", "Point the camera of your iPhone at this code" }
                            div { class: "set-note", "It opens Rune on the iPhone and connects it. The code works once, for 10 minutes." }
                            button {
                                class: "secondary-btn",
                                onclick: move |_| {
                                    crate::devsync::withdraw();
                                    offer.set(None);
                                },
                                "Done"
                            }
                        }
                    }
                } else {
                    Row { label: "Connect an iPhone", note: "Shows a code for the iPhone's camera.",
                        button {
                            id: "sync-connect",
                            class: "secondary-btn",
                            onclick: move |_| match crate::devsync::offer() {
                                Ok(o) => offer.set(Some(o)),
                                Err(e) => state.toast(format!("No code: {e}")),
                            },
                            "Connect an iPhone…"
                        }
                    }
                }
                for p in paired() {
                    div { key: "{p.device}", class: "set-row",
                        div { class: "set-copy",
                            div { class: "set-label", "{p.name}" }
                            div { class: "set-note", "Connected {crate::when::ago(p.at_ms).to_lowercase()}" }
                        }
                        div { class: "set-control",
                            button {
                                class: "ghost-btn",
                                onclick: {
                                    let device = p.device.clone();
                                    move |_| {
                                        if let Err(e) = crate::devsync::forget(&device) {
                                            state.toast(format!("Not removed: {e}"));
                                        }
                                        paired.set(crate::devsync::paired());
                                    }
                                },
                                "Remove"
                            }
                        }
                    }
                }
            }
        }
        if icloud || wifi {
            Group { title: "Where it stands",
                Row { label: "This Mac", note: state_note.clone(), tone: state_tone }
                for (key, name, seen) in s.devices.iter().map(|d| (
                    d.device.clone(),
                    if d.name.is_empty() { "A device".to_string() } else { d.name.clone() },
                    format!("{}, seen {}", if d.kind == "iphone" { "iPhone" } else { "Mac" }, crate::when::ago(d.at).to_lowercase()),
                )) {
                    div { key: "{key}", class: "set-row",
                        div { class: "set-copy",
                            div { class: "set-label", "{name}" }
                            div { class: "set-note", "{seen}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn CalendarSettings() -> Element {
    use crate::maccal::Access;
    let state = use_context::<AppState>();
    let mac = state.mac_cal.read().clone();
    let default_reminder = crate::notify::default_reminder();
    let (note, tone) = match (mac.on, mac.access) {
        (false, _) => ("From the Calendar app: iCloud, Google, Outlook. Read only.", ""),
        (true, Access::Allowed) => ("On. Read only, and they stay on this Mac.", "ok"),
        (true, Access::Denied) => ("macOS does not let Rune read your calendars.", ""),
        (true, Access::Unavailable) => ("Only the installed app can read your calendars.", ""),
        (true, Access::NotAsked) | (true, Access::Unknown) => ("Waiting for your answer to macOS…", ""),
    };
    let on = mac.on;
    let allowed = on && mac.access == Access::Allowed;
    rsx! {
        Group { title: "Calendars of this Mac",
            Row { label: "Show their events", note, tone,
                if on && mac.access == Access::Denied {
                    button { class: "secondary-btn", onclick: move |_| crate::maccal::open_settings(), "Open System Settings" }
                }
                button {
                    id: "mac-calendars-switch",
                    class: if on { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{on}",
                    "aria-label": "Show the events of the calendars of this Mac",
                    onclick: move |_| state.set_mac_calendars(!on),
                    span { class: "knob" }
                }
            }
            if allowed {
                for c in mac.calendars.clone() {
                    div { key: "{c.id}", class: "set-row", "data-calendar": "{c.title}",
                        div { class: "set-copy with-dot",
                            span {
                                class: "set-dot",
                                style: if c.color.is_empty() { "background: var(--text-3);".to_string() } else { format!("background: {};", c.color) },
                            }
                            div {
                                div { class: "set-label", "{c.title}" }
                                if !c.source.is_empty() && c.source != c.title {
                                    div { class: "set-note", "{c.source}" }
                                }
                            }
                        }
                        div { class: "set-control",
                            button {
                                class: if mac.hidden.contains(&c.id) { "switch" } else { "switch on" },
                                role: "switch",
                                "aria-checked": "{!mac.hidden.contains(&c.id)}",
                                "aria-label": "Show {c.title}",
                                onclick: {
                                    let (id, shown) = (c.id.clone(), !mac.hidden.contains(&c.id));
                                    move |_| state.show_mac_calendar(&id, !shown)
                                },
                                span { class: "knob" }
                            }
                        }
                    }
                }
            }
        }
        if allowed {
            Group { title: "Changes from Rune",
                Row { label: "Let Rune change them", note: "Drag one of their events to another time. Not a meeting with other people in it.",
                    button {
                        id: "mac-write-switch",
                        class: if mac.write { "switch on" } else { "switch" },
                        role: "switch",
                        "aria-checked": "{mac.write}",
                        "aria-label": "Let Rune change the calendars of this Mac",
                        onclick: move |_| state.set_mac_write(!mac.write),
                        span { class: "knob" }
                    }
                }
                if mac.write {
                    Row { label: "Make new events in", note: "For events you type in words or draw on the hours.",
                        select {
                            id: "mac-new-in",
                            class: "select",
                            "aria-label": "Make new events in",
                            onchange: move |e: FormEvent| state.set_mac_new_in(&e.value()),
                            option { value: "", selected: mac.new_in.is_empty(), "Rune" }
                            for c in mac.calendars.iter().filter(|c| c.writable) {
                                option { key: "{c.id}", value: "{c.id}", selected: mac.new_in == c.id, "{c.title}" }
                            }
                        }
                    }
                }
            }
        }
        Group { title: "New events",
            Row { label: "Reminder", note: "Each event can change its own.",
                select {
                    class: "select",
                    "aria-label": "Reminder for new events",
                    onchange: move |e: FormEvent| store::set_setting("default_reminder", &e.value()),
                    for (m, label) in crate::notify::REMINDERS {
                        option {
                            value: m.map(|x| x.to_string()).unwrap_or_else(|| "none".into()),
                            selected: default_reminder == m,
                            "{label}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NotificationSettings() -> Element {
    let mut notif_refresh = use_signal(|| 0u32);
    let notif = use_resource(move || async move {
        let _ = notif_refresh();
        crate::notify::permission().await
    });
    let mut meeting_notes = use_signal(crate::meeting::enabled);
    let mut meeting_pad = use_signal(|| crate::store::setting("meeting_pad").as_deref() != Some("off"));
    let permission = *notif.read();
    let (text, tone) = match permission {
        None => ("Checking…", ""),
        Some(crate::notify::Permission::Allowed) => ("On. Reminders arrive even when the app is closed.", "ok"),
        Some(crate::notify::Permission::NotAsked) => ("Not turned on yet.", ""),
        Some(crate::notify::Permission::Denied) => ("Turned off for Rune in System Settings.", ""),
        Some(crate::notify::Permission::Unavailable) => ("Only the installed app can show notifications.", ""),
    };
    rsx! {
        Group {
            Row { label: "Notifications", note: text, tone,
                if permission == Some(crate::notify::Permission::NotAsked) {
                    button {
                        class: "secondary-btn",
                        onclick: move |_| {
                            spawn(async move {
                                crate::notify::request_permission().await;
                                notif_refresh += 1;
                                crate::notify::mark_dirty();
                            });
                        },
                        "Turn on"
                    }
                }
                if permission == Some(crate::notify::Permission::Denied) {
                    button { class: "secondary-btn", onclick: move |_| crate::notify::open_settings(), "Open System Settings" }
                }
            }
        }
        Group { title: "Meetings",
            Row {
                label: "Meeting notes",
                note: "Makes a notes page when a call starts: in Teams, Zoom, Webex, Slack, Discord or FaceTime, or in a browser. Nothing is recorded.",
                button {
                    id: "meeting-notes-switch",
                    class: if meeting_notes() { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{meeting_notes()}",
                    "aria-label": "Meeting notes",
                    onclick: move |_| {
                        let on = !meeting_notes();
                        crate::meeting::set_enabled(on);
                        meeting_notes.set(on);
                    },
                    span { class: "knob" }
                }
            }
            Row {
                label: "Meeting pad",
                note: "A small card on the screen while the meeting is on: it asks whether to listen, takes your notes, and asks where to keep them.",
                button {
                    id: "meeting-pad-switch",
                    class: if meeting_pad() { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{meeting_pad()}",
                    "aria-label": "Meeting pad",
                    onclick: move |_| {
                        let on = !meeting_pad();
                        crate::pad::set_enabled(on);
                        meeting_pad.set(on);
                    },
                    span { class: "knob" }
                }
            }
        }
    }
}

#[component]
fn DataSettings() -> Element {
    let state = use_context::<AppState>();
    let path = use_hook(|| store::db_path().map(|p| p.display().to_string()));
    let backups = use_hook(|| store::db_path().map(|p| backup::dir_for(&p)));
    let mut last = use_signal(backup::last);
    let mut busy = use_signal(|| false);
    let backup_note = match last() {
        Some(t) => {
            let ago = when::ago(t);
            let ago = if ago == "Yesterday" { "yesterday".to_string() } else { ago };
            format!("Every day. Last copy: {ago}. The newest {} are kept.", backup::KEPT)
        },
        None => format!("Every day. No copy yet. The newest {} are kept.", backup::KEPT),
    };
    rsx! {
        Group {
            Row {
                label: "Workspace file",
                note: path.clone().unwrap_or_else(|| "Kept in memory for this run only.".into()),
                tone: "mono",
                if let Some(p) = path.clone() {
                    button {
                        class: "secondary-btn",
                        onclick: move |_| {
                            let _ = std::process::Command::new("open").arg("-R").arg(&p).spawn();
                        },
                        "Show in Finder"
                    }
                }
            }
            if let Some(dir) = backups.clone() {
                Row { label: "Backups", note: backup_note,
                    if last().is_some() {
                        button {
                            class: "ghost-btn",
                            onclick: move |_| {
                                let _ = std::process::Command::new("open").arg(&dir).spawn();
                            },
                            "Show"
                        }
                    }
                    button {
                        class: "secondary-btn",
                        id: "back-up-now",
                        disabled: busy(),
                        onclick: move |_| async move {
                            busy.set(true);
                            match backup::now().await {
                                Ok(_) => {
                                    last.set(backup::last());
                                    state.toast("Backed up");
                                }
                                Err(e) => state.toast(format!("Couldn't back up: {e}")),
                            }
                            busy.set(false);
                        },
                        if busy() { "Backing up…" } else { "Back up now" }
                    }
                }
            }
        }
        UpdateSettings {}
        Group { title: "Export and import",
            Row {
                label: "Export",
                note: "Every page as Markdown files.",
                button { class: "secondary-btn", onclick: move |_| state.export_all(), "Export…" }
            }
            Row {
                label: "Import",
                note: "A Notion export (.zip) or Markdown files.",
                button { class: "ghost-btn", onclick: move |_| state.import_folder(), "Folder…" }
                button { class: "secondary-btn", onclick: move |_| state.import(), "Import…" }
            }
        }
    }
}

/// Every open to-do of this space, grouped by the page it is on.
#[component]
pub fn TodosModal() -> Element {
    let state = use_context::<AppState>();
    let todos = use_memo(move || state.open_todos());
    // Pages in the order their to-dos come (the pages edited last first).
    let mut groups: Vec<(String, String, Vec<crate::state::OpenTodo>)> = Vec::new();
    for t in todos.read().iter() {
        match groups.last_mut() {
            Some((page, _, list)) if *page == t.page => list.push(t.clone()),
            _ => groups.push((t.page.clone(), t.page_title.clone(), vec![t.clone()])),
        }
    }
    let count = todos.read().len();
    rsx! {
        div { class: "modal-backdrop soft", onclick: move |_| state.set_modal(None),
            div { class: "modal todos", role: "dialog", "aria-label": "To do", onclick: move |e| e.stop_propagation(),
                div { class: "modal-head",
                    div { class: "modal-head-text",
                        h2 { "To do" }
                        div { class: "modal-sub", if count == 1 { "1 open to-do, from all your pages" } else { "{count} open to-dos, from all your pages" } }
                    }
                    button { class: "icon-btn", title: "Close", "aria-label": "Close", onclick: move |_| state.set_modal(None), Icon { name: "close", size: 15 } }
                }
                div { class: "todos-list",
                    if groups.is_empty() {
                        div { class: "history-empty", "Nothing to do." }
                    }
                    for (page, title, list) in groups {
                        div { key: "{page}", class: "todos-group",
                            button {
                                class: "todos-page",
                                onclick: {
                                    let page = page.clone();
                                    move |_| state.open_page(&page)
                                },
                                "{title}"
                            }
                            for t in list {
                                super::page::TodoRow { key: "{t.block}", todo: t }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Ask your notes: a question (typed or spoken) is answered by the AI from
/// the pages of this space that say most about it, and the answer names the
/// pages it used.
#[component]
pub fn AskNotesModal() -> Element {
    let state = use_context::<AppState>();
    let mut question = use_signal(String::new);
    let mut asked = use_signal(String::new);
    let mut reply = use_signal(String::new);
    let mut offered = use_signal(Vec::<crate::voice::Note>::new);
    // "" idle · "work" · "done" · anything else: what went wrong.
    let mut status = use_signal(String::new);
    // With the tools of Rune (see ai::Tools): the step the AI is at, and
    // what it changed in this run.
    let mut step = use_signal(String::new);
    let mut did = use_signal(Vec::<crate::mcp::Change>::new);
    // The pages it read with the tools, by name ("Looked in Run log and Week 4").
    let mut looked = use_signal(Vec::<String>::new);
    let model = use_hook(ai::chosen_label);
    // The plan for a large job that the person said yes to (the next run does the work).
    let mut plan_ok = use_signal(|| None::<String>);
    let ask = use_callback(move |_: ()| {
        let q = question.peek().trim().to_string();
        if q.is_empty() || status.peek().as_str() == "work" {
            return;
        }
        if state.ask_given.peek().iter().any(|g| g.pending) {
            return state.toast("Rune is still reading what you gave. Ask again in a moment.");
        }
        let tools = ai::takes_tools()
            .then(|| ai::Tools { journal: std::env::temp_dir().join(format!("rune-ask-{}-{}.jsonl", std::process::id(), crate::model::now_ms())) });
        let notes = state.notes_for(&q);
        if notes.is_empty() && tools.is_none() {
            status.set("There are no pages to look in yet.".into());
            return;
        }
        let system = match &tools {
            Some(_) => format!(
                "{}\n\n{}\n\nNow it is {} (local time).",
                crate::voice::ASK_NOTES,
                ai::ASK_NOTES_TOOLS,
                when::local(crate::model::now_ms()).format("%Y-%m-%d %H:%M, %A")
            ),
            None => crate::voice::ASK_NOTES.to_string(),
        };
        let request = match plan_ok.peek().as_ref() {
            Some(plan) => ai::approved(&q, plan),
            None => q.clone(),
        };
        plan_ok.set(None);
        // (With the files that were given with the question, as words.)
        let request = format!("{request}{}", crate::attach::for_request(&state.ask_given.peek()));
        let messages = vec![ai::ChatMessage::system(system), ai::ChatMessage::user(crate::voice::ask_message(&request, &notes, ai::MAX_CONTEXT_CHARS, tools.is_some()))];
        let journal = tools.as_ref().map(|t| t.journal.clone());
        asked.set(q);
        offered.set(notes);
        reply.set(String::new());
        step.set(String::new());
        did.set(Vec::new());
        looked.set(Vec::new());
        status.set("work".into());
        let mut rx = ai::start_with(ai::Prefs::load(), messages, tools, state.ask_pictures.peek().clone());
        spawn(async move {
            while let Some(event) = rx.next().await {
                match event {
                    AiEvent::Working { tool, title, chars } => step.set(ai::working_label(&tool, title.as_deref(), chars)),
                    AiEvent::Step(tool, input) => {
                        let label = {
                            let pages = state.pages.peek();
                            ai::step_label(&tool, &input, |id| pages.get(id).map(|p| p.title.clone()))
                        };
                        if let Some(name) = label.strip_prefix("Read “").and_then(|l| l.strip_suffix('”')) {
                            if !looked.peek().iter().any(|n| n == name) {
                                looked.write().push(name.to_string());
                            }
                        }
                        step.set(label);
                        // What it wrote before a step was on its way to the step, not the answer.
                        reply.set(String::new());
                        crate::sync::check_outside_changes();
                    }
                    AiEvent::Delta(text) => reply.write().push_str(&text),
                    AiEvent::Done | AiEvent::Error(_) => {
                        did.set(journal.as_deref().map(crate::mcp::read_journal).unwrap_or_default());
                        crate::sync::check_outside_changes();
                        match event {
                            AiEvent::Error(e) => status.set(e),
                            _ => status.set("done".into()),
                        }
                        break;
                    }
                }
            }
        });
    });
    let did_lines = crate::mcp::describe(&did.read());
    let with_tools = ai::takes_tools();
    // Pictures for this box go when the box goes.
    use_drop(move || {
        let mut pictures = state.ask_pictures;
        pictures.write().clear();
        let mut given = state.ask_given;
        given.write().clear();
    });
    let pictures = state.ask_pictures.read().clone();
    let given = state.ask_given.read().clone();
    let looked_in = match looked.read().as_slice() {
        [] => String::new(),
        [one] => format!("Looked in {one}"),
        [first @ .., last] => format!("Looked in {} and {last}", first.join(", ")),
    };
    let waiting = if step.read().is_empty() { format!("Reading {} of your pages…", offered.read().len()) } else { format!("{}…", step.read()) };
    let working = status.read().as_str() == "work";
    let done = status.read().as_str() == "done";
    let problem = if !working && !done { status.read().clone() } else { String::new() };
    // A plan for a large job: what it will build, its steps, and "Do it".
    let job = if done { ai::split_plan(&ai::clean_reply(&reply.read()).0) } else { None };
    let (answer, sources) = match &job {
        Some((about, _)) => (about.clone(), Vec::new()),
        None => crate::voice::split_answer(&ai::clean_reply(&reply.read()).0, &offered.read()),
    };
    let job_steps = job.as_ref().map(|(_, steps)| steps.clone()).unwrap_or_default();
    let job_text = job_steps.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n");
    let lines = crate::markdown::to_blocks(&answer);
    let read: Vec<String> = offered.read().iter().map(|n| n.title.clone()).collect();
    // (A long page goes to the AI in part: the person is told which.)
    let in_part = match crate::voice::cut_for_ask(&offered.read(), ai::MAX_CONTEXT_CHARS).as_slice() {
        [] => String::new(),
        names => format!(" Only the first part of: {}.", names.join(", ")),
    };
    let (answer_kept, sources_kept) = (answer.clone(), sources.clone());

    rsx! {
        div { class: "modal-backdrop soft ask-back", onclick: move |_| state.set_modal(None),
            div { class: "modal ask-notes", role: "dialog", "aria-label": "Ask your notes", onclick: move |e| e.stop_propagation(),
                if !given.is_empty() {
                    div { class: "ai-given in-ask", id: "ask-given", role: "list", "aria-label": "What goes with the question",
                        for g in given.iter() {
                            super::page::AiChip { key: "{g.id}", id: g.id.clone(), icon: g.kind.icon(), name: g.name.clone(), note: g.note.clone(), pending: g.pending }
                        }
                    }
                }
                div { class: "ask-row",
                    for picture in pictures.iter() {
                        super::page::AiShot { key: "{picture.id}", id: picture.id.clone(), src: picture.data_url() }
                    }
                    input {
                        id: "ask-notes-input",
                        class: "ask-input",
                        r#type: "text",
                        // ("@" lists the pages: the one chosen goes with the question.)
                        "data-at": "give",
                        placeholder: if with_tools { "Ask your notes, or say what to do…" } else { "Ask your notes anything…" },
                        "aria-label": "Ask your notes",
                        onmounted: move |e: MountedEvent| async move {
                            let _ = e.set_focus(true).await;
                        },
                        oninput: move |e: FormEvent| question.set(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Enter {
                                e.prevent_default();
                                ask.call(());
                            }
                        },
                    }
                    if let Some((dots, name)) = model.clone() {
                        span { class: "ask-model", title: "The model that answers (Settings, AI)",
                            span { class: "ai-pips", "aria-hidden": "true",
                                for i in 0..4u8 {
                                    i { "data-on": if i < dots { "1" } else { "0" } }
                                }
                            }
                            "{name}"
                        }
                    }
                    button { class: "ai-round mic-btn", "data-dictate": "ask-notes-input", title: "Speak your question", "aria-label": "Speak your question", Icon { name: "mic", size: 15 } }
                    // Round, in the ink of the text, as in the AI bar of a page.
                    button { class: "ai-round ai-send", id: "ask-notes-go", title: "Ask", "aria-label": "Ask", disabled: working || question.read().trim().is_empty(), onclick: move |_| ask.call(()), Icon { name: "arrow-up", size: 15 } }
                }
                if working || done {
                    div { class: "ask-body",
                        div { class: "ask-question", "{asked}" }
                        if !looked_in.is_empty() {
                            div { class: "ask-looked", id: "ask-looked", Icon { name: "search", size: 13 } span { "{looked_in}" } }
                        }
                        if lines.is_empty() && working {
                            div { class: "ask-wait", id: "ask-step", "{waiting}" }
                        }
                        div { class: "ask-answer",
                            for b in lines {
                                VersionBlock { key: "{b.id}", block: b }
                            }
                        }
                        if !job_steps.is_empty() {
                            div { class: "ai-plan", id: "ask-plan", role: "list", "aria-label": "What the AI will do",
                                for step in job_steps.iter() {
                                    div { key: "{step}", class: "ai-plan-row", role: "listitem", Icon { name: "plus", size: 14 } span { "{step}" } }
                                }
                            }
                        }
                        if !working && !did_lines.is_empty() {
                            div { class: "ai-plan ai-did", id: "ask-did", role: "list", "aria-label": "What the AI did",
                                for line in did_lines.iter().take(8) {
                                    div { key: "{line}", class: "ai-plan-row", role: "listitem", Icon { name: "check", size: 14 } span { "{line}" } }
                                }
                                if did_lines.len() > 8 {
                                    div { class: "ai-plan-row", role: "listitem", span { class: "ai-more", "and {did_lines.len() - 8} more" } }
                                }
                            }
                        }
                        if done && !sources.is_empty() {
                            div { class: "ask-sources",
                                span { class: "ask-sources-label", "From" }
                                for n in sources.clone() {
                                    button {
                                        key: "{n.id}",
                                        class: "chip ask-source",
                                        onclick: {
                                            let id = n.id.clone();
                                            move |_| state.open_page(&id)
                                        },
                                        "{n.title}"
                                    }
                                }
                            }
                        }
                    }
                }
                if !problem.is_empty() {
                    div { class: "ask-problem", "{problem}" }
                }
                // While it listens: a line that says so, with the time and how to stop.
                div { class: "ask-listening", "aria-hidden": "true",
                    i { class: "ask-dot" }
                    span { "Listening" }
                    span { class: "ask-clock", "data-hear-clock": "1", "0:00" }
                    span { class: "ask-stop-hint", "Press the microphone or esc to stop" }
                }
                div { class: "modal-foot ask-foot",
                    span { class: "modal-foot-note",
                        if read.is_empty() && with_tools {
                            "Reads and changes your notes and calendar. One Undo takes a run back."
                        } else if read.is_empty() {
                            "The AI reads only the pages of this space that match your question."
                        } else if done && with_tools && did_lines.is_empty() {
                            "The AI read: {read.join(\", \")}. It changed nothing.{in_part}"
                        } else {
                            "The AI read: {read.join(\", \")}.{in_part}"
                        }
                    }
                    if !working && !done {
                        span { class: "ask-keys", "aria-hidden": "true",
                            span { class: "ask-key", "↵" }
                            "Ask"
                            span { class: "ask-key", "esc" }
                            "Close"
                        }
                    }
                    if !working && !did_lines.is_empty() {
                        button {
                            class: "secondary-btn",
                            id: "ask-undo",
                            title: "Take back all that the AI did",
                            onclick: move |_| {
                                let changes = did.peek().clone();
                                did.set(Vec::new());
                                state.take_back(&changes);
                                state.toast("Taken back.");
                            },
                            "Undo"
                        }
                    }
                    if done && job.is_some() {
                        button {
                            class: "primary-btn",
                            id: "ask-plan-go",
                            title: "The AI does this now (Undo takes it back)",
                            onclick: move |_| {
                                plan_ok.set(Some(job_text.clone()));
                                question.set(asked.peek().clone());
                                ask.call(());
                            },
                            "Do it"
                        }
                    }
                    if done && did_lines.is_empty() && job.is_none() {
                        button {
                            class: "secondary-btn",
                            id: "ask-notes-save",
                            onclick: move |_| state.save_answer(&asked.peek(), &answer_kept, &sources_kept),
                            "Save as a page"
                        }
                    }
                }
            }
        }
    }
}

/// How long a pause in speech is taken to mean "that's all".
const VOICE_PAUSE_MS: i64 = 4000;

/// The voice box: macOS Dictation types what is said into it, and when the
/// speaker pauses (or presses the button) the AI makes a page from it.
#[component]
pub fn VoiceModal() -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(String::new);
    let mut last_change = use_signal(crate::model::now_ms);
    let mut on_pause = use_signal(|| store::setting("voice_on_pause").as_deref() != Some("off"));
    let testing = std::env::var_os("RUNE_SELFTEST").is_some();
    // What it was opened for: (heading, button, hint while it waits for a
    // pause, hint with the pause switch off, what to say).
    let what = use_hook(|| state.ui.peek().voice.clone());
    let (heading, button, when_paused, when_done, example) = match what {
        VoiceFor::NewPage => (
            "Speak a page",
            "Create page",
            "The page is made when you pause.",
            "Press Create page when you are done.",
            "Say what the page is about: a plan, a list, notes from a call. Or say something for your calendar, like \"lunch with Sam on Friday at 1\".",
        ),
        VoiceFor::Into { .. } => (
            "Speak into this page",
            "Write here",
            "It is written into the page when you pause.",
            "Press Write here when you are done.",
            "Say what to write on this line.",
        ),
        VoiceFor::Edit { .. } => (
            "Say what to do with the text",
            "Change it",
            "The text is changed when you pause.",
            "Press Change it when you are done.",
            "For example: make it shorter, turn it into a checklist, translate it to Spanish.",
        ),
    };
    // A request to change text can be one word ("shorter").
    let enough_words = if matches!(what, VoiceFor::Edit { .. }) { 1 } else { 3 };
    let finish = use_callback(move |spoken: String| state.finish_voice(spoken));

    // What hears the speaker: the Mac's Dictation, or (Settings) the app's
    // own listener, which shows the words as they are said.
    let own_ears = use_hook(crate::listen::own_ears);
    let ears = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(None::<crate::listen::Listener>)));
    use_hook({
        let ears = ears.clone();
        move || {
            // The cursor goes in the box first.
            document::eval("window.__rune && window.__rune.focus('voice-text', -1)");
            if !own_ears {
                // Ask macOS to start Dictation for it.
                spawn(async move {
                    ai::sleep_ms(250).await;
                    if !testing {
                        crate::dictation::start();
                    }
                });
                return;
            }
            match crate::listen::start(false) {
                Ok((listener, mut heard)) => {
                    *ears.borrow_mut() = Some(listener);
                    spawn(async move {
                        use crate::listen::Heard;
                        let join = |said: &str, more: &str| if said.is_empty() { more.to_string() } else { format!("{said} {more}") };
                        let mut said = String::new();
                        while let Some(event) = heard.next().await {
                            let shown = match event {
                                Heard::Partial { text: more, .. } => join(&said, &more),
                                Heard::Final { text: more, .. } => {
                                    said = join(&said, &more);
                                    said.clone()
                                }
                                Heard::Error(message) | Heard::Note(message) => {
                                    state.notice(message);
                                    continue;
                                }
                                Heard::Listening { .. } => continue,
                                Heard::Done => break,
                            };
                            let value = serde_json::to_string(&shown).unwrap_or_default();
                            document::eval(&format!(
                                "(()=>{{const t=document.getElementById('voice-text');if(t){{t.value={value};t.scrollTop=t.scrollHeight;}}}})()"
                            ));
                            text.set(shown);
                            last_change.set(crate::model::now_ms());
                        }
                    });
                }
                Err(e) => state.notice(e),
            }
        }
    });
    use_drop(move || {
        if ears.borrow_mut().take().is_none() {
            crate::dictation::stop();
        }
    });
    // When the speaker pauses, make the page.
    use_future(move || async move {
        loop {
            ai::sleep_ms(200).await;
            let enough = text.peek().split_whitespace().count() >= enough_words;
            if *on_pause.peek() && enough && crate::model::now_ms() - *last_change.peek() > VOICE_PAUSE_MS {
                let spoken = text.peek().clone();
                finish.call(spoken);
                break;
            }
        }
    });

    let words = text.read().split_whitespace().count();
    let waiting = on_pause() && words >= enough_words;
    let hint = if words == 0 {
        "Speak now. Your words appear here."
    } else if waiting {
        when_paused
    } else if on_pause() {
        "Keep going…"
    } else {
        when_done
    };
    let stamp = last_change();

    rsx! {
        div { class: "modal-backdrop soft", onclick: move |_| state.set_modal(None),
            div { class: "modal voice", role: "dialog", "aria-label": "Speak a page", onclick: move |e| e.stop_propagation(),
                div { class: "voice-top",
                    span { class: if words == 0 { "voice-mic listening" } else { "voice-mic" }, Icon { name: "mic", size: 18 } }
                    div { class: "voice-top-text",
                        h2 { "{heading}" }
                        div { class: "modal-sub", "{hint}" }
                    }
                    button { class: "icon-btn", title: "Close (esc)", "aria-label": "Close", onclick: move |_| state.set_modal(None), Icon { name: "close", size: 15 } }
                }
                textarea {
                    id: "voice-text",
                    class: "voice-text",
                    placeholder: "{example}",
                    "aria-label": "What you said",
                    spellcheck: "false",
                    oninput: move |e: FormEvent| {
                        text.set(e.value());
                        last_change.set(crate::model::now_ms());
                    },
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Enter && e.modifiers().meta() {
                            e.prevent_default();
                            let spoken = text.peek().clone();
                            finish.call(spoken);
                        }
                    },
                }
                div { class: "voice-wait",
                    if waiting {
                        span { key: "{stamp}", class: "voice-wait-bar" }
                    }
                }
                div { class: "modal-foot voice-foot",
                    button {
                        class: if on_pause() { "switch on" } else { "switch" },
                        role: "switch",
                        "aria-checked": "{on_pause()}",
                        "aria-label": "Make the page when I pause",
                        onclick: move |_| {
                            let on = !on_pause();
                            on_pause.set(on);
                            last_change.set(crate::model::now_ms());
                            store::set_setting("voice_on_pause", if on { "on" } else { "off" });
                        },
                        span { class: "knob" }
                    }
                    span { class: "modal-foot-note", "Finish when I pause" }
                    button {
                        class: "primary-btn",
                        id: "voice-create",
                        disabled: words == 0,
                        title: "⌘↩",
                        onclick: move |_| {
                            let spoken = text.peek().clone();
                            finish.call(spoken);
                        },
                        "{button}"
                    }
                }
                if !own_ears {
                    p { class: "voice-help",
                        "This uses the Mac's Dictation. If no words appear, turn Dictation on in "
                        button { class: "link-btn", onclick: move |_| crate::dictation::open_settings(), "Keyboard settings" }
                        "."
                    }
                }
            }
        }
    }
}

/// Page history: earlier versions of a page, kept as it is edited, with a
/// read-only preview of each and a way to put one back.
#[component]
pub fn HistoryModal() -> Element {
    let state = use_context::<AppState>();
    let page_id = state.ui.read().history.clone().unwrap_or_default();
    let versions = use_hook({
        let id = page_id.clone();
        move || store::with(|s| s.load_versions(&id)).unwrap_or_default()
    });
    let mut picked = use_signal(|| 0usize);
    // Versions kept in the same minute get seconds, so they can be told apart.
    let labels: Vec<String> = {
        let short: Vec<String> = versions.iter().map(|v| when::moment(v.at)).collect();
        versions
            .iter()
            .zip(&short)
            .map(|(v, s)| if short.iter().filter(|o| *o == s).count() > 1 { when::moment_exact(v.at) } else { s.clone() })
            .collect()
    };
    let title = state.page_title(&page_id);
    let shown = versions.get(picked()).cloned();
    // A locked page that is not open shows none of its earlier versions
    // (see `open_history`), also when it locks while this box is open
    // (another window opened it, and shows it no more).
    let is_shut = {
        let id = page_id.clone();
        move || crate::lock::page_lock(&state, &id) == crate::lock::LockState::Shut
    };
    use_effect({
        let is_shut = is_shut.clone();
        move || {
            if is_shut() {
                state.set_modal(None);
            }
        }
    });
    if is_shut() {
        return rsx! {};
    }

    rsx! {
        div { class: "modal-backdrop soft", onclick: move |_| state.set_modal(None),
            div { class: "modal history", role: "dialog", "aria-label": "Page history", onclick: move |e| e.stop_propagation(),
                div { class: "modal-head",
                    div { class: "modal-head-text",
                        h2 { "Page history" }
                        div { class: "modal-sub", "{title}" }
                    }
                    button { class: "icon-btn", title: "Close", "aria-label": "Close", onclick: move |_| state.set_modal(None), Icon { name: "close", size: 15 } }
                }
                if versions.is_empty() {
                    div { class: "history-empty",
                        "No earlier versions yet. As you edit this page, a copy of it is kept at most every ten minutes."
                    }
                } else {
                    div { class: "history-body",
                        div { class: "history-list",
                            for (i, v) in versions.iter().enumerate() {
                                button {
                                    key: "{v.id}",
                                    class: if i == picked() { "history-item on" } else { "history-item" },
                                    onclick: move |_| picked.set(i),
                                    span { class: "hi-when", "{labels[i]}" }
                                    span { class: "hi-meta", "{word_count(&v.blocks)} words" }
                                }
                            }
                        }
                        if let Some(v) = shown.clone() {
                            div { class: "history-preview", key: "{v.id}",
                                h1 { class: "hp-title", if v.title.trim().is_empty() { "Untitled" } else { "{v.title}" } }
                                for b in v.blocks {
                                    VersionBlock { key: "{b.id}", block: b }
                                }
                            }
                        }
                    }
                    div { class: "modal-foot",
                        span { class: "modal-foot-note", "The page as it is now is kept too, so you can switch back." }
                        if let Some(v) = shown {
                            button {
                                class: "primary-btn",
                                id: "restore-version",
                                onclick: {
                                    let page_id = page_id.clone();
                                    move |_| state.restore_version(&page_id, v.id)
                                },
                                "Restore this version"
                            }
                        }
                    }
                }
            }
        }
    }
}

fn word_count(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| b.plain_text().split_whitespace().count()).sum()
}

/// One block of a kept version, drawn read-only.
#[component]
fn VersionBlock(block: Block) -> Element {
    let state = use_context::<AppState>();
    let indent = block.indent.min(8);
    let class = format!("hp-block hp-{}{}", block.kind.as_str(), if block.checked { " checked" } else { "" });
    match block.kind {
        BlockKind::Divider => rsx! { hr { class: "hp-divider" } },
        BlockKind::Drawing => rsx! { div { class: "{class}", style: "--indent: {indent}", "Drawing" } },
        BlockKind::Image => rsx! { div { class: "{class}", style: "--indent: {indent}", "Image" } },
        BlockKind::Table => rsx! { div { class: "{class}", style: "--indent: {indent}", "Table: {block.plain_text()}" } },
        BlockKind::Chart => rsx! { div { class: "{class}", style: "--indent: {indent}", "Chart" } },
        BlockKind::PageLink => {
            let name = block.target.as_deref().map(|t| state.page_title(t)).unwrap_or_else(|| "Page".into());
            rsx! { div { class: "{class}", style: "--indent: {indent}", "{name}" } }
        }
        kind => {
            let html = crate::inline::to_html(kind, &block.text);
            rsx! { div { class: "{class}", style: "--indent: {indent}", dangerous_inner_html: "{html}" } }
        }
    }
}

#[component]
pub fn TrashModal() -> Element {
    let state = use_context::<AppState>();
    let mut confirm = use_signal(|| None::<String>);
    let space = state.ui.read().space.clone();
    let pages = state.pages.read();
    let mut trashed: Vec<Page> = pages
        .values()
        .filter(|p| p.trashed && p.space_id == space && p.parent_id.as_ref().and_then(|pid| pages.get(pid)).is_none_or(|parent| !parent.trashed))
        .cloned()
        .collect();
    drop(pages);
    trashed.sort_by_key(|p| std::cmp::Reverse(p.updated_at));

    rsx! {
        div { class: "modal-backdrop soft", onclick: move |_| state.set_modal(None),
            div { class: "modal trash", onclick: move |e| e.stop_propagation(),
                div { class: "modal-head",
                    h2 { "Trash" }
                    button { class: "icon-btn", title: "Close", "aria-label": "Close", onclick: move |_| state.set_modal(None), Icon { name: "close", size: 15 } }
                }
                if trashed.is_empty() {
                    div { class: "menu-empty", "Trash is empty." }
                }
                div { class: "trash-list",
                    for p in trashed {
                        div { key: "{p.id}", class: "trash-item",
                            span { class: "tree-icon", PageIcon { icon: p.icon.clone(), size: 16 } }
                            span { class: "trash-title", "{p.display_title()}" }
                            button {
                                class: "ghost-btn",
                                onclick: {
                                    let id = p.id;
                                    move |_| state.restore_page(&id)
                                },
                                "Restore"
                            }
                            if confirm.read().as_deref() == Some(p.id.as_str()) {
                                button {
                                    class: "danger-btn",
                                    onclick: {
                                        let id = p.id.clone();
                                        move |_| {
                                            state.delete_forever(&id);
                                            confirm.set(None);
                                        }
                                    },
                                    "Delete forever?"
                                }
                            } else {
                                button {
                                    class: "ghost-btn",
                                    title: "Permanently delete",
                                    onclick: {
                                        let id = p.id.clone();
                                        move |_| confirm.set(Some(id.clone()))
                                    },
                                    "Delete"
                                }
                            }
                        }
                    }
                }
                p { class: "note pad", "Pages in Trash can be restored. “Delete forever” removes them from this Mac and can't be undone." }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_fits_when_each_word_starts_one_of_its_words() {
        let cmd = Cmd { id: "dark_mode", label: "Dark mode".into(), keys: "⌘⇧L", also: "theme appearance" };
        assert_eq!(command_fit("dark", &cmd), Some(0));
        assert_eq!(command_fit("mode", &cmd), Some(1));
        assert_eq!(command_fit("the", &cmd), Some(1), "its other words count");
        assert_eq!(command_fit("dar mo", &cmd), Some(1));
        assert_eq!(command_fit("ark", &cmd), None, "the start of a word, not its middle");
        assert_eq!(command_fit("dark zebra", &cmd), None, "every word must fit");
        assert_eq!(command_fit("", &cmd), None);
    }

    #[test]
    fn a_page_named_by_the_typed_word_comes_before_a_command_that_only_holds_it() {
        let cmd = |label: &str, also: &'static str| Cmd { id: "x", label: label.into(), keys: "", also };
        let page = |title: &str| Hit { id: title.into(), icon: String::new(), title: title.into(), snippet: None, path: String::new() };
        // What search shows for `typed`, with these pages (name, a line of text).
        let lines = |typed: &str, pages: &[(&str, &str)]| -> Vec<String> {
            let mut hits: Vec<(u8, bool, Hit)> = pages
                .iter()
                .filter_map(|(name, text)| search_fit(typed, &name.to_lowercase(), &[text.to_string()]).map(|f| (f.0, named_by(typed, &name.to_lowercase()), page(name))))
                .collect();
            hits.sort_by_key(|h| h.0);
            let mut cmds: Vec<(u8, Cmd)> = [cmd("New page", "add create"), cmd("New meeting notes", "call"), cmd("Dark mode", "dark light theme"), cmd("Lock page with a password…", "private"), cmd("New tab", ""), cmd("Next space", "")]
                .into_iter()
                .filter_map(|c| command_fit(typed, &c).map(|r| (r, c)))
                .collect();
            cmds.sort_by_key(|c| c.0);
            in_order(hits, cmds)
                .into_iter()
                .map(|f| match f {
                    Found::Page(h) => h.title,
                    Found::Command(c) => format!("> {}", c.label),
                    Found::Create { .. } => unreachable!(),
                })
                .collect()
        };
        let pages = [("Project notes", ""), ("Home page", ""), ("Sales call", ""), ("Clock repair", ""), ("Diary", "a dark page of notes, then a call")];
        // A word of a page's name opens that page, not a command that makes something.
        assert_eq!(lines("notes", &pages), ["Project notes", "> New meeting notes", "Diary"]);
        assert_eq!(lines("page", &pages), ["Home page", "> New page", "> Lock page with a password…", "Diary"]);
        assert_eq!(lines("call", &pages), ["Sales call", "> New meeting notes", "Diary"]);
        // A command typed from the start of its name still leads…
        assert_eq!(lines("dark", &pages), ["> Dark mode", "Diary"]);
        assert_eq!(lines("lock", &pages), ["> Lock page with a password…", "Clock repair"]);
        assert_eq!(lines("new", &pages)[..2], ["> New page", "> New meeting notes"]);
        // …but not over a page whose name starts the same way.
        assert_eq!(lines("dark", &[("Dark matter", ""), ("On dark skies", "")]), ["Dark matter", "> Dark mode", "On dark skies"]);
        // Letters inside a longer word of a name are not that word: the command keeps its place.
        assert_eq!(lines("tab", &[("Database", ""), ("Tab sizes", ""), ("Guitar tab book", "")]), ["Tab sizes", "Guitar tab book", "> New tab", "Database"]);
        assert_eq!(lines("space", &[("Workspace plans", ""), ("Diary", "more space")]), ["> Next space", "Workspace plans", "Diary"]);
        // The words in another order in a name are still the name.
        assert_eq!(lines("notes meeting", &[("Meeting room notes", "")]), ["Meeting room notes", "> New meeting notes"]);
    }

    #[test]
    fn search_takes_several_words_in_any_order() {
        let lines = vec!["Packing for Rome".to_string(), "Passport, charger and the train tickets".to_string()];
        let fit = |typed: &str, name: &str| search_fit(typed, name, &lines);
        // The words as typed, in the name or in a line.
        assert_eq!(fit("trip", "trip to italy").map(|f| f.0), Some(0));
        assert_eq!(fit("italy", "trip to italy").map(|f| f.0), Some(1));
        assert_eq!(fit("train tickets", "trip to italy"), Some((3, Some("Passport, charger and the train tickets".to_string()))));
        // Words in another order, and spread over the name and the text.
        assert_eq!(fit("italy trip", "trip to italy").map(|f| f.0), Some(2));
        let spread = fit("rome packing", "trip to italy").unwrap();
        assert_eq!((spread.0, spread.1.as_deref()), (4, Some("Packing for Rome")));
        assert_eq!(fit("tickets italy", "trip to italy").map(|f| f.0), Some(4));
        // Every word must be there.
        assert_eq!(fit("rome hotel", "trip to italy"), None);
        assert_eq!(fit("   ", "trip to italy"), None);
    }
}
