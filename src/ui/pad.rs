//! The meeting pad as it is drawn, and the window it is drawn in (see pad.rs).
//!
//! `PadCard` draws a `pad::View` and says each click with a `pad::Command`.
//! The pad's own window (`PadRoot`) holds nothing but that card; a test run
//! draws the same card in the first window (`pad::inline`).

use super::icons::{Icon, PageIcon};
use crate::pad::{self, Command, Form, View};
use dioxus::desktop::tao::window::{Theme as WindowTheme, Window};
use dioxus::desktop::{LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use futures_util::StreamExt;
use std::cell::Cell;

const PAD_JS: &str = include_str!("../../assets/pad.js");

/// The screen to use when macOS does not say which one the pad is on.
const SOME_SCREEN: (f64, f64, f64, f64) = (0.0, 25.0, 1440.0, 800.0);

thread_local! {
    /// Where the app last put the pad's window: left, top, wide, high. (A
    /// pad found somewhere else was dragged there by the person.)
    static PUT: Cell<Option<(f64, f64, f64, f64)>> = const { Cell::new(None) };
    /// The screen the pad is on (the part windows may use), and the room
    /// between the pad and that screen's bottom right corner.
    static AREA: Cell<Option<(f64, f64, f64, f64)>> = const { Cell::new(None) };
    static GAP: Cell<(f64, f64)> = const { Cell::new(pad::CORNER) };
    /// The corner radius its glass has now.
    static RADIUS: Cell<f64> = const { Cell::new(0.0) };
}

/// Open the window of the pad. (Asked by the first window, when there is
/// something to show and no window for it.)
pub fn open_window() {
    pad::first_click_acts();
    let dom = VirtualDom::new(PadRoot);
    dioxus::desktop::window().new_window(dom, crate::pad_window_config());
}

/// If the person dragged the pad since the app last put it somewhere: where
/// it is now is its place, on the screen it is on now, and the next meeting's
/// pad comes there too.
fn note_drag(window: &Window) {
    let Some((left, top, wide, high)) = PUT.get() else { return };
    let Ok(at) = window.outer_position() else { return };
    let at = at.to_logical::<f64>(window.scale_factor());
    if (at.x - left).abs() < 2.0 && (at.y - top).abs() < 2.0 {
        return;
    }
    let area = pad::screen_area(Some((at.x + wide / 2.0, at.y + high / 2.0))).or(AREA.get()).unwrap_or(SOME_SCREEN);
    let gap = (area.0 + area.2 - (at.x + wide), area.1 + area.3 - (at.y + high));
    AREA.set(Some(area));
    GAP.set(gap);
    PUT.set(Some((at.x, at.y, wide, high)));
    pad::save_gap(gap);
}

/// The pad is `wide` by `high` now, with corners of `radius`: make its
/// window that big, with the same corner of the screen kept (so the pad
/// grows up and to the left), and show it if it is not shown yet.
fn fit(wide: f64, high: f64, radius: f64) {
    let desktop = dioxus::desktop::window();
    let window = &desktop.window;
    note_drag(window);
    let area = AREA.get().unwrap_or_else(|| {
        // The screen the pointer is on: the person has just joined the meeting there.
        let area = pad::screen_area(None).unwrap_or(SOME_SCREEN);
        AREA.set(Some(area));
        area
    });
    let (left, top) = pad::place_in(area, (wide, high), GAP.get());
    window.set_inner_size(LogicalSize::new(wide, high));
    window.set_outer_position(LogicalPosition::new(left, top));
    let first = PUT.replace(Some((left, top, wide, high))).is_none();
    if first || (RADIUS.get() - radius).abs() > 0.1 {
        RADIUS.set(radius);
        crate::glass::card(window, radius);
    }
    if first {
        pad::bring_out(window, pad::hidden_from_shares());
    }
}

/// (For the self-test.) Where the pad's window is and what macOS says about it.
pub fn probe() -> serde_json::Value {
    let mut facts = pad::window_facts().unwrap_or_else(|| serde_json::json!({}));
    facts["open"] = pad::window_facts().is_some().into();
    facts["form"] = pad::latest_form().map(|f| f.name()).into();
    facts["result"] = pad::result();
    if let (Some((left, top, wide, high)), Some(area)) = (PUT.get(), AREA.get()) {
        facts["wide"] = wide.into();
        facts["high"] = high.into();
        facts["gap_right"] = (area.0 + area.2 - (left + wide)).into();
        facts["gap_bottom"] = (area.1 + area.3 - (top + high)).into();
        facts["radius"] = RADIUS.get().into();
    }
    facts
}

/// What the page script says that is the window's business.
fn page_says(message: &serde_json::Value) {
    match message["t"].as_str() {
        Some("size") => {
            if let (Some(wide), Some(high)) = (message["w"].as_f64(), message["h"].as_f64()) {
                fit(wide, high, message["r"].as_f64().unwrap_or(13.0));
            }
        }
        Some("drag") => dioxus::desktop::window().drag(),
        // A press in the notepad: the pad takes the keyboard, to type. (The
        // press itself gives it the keyboard as well; this asks once more.)
        Some("keys") => pad::take_keyboard(&dioxus::desktop::window().window),
        _ => {}
    }
}

/// The pad's own window: the card, as the first window says it is.
#[component]
fn PadRoot() -> Element {
    let mut view = use_signal(|| None::<View>);
    use_hook(|| {
        PUT.set(None);
        AREA.set(None);
        GAP.set(pad::saved_gap());
        RADIUS.set(0.0);
    });

    use_future(move || async move {
        let (number, first, mut later) = pad::attach();
        if let Some(first) = first {
            pad::window_is(Some(&dioxus::desktop::window().window));
            view.set(Some(first));
            while let Some(message) = later.next().await {
                match message {
                    pad::ToPad::Show(next) => view.set(Some(*next)),
                    pad::ToPad::Run(script) => {
                        // (A script that is still running holds nothing up.)
                        spawn(async move {
                            if let Ok(value) = document::eval(&script).await {
                                pad::set_result(value);
                            }
                        });
                    }
                    pad::ToPad::Press(selector) => {
                        // Where the thing is in the page, then a true mouse event there.
                        let find = format!(
                            "const el = document.querySelector({}); if (!el) return null; const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2];",
                            serde_json::to_string(&selector).unwrap_or_default()
                        );
                        if let Ok(place) = document::eval(&find).await {
                            if let (Some(x), Some(y)) = (place[0].as_f64(), place[1].as_f64()) {
                                pad::press(&dioxus::desktop::window().window, x, y);
                            }
                        }
                    }
                    pad::ToPad::Close => break,
                }
            }
        }
        // The pad went away (or went before this window had opened).
        let desktop = dioxus::desktop::window();
        note_drag(&desktop.window);
        pad::window_is(None);
        pad::detach(number);
        desktop.close();
    });

    // The pad may take the keyboard only as the notepad (a press in it
    // gives it the keyboard, as in any window). When it stops being the
    // notepad it gives the keyboard back to the program in front, and cannot
    // have it: every press on the other forms acts at once.
    let form = use_memo(move || view.read().as_ref().map(|v| v.form));
    use_effect(move || {
        let Some(form) = form() else { return };
        let desktop = dioxus::desktop::window();
        if form != Form::Open {
            pad::leave_keyboard(&desktop.window);
        }
        desktop.window.set_focusable(form == Form::Open);
    });

    // Its glass is light or dark as the app is.
    let theme = use_memo(move || view.read().as_ref().map(|v| v.theme.clone()).unwrap_or_default());
    use_effect(move || {
        let theme = match theme().as_str() {
            "theme-light" => Some(WindowTheme::Light),
            "theme-dark" => Some(WindowTheme::Dark),
            _ => None,
        };
        dioxus::desktop::window().window.set_theme(theme);
    });

    let Some(current) = view.read().clone() else { return rsx! {} };
    rsx! {
        PadCard {
            view: current,
            on: move |command| pad::ask(command),
            page: move |message: serde_json::Value| page_says(&message),
        }
    }
}

/// The top of the card: the page's icon, the name of the meeting, its time,
/// and the card's own small buttons. The window is dragged by it.
#[component]
fn Head(view: View, children: Element) -> Element {
    rsx! {
        header { class: "pad-head", "data-drag": "head",
            span { class: "pad-icon", PageIcon { icon: view.icon.clone(), size: 15 } }
            div { class: "pad-title",
                b { "{view.title}" }
                span { "{view.when}" }
            }
            {children}
        }
    }
}

/// The line under the notepad: whether the app listens, the last words, and
/// the button to stop or to start.
#[component]
fn Foot(view: View, on: EventHandler<Command>) -> Element {
    if let Some(hears) = &view.hears {
        let stop = if view.has_ai { "Stop and write the notes" } else { "Stop and keep the transcript" };
        let latest = if hears.stopping {
            "Finishing…"
        } else if hears.latest.is_empty() {
            "Listening…"
        } else {
            hears.latest.as_str()
        };
        return rsx! {
            footer { class: "pad-foot", role: "status",
                span { class: "listen-dot" }
                span { class: "pad-clock", "{hears.clock}" }
                span { class: "pad-latest", "{latest}" }
                button { class: "secondary-btn small", id: "pad-stop", title: "{stop}", disabled: hears.stopping, onclick: move |_| on.call(Command::Stop), "Stop" }
            }
        };
    }
    let says = if !view.problem.is_empty() {
        view.problem.as_str()
    } else if view.can_listen {
        "Not listening"
    } else {
        "Rune cannot listen now"
    };
    rsx! {
        footer { class: "pad-foot", role: "status",
            span { class: "pad-dot" }
            span { class: if view.problem.is_empty() { "pad-latest" } else { "pad-latest pad-problem" }, title: "{says}", "{says}" }
            if view.can_listen {
                button { class: "secondary-btn small", id: "pad-listen", onclick: move |_| on.call(Command::Listen), "Listen" }
            }
        }
    }
}

/// The pages the notes can be kept in: Rune's own list of plain rows (as the
/// model menu of the Ask AI box), in the card itself, over the field.
#[component]
fn PlaceList(view: View, on: EventHandler<Command>) -> Element {
    rsx! {
        div { class: "pad-list", id: "pad-places", role: "listbox", "aria-label": "The page to keep the notes in",
            button {
                class: if view.here.is_empty() { "pad-place quiet active" } else { "pad-place quiet" },
                role: "option",
                "aria-selected": "{view.here.is_empty()}",
                "data-home": "",
                onclick: move |_| on.call(Command::Keep(String::new())),
                "No page (top of the list)"
            }
            div { class: "pad-list-sep" }
            for place in view.places.iter().cloned() {
                button {
                    key: "{place.id}",
                    class: if place.id == view.here { "pad-place active" } else { "pad-place" },
                    role: "option",
                    "aria-selected": "{place.id == view.here}",
                    "data-home": "{place.id}",
                    style: "padding-left: {10 + place.depth * 20}px",
                    onclick: move |_| on.call(Command::Keep(place.id.clone())),
                    PageIcon { icon: place.icon.clone(), size: 15 }
                    span { class: "pad-place-name", "{place.title}" }
                }
            }
        }
    }
}

/// The pad. `page` gets what the page script says about the window (its
/// size, a drag of its head): the pad's own window acts on it.
#[component]
pub fn PadCard(view: View, on: EventHandler<Command>, page: EventHandler<serde_json::Value>) -> Element {
    // The page script: the lines of the notepad go to the first window, the
    // rest is the window's business.
    use_future(move || async move {
        let mut script = document::eval(PAD_JS);
        while let Ok(message) = script.recv::<serde_json::Value>().await {
            if message["t"] == "lines" {
                let lines = message["lines"].as_array().map(|l| l.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
                on.call(Command::Lines(lines));
            } else {
                page.call(message);
            }
        }
    });
    let mut listing = use_signal(|| false);
    let form = view.form;
    let label = match form {
        Form::Ask => "Listen to this meeting",
        Form::Open | Form::Small => "Meeting pad",
        Form::End | Form::Kept => "The meeting is over",
    };
    let body = match form {
        Form::Ask => rsx! {
            Head { view: view.clone(),
                button { class: "icon-btn small", id: "pad-not-now", title: "Not now", "aria-label": "Not now", onclick: move |_| on.call(Command::NotNow), Icon { name: "close", size: 14 } }
            }
            div { class: "pad-ask",
                p { class: "pad-q", if view.has_ai { "Listen and write the notes?" } else { "Listen and keep the transcript?" } }
                p { class: "pad-sub", "{view.sound}" }
            }
            footer { class: "pad-actions",
                button { class: "ghost-btn small", id: "pad-notes-only", onclick: move |_| on.call(Command::NotesOnly), "Notes only" }
                button { class: "primary-btn small", id: "pad-listen", onclick: move |_| on.call(Command::Listen), "Listen" }
            }
        },
        Form::Open => {
            let lines = serde_json::to_string(&view.lines).unwrap_or_else(|_| "[]".into());
            rsx! {
                Head { view: view.clone(),
                    button { class: "icon-btn small", id: "pad-open-page", title: "Open the page in Rune", "aria-label": "Open the page in Rune", onclick: move |_| on.call(Command::OpenPage), Icon { name: "arrow-up-right", size: 15 } }
                    button { class: "icon-btn small", id: "pad-small", title: "Make small", "aria-label": "Make small", onclick: move |_| on.call(Command::Small), Icon { name: "minus", size: 15 } }
                }
                // What is typed here is the page script's (pad.js): nothing is drawn in it from here.
                ul {
                    class: "pad-notes",
                    id: "pad-notes",
                    contenteditable: "true",
                    role: "textbox",
                    spellcheck: "false",
                    "aria-multiline": "true",
                    "aria-label": "Your notes",
                    "data-lines": "{lines}",
                    "data-placeholder": "Type your notes. They go into the page.",
                }
                Foot { view: view.clone(), on }
            }
        }
        Form::Small => rsx! {
            div {
                class: "pad-pill",
                id: "pad-pill",
                role: "button",
                tabindex: "0",
                title: "Open the pad",
                "aria-label": "Open the meeting pad",
                "data-drag": "move",
                onclick: move |_| on.call(Command::Open),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        on.call(Command::Open);
                    }
                },
                if let Some(hears) = &view.hears {
                    span { class: "listen-dot" }
                    span { class: "pad-clock", "{hears.clock}" }
                } else {
                    span { class: "pad-dot" }
                }
                span { class: "pad-pill-name", "{view.title}" }
            }
            // While it listens the pad stays in view: it is the sign that it listens.
            if view.hears.is_none() {
                button { class: "icon-btn small", id: "pad-away", title: "Put away for this meeting", "aria-label": "Put away for this meeting", onclick: move |_| on.call(Command::NotNow), Icon { name: "close", size: 13 } }
            }
        },
        Form::End => rsx! {
            Head { view: view.clone(),
                button { class: "icon-btn small", id: "pad-open-page", title: "Open the page in Rune", "aria-label": "Open the page in Rune", onclick: move |_| on.call(Command::OpenPage), Icon { name: "arrow-up-right", size: 15 } }
            }
            div { class: "pad-end",
                if !view.status.is_empty() {
                    p { class: "pad-status", id: "pad-status", role: "status", "{view.status}" }
                }
                if view.asks_place {
                    p { class: "pad-label", id: "pad-keep-label", "Keep these notes in" }
                    if listing() {
                        PlaceList { view: view.clone(), on }
                    }
                    button {
                        class: "pad-pick",
                        id: "pad-keep-in",
                        "aria-haspopup": "listbox",
                        "aria-expanded": "{listing()}",
                        "aria-labelledby": "pad-keep-label pad-keep-name",
                        onclick: move |_| {
                            let open = !listing();
                            listing.set(open);
                        },
                        if !view.here.is_empty() {
                            PageIcon { icon: view.here_icon.clone(), size: 15 }
                        }
                        span { class: "pad-pick-name", id: "pad-keep-name", if view.here.is_empty() { "No page (top of the list)" } else { "{view.here_title}" } }
                        span { class: "pad-chev", Icon { name: "chevron-down", size: 13 } }
                    }
                    div { class: "pad-row",
                        // The notes of a meeting at this time were kept in these pages before: one click.
                        if !view.before.is_empty() {
                            span { class: "pad-before", "Last time:" }
                            for (home, title) in view.before.iter().cloned() {
                                button {
                                    key: "{home}",
                                    class: "secondary-btn small pad-same",
                                    "data-home": "{home}",
                                    title: "Keep these notes in {title}, as the last time",
                                    onclick: move |_| on.call(Command::Keep(home.clone())),
                                    "{title}"
                                }
                            }
                        }
                        span { class: "pad-grow" }
                        button { class: "secondary-btn small", id: "pad-done", onclick: move |_| on.call(Command::Done), "Done" }
                    }
                } else {
                    if !view.told.is_empty() {
                        p { class: "pad-told", id: "pad-told", "{view.told}" }
                    }
                    div { class: "pad-row",
                        span { class: "pad-grow" }
                        button { class: "secondary-btn small", id: "pad-done", onclick: move |_| on.call(Command::Done), "Done" }
                    }
                }
            }
        },
        Form::Kept => rsx! {
            div { class: "pad-kept", id: "pad-kept", role: "status",
                span { class: "pad-ok", Icon { name: "check", size: 16 } }
                span { if view.kept.is_empty() { "Kept at the top of the list" } else { "Kept in {view.kept}" } }
            }
        },
    };
    rsx! {
        div { class: "padwin {view.theme}", style: "{view.glass}",
            div {
                class: "pad-card is-{form.name()}",
                id: "pad-card",
                role: "group",
                "aria-label": "{label}",
                "data-form": "{form.name()}",
                onkeydown: move |e| {
                    if e.key() == Key::Escape && listing() {
                        listing.set(false);
                    }
                },
                {body}
            }
        }
    }
}
