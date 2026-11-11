//! The interface: each window's shell (top row, sidebar, page card) plus the
//! page editor, menus and modals.

mod calendar;
mod charts;
mod chrome;
mod graph;
pub mod icons;
mod modals;
pub mod pad;
mod page;
mod sidebar;
mod tables;

use crate::state::{AppState, Bridge, Look, Modal, Route, Tab, Theme};
use crate::sync;
use dioxus::desktop::tao::window::Theme as WindowTheme;
use dioxus::prelude::*;
use crate::layout::{Dir, Layout};
use futures_util::StreamExt;
use icons::Icon;

const BRIDGE_JS: &str = include_str!("../../assets/bridge.js");
const SELFTEST_JS: &str = include_str!("../../assets/selftest.js");

/// The first window.
#[component]
pub fn App() -> Element {
    rsx! { Root { primary: true, route: None } }
}

/// Open another window showing `route`.
pub fn open_window(route: Route) {
    let dom = VirtualDom::new_with_props(Root, RootProps { primary: false, route: Some(route) });
    dioxus::desktop::window().new_window(dom, crate::window_config());
}

/// Whether this window is showing dark mode right now (the app's own choice,
/// or the Mac's when it follows the system). Cheap: no process is started.
pub fn window_is_dark() -> bool {
    dioxus::desktop::window().window.theme() == WindowTheme::Dark
}

/// Colours for the glass, from the Transparency and Tint settings.
fn glass_vars(look: &Look) -> String {
    crate::glass::css_vars(look.glass, parse_hex(&look.tint))
}

/// The class that gives a page the colours of a theme.
fn theme_class(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "theme-system",
        Theme::Light => "theme-light",
        Theme::Dark => "theme-dark",
    }
}

fn parse_hex(s: &str) -> Option<(f64, f64, f64)> {
    if !crate::state::valid_hex(s) {
        return None;
    }
    let n = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(f64::from);
    Some((n(1)?, n(3)?, n(5)?))
}

#[component]
pub fn Root(primary: bool, route: Option<Route>) -> Element {
    let state = use_context_provider(move || AppState::load(primary, route));

    // Keyboard, focus, editing and drag-and-drop events from the page.
    use_future(move || async move {
        let mut bridge = document::eval(BRIDGE_JS);
        loop {
            match bridge.recv::<serde_json::Value>().await {
                Ok(value) => match serde_json::from_value::<Bridge>(value.clone()) {
                    Ok(msg) => state.handle_bridge(msg),
                    Err(e) => eprintln!("[bridge] unreadable message {value}: {e}"),
                },
                Err(e) => {
                    eprintln!("[bridge] stopped: {e:?}");
                    break;
                }
            }
        }
    });

    // Changes made in other windows.
    use_future(move || async move {
        let Some(mut rx) = sync::take_receiver(state.win.key) else { return };
        while let Some(msg) = rx.next().await {
            state.apply_sync(msg);
        }
    });

    // Changes made by other programs (an agent using the MCP server).
    use_future(move || async move {
        loop {
            crate::ai::sleep_ms(1500).await;
            sync::check_outside_changes();
            // A read or a write of the notes file that failed.
            if let Some(fault) = crate::store::trouble() {
                state.notice(fault);
            }
        }
    });
    use_drop(move || sync::leave(state.win.key));

    // The calendars of this Mac, when the person has turned them on: read
    // when the window opens, and again every five minutes.
    use_future(move || async move {
        if !primary {
            return;
        }
        crate::ai::sleep_ms(1500).await;
        loop {
            state.read_mac_calendars(false);
            crate::ai::sleep_ms(5 * 60 * 1000).await;
        }
    });

    // Notifications (first window only): keep the reminder plan current and
    // open whatever a clicked notification is about.
    use_hook(move || {
        if primary {
            crate::notify::install();
        }
    });
    use_future(move || async move {
        if !primary {
            return;
        }
        let mut since_plan = 0u32;
        loop {
            crate::ai::sleep_ms(500).await;
            since_plan += 1;
            // Replan when events change (debounced), and hourly so the
            // two-week window keeps moving.
            if (since_plan >= 4 && crate::notify::take_dirty()) || since_plan >= 7200 {
                since_plan = 0;
                // Every event and repeat in the next fortnight (and a day).
                let now = crate::model::now_ms();
                let events = crate::repeat::expand(state.events.peek().values(), now, now + 15 * 24 * crate::when::HOUR);
                crate::notify::plan_reminders(events).await;
            }
            if let Some(id) = crate::notify::take_clicked() {
                let window = dioxus::desktop::window();
                window.set_visible(true);
                window.set_focus();
                if let Some(page) = id.strip_prefix("page:") {
                    state.open_page(page);
                } else if let Some(page) = id.strip_prefix("recap:") {
                    // "The meeting ended": its page, with the AI box to take a recap.
                    state.open_page(page);
                    state.ai_recap(page);
                } else if id != "ai" {
                    state.reminder_clicked(&id);
                }
            }
        }
    });

    // Find out which AI tools are installed (off the UI thread), then tell
    // the page script whether there is one: what needs an AI shows only then.
    use_future(move || async move {
        crate::ai::local::installed_async(false).await;
        state.show_ai_state();
    });

    // The AI from anywhere: Control-Option-Space in any app brings this window
    // forward with the AI open (first window only; not in a test run, which
    // must not take the shortcut from the installed app).
    if primary && std::env::var_os("NOTION2_SELFTEST").is_none() {
        let _ = dioxus::desktop::use_global_shortcut(crate::state::AI_SHORTCUT, move |key| {
            let on = crate::store::setting("voice_anywhere").as_deref() != Some("off");
            if on && key == dioxus::desktop::HotKeyState::Pressed && crate::ai::shown() {
                let window = dioxus::desktop::window();
                window.set_visible(true);
                window.set_focus();
                state.command("ai_anywhere");
            }
        });
    }

    // Once, as the first window opens: where speech is turned into text for
    // this Mac's language, and whether the app runs from a temporary place.
    use_hook(move || {
        if !primary {
            return;
        }
        crate::listen::learn_where_speech_goes();
        // Opened straight from Downloads, macOS runs a copy from a place
        // that changes each time: updates and the AI connector need the real one.
        let moved = std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains("/AppTranslocation/"));
        if moved && std::env::var_os("NOTION2_SELFTEST").is_none() {
            spawn(async move {
                crate::ai::sleep_ms(1500).await;
                state.notice("Rune runs from a temporary place. Quit it, move Rune into your Applications folder, and open it again. Your notes are safe.");
            });
        }
    });
    // Updates (first window only; never during a self-test): look for a
    // newer build soon after opening, then now and then.
    use_future(move || async move {
        if !primary || std::env::var_os("NOTION2_SELFTEST").is_some() || crate::update::own_build().is_none() {
            return;
        }
        crate::ai::sleep_ms(15_000).await;
        loop {
            state.check_update(false);
            crate::ai::sleep_ms(10 * 60 * 1000).await;
        }
    });

    // Meeting notes (first window only; never during a self-test): look every
    // few seconds for a Teams meeting starting or ending.
    use_future(move || async move {
        if !primary || std::env::var_os("NOTION2_SELFTEST").is_some() {
            return;
        }
        let mut watch = crate::meeting::Watch::default();
        loop {
            crate::ai::sleep_ms(crate::meeting::POLL_MS).await;
            if !crate::meeting::enabled() {
                watch = crate::meeting::Watch::default();
                continue;
            }
            let heard = crate::meeting::poll().await;
            match watch.step(&heard) {
                Some(crate::meeting::Change::Started(app)) => state.meeting_started(app),
                Some(crate::meeting::Change::Ended) => state.meeting_ended(crate::model::now_ms() - crate::meeting::END_LAG_MS),
                None => {}
            }
        }
    });

    // The meeting pad (first window only; see pad.rs). What is clicked in the
    // pad's window arrives here; what the pad shows is worked out here and
    // handed to its window, which is opened when there is something to show
    // and closed when there is not, or when this window goes.
    use_future(move || async move {
        if !primary {
            return;
        }
        let mut asked = crate::pad::owner();
        while let Some(command) = asked.next().await {
            state.pad_command(command);
        }
    });
    use_effect(move || {
        if !primary {
            return;
        }
        let view = state.pad_view();
        // (A test run draws the pad in this window: see below.)
        if crate::pad::inline() {
            return;
        }
        match view {
            Some(mut view) => {
                view.theme = theme_class(state.ui.read().theme).to_string();
                view.glass = glass_vars(&state.look.read());
                if crate::pad::show(view) {
                    pad::open_window();
                }
            }
            None => crate::pad::close(),
        }
    });
    use_drop(move || {
        if primary {
            crate::pad::close();
        }
    });

    // Daily backups (first window only; never during a self-test). The first
    // check waits a little so it never competes with opening the app.
    use_future(move || async move {
        if !primary || std::env::var_os("NOTION2_SELFTEST").is_some() {
            return;
        }
        crate::ai::sleep_ms(30_000).await;
        loop {
            crate::backup::if_due().await;
            crate::ai::sleep_ms(60 * 60 * 1000).await;
        }
    });

    // Developer self-test: NOTION2_SELFTEST=<file> (always with a throwaway
    // NOTION2_DB) clicks through the interface, writes a report and quits.
    use_future(move || async move {
        let Some(path) = std::env::var_os("NOTION2_SELFTEST").filter(|_| primary) else { return };
        // The test clicks and types through the whole app: never on the real
        // workspace, only on a throwaway one.
        if std::env::var_os("NOTION2_DB").filter(|p| !p.is_empty()).is_none() {
            eprintln!("The self-test needs NOTION2_DB=<a throwaway file>; it does not run on the real workspace.");
            std::process::exit(2);
        }
        // Keep the test window in view: macOS pauses drawing in a window
        // hidden behind others or on another desktop, which would stall the
        // test.
        let quiet = crate::quiet_test();
        if !quiet {
            dioxus::desktop::window().set_always_on_top(true);
        }
        crate::glass::show_over_full_screen(&dioxus::desktop::window().window, !quiet);
        if quiet {
            // Behind the person's windows; the page is drawn all the same
            // (see `draw_when_covered`).
            crate::glass::show_behind(&dioxus::desktop::window().window);
        }
        crate::glass::draw_when_covered(&dioxus::desktop::window().webview);
        // No AI tool is asked during the test; this stands in for its reply.
        crate::ai::set_canned_reply(Some("# Shop and call list\n\nFor the weekend.\n\n## To do\n\n- [ ] Buy milk\n- [ ] Call Sam about Friday\n".to_string()));
        crate::ai::sleep_ms(1200).await;
        // NOTION2_SELFTEST_ONLY=<part of a name>[,…] runs only those checks.
        let only = std::env::var("NOTION2_SELFTEST_ONLY").unwrap_or_default();
        let real = std::env::var_os("NOTION2_REAL_LISTENER").is_some();
        let dictation = std::env::var_os("NOTION2_REAL_DICTATION").is_some();
        let video = std::env::var_os("NOTION2_REAL_VIDEO").is_some();
        let real_pad = std::env::var_os("NOTION2_REAL_PAD").is_some();
        let pad_try = std::env::var_os("NOTION2_PAD_TRY").is_some();
        let script = format!(
            "window.__n2_only = {}; window.__n2_real_listener = {real}; window.__n2_real_dictation = {dictation}; window.__n2_real_video = {video}; window.__n2_real_pad = {real_pad}; window.__n2_pad_try = {pad_try};\n{SELFTEST_JS}",
            serde_json::to_string(&only).unwrap_or_default()
        );
        let mut test = document::eval(&script);
        // The test reports each check as it starts (so a hang can be found),
        // then the results.
        let progress = format!("{}.progress", std::path::Path::new(&path).display());
        let report = loop {
            match test.recv::<serde_json::Value>().await {
                Ok(v) if v.get("progress").is_some() => {
                    let _ = std::fs::write(&progress, v["progress"].to_string());
                }
                Ok(v) => break v,
                Err(e) => break serde_json::json!({ "error": format!("{e:?}") }),
            }
        };
        crate::ai::sleep_ms(400).await;
        // A drawing is reported by how many shapes its saved scene has.
        let shapes = |data: &str| match serde_json::from_str::<serde_json::Value>(data).ok().and_then(|v| v["elements"].as_array().map(Vec::len)) {
            Some(n) => format!("[{n} shapes]"),
            None => String::new(),
        };
        let saved = crate::store::with(|s| s.load()).map(|(pages, blocks)| {
            let mut out = serde_json::Map::new();
            for p in pages.values() {
                let body: Vec<String> = blocks.get(&p.id).map(|bs| bs.iter().map(|b| format!("{}: {}{}", b.kind.as_str(), b.text, shapes(&b.data))).collect()).unwrap_or_default();
                out.insert(format!("{}{}", p.display_title(), if p.trashed { " [trash]" } else { "" }), serde_json::json!(body));
            }
            out
        });
        let full = serde_json::json!({
            "ui": report,
            "saved_pages": saved,
            "glass_setting": crate::store::setting("glass"),
            "windows": sync::window_count(),
        });
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&full).unwrap_or_default());
        std::process::exit(0);
    });

    // Developer aid: NOTION2_DUMP_HTML=<file> saves the rendered window as HTML
    // and quits, so the layout can be checked in an ordinary browser.
    use_future(move || async move {
        let Some(path) = std::env::var_os("NOTION2_DUMP_HTML").filter(|_| primary) else { return };
        crate::ai::sleep_ms(2500).await;
        if let Ok(v) = document::eval("return document.documentElement.outerHTML").await {
            if let Some(html) = v.as_str() {
                let _ = std::fs::write(&path, html);
            }
        }
        std::process::exit(0);
    });

    // Menu bar commands go to the window in front.
    dioxus::desktop::use_muda_event_handler(move |event| {
        if dioxus::desktop::window().window.is_focused() {
            state.command(&event.id().0);
        }
    });

    // Window title follows the open page (set only when it actually changes).
    let window_title = use_memo(move || match state.route() {
        Route::Home => "Home".to_string(),
        Route::Calendar => "Calendar".to_string(),
        Route::Graph => "Graph".to_string(),
        Route::Page(id) => state.pages.read().get(&id).map(|p| p.display_title().to_string()).unwrap_or_default(),
    });
    use_effect(move || {
        dioxus::desktop::window().set_title(&format!("{} — Rune", window_title()));
    });

    // The sidebar shows the space of the page on screen.
    use_effect(move || {
        let _ = state.route();
        state.follow_route_space();
    });

    // The blur behind the window (see glass.rs), set once the page exists and
    // again whenever Settings changes it (not on every move of the
    // Transparency slider).
    let blur = use_memo(move || state.look.read().blur);
    use_effect(move || {
        crate::glass::set_blur(&dioxus::desktop::window().window, blur());
    });

    // The window's glass follows the app's theme, not only the Mac's.
    let theme_choice = use_memo(move || state.ui.read().theme);
    use_effect(move || {
        let theme = match theme_choice() {
            Theme::System => None,
            Theme::Light => Some(WindowTheme::Light),
            Theme::Dark => Some(WindowTheme::Dark),
        };
        dioxus::desktop::window().window.set_theme(theme);
    });

    let ui = state.ui.read();
    let theme = theme_class(ui.theme);
    let zoom = ui.zoom;
    let pinned = ui.sidebar_open;
    let width = ui.sidebar_width;
    let modal = ui.modal;
    let tab = ui.tab().clone();
    let popover_open = ui.popover.is_some() || state.slash.read().is_some();
    drop(ui);
    let toast = state.toast_msg.read().clone().map(|t| (t.1, t.2));
    let glass = glass_vars(&state.look.read());

    rsx! {
        div {
            class: if pinned { "app {theme} pinned" } else { "app {theme}" },
            style: "--sidebar-w: {width}px; --zoom: {zoom}; {glass}",
            chrome::TopRow {}
            div { class: if modal == Some(Modal::Settings) { "shell-body settings-open" } else { "shell-body" },
                if !pinned {
                    div {
                        class: "edge-strip",
                        title: "Click to keep the sidebar open",
                        onclick: move |_| state.toggle_sidebar(),
                    }
                }
                sidebar::Sidebar {}
                main { class: "work-card",
                    // While something is dragged over a pane: where it will
                    // land (placed by bridge.js).
                    div { class: "pane-drop" }
                    div { class: "card-zoom",
                        div { class: if tab.is_split() { "panes split" } else { "panes" },
                            LayoutView { node: tab.layout.clone(), tab: tab.clone() }
                        }
                    }
                }
                // Settings takes the place of the sidebar and the page.
                if modal == Some(Modal::Settings) {
                    modals::SettingsView {}
                }
            }
            if popover_open {
                div { class: "backdrop", onclick: move |_| state.command("escape") }
            }
            PlayerDock {}
            // A test run draws the meeting pad here, not in a window of its own.
            if primary && crate::pad::inline() {
                if let Some(view) = state.pad_view() {
                    div { class: "pad-inline", id: "pad-inline",
                        pad::PadCard { view, on: move |command| state.pad_command(command), page: move |_| {} }
                    }
                }
            }
            match modal {
                Some(Modal::QuickFind) => rsx! { modals::QuickFind {} },
                Some(Modal::Trash) => rsx! { modals::TrashModal {} },
                Some(Modal::History) => rsx! { modals::HistoryModal {} },
                Some(Modal::Voice) => rsx! { modals::VoiceModal {} },
                Some(Modal::AskNotes) => rsx! { modals::AskNotesModal {} },
                Some(Modal::Todos) => rsx! { modals::TodosModal {} },
                Some(Modal::Settings) | None => rsx! {},
            }
            if let Some((message, leaving)) = toast {
                div { class: if leaving { "toast leaving" } else { "toast" }, "{message}" }
            }
            FormatBar {}
            LineMic {}
            TextMenu {}
            FindBar {}
            LinkEditor {}
        }
    }
}

/// A tab's pages, laid out in its rows and columns (see layout.rs). Each
/// row or column member after the first has a line before it to resize by.
#[component]
fn LayoutView(node: Layout, tab: Tab) -> Element {
    let state = use_context::<AppState>();
    match node {
        Layout::Pane { id } => {
            let Some(pane) = tab.pane(&id) else { return rsx! {} };
            let route = pane.route().clone();
            let multi = tab.is_split();
            let focused = multi && tab.focus == id;
            // The same page can't be edited in two panes at once (its blocks
            // would clash), so later copies say where it is.
            let duplicate =
                matches!(route, Route::Page(_)) && tab.ordered().iter().take_while(|p| p.id != id).any(|p| *p.route() == route);
            let pid = id.clone();
            rsx! {
                div {
                    class: if focused { "pane focused" } else { "pane" },
                    "data-pane": "{id}",
                    onmousedown: move |_| state.focus_pane(&pid),
                    chrome::PageBar { route: route.clone(), pane: id.clone(), multi }
                    if let Route::Page(page) = &route {
                        ListenBar { page: page.clone() }
                        PlaceBar { page: page.clone() }
                    }
                    div { class: "scroller",
                        PaneBody { route: route.clone(), duplicate }
                    }
                    // The AI: a small button at the foot that opens into a bar.
                    page::AiDock { route, duplicate }
                }
            }
        }
        Layout::Split { id, dir, children, sizes } => {
            // A folded pane is a slim rail of a fixed width; the open parts
            // share the rest in proportion (their shares made to add up to
            // 1, or the row would leave a gap).
            let rails = tab.rails();
            let is_rail = |c: &Layout| matches!(c, Layout::Pane { id } if rails.contains(id));
            let open_total: f64 = children.iter().zip(&sizes).filter(|(c, _)| !is_rail(c)).map(|(_, s)| *s).sum();
            let cells: Vec<(usize, String, Option<String>, bool, String, Layout)> = children
                .into_iter()
                .zip(sizes)
                .enumerate()
                .map(|(i, (c, s))| {
                    let leaf = match &c {
                        Layout::Pane { id } => Some(id.clone()),
                        _ => None,
                    };
                    let railed = is_rail(&c);
                    let style = if railed { "flex: 0 0 28px".to_string() } else { format!("flex: {} 1 0", s / open_total.max(f64::EPSILON)) };
                    (i, c.key().to_string(), leaf, railed, style, c)
                })
                .collect();
            rsx! {
                div {
                    class: if dir == Dir::Row { "split-group split-row" } else { "split-group split-col" },
                    "data-split": "{id}",
                    for (i, key, leaf, railed, style, child) in cells {
                        div {
                            key: "{key}",
                            class: if railed { "split-cell railed" } else { "split-cell" },
                            style: "{style}",
                            "data-leaf": leaf.clone().unwrap_or_default(),
                            if i > 0 {
                                div {
                                    class: "sash",
                                    "data-split": "{id}",
                                    "data-index": "{i - 1}",
                                    title: "Drag to resize · Drag to the edge to fold · Double-click to even out",
                                }
                            }
                            if railed {
                                PaneRail { tab: tab.clone(), pane: leaf.clone().unwrap_or_default() }
                            } else {
                                LayoutView { node: child, tab: tab.clone() }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The line under a meeting page's bar: first it asks whether to listen and
/// write the notes (each time, never by itself); while listening it says so,
/// with the time, the last words heard and the way to stop. Any other page
/// shows nothing.
#[component]
fn ListenBar(page: String) -> Element {
    let state = use_context::<AppState>();
    // While the meeting's pad asks on the screen, the page does not. (When
    // the pad has become its small form, or the notepad, the page asks as
    // before: the person may never have seen the pad's question.)
    let padded = state.pad.read().as_ref().is_some_and(|p| p.page == page && p.form == crate::pad::Form::Ask);
    let asking = !padded && state.listen_offer.read().as_deref() == Some(page.as_str());
    let listening = state.listening.read().clone().filter(|l| l.page == page);
    let (p1, p2) = (page.clone(), page.clone());
    if let Some(l) = listening {
        let clock = format!("{}:{:02}", l.seconds / 60, l.seconds % 60);
        let hears = if l.video {
            "the video (only what the Mac plays)"
        } else if l.others {
            "you and the others"
        } else {
            "your microphone"
        };
        // With no AI tool nothing is said of notes that an AI would write.
        let stop = if crate::ai::has_ai() { "Stop and write the notes" } else { "Stop" };
        return rsx! {
            div { class: "listen-bar on", role: "status",
                span { class: "listen-dot" }
                span { class: "listen-what", if l.stopping { "Finishing…" } else { "Listening" } }
                span { class: "listen-time", "{clock}" }
                span { class: "listen-latest", title: "It hears {hears}. {l.words} words so far.", "{l.latest}" }
                button { class: "secondary-btn small", id: "listen-stop", disabled: l.stopping, onclick: move |_| state.stop_listening(), "{stop}" }
            }
        };
    }
    if asking {
        return rsx! {
            div { class: "listen-bar", role: "group", "aria-label": "Listen to this meeting",
                span { class: "listen-ask", Icon { name: "mic", size: 15 } }
                span { class: "listen-what", if crate::ai::has_ai() { "Listen to this meeting and write the notes?" } else { "Listen to this meeting and keep the transcript?" } }
                span { class: "listen-latest", "{crate::listen::where_speech_goes()}" }
                button { class: "primary-btn small", id: "listen-start", onclick: move |_| state.start_listening(&p1), "Listen" }
                button { class: "ghost-btn small", id: "listen-no", onclick: move |_| {
                        if state.listen_offer.peek().as_deref() == Some(p2.as_str()) {
                            let mut offer = state.listen_offer;
                            offer.set(None);
                        }
                    }, "Not now" }
            }
        };
    }
    rsx! {}
}

/// A pane folded to a slim rail (its divider dragged to the edge, or
/// "Minimize pane" in its menu), as in Aven: its icon and name. One click
/// opens it at the size it had.
#[component]
fn PaneRail(tab: Tab, pane: String) -> Element {
    let state = use_context::<AppState>();
    let Some(p) = tab.pane(&pane) else { return rsx! {} };
    let (icon, title) = chrome::label(p.route(), &state.pages.read());
    rsx! {
        button {
            class: "pane-rail",
            "data-rail": "{pane}",
            title: "{title}. Click to open",
            "aria-label": "Open {title}",
            onclick: move |_| state.fold_pane(&pane, false),
            span { class: "rail-icon", icons::PageIcon { icon, size: 14 } }
            span { class: "rail-title", "{title}" }
        }
    }
}

/// On the page of a meeting that is over, and on any page after "Move to…"
/// in its menu: which page to keep it in. The list is the pages of the
/// space, with the one it is in now chosen; a new choice moves it there at
/// once. "Done" puts the bar away, and the page asks no more.
#[component]
fn PlaceBar(page: String) -> Element {
    let state = use_context::<AppState>();
    // While the meeting's pad asks this on the screen, the page does not.
    let padded = state.pad.read().as_ref().is_some_and(|p| p.page == page && matches!(p.form, crate::pad::Form::End | crate::pad::Form::Kept));
    if padded || !state.asks_place(&page) {
        return rsx! {};
    }
    let here = state.pages.read().get(&page).and_then(|p| p.parent_id.clone()).unwrap_or_default();
    let places = state.meeting_places(&page);
    let what = if state.is_notes_page(&page) { "Keep these notes in" } else { "Keep this page in" };
    // A meeting remembers its page: the next one of the same name starts there.
    let (before, by_itself) = state.slot_of_page(&page);
    let kept = match state.next_notes_go_here(&page) {
        Some(name) => format!("The next “{name}” notes go here too."),
        None if by_itself => "The notes of a meeting at this time go here by themselves.".to_string(),
        None => String::new(),
    };
    let (p1, p2) = (page.clone(), page.clone());
    rsx! {
        div { class: "listen-bar place-bar", role: "group", "aria-label": "{what}",
            span { class: "listen-ask", icons::Icon { name: "folder", size: 15 } }
            span { class: "listen-what", "{what}" }
            select {
                id: "meeting-place",
                class: "select",
                "aria-label": "The page to keep it in",
                onchange: move |e: FormEvent| state.keep_page_in(&p1, &e.value()),
                option { value: "", selected: here.is_empty(), "No page (top of the list)" }
                for (id, title, depth) in places {
                    option { key: "{id}", value: "{id}", selected: id == here, "{\"\u{2003}\".repeat(depth)}{title}" }
                }
            }
            // A meeting at this time was kept in these pages before: one click.
            if !before.is_empty() {
                span { class: "place-before", "Last time:" }
                for (home, title) in before {
                    button {
                        key: "{home}",
                        class: "secondary-btn small place-same",
                        "data-home": "{home}",
                        title: "Keep these notes in {title}, as the last time",
                        onclick: {
                            let (page, home) = (page.clone(), home.clone());
                            move |_| state.keep_page_in(&page, &home)
                        },
                        "{title}"
                    }
                }
            }
            span { class: "listen-latest", id: "meeting-place-kept", "{kept}" }
            button { class: "secondary-btn small", id: "meeting-place-done", onclick: move |_| state.place_done(&p2), "Done" }
        }
    }
}

/// The video player at the corner of the window: a bar to close it or to
/// open the video in the browser, and the frame the player is put on (the
/// player is a view of its own, over the frame: see player.rs).
#[component]
fn PlayerDock() -> Element {
    let state = use_context::<AppState>();
    let Some(playing) = state.player.read().clone() else { return rsx! {} };
    let watch = crate::video::watch_url(&playing.video);
    let watch2 = watch.clone();
    // Where the person put it, and how wide (bridge.js moves and sizes it).
    let dock = state.player_dock.read().clone();
    // Notes of the video: the option is there while a video plays and the
    // app can listen and listens to nothing else. With no AI tool it is the
    // words alone, and says so.
    let listening = state.listening.read().clone();
    let hears = listening.clone().filter(|l| l.video);
    let can_note = listening.is_none() && playing.problem.is_none() && crate::listen::available();
    let has_ai = crate::ai::has_ai();
    rsx! {
        section {
            class: "player-dock",
            id: "player-dock",
            "aria-label": "Video player",
            "data-video": "{playing.video.id}",
            "data-ready": "{playing.ready}",
            "data-h": "{dock.h}",
            "data-v": "{dock.v}",
            "data-x": "{dock.x}",
            "data-y": "{dock.y}",
            "data-w": "{dock.w}",
            style: "{dock.style()}",
            for corner in ["nw", "ne", "sw", "se"] {
                span { key: "{corner}", class: "player-grip", "data-corner": "{corner}", "aria-hidden": "true" }
            }
            div { class: "player-bar", title: "Drag to move. Drag a corner for another size. Double-click for large or small.",
                span { class: "player-title", "Video" }
                if let Some(l) = hears {
                    span { class: "listen-dot", title: "Rune hears the video (only what the Mac plays, not the microphone)." }
                    span { class: "player-clock", "{crate::pad::clock(l.seconds)}" }
                    button {
                        class: "secondary-btn small",
                        id: "player-notes-stop",
                        title: if has_ai { "Stop and write the notes under the video's line" } else { "Stop. The words are under the video's line." },
                        disabled: l.stopping,
                        onclick: move |_| state.stop_listening(),
                        "Stop"
                    }
                } else if can_note {
                    button {
                        class: "ghost-btn small",
                        id: "player-notes",
                        title: if has_ai {
                            "Write down what the video says and make notes of it, under the video's line. Rune hears only what the Mac plays, not the microphone."
                        } else {
                            "Write down what the video says, under the video's line. Rune hears only what the Mac plays, not the microphone."
                        },
                        onclick: move |_| state.take_video_notes(),
                        if has_ai { "Take notes" } else { "Write it down" }
                    }
                }
                button {
                    class: "ghost-btn small",
                    id: "player-browser",
                    title: "Open the video in the browser",
                    onclick: move |_| state.open_url(&watch),
                    "Open in browser"
                }
                button {
                    class: "icon-btn small",
                    id: "player-close",
                    title: "Close the video",
                    "aria-label": "Close the video",
                    onclick: move |_| state.close_player(),
                    icons::Icon { name: "close", size: 14 }
                }
            }
            div { class: "player-screen", id: "player-screen",
                if let Some(problem) = playing.problem {
                    div { class: "player-problem", role: "status",
                        span { "{problem}" }
                        button { class: "secondary-btn small", onclick: move |_| state.open_url(&watch2), "Open in browser" }
                    }
                }
            }
        }
    }
}

/// What a pane shows. The same page can't be edited in two panes at once.
#[component]
fn PaneBody(route: Route, duplicate: bool) -> Element {
    if duplicate {
        return rsx! { div { class: "empty", "This page is already open in another pane of this tab." } };
    }
    match route {
        Route::Home => rsx! { page::HomeView {} },
        Route::Calendar => rsx! { calendar::CalendarView {} },
        Route::Graph => rsx! { graph::GraphView {} },
        Route::Page(id) => rsx! {
            for id in std::iter::once(id) {
                page::PageView { key: "{id}", page_id: id }
            }
        },
    }
}

/// Floating bar shown over selected text. Shown, placed and handled by
/// assets/bridge.js; Rust only draws it.
#[component]
fn FormatBar() -> Element {
    rsx! {
        div { id: "fmt-bar", class: "fmt-bar", role: "toolbar", "aria-label": "Text formatting",
            button { "data-fmt": "bold", title: "Bold (⌘B)", Icon { name: "bold", size: 16 } }
            button { "data-fmt": "italic", title: "Italic (⌘I)", Icon { name: "italic", size: 16 } }
            button { "data-fmt": "strike", title: "Strikethrough (⇧⌘S)", Icon { name: "strikethrough", size: 16 } }
            button { "data-fmt": "code", title: "Code (⌘E)", Icon { name: "inline-code", size: 16 } }
            span { class: "fmt-sep" }
            button { "data-fmt": "link", title: "Link (⌘K)", Icon { name: "link", size: 16 } }
            span { class: "fmt-sep" }
            button { "data-fmt": "speak", title: "Say what to do with this text", "aria-label": "Say what to do with this text", Icon { name: "mic", size: 16 } }
        }
    }
}

/// The short menu for a right-click on text: Cut, Copy, Paste and no more.
/// (The system's own menu is never shown; see "right-click" in bridge.js.)
#[component]
fn TextMenu() -> Element {
    rsx! {
        div { id: "ctx-menu", class: "ctx-menu", role: "menu",
            div { class: "menu-item compact", role: "menuitem", "data-ctx": "cut",
                span { "Cut" }
                span { class: "menu-shortcut", "⌘X" }
            }
            div { class: "menu-item compact", role: "menuitem", "data-ctx": "copy",
                span { "Copy" }
                span { class: "menu-shortcut", "⌘C" }
            }
            div { class: "menu-item compact", role: "menuitem", "data-ctx": "paste",
                span { "Paste" }
                span { class: "menu-shortcut", "⌘V" }
            }
        }
    }
}

/// The microphone on an empty line: speak, and the AI writes it into the
/// page there. One button for the whole window; assets/bridge.js shows it
/// beside the hint text of the empty line that has the caret.
#[component]
fn LineMic() -> Element {
    rsx! {
        button {
            id: "line-mic",
            class: "line-mic",
            title: "Speak, and the AI writes it here",
            "aria-label": "Speak, and the AI writes it here",
            tabindex: "-1",
            Icon { name: "mic", size: 14 }
        }
    }
}

/// The find bar (⌘F): finds text in the page in front. Shown, filled and
/// handled by assets/bridge.js; Rust only draws it.
#[component]
fn FindBar() -> Element {
    rsx! {
        div { id: "find-bar", class: "find-bar", role: "search",
            Icon { name: "search", size: 15 }
            input {
                id: "find-input",
                class: "find-input",
                placeholder: "Find in page",
                "aria-label": "Find in page",
                spellcheck: "false",
                autocomplete: "off",
            }
            span { id: "find-count", class: "find-count", "aria-live": "polite" }
            button { class: "icon-btn", "data-find": "prev", title: "Previous (⇧⌘G)", "aria-label": "Previous match", Icon { name: "chevron-up", size: 15 } }
            button { class: "icon-btn", "data-find": "next", title: "Next (⌘G)", "aria-label": "Next match", Icon { name: "chevron-down", size: 15 } }
            button { class: "icon-btn", "data-find": "close", title: "Close (esc)", "aria-label": "Close", Icon { name: "close", size: 14 } }
        }
    }
}

/// The small box for adding or editing a link (⌘K on selected text).
#[component]
fn LinkEditor() -> Element {
    rsx! {
        div { id: "link-editor", class: "link-editor",
            Icon { name: "link", size: 16 }
            input {
                id: "link-input",
                class: "le-input",
                r#type: "text",
                placeholder: "Paste or type a link",
                spellcheck: false,
                autocomplete: "off",
            }
            button { class: "le-apply", "data-link": "apply", "Link" }
            button { class: "le-remove", "data-link": "remove", title: "Remove link", Icon { name: "trash", size: 14 } }
        }
    }
}
