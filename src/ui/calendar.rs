//! The calendar: a day, week or month view of the space's events, with an
//! editor panel for the selected event, in the spirit of Notion Calendar.
//! The day view has the day at the left (what is due, then the hours) and
//! the notes of one of its events at the right, written in place.
//! Click an empty time in the week to add an event and type its name there
//! (double-click a day for an all-day one); click an event to edit it.

use super::icons::{Icon, ICON_COLORS};
use crate::model::Event;
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
    // The open to-dos that have a day: they show on that day.
    let due: Vec<OpenTodo> = state.open_todos().into_iter().filter(|t| t.due.is_some()).collect();
    let mut events: Vec<Event> = repeat::expand(state.events.read().values().filter(|e| e.space_id == space), from, to);
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
    let _ = tick.read();
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
        .and_then(|id| repeat::find(&state.events.read(), id).or_else(|| state.mac_event(id)))
        .filter(|e| e.space_id == space);
    let view = cal.view;
    // The day view: the event whose notes show. The one that was clicked,
    // when it is on this day; else the one that is on now, the next one,
    // or the first of the day.
    let shown = (view == CalView::Day)
        .then(|| {
            let day = day_events(&events, cal.anchor);
            let clicked = cal.selected.as_ref().and_then(|id| day.iter().find(|e| &e.id == id).cloned());
            clicked.or_else(|| event_in_front(&day, crate::model::now_ms()))
        })
        .flatten();
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
                    CalView::Month => rsx! { MonthGrid { anchor: cal.anchor, events: events, selected: cal.selected, todos: due } },
                    CalView::Week => rsx! { WeekGrid { days: when::week_days(cal.anchor), events: events, selected: cal.selected, panel_open, todos: due } },
                    CalView::Day => rsx! {
                        div { class: "cal-day-split",
                            div { class: "cal-day-col",
                                DayTodos { day: cal.anchor }
                                // The day has its to-dos in the card over the hours.
                                WeekGrid { days: vec![cal.anchor], events: events, selected: shown_id, panel_open, todos: Vec::new() }
                            }
                            // Keyed, so the notes of another event start afresh.
                            for ev in shown.clone() {
                                DayNotes { key: "{ev.id}", event: ev }
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
fn DayNotes(event: Event) -> Element {
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
    let joins = crate::meeting::join_link(&event.notes).is_some();
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
        }
    }
}

#[component]
fn MonthGrid(anchor: NaiveDate, events: Vec<Event>, selected: Option<String>, todos: Vec<OpenTodo>) -> Element {
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
fn WeekGrid(days: Vec<NaiveDate>, events: Vec<Event>, selected: Option<String>, panel_open: bool, todos: Vec<OpenTodo>) -> Element {
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
                                if *closing.peek() {
                                    if panel_open {
                                        state.close_event_panel();
                                    }
                                    return;
                                }
                                let slots = 24 * 60 / SLOT_MINUTES;
                                let y = e.element_coordinates().y;
                                let mins = ((y / HOUR_PX * 60.0) / f64::from(SLOT_MINUTES)).floor().clamp(0.0, f64::from(slots - 1)) as u32 * SLOT_MINUTES;
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
    rsx! {
        div {
            class: if selected { "cal-event on" } else { "cal-event" },
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

/// The panel for editing the selected event. Keyed by event, so its fields
/// start fresh when another event is chosen.
/// The panel of an event that comes from the Mac's calendars. It is only
/// shown here: it is changed in the Calendar app. A notes page can be made
/// for it, as for any event.
#[component]
fn MacEventPanel(event: Event) -> Element {
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let day = when::date_of(event.start);
    let summary = if event.all_day {
        format!("{} · all day", when::day_label(day))
    } else {
        format!("{} · {} – {}", when::day_label(day), when::time_label(event.start), when::time_label(event.end))
    };
    let calendar = {
        let m = state.mac_cal.read();
        m.events.iter().find(|e| e.id == id).map(|e| m.name_of(&e.calendar)).unwrap_or_default()
    };
    let has_page = event.page_id.is_some();
    // Its place, without the address to join by (that is the button).
    let joins = crate::meeting::join_link(&event.notes).is_some();
    let place = event.notes.lines().filter(|l| crate::meeting::join_link(l).is_none()).collect::<Vec<_>>().join("\n");
    let join_id = id.clone();
    rsx! {
        aside { class: "cal-editor of-the-mac", "data-event": "{id}",
            div { class: "ed-head",
                span { class: "ed-swatch", style: "{color_vars(&event.color)}" }
                span { class: "ed-kicker", "{summary}" }
                button { class: "icon-btn small", title: "Close", onclick: move |_| state.close_event_panel(), Icon { name: "close", size: 14 } }
            }
            h3 { class: "ed-title-plain", "{event.display_title()}" }
            div { class: "ed-plain", "{calendar}" }
            if !place.trim().is_empty() {
                div { class: "ed-plain", "{place}" }
            }
            div { class: "ed-actions",
                if joins {
                    button { class: "secondary-btn", id: "ev-join", title: "Open the meeting and its notes", onclick: move |_| state.join_event(&join_id), "Join" }
                }
                button { class: "secondary-btn with-icon", id: "mac-event-notes", onclick: move |_| state.event_notes(&id),
                    Icon { name: "page", size: 15 }
                    span { if has_page { "Open notes page" } else { "Make a notes page" } }
                }
            }
            div { class: "ed-plain quiet",
                if state.mac_cal.read().can_write() {
                    "From the calendars of this Mac. Drag it to give it another time."
                } else {
                    "From the calendars of this Mac. Change it in the Calendar app."
                }
            }
        }
    }
}

#[component]
fn EventEditor(event: Event) -> Element {
    let state = use_context::<AppState>();
    let id = event.id.clone();
    let all_day = event.all_day;
    let start_date = when::date_of(event.start);
    let last_day = if all_day { when::date_of(event.end - 1) } else { when::date_of(event.end) };
    let duration = event.end - event.start;
    let summary = if all_day {
        if last_day == start_date {
            format!("{} · all day", when::day_label(start_date))
        } else {
            format!("{} – {}", when::day_label(start_date), when::day_label(last_day))
        }
    } else {
        format!("{} · {} – {}", when::day_label(start_date), when::time_label(event.start), when::time_label(event.end))
    };
    let has_page = event.page_id.as_ref().is_some_and(|p| state.pages.read().get(p).is_some_and(|p| !p.trashed));

    let (i1, i2, i3, i4, i5, i6, i7, i8, i9) =
        (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
    let i10 = id.clone();
    let remind = event.remind;
    let (i11, i12, i13, i14, i15, i16) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
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
    let until = event.repeat.as_ref().and_then(|r| r.until_date());
    let mut asking_delete = use_signal(|| false);

    rsx! {
        aside { class: "cal-editor",
            div { class: "ed-head",
                span { class: "ed-swatch", style: "{color_vars(&event.color)}" }
                span { class: "ed-kicker", "{summary}" }
                button { class: "icon-btn small", title: "Close", onclick: move |_| state.close_event_panel(), Icon { name: "close", size: 14 } }
            }
            input {
                id: "ev-title-{id}",
                class: "ed-title",
                placeholder: "New event",
                initial_value: "{event.title}",
                oninput: move |e: FormEvent| {
                    let v = e.value();
                    state.update_event(&i1, |ev| ev.title = v);
                },
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        e.prevent_default();
                    }
                },
            }
            if all_day {
                div { class: "ed-grid",
                    label { "Starts" }
                    input {
                        r#type: "date",
                        class: "text-input",
                        value: "{when::date_value(start_date)}",
                        onchange: move |e: FormEvent| {
                            if let Some(d) = when::parse_date(&e.value()) {
                                state.update_event(&i2, |ev| {
                                    let days = ((ev.end - ev.start) as f64 / (24 * HOUR) as f64).round().max(1.0) as i64;
                                    ev.start = when::midnight(d);
                                    ev.end = when::midnight(when::add_days(d, days));
                                });
                            }
                        },
                    }
                    label { "Ends" }
                    input {
                        r#type: "date",
                        class: "text-input",
                        value: "{when::date_value(last_day)}",
                        onchange: move |e: FormEvent| {
                            if let Some(d) = when::parse_date(&e.value()) {
                                state.update_event(&i3, |ev| ev.end = when::midnight(when::next_day(d)));
                            }
                        },
                    }
                }
            } else {
                div { class: "ed-grid",
                    label { "Date" }
                    input {
                        r#type: "date",
                        class: "text-input",
                        value: "{when::date_value(start_date)}",
                        onchange: move |e: FormEvent| {
                            if let Some(d) = when::parse_date(&e.value()) {
                                state.update_event(&i2, |ev| {
                                    ev.start = when::at(d, time_of(ev.start));
                                    ev.end = ev.start + duration;
                                });
                            }
                        },
                    }
                    label { "From" }
                    div { class: "ed-times",
                        input {
                            r#type: "time",
                            class: "text-input",
                            value: "{when::time_value(event.start)}",
                            onchange: move |e: FormEvent| {
                                if let Some(t) = when::parse_time(&e.value()) {
                                    state.update_event(&i3, |ev| {
                                        ev.start = when::at(start_date, t);
                                        ev.end = ev.start + duration;
                                    });
                                }
                            },
                        }
                        span { class: "ed-to", "to" }
                        input {
                            r#type: "time",
                            class: "text-input",
                            value: "{when::time_value(event.end)}",
                            onchange: move |e: FormEvent| {
                                if let Some(t) = when::parse_time(&e.value()) {
                                    state.update_event(&i4, |ev| {
                                        let mut end = when::at(when::date_of(ev.start), t);
                                        if end <= ev.start {
                                            end += 24 * HOUR;
                                        }
                                        ev.end = end;
                                    });
                                }
                            },
                        }
                    }
                }
            }
            div { class: "ed-row",
                span { class: "ed-label", "All day" }
                button {
                    class: if all_day { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{all_day}",
                    onclick: move |_| {
                        state.update_event(&i5, |ev| {
                            if ev.all_day {
                                ev.all_day = false;
                                ev.start = when::at(start_date, NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN));
                                ev.end = ev.start + HOUR;
                            } else {
                                ev.all_day = true;
                                ev.start = when::midnight(start_date);
                                ev.end = when::midnight(when::next_day(last_day.max(start_date)));
                            }
                        });
                    },
                    span { class: "knob" }
                }
            }
            div { class: "ed-row",
                span { class: "ed-label", "Repeat" }
                select {
                    id: "ev-repeat",
                    class: "select small",
                    onchange: move |e: FormEvent| state.set_event_repeat(&i11, &e.value()),
                    for (key, label) in repeat::choices(first_day) {
                        option { value: "{key}", selected: rule_choice == key, "{label}" }
                    }
                    if rule_choice == "custom" {
                        option { value: "custom", selected: true, disabled: true, "{custom_label}" }
                    }
                }
            }
            if event.repeat.is_some() {
                div { class: "ed-row",
                    span { class: "ed-label", "Ends" }
                    div { class: "ed-until",
                        select {
                            class: "select small",
                            onchange: move |e: FormEvent| {
                                let end = (e.value() == "date").then(|| when::add_days(start_date, 12 * 7));
                                state.set_event_until(&i12, end);
                            },
                            option { value: "never", selected: until.is_none(), "Never" }
                            option { value: "date", selected: until.is_some(), "On a date" }
                        }
                        if let Some(u) = until {
                            input {
                                r#type: "date",
                                class: "text-input",
                                "aria-label": "Last day",
                                value: "{when::date_value(u)}",
                                onchange: move |e: FormEvent| {
                                    if let Some(d) = when::parse_date(&e.value()) {
                                        state.set_event_until(&i13, Some(d));
                                    }
                                },
                            }
                        }
                    }
                }
            }
            if one_repeat {
                p { class: "note tight ed-repeat-note",
                    "Changes here apply to every repeat. "
                    button { class: "link-btn", onclick: move |_| state.detach_event(&i14), "Change only this one" }
                }
            }
            div { class: "ed-row",
                span { class: "ed-label", "Reminder" }
                select {
                    class: "select small",
                    title: "A Mac notification before the event",
                    onchange: move |e: FormEvent| {
                        let v = e.value();
                        state.update_event(&i10, |ev| ev.remind = crate::notify::parse_reminder(&v));
                    },
                    for (m, label) in crate::notify::REMINDERS {
                        option {
                            value: m.map(|x| x.to_string()).unwrap_or_else(|| "none".into()),
                            selected: remind == m,
                            "{label}"
                        }
                    }
                }
            }
            div { class: "ed-row",
                span { class: "ed-label", "Colour" }
                div { class: "tints small",
                    for (color, label) in ICON_COLORS {
                        button {
                            class: if event.color == *color { "tint on" } else { "tint" },
                            title: "{label}",
                            style: "--c: var(--icon-{color})",
                            onclick: {
                                let id = i6.clone();
                                move |_| state.update_event(&id, |ev| ev.color = color.to_string())
                            },
                        }
                    }
                }
            }
            div { class: "ed-notes-wrap",
                textarea {
                    id: "ev-notes",
                    class: "ed-notes",
                    placeholder: "Add notes",
                    initial_value: "{event.notes}",
                    oninput: move |e: FormEvent| {
                        let v = e.value();
                        state.update_event(&i7, |ev| ev.notes = v);
                    },
                }
                button { class: "icon-btn small mic-btn ed-notes-mic", "data-dictate": "ev-notes", title: "Speak the notes", "aria-label": "Speak the notes", Icon { name: "mic", size: 14 } }
            }
            div { class: "ed-actions",
                if crate::meeting::join_link(&event.notes).is_some() {
                    button {
                        class: "secondary-btn",
                        id: "ev-join",
                        title: "Open the meeting and its notes",
                        onclick: {
                            let id = event.id.clone();
                            move |_| state.join_event(&id)
                        },
                        "Join"
                    }
                }
                button { class: "secondary-btn with-icon", onclick: move |_| state.event_notes(&i8),
                    Icon { name: "file", size: 14 }
                    span { if has_page { "Open notes page" } else { "Make a notes page" } }
                }
                // For a repeat, it asks first: this one or all of them.
                button {
                    class: "ghost-btn danger-text",
                    "aria-expanded": if one_repeat { "{asking_delete()}" } else { "" },
                    onclick: move |_| {
                        if one_repeat {
                            asking_delete.toggle();
                        } else {
                            state.delete_event(&i9);
                        }
                    },
                    Icon { name: "trash", size: 14 }
                    span { if one_repeat { "Delete…" } else { "Delete" } }
                }
            }
            if one_repeat && asking_delete() {
                div { class: "ed-delete-choice", role: "group", "aria-label": "Delete",
                    button { class: "secondary-btn small", onclick: move |_| state.skip_event(&i15), "Only this one" }
                    button { class: "secondary-btn small danger-text", onclick: move |_| state.delete_event(&i16), "All repeats" }
                    button { class: "ghost-btn small", onclick: move |_| asking_delete.set(false), "Cancel" }
                }
            }
            p { class: "note tight", "Tip: click an empty time in the week to add an event there." }
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
