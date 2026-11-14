//! Editors that open from a cell, by the kind of its column (the choices
//! are in table_choices.rs, the rows of a relation in table_menus.rs): a
//! day from a month, and what a worked-out cell is worked out from. Built
//! the way Notion's date picker works: a box to type the day in at the top,
//! the month under it, then an end date, a time and a reminder, and Clear
//! at the foot.

use super::icons::Icon;
use crate::state::{AppState, Popover};
use crate::table::date::{read_time, remind_words, time_words, When, DAY_REMINDERS, TIMED_REMINDERS};
use crate::when;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use dioxus::prelude::*;

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap_or(d)
}

/// The hour a time starts at when "Include time" is switched on.
fn first_time() -> NaiveTime {
    NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN)
}

/// A date cell with its end switched on or off: an end on the start day
/// (an hour later, with a time), or none.
fn with_end(w: When, on: bool) -> When {
    if on {
        When { end: Some(w.start), end_time: w.time.map(|t| t + Duration::hours(1)), ..w }
    } else {
        When { end: None, end_time: None, ..w }
    }
}

/// A date cell with its time switched on (9 AM, its end at 10 AM) or off.
fn with_time(w: When, on: bool) -> When {
    if on {
        let t = first_time();
        When { time: Some(t), end_time: w.end.map(|_| t + Duration::hours(1)), ..w }
    } else {
        When { time: None, end_time: None, ..w }
    }
}

/// A click on a day: the day of a single date (its time and reminder
/// kept); with an end, the start or the end, as `at_end` says (an end
/// before the start becomes the start).
fn click_day(w: Option<When>, d: NaiveDate, at_end: bool) -> When {
    match w {
        None => When::on(d),
        Some(w) if w.end.is_some() && at_end => When { end: Some(d), ..w }.in_order(),
        Some(w) if w.end.is_some() => When { start: d, end: w.end.map(|e| e.max(d)), ..w },
        Some(w) => When { start: d, ..w },
    }
}

/// The day picker of a date cell. What is typed in its box goes into the
/// cell at once (as in a cell that is a box to type in), and the month
/// follows it when it reads as a day; the cell is written the way its
/// column writes days when the picker closes. A click on a day, or Return
/// on a day of the month, writes that day and closes; with an end date it
/// sets the start, then the end, and stays open. ↓ goes from the box to
/// the month, where the arrows move by a day or a week and Page Up and Page
/// Down by a month. Under the month: the end date, the time, the reminder.
#[component]
pub fn DatePicker(page_id: String, id: String, r: usize, c: usize, text: String) -> Element {
    let state = use_context::<AppState>();
    let today = when::today();
    let now = When::read(&text, today, false);
    let start = now.map_or(today, |w| w.start);
    let mut field = use_signal(|| When::read(&text, today, true).map_or_else(|| text.clone(), |w| w.said(today)));
    let mut focus = use_signal(|| start);
    let mut month = use_signal(|| first_of_month(start));
    // Words typed in the box that the cell holds as typed, to be written
    // the way the column writes when the picker closes.
    let mut typed = use_signal(|| false);
    let mut in_grid = use_signal(|| false);
    // With an end date: whether the next click on a day sets the end.
    let mut at_end = use_signal(|| false);
    let mut remind_open = use_signal(|| false);
    {
        let (p, b) = (page_id.clone(), id.clone());
        use_drop(move || {
            if *typed.peek() {
                state.settle_table_cell(&p, &b, r, c);
            }
        });
    }
    // The keyboard is in the box, or on the day the arrows moved to.
    use_effect(move || {
        let target = if in_grid() { format!(".date-picker [data-day=\"{}\"]", when::date_value(focus())) } else { ".date-picker .pick-input".to_string() };
        document::eval(&format!(
            "let n=0;const f=()=>{{const b=document.querySelector({target:?});if(b){{if(document.activeElement!==b)b.focus({{preventScroll:true}});}}else if(n++<60){{requestAnimationFrame(f);}}}};f();"
        ));
    });
    let mut go = move |d: NaiveDate| {
        focus.set(d);
        if first_of_month(d) != *month.peek() {
            month.set(first_of_month(d));
        }
    };
    let close = {
        let b = id.clone();
        move || state.close_table_cell(&b, r, c)
    };
    let finish = {
        let (p, b) = (page_id.clone(), id.clone());
        let close = close.clone();
        move || {
            let mut typed = typed;
            if *typed.peek() {
                typed.set(false);
                state.settle_table_cell(&p, &b, r, c);
            }
            close();
        }
    };
    // Write a date in the cell (the picker stays open), and in the box.
    let write = {
        let (p, b) = (page_id.clone(), id.clone());
        move |w: Option<When>| {
            let (mut typed, mut field) = (typed, field);
            let words = w.map(|w| w.in_order().write(when::today())).unwrap_or_default();
            typed.set(false);
            field.set(w.map(|w| w.in_order().said(when::today())).unwrap_or_default());
            state.put_table_cell(&p, &b, r, c, &words);
        }
    };
    let pick = {
        let write = write.clone();
        let close = close.clone();
        move |d: NaiveDate| {
            let mut at_end = at_end;
            let w = click_day(now, d, *at_end.peek());
            write(Some(w));
            if w.end.is_some() {
                // (A range stays open: the start, then the end.)
                let next = !*at_end.peek();
                at_end.set(next);
            } else {
                close();
            }
        }
    };
    let typing = {
        let (p, b) = (page_id.clone(), id.clone());
        move |e: FormEvent| {
            let words = e.value();
            field.set(words.clone());
            // (Only words that say a day, or no words at all, go to the
            // cell. One key pressed by mistake on the cell opens this box
            // with that letter in it: written to the cell, it took the
            // day, the time and the reminder away, and Escape kept it.)
            if words.trim().is_empty() || When::read(&words, when::today(), false).is_some() {
                typed.set(true);
                state.set_table_cell(&p, &b, r, c, &words);
            }
            if let Some(w) = When::read(&words, when::today(), true) {
                go(w.start);
            }
        }
    };
    let field_keys = {
        let finish = finish.clone();
        move |e: KeyboardEvent| match e.key() {
            Key::Enter | Key::Tab => {
                e.prevent_default();
                finish();
            }
            Key::ArrowDown => {
                e.prevent_default();
                in_grid.set(true);
            }
            _ => {}
        }
    };
    let grid_keys = {
        let finish = finish.clone();
        move |e: KeyboardEvent| {
            let f = *focus.peek();
            let to = match e.key() {
                Key::ArrowLeft => Some(when::add_days(f, -1)),
                Key::ArrowRight => Some(when::add_days(f, 1)),
                // (Up from the first week goes back to the box.)
                Key::ArrowUp if (f - when::week_start(first_of_month(*month.peek()))).num_days() < 7 => {
                    e.prevent_default();
                    in_grid.set(false);
                    None
                }
                Key::ArrowUp => Some(when::add_days(f, -7)),
                Key::ArrowDown => Some(when::add_days(f, 7)),
                Key::PageUp => Some(when::add_months(f, -1).with_day(f.day().min(28)).unwrap_or(f)),
                Key::PageDown => Some(when::add_months(f, 1).with_day(f.day().min(28)).unwrap_or(f)),
                Key::Tab => {
                    e.prevent_default();
                    finish();
                    None
                }
                _ => None,
            };
            if let Some(d) = to {
                e.prevent_default();
                go(d);
            }
        }
    };
    let shown = month();
    let heading = shown.format("%B %Y").to_string();
    let base = now.unwrap_or(When::on(today));
    let has_end = now.is_some_and(|w| w.end.is_some());
    let timed = now.is_some_and(|w| w.time.is_some());
    let reminders: &[(i64, &str)] = if timed { &TIMED_REMINDERS } else { &DAY_REMINDERS };
    let remind_label = match now.and_then(|w| w.remind) {
        Some(m) => {
            let words = remind_words(m, timed);
            let mut chars = words.chars();
            chars.next().map(|f| f.to_uppercase().chain(chars).collect::<String>()).unwrap_or(words)
        }
        None => "None".to_string(),
    };
    // A time typed in its box (on Return or when the box is left).
    let set_time = {
        let write = write.clone();
        move |end: bool, words: String| {
            let Some(t) = read_time(&words) else { return };
            let w = if end { When { end_time: Some(t), ..base } } else { When { time: Some(t), ..base } };
            write(Some(w));
        }
    };
    rsx! {
        div { class: "popover table-menu date-picker", role: "dialog", "aria-label": "Choose a day", onclick: move |e| e.stop_propagation(),
            div { class: "pick-bar",
                input {
                    class: "pick-input",
                    placeholder: "Type a day: 12 Dec, tomorrow…",
                    "aria-label": "Day",
                    spellcheck: false,
                    value: "{field}",
                    onfocus: move |_| in_grid.set(false),
                    oninput: typing,
                    onkeydown: field_keys,
                }
            }
            div { class: "date-head",
                span { class: "date-month", "{heading}" }
                button {
                    class: "date-today",
                    title: "Show this month",
                    "data-item": "today",
                    onclick: move |_| go(today),
                    "Today"
                }
                button { class: "date-step", title: "Previous month", "aria-label": "Previous month", onclick: move |_| go(when::add_months(shown, -1)), Icon { name: "chevron-left", size: 14 } }
                button { class: "date-step", title: "Next month", "aria-label": "Next month", onclick: move |_| go(when::add_months(shown, 1)), Icon { name: "chevron-right", size: 14 } }
            }
            div { class: "date-grid", role: "grid", onkeydown: grid_keys,
                for w in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] {
                    span { class: "wd", "aria-hidden": "true", "{w}" }
                }
                for d in when::month_grid(shown) {
                    button {
                        key: "{d}",
                        class: {
                            let mut k = String::from("date-day");
                            if d.month() != shown.month() {
                                k.push_str(" other");
                            }
                            if d == today {
                                k.push_str(" today");
                            }
                            match now {
                                Some(w) if d == w.start || Some(d) == w.end => k.push_str(" on"),
                                Some(w) if w.end.is_some() && w.covers(d) => k.push_str(" in-range"),
                                _ => {}
                            }
                            if in_grid() && d == focus() {
                                k.push_str(" at");
                            }
                            k
                        },
                        "data-day": "{when::date_value(d)}",
                        tabindex: if d == focus() { "0" } else { "-1" },
                        "aria-label": "{d.format(\"%A, %B %-d, %Y\")}",
                        "aria-pressed": if now.is_some_and(|w| d == w.start || Some(d) == w.end) { "true" } else { "false" },
                        "aria-current": if d == today { "date" } else { "false" },
                        onclick: {
                            let pick = pick.clone();
                            move |_| pick(d)
                        },
                        "{d.day()}"
                    }
                }
            }
            if has_end {
                div { class: "date-hint", if at_end() { "Click the day it ends" } else { "Click the day it starts" } }
            }
            div { class: "ai-menu-sep" }
            div { class: "date-option",
                span { class: "date-option-name", id: "date-end-label", "End date" }
                button {
                    class: if has_end { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{has_end}",
                    "aria-labelledby": "date-end-label",
                    "data-item": "end",
                    onclick: {
                        let write = write.clone();
                        move |_| {
                            write(Some(with_end(base, !has_end)));
                            at_end.set(!has_end);
                        }
                    },
                    span { class: "knob" }
                }
            }
            div { class: "date-option",
                span { class: "date-option-name", id: "date-time-label", "Include time" }
                button {
                    class: if timed { "switch on" } else { "switch" },
                    role: "switch",
                    "aria-checked": "{timed}",
                    "aria-labelledby": "date-time-label",
                    "data-item": "time",
                    onclick: {
                        let write = write.clone();
                        move |_| write(Some(with_time(base, !timed)))
                    },
                    span { class: "knob" }
                }
            }
            if let Some(t) = now.and_then(|w| w.time) {
                div { class: "date-times",
                    input {
                        class: "date-time",
                        "aria-label": "Start time",
                        "data-item": "start-time",
                        spellcheck: false,
                        value: "{time_words(t)}",
                        onchange: {
                            let set_time = set_time.clone();
                            move |e: FormEvent| set_time(false, e.value())
                        },
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Enter {
                                e.prevent_default();
                                document::eval("document.activeElement && document.activeElement.blur()");
                            }
                        },
                    }
                    if let Some(end) = now.and_then(|w| w.end_time) {
                        span { class: "date-to", "→" }
                        input {
                            class: "date-time",
                            "aria-label": "End time",
                            "data-item": "end-time",
                            spellcheck: false,
                            value: "{time_words(end)}",
                            onchange: {
                                let set_time = set_time.clone();
                                move |e: FormEvent| set_time(true, e.value())
                            },
                            onkeydown: move |e: KeyboardEvent| {
                                if e.key() == Key::Enter {
                                    e.prevent_default();
                                    document::eval("document.activeElement && document.activeElement.blur()");
                                }
                            },
                        }
                    }
                }
            }
            button {
                class: if remind_open() { "ai-menu-row active" } else { "ai-menu-row" },
                role: "menuitem",
                "data-item": "remind",
                "aria-expanded": "{remind_open()}",
                onclick: move |_| remind_open.set(!remind_open()),
                span { class: "ai-menu-value", Icon { name: "bell", size: 14 } "Remind" }
                span { class: "ai-menu-aside",
                    "{remind_label}"
                    Icon { name: if remind_open() { "chevron-down" } else { "chevron-right" }, size: 12 }
                }
            }
            if remind_open() {
                div { class: "date-reminders", role: "menu",
                    for (m, words) in std::iter::once((None, "None")).chain(reminders.iter().map(|(m, w)| (Some(*m), *w))) {
                        button {
                            key: "{words}",
                            class: if now.and_then(|w| w.remind) == m { "ai-menu-row active" } else { "ai-menu-row" },
                            role: "menuitemradio",
                            "data-remind": "{m.map(|m| m.to_string()).unwrap_or_default()}",
                            onclick: {
                                let write = write.clone();
                                move |_| {
                                    write(Some(When { remind: m, ..base }));
                                    remind_open.set(false);
                                }
                            },
                            span { class: "ai-menu-value",
                                {
                                    let mut chars = words.chars();
                                    chars.next().map(|f| f.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
                                }
                                if m.is_some() && !timed {
                                    span { class: "date-at", " (9 AM)" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "ai-menu-sep" }
            button {
                class: "ai-menu-row",
                role: "menuitem",
                "data-item": "clear",
                disabled: text.trim().is_empty(),
                onclick: {
                    let write = write.clone();
                    let close = close.clone();
                    move |_| {
                        write(None);
                        close();
                    }
                },
                span { class: "ai-menu-value", Icon { name: "close", size: 14 } "Clear" }
            }
        }
    }
}

/// What a worked-out cell (a formula, a pace) is worked out from: it is
/// not typed in, and a click on it says so. From a cell of the table, the
/// column's menu (where the formula is written) is a click away.
#[component]
pub fn WorkedOut(
    id: String,
    c: usize,
    heading: String,
    from: String,
    formula: bool,
    /// It takes a total from the row's page (see `Kind::Rollup`).
    #[props(default)]
    rollup: bool,
    in_table: bool,
) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "popover table-menu worked-out", role: "dialog", "aria-label": "How {heading} is worked out", onclick: move |e| e.stop_propagation(),
            div { class: "ai-menu-label", if formula { "Formula" } else { "Worked out" } }
            div { class: if formula { "worked-from code" } else { "worked-from" }, "{from}" }
            div { class: "menu-hint",
                if rollup {
                    "This cell is worked out from the row's page. It cannot be typed in."
                } else {
                    "This cell is worked out from the other cells of its row. It cannot be typed in."
                }
            }
            if (formula || rollup) && in_table {
                div { class: "ai-menu-sep" }
                button {
                    class: "ai-menu-row",
                    role: "menuitem",
                    "data-item": "edit-formula",
                    onclick: move |_| state.set_popover(Some(Popover::TableColumn { block_id: id.clone(), column: c })),
                    span { class: "ai-menu-value", Icon { name: "sigma", size: 14 } if rollup { "Change what it takes" } else { "Edit the formula" } }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_month_of_a_day_starts_on_its_first() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
        assert_eq!(first_of_month(d), NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
    }

    #[test]
    fn a_click_on_a_day_sets_the_day_or_an_end() {
        let d = |m: u32, day: u32| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        // An empty cell: that day.
        assert_eq!(click_day(None, d(11, 3), false), When::on(d(11, 3)));
        // A single day: the new day, its time and reminder kept.
        let timed = When { time: Some(first_time()), remind: Some(15), ..When::on(d(11, 3)) };
        assert_eq!(click_day(Some(timed), d(11, 9), true), When { start: d(11, 9), ..timed });
        // A range: the start (the end moves with it if it would be before), then the end.
        let range = When { end: Some(d(11, 6)), ..When::on(d(11, 3)) };
        assert_eq!(click_day(Some(range), d(11, 4), false).end, Some(d(11, 6)));
        assert_eq!(click_day(Some(range), d(11, 8), false), When { start: d(11, 8), end: Some(d(11, 8)), ..range });
        assert_eq!(click_day(Some(range), d(11, 20), true).end, Some(d(11, 20)));
        // An end before the start becomes the start.
        let back = click_day(Some(range), d(11, 1), true);
        assert_eq!((back.start, back.end), (d(11, 1), Some(d(11, 3))));
    }

    #[test]
    fn an_end_and_a_time_are_switched_on_and_off() {
        let day = When::on(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap());
        let nine = first_time();
        let ten = nine + Duration::hours(1);
        assert_eq!(with_end(day, true).end, Some(day.start));
        assert_eq!(with_time(with_end(day, true), true).end_time, Some(ten));
        assert_eq!(with_end(with_time(day, true), true).end_time, Some(ten));
        assert_eq!(with_time(day, true).time, Some(nine));
        let off = with_time(with_end(with_time(day, true), true), false);
        assert_eq!((off.time, off.end_time, off.end), (None, None, Some(day.start)));
        assert_eq!(with_end(off, false).end, None);
    }
}
