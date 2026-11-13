//! The calendar: a day, week or month view of the space's events, with an
//! editor panel for the selected event, in the spirit of Notion Calendar.
//! The day view has the day at the left (what is due, then the hours) and
//! the notes of one of its events at the right, written in place.
//! Click an empty time in the week to add an event and type its name there
//! (double-click a day for an all-day one); click an event to edit it.

use super::icons::{Icon, ICON_COLORS};
use crate::model::{Event, Space};
use crate::repeat;
use crate::state::{AppState, CalView, OpenTodo, Route};
use crate::when::{self, HOUR, MINUTE};
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use dioxus::prelude::*;

const HOUR_PX: f64 = 48.0;
/// A click on an empty time adds an event this long, starting on a quarter hour.
const SLOT_MINUTES: u32 = 15;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Events touching a day: all-day ones first, then by start time.
fn day_events(events: &[Event], day: NaiveDate) -> Vec<Event> {
    let (from, to) = (when::midnight(day), when::midnight(when::next_day(day)));
    let mut v: Vec<Event> = events.iter().filter(|e| e.overlaps(from, to)).cloned().collect();
    v.sort_by(|a, b| b.all_day.cmp(&a.all_day).then(a.start.cmp(&b.start)).then(a.title.cmp(&b.title)));
    v
}

/// A timed event placed in a day column.
#[derive(Clone, PartialEq)]
struct Placed {
    event: Event,
    top: f64,
    height: f64,
    left: f64,
    width: f64,
}

/// Lay out a day's timed events: overlapping events share the width.
fn place(events: Vec<Event>, day: NaiveDate) -> Vec<Placed> {
    let (from, to) = (when::midnight(day), when::midnight(when::next_day(day)));
    let mut timed: Vec<Event> = events.into_iter().filter(|e| !e.all_day).collect();
    timed.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut out = Vec::new();
    let mut cluster: Vec<(Event, usize)> = Vec::new();
    let mut col_ends: Vec<i64> = Vec::new();
    let mut cluster_end = i64::MIN;
    let flush = |cluster: &mut Vec<(Event, usize)>, cols: usize, out: &mut Vec<Placed>| {
        let width = 100.0 / cols.max(1) as f64;
        for (e, col) in cluster.drain(..) {
            let s = e.start.max(from);
            let t = e.end.min(to);
            let top = (s - from) as f64 / HOUR as f64 * HOUR_PX;
            let height = ((t - s) as f64 / HOUR as f64 * HOUR_PX).max(20.0);
            out.push(Placed { event: e, top, height, left: col as f64 * width, width });
        }
    };
    for e in timed {
        if e.start >= cluster_end && !cluster.is_empty() {
            let cols = col_ends.len();
            flush(&mut cluster, cols, &mut out);
            col_ends.clear();
        }
        let col = match col_ends.iter().position(|end| *end <= e.start) {
            Some(c) => {
                col_ends[c] = e.end;
                c
            }
            None => {
                col_ends.push(e.end);
                col_ends.len() - 1
            }
        };
        cluster_end = cluster_end.max(e.end);
        if cluster.is_empty() {
            cluster_end = e.end;
        }
        cluster.push((e, col));
    }
    let cols = col_ends.len();
    flush(&mut cluster, cols, &mut out);
    out
}

fn color_vars(color: &str) -> String {
    // An event of the Mac's calendars has the colour of its calendar there.
    match crate::maccal::safe_color(color) {
        Some(hex) => format!("--c: {hex}; --c-soft: color-mix(in srgb, {hex} 18%, transparent);"),
        None => format!("--c: var(--icon-{color}); --c-soft: var(--icon-{color}-soft);"),
    }
}

#[component]
pub fn CalendarView() -> Element {
    let state = use_context::<AppState>();
    let cal = state.cal.read().clone();
    let space = state.ui.read().space.clone();
    // The days on screen, and every event and repeat on them.
    let shown_days = match cal.view {
        CalView::Month => when::month_grid(cal.anchor),
        CalView::Week => when::week_days(cal.anchor),
        CalView::Day => vec![cal.anchor],
    };
    let (from, to) = match (shown_days.first(), shown_days.last()) {
        (Some(a), Some(b)) => (when::midnight(*a), when::midnight(when::next_day(*b))),
        _ => (0, 0),
    };
    // One calendar for every space (each event marked with its space),
    // unless the person chose to see only the space in front.
    let every_space = cal.all_spaces;
    let many_spaces = state.spaces.read().len() > 1;
    // The open to-dos that have a day: they show on that day.
    let due: Vec<OpenTodo> = state.open_todos_of(every_space).into_iter().filter(|t| t.due.is_some()).collect();
    // (Each repeat with the notes page that is its own.)
    let mut events: Vec<Event> = repeat::expand(state.events.read().values().filter(|e| every_space || e.space_id == space), from, to).into_iter().map(|e| state.with_own_page(e)).collect();
    // With the events of the Mac's calendars, when the person has them on.
    events.extend(state.mac_events(&space, from, to));
    // Redraw once a minute so the "now" line keeps up.
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            crate::ai::sleep_ms(60_000).await;
            tick += 1;
        }
    });
    // (Given to the parts below: a part that gets the same as before is
    // not drawn again, and the line of now, the day of today and "In 40
    // minutes" would stay as they were.)
    let minute = *tick.read();
    // "New event" was pressed: the box to type the event in words is open.
    let mut adding = use_signal(|| false);
    let mut words = use_signal(String::new);

    // The day view names its day in large letters and its month beside it.
    let title_more = match cal.view {
        CalView::Day if cal.anchor.year() == when::today().year() => cal.anchor.format("%B").to_string(),
        CalView::Day => cal.anchor.format("%B %Y").to_string(),
        _ => String::new(),
    };
    let title = match cal.view {
        CalView::Day => cal.anchor.format("%A %-d").to_string(),
        CalView::Month => cal.anchor.format("%B %Y").to_string(),
        CalView::Week => {
            let days = when::week_days(cal.anchor);
            let (a, b) = (days[0], days[6]);
            if a.month() == b.month() {
                format!("{} {}–{}, {}", a.format("%B"), a.day(), b.day(), b.year())
            } else if a.year() == b.year() {
                format!("{} – {}, {}", a.format("%b %-d"), b.format("%b %-d"), b.year())
            } else {
                format!("{} – {}", a.format("%b %-d, %Y"), b.format("%b %-d, %Y"))
            }
        }
    };
    let selected = cal
        .selected
        .as_ref()
        .and_then(|id| repeat::find(&state.events.read(), id).map(|e| state.with_own_page(e)).or_else(|| state.mac_event(id)))
        .filter(|e| every_space || e.space_id == space);
    let view = cal.view;
    // The day view: the event whose notes show. The one that was clicked,
    // when it is on this day; else the one that is on now, the next one,
    // or the first of the day.
    // (While the notes of an event are written in, it stays the one shown:
    // when its time ends, or another event starts, the day view goes on to
    // that one only after two minutes with no change to these notes. Else
    // the notes were taken away in the middle of a sentence.)
    let kept = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(None::<(NaiveDate, String)>)));
    let shown = (view == CalView::Day)
        .then(|| {
            let day = day_events(&events, cal.anchor);
            let clicked = cal.selected.as_ref().and_then(|id| day.iter().find(|e| &e.id == id).cloned());
            let now = crate::model::now_ms();
            let written = |e: &Event| e.page_id.as_ref().and_then(|p| state.pages.peek().get(p).map(|p| p.updated_at)).is_some_and(|at| now - at < 120_000);
            let still = kept.borrow().clone().filter(|(d, _)| *d == cal.anchor).and_then(|(_, id)| day.iter().find(|e| e.id == id).cloned()).filter(written);
            clicked.or(still).or_else(|| event_in_front(&day, now))
        })
        .flatten();
    *kept.borrow_mut() = shown.as_ref().map(|e| (cal.anchor, e.id.clone()));
    let shown_id = shown.as_ref().map(|e| e.id.clone());
    // The panel (time, repeat, reminder): beside the grid for a clicked event;
    // in the day view only when it is asked for, the notes are what shows.
    let panel = selected.filter(|_| view != CalView::Day || cal.details);
    let panel_open = panel.is_some();

    let mac_write = state.mac_cal.read().can_write();
    rsx! {
        div { class: "calendar", "data-mac-write": if mac_write { "1" } else { "0" },
            div { class: "cal-main",
                div { class: "cal-toolbar",
                    h2 { class: "cal-title",
                        "{title}"
                        if !title_more.is_empty() {
                            span { class: "cal-title-more", "{title_more}" }
                        }
                    }
                    div { class: "cal-nav",
                        button { class: "secondary-btn small", onclick: move |_| state.cal_today(), "Today" }
                        button { class: "icon-btn", title: "Previous", onclick: move |_| state.cal_step(-1), Icon { name: "chevron-left", size: 16 } }
                        button { class: "icon-btn", title: "Next", onclick: move |_| state.cal_step(1), Icon { name: "chevron-right", size: 16 } }
                    }
                    div { class: "cal-spacer" }
                    if many_spaces {
                        SpacesShown {}
                    }
                    div { class: "segmented small",
                        button { class: if view == CalView::Day { "on" } else { "" }, onclick: move |_| state.cal_set_view(CalView::Day), "Day" }
                        button { class: if view == CalView::Week { "on" } else { "" }, onclick: move |_| state.cal_set_view(CalView::Week), "Week" }
                        button { class: if view == CalView::Month { "on" } else { "" }, onclick: move |_| state.cal_set_view(CalView::Month), "Month" }
                    }
                    if adding() {
                        input {
                            id: "cal-quick",
                            class: "text-input cal-quick",
                            r#type: "text",
                            placeholder: "Lunch with Sam Friday 1pm",
                            title: "Type the event and press Return. With no words, Return makes a plain new event.",
                            "aria-label": "New event, in words",
                            autocomplete: "off",
                            spellcheck: false,
                            onmounted: move |e| async move {
                                let _ = e.set_focus(true).await;
                            },
                            oninput: move |e: FormEvent| words.set(e.value()),
                            onkeydown: move |e: KeyboardEvent| {
                                if e.key() == Key::Enter {
                                    e.prevent_default();
                                    let typed = words.peek().clone();
                                    adding.set(false);
                                    state.quick_event(&typed);
                                } else if e.key() == Key::Escape {
                                    e.stop_propagation();
                                    adding.set(false);
                                }
                            },
                            onblur: move |_| adding.set(false),
                        }
                    } else {
                        button {
                            class: "primary-btn small with-icon",
                            title: "New event: type it in words, or press Return for a plain one",
                            "aria-label": "New event",
                            onclick: move |_| {
                                words.set(String::new());
                                adding.set(true);
                            },
                            Icon { name: "plus", size: 14 }
                            span { "New event" }
                        }
                    }
                }
                match view {
                    CalView::Month => rsx! { MonthGrid { anchor: cal.anchor, events: events, selected: cal.selected, todos: due, minute } },
                    CalView::Week => rsx! { WeekGrid { days: when::week_days(cal.anchor), events: events, selected: cal.selected, panel_open, todos: due, minute } },
                    CalView::Day => rsx! {
                        div { class: "cal-day-split",
                            div { class: "cal-day-col",
                                DayTodos { day: cal.anchor }
                                // The day has its to-dos in the card over the hours.
                                WeekGrid { days: vec![cal.anchor], events: events, selected: shown_id, panel_open, todos: Vec::new(), minute }
                            }
                            // Keyed, so the notes of another event start afresh.
                            for ev in shown.clone() {
                                DayNotes { key: "{ev.id}", event: ev, minute }
                            }
                            if shown.is_none() {
                                div { class: "day-notes none",
                                    div { class: "day-notes-none", "Nothing is on this day." }
                                    div { class: "day-notes-hint", "Click a time at the left to add an event. Its notes are written here." }
                                }
                            }
                        }
                    },
                }
            }
            // In a keyed list, so another event gets a fresh panel (its boxes
            // keep what was typed in them).
            for ev in panel {
                if ev.id.starts_with(crate::maccal::PREFIX) {
                    MacEventPanel { key: "{ev.id}", event: ev }
                } else {
                    EventEditor { key: "{ev.id}", event: ev }
                }
            }
        }
    }
}

/// The event of a day that its notes are most likely wanted for: the one
/// that is on now, else the next one to start, else the first of the day.
fn event_in_front(day: &[Event], now: i64) -> Option<Event> {
    let timed = || day.iter().filter(|e| !e.all_day);
    timed()
        .find(|e| e.start <= now && now < e.end)
        .or_else(|| timed().filter(|e| e.start > now).min_by_key(|e| e.start))
        .or_else(|| timed().min_by_key(|e| e.start))
        .or_else(|| day.first())
        .cloned()
}

/// The day view: the open to-dos that are due on this day, above its hours.
/// Today also lists those that were due before and are not done.
#[component]
fn DayTodos(day: NaiveDate) -> Element {
    let state = use_context::<AppState>();
    let todos = state.todos_due(day);
    if todos.is_empty() {
        return rsx! {};
    }
    let label = if day == when::today() { "To do today".to_string() } else { format!("To do on {}", day.format("%A")) };
    rsx! {
        div { class: "day-todos", id: "day-todos",
            div { class: "day-todos-label", "{label}" }
            for mut t in todos {
                // The day is said once, by the label; only a late one says its own.
                {
                    if t.due == Some(day) {
                        t.due = None;
                    }
                    rsx! { super::page::TodoRow { key: "{t.block}", todo: t } }
                }
            }
        }
    }
}

/// The day view: the notes of one event, written in place. The event's page
/// is its notes; an event without one gets it at the first click.
#[component]
fn DayNotes(event: Event, minute: u64) -> Element {
    let _ = minute;
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let now = crate::model::now_ms();
    let day = when::date_of(event.start);
    let time = if event.all_day { "All day".to_string() } else { format!("{} – {}", when::time_label(event.start), when::time_label(event.end)) };
    let from_now = if event.all_day { when::day_label(day) } else { when::from_now(event.start, event.end, now) };
    let page = event.page_id.clone().filter(|p| state.pages.read().get(p).is_some_and(|p| !p.trashed));
    // The same page cannot be written in two places of one window at once.
    let elsewhere = page.as_ref().is_some_and(|p| state.ui.read().tab().ordered().iter().any(|pane| *pane.route() == Route::Page(p.clone())));
    // While its page shows here, the app knows it as a page on screen: what
    // is done to "the page in front" is done to it, and text the app
    // changes in it is drawn again.
    let showing = page.clone().filter(|_| !elsewhere);
    let mut mine = use_signal(|| None::<String>);
    if *mine.peek() != showing {
        mine.set(showing.clone());
    }
    state.set_day_page(showing);
    use_drop(move || {
        if let Some(page) = mine.peek().as_ref() {
            state.clear_day_page(page);
        }
    });
    let listening = state.listening.read().is_some();
    // A meeting can be listened to from a little before it starts until it ends.
    let can_listen = !event.all_day && !listening && now >= event.start - 15 * MINUTE && now < event.end;
    let joins = event.join_url().is_some();
    let (i1, i2, i3) = (id.clone(), id.clone(), id.clone());
    let i4 = id.clone();
    rsx! {
        article { class: "day-notes", "data-event": "{id}", "aria-label": "Notes of {event.display_title()}",
            div { class: "day-notes-head",
                span { class: "ed-swatch", style: "{color_vars(&event.color)}" }
                span { class: "day-notes-time", "{time}" }
                span { class: "day-notes-from", "{from_now}" }
                span { class: "cal-spacer" }
                if joins {
                    button { class: "secondary-btn small", id: "day-join", title: "Open the meeting", onclick: move |_| state.join_event(&i4), "Join" }
                }
                if can_listen {
                    button { class: "secondary-btn small", id: "day-listen", onclick: move |_| state.listen_to_event(&i1), "Listen and write the notes" }
                }
                button {
                    class: "icon-btn small",
                    id: "day-details",
                    title: "Time, repeat and reminder",
                    "aria-label": "Time, repeat and reminder",
                    onclick: move |_| state.cal_details(&i2),
                    Icon { name: "clock", size: 15 }
                }
            }
            match (page, elsewhere) {
                (Some(p), false) => rsx! {
                    super::ListenBar { page: p.clone() }
                    super::PlaceBar { page: p.clone() }
                    div { class: "day-notes-page scroller",
                        for p in std::iter::once(p) {
                            super::page::PageView { key: "{p}", page_id: p }
                        }
                    }
                },
                (Some(p), true) => rsx! {
                    div { class: "day-notes-bare",
                        h2 { class: "day-notes-title", "{event.display_title()}" }
                        p { class: "day-notes-hint", "Its notes are open in another part of this tab." }
                        button { class: "secondary-btn small", onclick: move |_| state.open_page(&p), "Go to the notes" }
                    }
                },
                (None, _) => rsx! {
                    div { class: "day-notes-bare",
                        h2 { class: "day-notes-title", "{event.display_title()}" }
                        if !event.notes.trim().is_empty() {
                            p { class: "day-notes-short", "{event.notes}" }
                        }
                        button { class: "day-notes-start", id: "day-notes-start", onclick: move |_| state.event_notes_here(&i3), "Write the notes here" }
                    }
                },
            }
        }
    }
}

#[component]
fn EventChip(event: Event, selected: bool) -> Element {
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let mark = space_mark(&state, &event);
    let time = if event.all_day { String::new() } else { when::time_label(event.start) };
    let class = format!(
        "cal-chip{}{}",
        if event.all_day { " all-day" } else { "" },
        if selected { " on" } else { "" }
    );
    rsx! {
        div {
            class: "{class}",
            "data-event": "{event.id}",
            style: "{color_vars(&event.color)}",
            title: "{event.display_title()}",
            onclick: move |e| {
                e.stop_propagation();
                state.select_event(Some(id.clone()));
            },
            ondoubleclick: move |e| e.stop_propagation(),
            if !event.all_day {
                span { class: "chip-dot" }
                span { class: "chip-time", "{time}" }
            }
            span { class: "chip-title", "{event.display_title()}" }
            if event.repeat.is_some() {
                span { class: "repeat-mark", title: "Repeats", Icon { name: "repeat", size: 11 } }
            }
            if let Some(sp) = mark {
                SpaceMark { space: sp }
            }
        }
    }
}

#[component]
fn MonthGrid(anchor: NaiveDate, events: Vec<Event>, selected: Option<String>, todos: Vec<OpenTodo>, minute: u64) -> Element {
    let _ = minute;
    let state = use_context::<AppState>();
    let month = anchor.month();
    let today = when::today();
    let cells: Vec<(NaiveDate, Vec<Event>, usize, usize)> = when::month_grid(anchor)
        .into_iter()
        .map(|d| {
            let evs = day_events(&events, d);
            let more = evs.len().saturating_sub(3);
            (d, evs.into_iter().take(3).collect(), more, todos_on(&todos, d, today).len())
        })
        .collect();

    rsx! {
        div { class: "cal-month",
            div { class: "cal-weekdays",
                for d in WEEKDAYS {
                    div { "{d}" }
                }
            }
            div { class: "cal-grid",
                for (day, shown, more, to_do) in cells {
                    div {
                        key: "{day}",
                        class: format!("cal-day{}{}", if day.month() != month { " other" } else { "" }, if day == today { " today" } else { "" }),
                        "data-day": "{day}",
                        title: "Double-click to add an event",
                        ondoubleclick: move |_| state.new_event(when::midnight(day), when::midnight(when::next_day(day)), true),
                        div { class: "cal-date",
                            button {
                                class: "num",
                                title: "Open this day",
                                "aria-label": "Open {day.format(\"%A, %B %-d\")}",
                                "data-day": "{day}",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    state.cal_open_day(day);
                                },
                                ondoubleclick: move |e| e.stop_propagation(),
                                "{day.day()}"
                            }
                        }
                        for e in shown {
                            EventChip { key: "{e.id}", selected: selected.as_deref() == Some(e.id.as_str()), event: e }
                        }
                        if more > 0 {
                            button {
                                class: "cal-more",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    state.cal_open_day(day);
                                },
                                "+{more} more"
                            }
                        }
                        // What is due that day: how many, and a click opens the day.
                        if to_do > 0 {
                            button {
                                class: "cal-more cal-todo-count",
                                "data-todos": "{to_do}",
                                title: "Open this day",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    state.cal_open_day(day);
                                },
                                ondoubleclick: move |e| e.stop_propagation(),
                                "{to_do} to do"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The to-dos to show on a day: those due on it. Today also has those
/// that were due before and are not done.
fn todos_on(todos: &[OpenTodo], day: NaiveDate, today: NaiveDate) -> Vec<OpenTodo> {
    todos.iter().filter(|t| t.due.is_some_and(|d| d == day || (day == today && d < today))).cloned().collect()
}

/// A to-do that is due, in the week with the all-day events: tick it there,
/// click its words to go to its page, or drag it onto a time to plan it.
#[component]
fn TodoChip(todo: OpenTodo, late: bool) -> Element {
    let state = use_context::<AppState>();
    let (page, block, go) = (todo.page.clone(), todo.block.clone(), todo.page.clone());
    rsx! {
        div {
            class: if late { "cal-chip cal-todo late" } else { "cal-chip cal-todo" },
            "data-todo-page": "{todo.page}",
            "data-todo-block": "{todo.block}",
            title: "{todo.text} · {todo.page_title}",
            ondoubleclick: move |e| e.stop_propagation(),
            button {
                class: "todo-box",
                role: "checkbox",
                "aria-checked": "false",
                "aria-label": "Done: {todo.text}",
                onclick: move |e| {
                    e.stop_propagation();
                    state.toggle_check(&page, &block);
                },
            }
            span { class: "chip-title", onclick: move |_| state.open_page(&go), "{todo.text}" }
        }
    }
}

/// The hours of some days side by side: the seven of a week, or one day
/// (the day view). `panel_open` says that an event's panel is open, so a
/// click on the grid closes it and adds nothing.
#[component]
fn WeekGrid(days: Vec<NaiveDate>, events: Vec<Event>, selected: Option<String>, panel_open: bool, todos: Vec<OpenTodo>, minute: u64) -> Element {
    let _ = minute;
    let state = use_context::<AppState>();
    let today = when::today();
    let now = crate::model::now_ms();
    let one_day = days.len() == 1;
    let columns: Vec<(NaiveDate, Vec<Event>, Vec<Placed>)> = days
        .into_iter()
        .map(|d| {
            let evs = day_events(&events, d);
            let all: Vec<Event> = evs.iter().filter(|e| e.all_day).cloned().collect();
            (d, all, place(evs, d))
        })
        .collect();
    let now_top = when::minutes_into_day(now) as f64 / 60.0 * HOUR_PX;
    let total = 24.0 * HOUR_PX;
    // The start of a new event still without a name: a slot on the grid to
    // type the name in.
    let mut draft = state.cal_draft;
    // Whether a slot or an event's panel was open when the mouse went down on
    // the grid: that click only closes it, as in other calendars.
    let mut closing = use_signal(|| false);

    rsx! {
        div { class: if one_day { "cal-week one-day" } else { "cal-week" },
            div { class: "cal-week-head",
                div { class: "cal-gutter" }
                for (d, _, _) in columns.clone() {
                    button {
                        key: "{d}",
                        class: if d == today { "cal-wday today" } else { "cal-wday" },
                        title: "Open this day",
                        "aria-label": "Open {d.format(\"%A, %B %-d\")}",
                        "data-day": "{d}",
                        onclick: move |_| state.cal_open_day(d),
                        span { class: "wd", "{WEEKDAYS[d.weekday().num_days_from_sunday() as usize]}" }
                        span { class: "num", "{d.day()}" }
                    }
                }
            }
            div { class: "cal-allday",
                div { class: "cal-gutter label", "all-day" }
                for (d, all, _) in columns.clone() {
                    div {
                        key: "{d}",
                        class: "cal-allday-cell",
                        "data-day": "{d}",
                        ondoubleclick: move |_| state.new_event(when::midnight(d), when::midnight(when::next_day(d)), true),
                        for e in all {
                            EventChip { key: "{e.id}", selected: selected.as_deref() == Some(e.id.as_str()), event: e }
                        }
                        for t in todos_on(&todos, d, today) {
                            TodoChip { key: "{t.block}", late: t.due.is_some_and(|due| due < today), todo: t }
                        }
                    }
                }
            }
            div {
                class: "cal-scroll",
                onmounted: move |_| async move {
                    // Start the day at 8 AM.
                    document::eval(&format!("const s=document.querySelector('.cal-scroll');if(s)s.scrollTop={}", 8.0 * HOUR_PX - 8.0));
                },
                div { class: "cal-hours", style: "height: {total}px",
                    // (Before the days, so that the days lie over them.)
                    for h in 0..24u32 {
                        div { key: "line-{h}", class: "cal-line", style: "top: {h as f64 * HOUR_PX}px" }
                    }
                    div { class: "cal-gutter times",
                        for h in 1..24u32 {
                            div { class: "cal-hour-label", style: "top: {h as f64 * HOUR_PX}px", "{when::hour_label(h)}" }
                        }
                    }
                    for (d, _, placed) in columns {
                        div {
                            key: "{d}",
                            class: if d == today { "cal-col today" } else { "cal-col" },
                            "data-day": "{d}",
                            title: "Click to add an event, or drag for a longer one",
                            onmousedown: move |_| closing.set(panel_open || draft.peek().is_some()),
                            onclick: move |e| {
                                // The time that was pressed, as the page script
                                // measured it in the column. (Without it: from
                                // the place inside what was pressed.) It is
                                // taken by a click that only closes a panel
                                // too: left there, the next click had it.
                                let pressed = AppState::cal_press_minute();
                                if *closing.peek() {
                                    if panel_open {
                                        state.close_event_panel();
                                    }
                                    return;
                                }
                                let slots = 24 * 60 / SLOT_MINUTES;
                                let y = e.element_coordinates().y;
                                let mins = pressed
                                    .unwrap_or_else(|| ((y / HOUR_PX * 60.0) / f64::from(SLOT_MINUTES)).floor().clamp(0.0, f64::from(slots - 1)) as u32 * SLOT_MINUTES);
                                let start = when::at(d, NaiveTime::from_hms_opt(mins / 60, mins % 60, 0).unwrap_or(NaiveTime::MIN));
                                draft.set(Some((start, start + i64::from(SLOT_MINUTES) * MINUTE)));
                            },
                            for p in placed {
                                TimedEvent { key: "{p.event.id}", selected: selected.as_deref() == Some(p.event.id.as_str()), top: p.top, height: p.height, left: p.left, width: p.width, event: p.event }
                            }
                            if let Some((start, end)) = draft().filter(|(start, _)| when::date_of(*start) == d) {
                                DraftEvent {
                                    key: "{start}-{end}",
                                    start,
                                    end,
                                    on_done: move |title: Option<String>| {
                                        draft.set(None);
                                        if let Some(title) = title {
                                            state.make_event(&title, start, end, false);
                                        }
                                    },
                                }
                            }
                            if d == today {
                                div { class: "cal-now", style: "top: {now_top}px" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TimedEvent(event: Event, selected: bool, top: f64, height: f64, left: f64, width: f64) -> Element {
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let range = format!("{} – {}", when::time_label(event.start), when::time_label(event.end));
    let mark = space_mark(&state, &event);
    let class = format!("cal-event{}{}", if selected { " on" } else { "" }, if mark.is_some() { " has-space" } else { "" });
    rsx! {
        div {
            class: "{class}",
            // For the drag (see the page script): which event this is.
            "data-event": "{event.id}",
            style: "top: {top}px; height: {height}px; left: calc({left}% + 2px); width: calc({width}% - 4px); {color_vars(&event.color)}",
            onclick: move |e| {
                e.stop_propagation();
                state.select_event(Some(id.clone()));
            },
            ondoubleclick: move |e| e.stop_propagation(),
            div { class: "ev-title",
                if event.repeat.is_some() {
                    span { class: "repeat-mark", title: "Repeats", Icon { name: "repeat", size: 11 } }
                }
                "{event.display_title()}"
            }
            if height >= 34.0 {
                div { class: "ev-time", "{range}" }
            }
            if let Some(sp) = mark {
                SpaceMark { space: sp }
            }
        }
    }
}

/// The space an event belongs to, for the small mark beside it: only when
/// the calendar shows every space and there is more than one. The Mac's own
/// events have none (their panel names their calendar).
pub(crate) fn space_mark(state: &AppState, e: &Event) -> Option<Space> {
    if e.id.starts_with(crate::maccal::PREFIX) || !state.cal.read().all_spaces {
        return None;
    }
    let spaces = state.spaces.read();
    if spaces.len() < 2 {
        return None;
    }
    spaces.iter().find(|s| s.id == e.space_id).cloned()
}

/// The letter of a space in its colour, quiet: which space an event is in.
#[component]
pub(crate) fn SpaceMark(space: Space) -> Element {
    let name = space.display_name().to_string();
    rsx! {
        span {
            class: "ev-space",
            style: "--s: var(--icon-{space.color}, var(--accent)); --s-soft: var(--icon-{space.color}-soft, var(--accent-soft));",
            title: "{name}",
            "aria-label": "In {name}",
            "{space.initial()}"
        }
    }
}

/// Which spaces the calendar shows: all of them (each event marked with its
/// space) or only the one in front. One quiet button in the toolbar, shown
/// when there is more than one space; Rune's own menu; kept as a setting.
#[component]
fn SpacesShown() -> Element {
    let state = use_context::<AppState>();
    let all = state.cal.read().all_spaces;
    let spaces = state.spaces.read().clone();
    let here = state.ui.read().space.clone();
    let here_space = spaces.iter().find(|s| s.id == here).cloned();
    let here_name = here_space.as_ref().map(|s| s.display_name().to_string()).unwrap_or_else(|| "this space".into());
    // The badges on the button: every space's (three at most), or this one's.
    let badges: Vec<Space> = if all { spaces.iter().take(3).cloned().collect() } else { here_space.into_iter().collect() };
    let said = if all { "All spaces".to_string() } else { format!("Only {here_name}") };
    let mut open = use_signal(|| false);
    let rows = vec![
        ("all".to_string(), "All spaces".to_string(), String::new(), all, false),
        ("here".to_string(), format!("Only {here_name}"), String::new(), !all, false),
    ];
    rsx! {
        div { class: "ed-pick cal-spaces",
            button {
                class: if open() { "cal-spaces-btn open" } else { "cal-spaces-btn" },
                id: "cal-spaces",
                "data-value": if all { "all" } else { "here" },
                title: "Which spaces the calendar shows: {said}",
                "aria-label": "Calendar shows: {said}",
                "aria-haspopup": "menu",
                "aria-expanded": "{open()}",
                onclick: move |_| open.toggle(),
                span { class: "cal-spaces-badges",
                    for sp in badges {
                        span {
                            key: "{sp.id}",
                            class: "space-badge",
                            style: "--c: var(--icon-{sp.color}, var(--accent));",
                            "{sp.initial()}"
                        }
                    }
                }
                span { class: "ed-chev", Icon { name: "chevron-down", size: 12 } }
            }
            if open() {
                div {
                    class: "ed-shade",
                    onclick: move |_| {
                        open.set(false);
                    },
                }
                PickMenu {
                    label: "Spaces shown",
                    heading: "The calendar shows",
                    right: true,
                    rows,
                    on_pick: move |v: String| {
                        open.set(false);
                        state.cal_set_all_spaces(v == "all");
                        focus_button("cal-spaces");
                    },
                    on_close: move |_| {
                        open.set(false);
                        focus_button("cal-spaces");
                    },
                }
            }
        }
    }
}

/// A new event still without a name: a slot on the grid to type the name in.
/// Return, or a click elsewhere, keeps it; Escape, or no name, drops it.
#[component]
fn DraftEvent(start: i64, end: i64, on_done: EventHandler<Option<String>>) -> Element {
    let mut text = use_signal(String::new);
    let mut done = use_signal(|| false);
    let finish = use_callback(move |keep: bool| {
        // Return takes the box away, which can also count as leaving it.
        if *done.peek() {
            return;
        }
        done.set(true);
        let title = text.peek().trim().to_string();
        on_done.call((keep && !title.is_empty()).then_some(title));
    });
    let top = when::minutes_into_day(start) as f64 / 60.0 * HOUR_PX;
    // As long as the time that was dragged over (a click: a quarter hour,
    // shown a little higher so that the name can be typed in it).
    let height = (((end - start) as f64 / HOUR as f64) * HOUR_PX).max(22.0);
    let long = end - start > i64::from(SLOT_MINUTES) * MINUTE;
    let time = if long { format!("{} – {}", when::time_label(start), when::time_label(end)) } else { when::time_label(start) };
    rsx! {
        div {
            class: if height > 30.0 { "cal-event draft tall" } else { "cal-event draft" },
            style: "top: {top}px; height: {height}px; {color_vars(\"blue\")}",
            onmousedown: move |e| e.stop_propagation(),
            onclick: move |e| e.stop_propagation(),
            ondoubleclick: move |e| e.stop_propagation(),
            input {
                class: "ev-draft-input",
                id: "ev-draft",
                placeholder: "New event · {time}",
                "aria-label": "Name of the new event at {time}",
                autocomplete: "off",
                onmounted: move |e| async move {
                    let _ = e.set_focus(true).await;
                },
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e| match e.key() {
                    Key::Enter => finish.call(true),
                    Key::Escape => finish.call(false),
                    _ => {}
                },
                onblur: move |_| finish.call(true),
            }
        }
    }
}

fn time_of(ms: i64) -> NaiveTime {
    let t = when::local(ms);
    NaiveTime::from_hms_opt(t.hour(), t.minute(), 0).unwrap_or(NaiveTime::MIN)
}

/// What is open in an event's panel: one of its menus or day pickers.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Open {
    Space,
    Day,
    LastDay,
    Start,
    End,
    Repeat,
    Until,
    UntilDay,
    Remind,
    Delete,
}

impl Open {
    /// The button that opens it: the keyboard goes back there when it closes.
    fn button(self) -> &'static str {
        match self {
            Open::Day => "ev-day",
            Open::LastDay => "ev-last-day",
            Open::Start => "ev-start",
            Open::End => "ev-end",
            Open::Repeat => "ev-repeat",
            Open::Until | Open::UntilDay => "ev-until",
            Open::Remind => "ev-remind",
            Open::Space => "ev-space",
            Open::Delete => "ev-delete",
        }
    }
}

/// Put the keyboard back on a button of the panel once a menu has closed
/// (the panel may be drawn afresh first, for another repeat). It waits until
/// the menu is gone, and gives way when another menu or picker opened
/// meanwhile (they clear `__edFocus`): a late return must never take the
/// keyboard away from a menu that is open.
fn focus_button(id: &str) {
    document::eval(&format!(
        "window.__edFocus='{id}';let n=0;const f=()=>{{if(window.__edFocus!=='{id}')return;\
         if(document.querySelector('.ed-menu')){{if(n++<60)setTimeout(f,20);return;}}\
         window.__edFocus=null;const b=document.getElementById('{id}');if(b)b.focus({{preventScroll:true}});}};setTimeout(f,20);"
    ));
}

/// "Tue, Nov 10", with the year when it is not this year.
fn day_chip(d: NaiveDate) -> String {
    if d.year() == when::today().year() {
        d.format("%a, %b %-d").to_string()
    } else {
        d.format("%a, %b %-d, %Y").to_string()
    }
}

/// "4 PM", "4:30 PM": a time of day as the calendar writes it.
fn clock_label(t: NaiveTime) -> String {
    let (pm, h) = t.hour12();
    let half = if pm { "PM" } else { "AM" };
    if t.minute() == 0 {
        format!("{h} {half}")
    } else {
        format!("{h}:{:02} {half}", t.minute())
    }
}

/// The two ends of an event, short: "4" and "4:30 PM" say PM once.
fn short_times(start: i64, end: i64) -> (String, String) {
    let (a, b) = (when::time_label(start), when::time_label(end));
    let pm = |ms: i64| when::local(ms).hour12().0;
    if pm(start) == pm(end) && end - start < 12 * HOUR {
        (a.trim_end_matches(" AM").trim_end_matches(" PM").to_string(), b)
    } else {
        (a, b)
    }
}

/// How long an event lasts, beside an end time: "45 min", "1 hour 30 min".
fn length_label(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (1, 0) => "1 hour".into(),
        (h, 0) => format!("{h} hours"),
        (1, m) => format!("1 hour {m} min"),
        (h, m) => format!("{h} hours {m} min"),
    }
}

/// The times to choose from, a quarter of an hour apart: for the start every
/// one of the day; for the end those after the start, with how long the
/// event then is.
fn time_choices(start: Option<NaiveTime>) -> Vec<(NaiveTime, String, String)> {
    match start {
        None => (0..96)
            .map(|q| {
                let t = NaiveTime::MIN + chrono::Duration::minutes(15 * q);
                (t, clock_label(t), String::new())
            })
            .collect(),
        Some(s) => (1..96)
            .map(|q| {
                let t = s + chrono::Duration::minutes(15 * q);
                (t, clock_label(t), length_label(15 * q))
            })
            .collect(),
    }
}

/// The panel of an event that comes from the Mac's calendars. It is only
/// shown here: it is changed in the Calendar app. A notes page can be made
/// for it, as for any event.
#[component]
fn MacEventPanel(event: Event) -> Element {
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let day = when::date_of(event.start);
    let when_text = if event.all_day {
        format!("{} · all day", day_chip(day))
    } else {
        let (a, b) = short_times(event.start, event.end);
        format!("{} · {a} – {b}", day_chip(day))
    };
    let calendar = {
        let m = state.mac_cal.read();
        m.events.iter().find(|e| e.id == id).map(|e| m.name_of(&e.calendar)).unwrap_or_default()
    };
    let has_page = event.page_id.is_some();
    // The address to join by, shown without its "https://"; the button joins.
    let link = event.join_url().map(|l| l.trim_start_matches("https://").trim_start_matches("http://").to_string());
    // Its place, without the address to join by.
    let place = event.notes.lines().filter(|l| crate::meeting::join_link(l).is_none()).collect::<Vec<_>>().join("\n");
    let join_id = id.clone();
    let title = event.display_title();
    rsx! {
        aside { class: "cal-editor of-the-mac", "data-event": "{id}", "aria-label": "{title}, from the calendars of this Mac",
            button { class: "icon-btn ed-close", title: "Close", "aria-label": "Close", onclick: move |_| state.close_event_panel(), Icon { name: "close", size: 16 } }
            div { class: "ed-body",
                h3 { class: "ed-title-plain", "{title}" }
                div { class: "ed-row ed-still",
                    span { class: "ed-ico", Icon { name: "clock", size: 16 } }
                    span { class: "ed-val", "{when_text}" }
                }
                div { class: "ed-row ed-still",
                    span { class: "ed-ico", Icon { name: "calendar", size: 16 } }
                    span { class: "ed-swatch", style: "{color_vars(&event.color)}" }
                    span { class: "ed-val", "{calendar}" }
                }
                if let Some(l) = link {
                    div { class: "ed-sep" }
                    div { class: "ed-row ed-still",
                        span { class: "ed-ico", Icon { name: "link", size: 16 } }
                        span { class: "ed-val ed-dim ed-grow", "{l}" }
                        button { class: "ed-small-btn", id: "ev-join", title: "Open the meeting and its notes", onclick: move |_| state.join_event(&join_id), "Join" }
                    }
                }
                if !place.trim().is_empty() {
                    div { class: "ed-sep" }
                    div { class: "ed-notes-row ed-still",
                        span { class: "ed-ico", Icon { name: "note", size: 16 } }
                        div { class: "ed-plain-text", "{place}" }
                    }
                }
            }
            div { class: "ed-lock",
                Icon { name: "lock", size: 14 }
                span {
                    if state.mac_cal.read().can_write() {
                        "From the calendars of this Mac. Drag it to give it another time."
                    } else {
                        "From the calendars of this Mac. Change it in the Calendar app."
                    }
                }
            }
            div { class: "ed-foot",
                button { class: "ghost-btn", id: "mac-event-notes", onclick: move |_| state.event_notes(&id),
                    Icon { name: "file", size: 15 }
                    span { if has_page { "Open notes page" } else { "Make a notes page" } }
                }
            }
        }
    }
}

/// The panel for editing the selected event. Keyed by event, so its fields
/// start fresh when another event is chosen. One column of quiet rows: an
/// icon, then the value as words to click. Rune's own menus and day picker
/// open from them, never the system's lists or pop-ups.
#[component]
fn EventEditor(event: Event) -> Element {
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let all_day = event.all_day;
    let start_date = when::date_of(event.start);
    let last_day = if all_day { when::date_of(event.end - 1) } else { when::date_of(event.end) };
    let duration = event.end - event.start;
    let has_page = event.page_id.as_ref().is_some_and(|p| state.pages.read().get(p).is_some_and(|p| !p.trashed));
    let remind = event.remind;
    // A repeat of a repeating event (rather than the event itself).
    let one_repeat = repeat::split_id(&id).1.is_some() && event.repeat.is_some();
    let first_day = state
        .events
        .read()
        .get(repeat::split_id(&id).0)
        .map(|e| when::date_of(e.start))
        .unwrap_or(start_date);
    let rule_choice = repeat::choice_of(event.repeat.as_ref());
    let custom_label = event.repeat.as_ref().map(|r| repeat::describe(r, first_day)).unwrap_or_default();
    let choices = repeat::choices(first_day);
    let repeat_label = if rule_choice == "custom" {
        custom_label.clone()
    } else {
        choices.iter().find(|(k, _)| *k == rule_choice).map(|(_, l)| l.clone()).unwrap_or_default()
    };
    let mut repeat_rows: Vec<(String, String, String, bool, bool)> =
        choices.iter().map(|(k, l)| (k.to_string(), l.clone(), String::new(), *k == rule_choice, false)).collect();
    if rule_choice == "custom" {
        repeat_rows.push(("custom".into(), custom_label, String::new(), true, false));
    }
    let until = event.repeat.as_ref().and_then(|r| r.until_date());
    let until_label = until.map(|u| format!("Until {}", day_chip(u))).unwrap_or_else(|| "No end date".into());
    let until_rows = vec![
        ("never".to_string(), "No end date".to_string(), String::new(), until.is_none(), false),
        ("date".to_string(), "Until a day…".to_string(), String::new(), until.is_some(), false),
    ];
    let remind_value = remind.map(|m| m.to_string()).unwrap_or_else(|| "none".into());
    let remind_label = match remind {
        None => "No reminder".to_string(),
        Some(m) => crate::notify::REMINDERS
            .iter()
            .find(|(r, _)| *r == Some(m))
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| format!("{m} min before")),
    };
    let remind_rows: Vec<(String, String, String, bool, bool)> = crate::notify::REMINDERS
        .iter()
        .map(|(m, l)| (m.map(|x| x.to_string()).unwrap_or_else(|| "none".into()), l.to_string(), String::new(), *m == remind, false))
        .collect();
    // The space it is in, when there is more than one: a row to move it.
    let spaces = state.spaces.read().clone();
    let own_space = spaces.iter().find(|s| s.id == event.space_id).cloned();
    let space_name = own_space.as_ref().map(|s| s.display_name().to_string()).unwrap_or_else(|| "No space".into());
    let space_rows: Vec<(String, String, String, bool, bool)> =
        spaces.iter().map(|s| (s.id.clone(), s.display_name().to_string(), String::new(), s.id == event.space_id, false)).collect();
    let delete_rows = vec![
        ("one".to_string(), "Delete only this one".to_string(), day_chip(start_date), false, false),
        ("all".to_string(), "Delete all repeats".to_string(), String::new(), false, true),
    ];
    let (from_label, to_label) = short_times(event.start, event.end);
    // An event that goes on past midnight says so after its end.
    let past_midnight = !all_day && when::date_of(event.end - 1) > start_date;
    let long_day = |d: NaiveDate| d.format("%A, %B %-d, %Y").to_string();
    let day_said = if all_day { format!("First day: {}", long_day(start_date)) } else { format!("Day: {}", long_day(start_date)) };
    let last_said = format!("Last day: {}", long_day(last_day));
    let start_said = format!("Starts at {}", when::time_label(event.start));
    let end_said = format!("Ends at {}", when::time_label(event.end));
    let title = event.display_title();

    // What is open: one menu or day picker at a time.
    let mut open = use_signal(|| None::<Open>);
    let is = move |w: Open| *open.read() == Some(w);
    let mut toggle = move |w: Open| {
        let now = *open.peek();
        open.set(if now == Some(w) { None } else { Some(w) });
    };
    let mut close = move || {
        if let Some(w) = *open.peek() {
            focus_button(w.button());
        }
        open.set(None);
    };
    let chip = move |w: Open| if is(w) { "ed-chip open" } else { "ed-chip" };
    let row = move |w: Open| if is(w) { "ed-row open" } else { "ed-row" };

    // The link to join the call. What is typed in its field waits for
    // "Save" (or Return): the button beside the field says so, and becomes
    // "Join" when the link is kept. A link still waiting when this panel
    // closes is kept then, so that it cannot be lost.
    let shown_link = event.join_url().unwrap_or_default();
    let mut typed_link = use_signal(|| None::<String>);
    let link_waits = typed_link.read().as_ref().is_some_and(|typed| *typed != shown_link);
    let (link_id, link_was) = (id.clone(), shown_link.clone());
    use_drop(move || {
        if let Some(link) = typed_link.peek().clone().filter(|typed| *typed != link_was) {
            state.update_event(&link_id, |ev| ev.join = link);
        }
    });

    rsx! {
        aside { class: "cal-editor", "aria-label": "Event: {title}",
            // A press anywhere else in the panel puts a menu away.
            if open.read().is_some() {
                div { class: "ed-shade", onclick: move |_| close() }
            }
            button { class: "icon-btn ed-close", title: "Close", "aria-label": "Close", onclick: move |_| state.close_event_panel(), Icon { name: "close", size: 16 } }
            div { class: "ed-body",
                textarea {
                    id: "ev-title-{id}",
                    class: "ed-title",
                    rows: "1",
                    placeholder: "New event",
                    "aria-label": "Event name",
                    initial_value: "{event.title}",
                    oninput: {
                        let id = id.clone();
                        move |e: FormEvent| {
                            let v = e.value().replace('\n', " ");
                            state.update_event(&id, |ev| ev.title = v);
                        }
                    },
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Enter {
                            e.prevent_default();
                        }
                    },
                }
                // When: the day and the times, as words to click.
                div { class: "ed-row ed-when",
                    span { class: "ed-ico", Icon { name: "clock", size: 16 } }
                    div { class: "ed-pick",
                        button {
                            class: chip(Open::Day),
                            id: "ev-day",
                            "data-value": "{when::date_value(start_date)}",
                            "aria-label": "{day_said}",
                            "aria-haspopup": "dialog",
                            "aria-expanded": "{is(Open::Day)}",
                            onclick: move |_| toggle(Open::Day),
                            "{day_chip(start_date)}"
                        }
                        if is(Open::Day) {
                            DayPicker {
                                label: if all_day { "First day" } else { "Day" },
                                day: start_date,
                                on_pick: {
                                    let id = id.clone();
                                    move |d: NaiveDate| {
                                        if all_day {
                                            state.update_event(&id, |ev| {
                                                let days = ((ev.end - ev.start) as f64 / (24 * HOUR) as f64).round().max(1.0) as i64;
                                                ev.start = when::midnight(d);
                                                ev.end = when::midnight(when::add_days(d, days));
                                            });
                                        } else {
                                            state.update_event(&id, |ev| {
                                                ev.start = when::at(d, time_of(ev.start));
                                                ev.end = ev.start + duration;
                                            });
                                        }
                                        close();
                                    }
                                },
                                on_close: move |_| close(),
                            }
                        }
                    }
                    if all_day {
                        span { class: if last_day == start_date { "ed-mid" } else { "ed-dash" }, "aria-hidden": "true",
                            if last_day == start_date { "·" } else { "–" }
                        }
                        div { class: "ed-pick",
                            button {
                                class: chip(Open::LastDay),
                                id: "ev-last-day",
                                "data-value": "{when::date_value(last_day)}",
                                "aria-label": "{last_said}",
                                "aria-haspopup": "dialog",
                                "aria-expanded": "{is(Open::LastDay)}",
                                onclick: move |_| toggle(Open::LastDay),
                                if last_day == start_date { "1 day" } else { "{day_chip(last_day)}" }
                            }
                            if is(Open::LastDay) {
                                DayPicker {
                                    label: "Last day",
                                    day: last_day,
                                    on_pick: {
                                        let id = id.clone();
                                        move |d: NaiveDate| {
                                            let d = d.max(start_date);
                                            state.update_event(&id, |ev| ev.end = when::midnight(when::next_day(d)));
                                            close();
                                        }
                                    },
                                    on_close: move |_| close(),
                                }
                            }
                        }
                    } else {
                        span { class: "ed-mid", "aria-hidden": "true", "·" }
                        div { class: "ed-pick",
                            button {
                                class: chip(Open::Start),
                                id: "ev-start",
                                "data-value": "{when::time_value(event.start)}",
                                "aria-label": "{start_said}",
                                "aria-haspopup": "menu",
                                "aria-expanded": "{is(Open::Start)}",
                                onclick: move |_| toggle(Open::Start),
                                "{from_label}"
                            }
                            if is(Open::Start) {
                                TimeMenu {
                                    label: "Start",
                                    choices: time_choices(None),
                                    current: time_of(event.start),
                                    on_pick: {
                                        let id = id.clone();
                                        move |t: NaiveTime| {
                                            state.update_event(&id, |ev| {
                                                ev.start = when::at(start_date, t);
                                                ev.end = ev.start + duration;
                                            });
                                            close();
                                        }
                                    },
                                    on_close: move |_| close(),
                                }
                            }
                        }
                        span { class: "ed-dash", "aria-hidden": "true", "–" }
                        div { class: "ed-pick",
                            button {
                                class: chip(Open::End),
                                id: "ev-end",
                                "data-value": "{when::time_value(event.end)}",
                                "aria-label": "{end_said}",
                                "aria-haspopup": "menu",
                                "aria-expanded": "{is(Open::End)}",
                                onclick: move |_| toggle(Open::End),
                                "{to_label}"
                            }
                            if is(Open::End) {
                                TimeMenu {
                                    label: "End",
                                    choices: time_choices(Some(time_of(event.start))),
                                    current: time_of(event.end),
                                    on_pick: {
                                        let id = id.clone();
                                        move |t: NaiveTime| {
                                            state.update_event(&id, |ev| {
                                                let mut end = when::at(when::date_of(ev.start), t);
                                                if end <= ev.start {
                                                    end += 24 * HOUR;
                                                }
                                                ev.end = end;
                                            });
                                            close();
                                        }
                                    },
                                    on_close: move |_| close(),
                                }
                            }
                        }
                        if past_midnight {
                            span { class: "ed-next", "next day" }
                        }
                    }
                }
                div { class: "ed-row",
                    span { class: "ed-ico" }
                    span { class: "ed-label ed-grow", id: "ev-all-day-label", "All day" }
                    button {
                        class: if all_day { "switch on" } else { "switch" },
                        role: "switch",
                        "aria-checked": "{all_day}",
                        "aria-labelledby": "ev-all-day-label",
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                state.update_event(&id, |ev| {
                                    if ev.all_day {
                                        ev.all_day = false;
                                        ev.start = when::at(start_date, NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN));
                                        ev.end = ev.start + HOUR;
                                    } else {
                                        // (An event that ends at midnight ends on the day before: not two days.)
                                        let last_day = when::date_of((ev.end - 1).max(ev.start));
                                        ev.all_day = true;
                                        ev.start = when::midnight(start_date);
                                        ev.end = when::midnight(when::next_day(last_day.max(start_date)));
                                    }
                                });
                            }
                        },
                        span { class: "knob" }
                    }
                }
                div { class: "ed-sep" }
                // How it repeats, when it reminds, its colour.
                div { class: "ed-pick ed-pick-row",
                    button {
                        class: row(Open::Repeat),
                        id: "ev-repeat",
                        "data-value": "{rule_choice}",
                        "aria-label": "Repeat: {repeat_label}",
                        "aria-haspopup": "menu",
                        "aria-expanded": "{is(Open::Repeat)}",
                        onclick: move |_| toggle(Open::Repeat),
                        span { class: "ed-ico", Icon { name: "repeat", size: 16 } }
                        span { class: "ed-val", "{repeat_label}" }
                        span { class: "ed-chev", Icon { name: "chevron-down", size: 12 } }
                    }
                    if is(Open::Repeat) {
                        PickMenu {
                            label: "Repeat",
                            rows: repeat_rows,
                            on_pick: {
                                let id = id.clone();
                                move |v: String| {
                                    close();
                                    state.set_event_repeat(&id, &v);
                                }
                            },
                            on_close: move |_| close(),
                        }
                    }
                }
                if event.repeat.is_some() {
                    div { class: "ed-pick ed-pick-row",
                        button {
                            class: if is(Open::Until) || is(Open::UntilDay) { "ed-row open" } else { "ed-row" },
                            id: "ev-until",
                            "data-value": "{until.map(when::date_value).unwrap_or_default()}",
                            "aria-label": "Last repeat: {until_label}",
                            "aria-haspopup": "menu",
                            "aria-expanded": "{is(Open::Until) || is(Open::UntilDay)}",
                            onclick: move |_| {
                                if *open.peek() == Some(Open::UntilDay) {
                                    close();
                                } else {
                                    toggle(Open::Until);
                                }
                            },
                            span { class: "ed-ico" }
                            span { class: "ed-val", "{until_label}" }
                            span { class: "ed-chev", Icon { name: "chevron-down", size: 12 } }
                        }
                        if is(Open::Until) {
                            PickMenu {
                                label: "Last repeat",
                                rows: until_rows,
                                on_pick: {
                                    let id = id.clone();
                                    move |v: String| {
                                        if v == "date" {
                                            state.set_event_until(&id, Some(until.unwrap_or_else(|| when::add_days(start_date, 12 * 7))));
                                            open.set(Some(Open::UntilDay));
                                        } else {
                                            state.set_event_until(&id, None);
                                            close();
                                        }
                                    }
                                },
                                on_close: move |_| close(),
                            }
                        }
                        if is(Open::UntilDay) {
                            DayPicker {
                                label: "Last repeat",
                                day: until.unwrap_or_else(|| when::add_days(start_date, 12 * 7)),
                                on_pick: {
                                    let id = id.clone();
                                    move |d: NaiveDate| {
                                        state.set_event_until(&id, Some(d.max(start_date)));
                                        close();
                                    }
                                },
                                on_close: move |_| close(),
                            }
                        }
                    }
                }
                if one_repeat {
                    p { class: "ed-note",
                        "Changes apply to every repeat. "
                        button {
                            class: "link-btn",
                            onclick: {
                                let id = id.clone();
                                move |_| state.detach_event(&id)
                            },
                            "Change only this one"
                        }
                    }
                }
                div { class: "ed-pick ed-pick-row",
                    button {
                        class: row(Open::Remind),
                        id: "ev-remind",
                        "data-value": "{remind_value}",
                        title: "A Mac notification before the event",
                        "aria-label": "Reminder: {remind_label}",
                        "aria-haspopup": "menu",
                        "aria-expanded": "{is(Open::Remind)}",
                        onclick: move |_| toggle(Open::Remind),
                        span { class: "ed-ico", Icon { name: "bell", size: 16 } }
                        span { class: "ed-val", "{remind_label}" }
                        span { class: "ed-chev", Icon { name: "chevron-down", size: 12 } }
                    }
                    if is(Open::Remind) {
                        PickMenu {
                            label: "Reminder",
                            rows: remind_rows,
                            on_pick: {
                                let id = id.clone();
                                move |v: String| {
                                    state.update_event(&id, |ev| ev.remind = crate::notify::parse_reminder(&v));
                                    close();
                                }
                            },
                            on_close: move |_| close(),
                        }
                    }
                }
                if spaces.len() > 1 {
                    div { class: "ed-pick ed-pick-row",
                        button {
                            class: row(Open::Space),
                            id: "ev-space",
                            "data-value": "{event.space_id}",
                            "aria-label": "Space: {space_name}",
                            "aria-haspopup": "menu",
                            "aria-expanded": "{is(Open::Space)}",
                            onclick: move |_| toggle(Open::Space),
                            span { class: "ed-ico",
                                if let Some(sp) = own_space.clone() {
                                    span { class: "space-badge ed-space-badge", style: "--c: var(--icon-{sp.color}, var(--accent));", "{sp.initial()}" }
                                }
                            }
                            span { class: "ed-val", "{space_name}" }
                            span { class: "ed-chev", Icon { name: "chevron-down", size: 12 } }
                        }
                        if is(Open::Space) {
                            PickMenu {
                                label: "Space",
                                heading: "Move to the space",
                                rows: space_rows,
                                on_pick: {
                                    let id = id.clone();
                                    move |v: String| {
                                        close();
                                        state.set_event_space(&id, &v);
                                    }
                                },
                                on_close: move |_| close(),
                            }
                        }
                    }
                }
                div { class: "ed-row ed-still",
                    span { class: "ed-ico", Icon { name: "palette", size: 16 } }
                    div { class: "ed-tints", role: "radiogroup", "aria-label": "Colour",
                        for (color, label) in ICON_COLORS {
                            button {
                                class: if event.color == *color { "tint on" } else { "tint" },
                                role: "radio",
                                "aria-checked": "{event.color == *color}",
                                title: "{label}",
                                "aria-label": "{label}",
                                style: "--c: var(--icon-{color})",
                                onclick: {
                                    let id = id.clone();
                                    move |_| state.update_event(&id, |ev| ev.color = color.to_string())
                                },
                            }
                        }
                    }
                }
                div { class: "ed-sep" }
                div { class: "ed-row ed-still ed-link-row",
                    span { class: "ed-ico", Icon { name: "link", size: 16 } }
                    input {
                        id: "ev-link",
                        class: "ed-link",
                        r#type: "text",
                        placeholder: "Add a meeting link",
                        title: "The link to join the call of this event (Teams, Zoom, Meet…)",
                        "aria-label": "Meeting link",
                        autocomplete: "off",
                        spellcheck: false,
                        // An older event with its link in its notes shows it here too.
                        initial_value: "{shown_link}",
                        oninput: move |e: FormEvent| typed_link.set(Some(e.value().trim().to_string())),
                        onkeydown: {
                            let id = id.clone();
                            move |e: KeyboardEvent| {
                                if e.key() == Key::Enter {
                                    let typed = typed_link.peek().clone();
                                    if let Some(link) = typed {
                                        typed_link.set(None);
                                        state.update_event(&id, |ev| ev.join = link);
                                    }
                                }
                            }
                        },
                    }
                    if link_waits {
                        button {
                            class: "ed-small-btn",
                            id: "ev-link-save",
                            title: "Keep this link in the event",
                            onclick: {
                                let id = id.clone();
                                move |_| {
                                    let typed = typed_link.peek().clone();
                                    if let Some(link) = typed {
                                        typed_link.set(None);
                                        state.update_event(&id, |ev| ev.join = link);
                                    }
                                }
                            },
                            "Save"
                        }
                    } else if event.join_url().is_some() {
                        button {
                            class: "ed-small-btn",
                            id: "ev-join",
                            title: "Open the meeting and its notes",
                            onclick: {
                                let id = id.clone();
                                move |_| state.join_event(&id)
                            },
                            "Join"
                        }
                    }
                }
                div { class: "ed-sep" }
                div { class: "ed-notes-row",
                    span { class: "ed-ico", Icon { name: "note", size: 16 } }
                    div { class: "linked-box ed-notes-box",
                        textarea {
                            id: "ev-notes",
                            class: "ed-notes",
                            placeholder: "Add notes",
                            "aria-label": "Notes",
                            // ("@" lists the pages here.)
                            "data-at": "link",
                            initial_value: "{event.notes}",
                            oninput: {
                                let id = id.clone();
                                move |e: FormEvent| {
                                    let v = e.value();
                                    state.update_event(&id, |ev| ev.notes = v);
                                }
                            },
                        }
                        if crate::inline::has_page_link(&event.notes) {
                            super::tables::LinkedText { text: event.notes.clone() }
                        }
                    }
                    button { class: "icon-btn small mic-btn ed-notes-mic", "data-dictate": "ev-notes", title: "Speak the notes", "aria-label": "Speak the notes", Icon { name: "mic", size: 14 } }
                }
            }
            div { class: "ed-foot",
                button {
                    class: "ghost-btn",
                    id: "ev-page",
                    onclick: {
                        let id = id.clone();
                        move |_| state.event_notes(&id)
                    },
                    Icon { name: "file", size: 15 }
                    span { if has_page { "Open notes page" } else { "Make a notes page" } }
                }
                // For a repeat it asks first: this one or all of them.
                div { class: "ed-pick",
                    button {
                        class: if is(Open::Delete) { "icon-btn ed-trash open" } else { "icon-btn ed-trash" },
                        id: "ev-delete",
                        title: "Delete event",
                        "aria-label": if one_repeat { "Delete…" } else { "Delete event" },
                        "aria-haspopup": if one_repeat { "menu" } else { "false" },
                        "aria-expanded": "{is(Open::Delete)}",
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                if one_repeat {
                                    toggle(Open::Delete);
                                } else {
                                    state.delete_event(&id);
                                }
                            }
                        },
                        Icon { name: "trash", size: 16 }
                    }
                    if is(Open::Delete) {
                        PickMenu {
                            label: "Delete",
                            heading: "This event repeats",
                            right: true,
                            rows: delete_rows,
                            on_pick: {
                                let id = id.clone();
                                move |v: String| {
                                    open.set(None);
                                    if v == "one" {
                                        state.skip_event(&id);
                                    } else {
                                        state.delete_event(&id);
                                    }
                                }
                            },
                            on_close: move |_| close(),
                        }
                    }
                }
            }
        }
    }
}

/// One of the panel's lists to choose from, in Rune's own menu: plain rows,
/// the chosen one in grey. bridge.js lifts it out of the panel's scrolling
/// part, gives the chosen row the keyboard and moves it with the arrows.
#[component]
fn PickMenu(
    #[props(into)] label: String,
    rows: Vec<(String, String, String, bool, bool)>,
    on_pick: EventHandler<String>,
    on_close: EventHandler<()>,
    #[props(default, into)] heading: String,
    #[props(default)] right: bool,
) -> Element {
    let choice = rows.iter().any(|r| r.3);
    rsx! {
        div {
            class: if right { "ed-menu ed-menu-right" } else { "ed-menu" },
            role: "menu",
            "aria-label": "{label}",
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    on_close.call(());
                }
            },
            if !heading.is_empty() {
                div { class: "ed-menu-label", "{heading}" }
            }
            for (value, text, aside, on, danger) in rows {
                button {
                    key: "{value}",
                    class: if on { "ed-menu-row on" } else if danger { "ed-menu-row danger" } else { "ed-menu-row" },
                    role: if choice { "menuitemradio" } else { "menuitem" },
                    "aria-checked": choice.then(|| on.to_string()),
                    "data-value": "{value}",
                    onclick: {
                        let v = value.clone();
                        move |_| on_pick.call(v.clone())
                    },
                    span { "{text}" }
                    span { class: "ed-menu-aside", "{aside}" }
                }
            }
        }
    }
}

/// The times to choose from, with a box to type any other time in.
#[component]
fn TimeMenu(
    #[props(into)] label: String,
    choices: Vec<(NaiveTime, String, String)>,
    current: NaiveTime,
    on_pick: EventHandler<NaiveTime>,
    on_close: EventHandler<()>,
) -> Element {
    let mut typed = use_signal(String::new);
    let mut wrong = use_signal(|| false);
    rsx! {
        div {
            class: "ed-menu ed-times-menu",
            role: "menu",
            "aria-label": "{label}",
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    on_close.call(());
                }
            },
            input {
                class: if wrong() { "ed-menu-field wrong" } else { "ed-menu-field" },
                r#type: "text",
                placeholder: "Type a time",
                "aria-label": "Type a time",
                "aria-invalid": "{wrong()}",
                autocomplete: "off",
                spellcheck: false,
                oninput: move |e: FormEvent| {
                    typed.set(e.value());
                    wrong.set(false);
                },
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        e.prevent_default();
                        match crate::quickadd::typed_time(&typed.peek()) {
                            Some(t) => on_pick.call(t),
                            None => wrong.set(true),
                        }
                    }
                },
            }
            div { class: "ed-menu-list",
                for (t, name, aside) in choices {
                    button {
                        key: "{t}",
                        class: if t == current { "ed-menu-row on" } else { "ed-menu-row" },
                        role: "menuitemradio",
                        "aria-checked": "{t == current}",
                        "data-value": t.format("%H:%M").to_string(),
                        onclick: move |_| on_pick.call(t),
                        span { "{name}" }
                        span { class: "ed-menu-aside", "{aside}" }
                    }
                }
            }
        }
    }
}

/// Rune's own day picker: a month of days, the arrow keys move a day or a
/// week, Page Up and Page Down a month, Escape puts it away.
#[component]
fn DayPicker(#[props(into)] label: String, day: NaiveDate, on_pick: EventHandler<NaiveDate>, on_close: EventHandler<()>) -> Element {
    let mut month = use_signal(|| day.with_day(1).unwrap_or(day));
    let mut focused = use_signal(|| day);
    // The keyboard follows the day that the arrows move to (the chosen day
    // first). The days may not be in the page yet when this runs, so it tries
    // again on the next frames; it also stops a late return of the keyboard
    // to the panel's buttons (see `focus_button`).
    use_effect(move || {
        let d = when::date_value(focused());
        document::eval(&format!(
            "window.__edFocus=null;let n=0;const f=()=>{{const b=document.querySelector('.ed-days [data-day=\"{d}\"]');\
             if(b){{b.focus({{preventScroll:true}});}}else if(n++<60){{requestAnimationFrame(f);}}}};f();"
        ));
    });
    let mut go = move |d: NaiveDate| {
        focused.set(d);
        let shown = *month.peek();
        if d.month() != shown.month() || d.year() != shown.year() {
            month.set(d.with_day(1).unwrap_or(d));
        }
    };
    let shown = month();
    let today = when::today();
    let heading = shown.format("%B %Y").to_string();
    rsx! {
        div {
            class: "ed-menu ed-days",
            role: "dialog",
            "aria-label": "{label}",
            onkeydown: move |e: KeyboardEvent| {
                let f = *focused.peek();
                let to = match e.key() {
                    Key::ArrowLeft => Some(when::add_days(f, -1)),
                    Key::ArrowRight => Some(when::add_days(f, 1)),
                    Key::ArrowUp => Some(when::add_days(f, -7)),
                    Key::ArrowDown => Some(when::add_days(f, 7)),
                    Key::PageUp => Some(when::add_months(f, -1)),
                    Key::PageDown => Some(when::add_months(f, 1)),
                    Key::Escape => {
                        e.stop_propagation();
                        on_close.call(());
                        None
                    }
                    _ => None,
                };
                if let Some(d) = to {
                    e.prevent_default();
                    go(d);
                }
            },
            div { class: "ed-days-head",
                span { "{heading}" }
                button { class: "icon-btn small", title: "Previous month", "aria-label": "Previous month", onclick: move |_| go(when::add_months(shown, -1)), Icon { name: "chevron-left", size: 14 } }
                button { class: "icon-btn small", title: "Next month", "aria-label": "Next month", onclick: move |_| go(when::add_months(shown, 1)), Icon { name: "chevron-right", size: 14 } }
            }
            div { class: "ed-days-grid",
                for w in ["S", "M", "T", "W", "T", "F", "S"] {
                    span { class: "wd", "aria-hidden": "true", "{w}" }
                }
                for d in when::month_grid(shown) {
                    button {
                        key: "{d}",
                        class: {
                            let mut c = String::new();
                            if d.month() != shown.month() {
                                c.push_str(" other");
                            }
                            if d == today {
                                c.push_str(" today");
                            }
                            if d == day {
                                c.push_str(" on");
                            }
                            c.trim().to_string()
                        },
                        "data-day": "{when::date_value(d)}",
                        tabindex: if d == focused() { "0" } else { "-1" },
                        "aria-label": "{d.format(\"%A, %B %-d, %Y\")}",
                        "aria-pressed": "{d == day}",
                        "aria-current": if d == today { "date" } else { "false" },
                        onclick: move |_| on_pick.call(d),
                        "{d.day()}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_events_share_the_column() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let at = |h: u32, m: u32| when::at(d, NaiveTime::from_hms_opt(h, m, 0).unwrap());
        let a = Event::new("s", "a", at(9, 0), at(10, 0), false);
        let b = Event::new("s", "b", at(9, 30), at(10, 30), false);
        let c = Event::new("s", "c", at(11, 0), at(12, 0), false);
        let placed = place(vec![a, b, c], d);
        assert_eq!(placed.len(), 3);
        assert_eq!((placed[0].width, placed[1].width, placed[2].width), (50.0, 50.0, 100.0));
        assert_eq!(placed[1].left, 50.0);
        assert_eq!(placed[0].top, 9.0 * HOUR_PX);
        assert!((placed[2].height - HOUR_PX).abs() < 0.01);
    }
}
