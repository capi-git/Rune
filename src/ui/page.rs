//! Home view, the page editor and its block-level menus, and the inline AI panel.

use super::icons::{format_icon, parse_icon, Icon, PageIcon, ICON_COLORS, PAGE_ICONS};
use crate::ai::AiAction;
use crate::inline;
use crate::model::*;
use crate::state::{filter_slash, AiStatus, AppState, Popover, Route, SlashState};
use dioxus::prelude::*;

/// One open to-do, with the page it is on: tick it here, or click it to go there.
#[component]
pub fn TodoRow(todo: crate::state::OpenTodo) -> Element {
    let state = use_context::<AppState>();
    let (page, block, go) = (todo.page.clone(), todo.block.clone(), todo.page.clone());
    rsx! {
        // Which to-do this is, for the drag onto a time in the calendar.
        div { class: "todo-row", "data-todo-page": "{todo.page}", "data-todo-block": "{todo.block}",
            button {
                class: "todo-box",
                role: "checkbox",
                "aria-checked": "false",
                "aria-label": "Done: {todo.text}",
                title: "Done",
                onclick: move |_| state.toggle_check(&page, &block),
            }
            div { class: "todo-body", onclick: move |_| state.open_page(&go),
                div { class: "todo-text", "{todo.text}" }
                div { class: "todo-from", "{todo.page_title}" }
            }
            if let Some(day) = todo.due {
                span { class: if day < crate::when::today() { "due-chip late" } else { "due-chip" }, "data-due": "{day}", "{crate::when::due_label(day, crate::when::today())}" }
            }
        }
    }
}

/// "1,240" for 1240.
fn with_commas(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "3:05" for a number of seconds.
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The colours of the voices after the first ("Me" has the colour of the text).
const VOICE_COLORS: [&str; 6] = ["brown", "green", "blue", "purple", "orange", "pink"];

/// The band at the top of a meeting's page: one lane for each voice, with
/// the stretches it spoke in, and its part of all the speaking. A click on a
/// voice's name gives it another name ("Others" becomes "Sam"): the lines
/// of the transcript change with it.
#[component]
fn MeetingBand(page_id: String, voices: crate::listen::Voices) -> Element {
    let state = use_context::<AppState>();
    // The voice whose name is being typed, and what is typed so far.
    let mut naming = use_signal(|| None::<String>);
    let mut typed = use_signal(String::new);
    // Return, or leaving the field, gives the name (one time: the field
    // goes away, and is left as it goes).
    let mut give_name = move |page: &str| {
        let old = naming.write().take();
        if let Some(old) = old {
            let new = typed.peek().clone();
            state.rename_voice(page, &old, &new);
        }
    };
    let length = voices.length;
    let minutes = (length / 60.0).round().max(1.0) as u64;
    let count = voices.voices.len();
    let summary = format!(
        "{minutes} min · {count} {} · {} words",
        if count == 1 { "voice" } else { "voices" },
        with_commas(voices.words)
    );
    let lanes: Vec<(String, String, u32, Vec<(f64, f64)>)> = voices
        .voices
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let color = if v.name == "Me" || count == 1 {
                "color: var(--text);".to_string()
            } else {
                let n = if voices.voices[0].name == "Me" { i - 1 } else { i };
                format!("color: var(--icon-{});", VOICE_COLORS[n % VOICE_COLORS.len()])
            };
            // Each stretch as a place and a width along the lane, in hundredths.
            let parts = v.parts.iter().map(|(a, b)| (a / length * 100.0, ((b - a) / length * 100.0).max(0.35))).collect();
            (v.name.clone(), color, voices.share(v), parts)
        })
        .collect();
    rsx! {
        section { class: "meeting-band", "aria-label": "Who spoke when",
            div { class: "band-meta", "{summary}" }
            for (name, color, share, parts) in lanes {
                div { key: "{name}", class: "band-row", "data-voice": "{name}", style: "{color}",
                    if naming.read().as_deref() == Some(name.as_str()) {
                        input {
                            class: "band-name-input",
                            r#type: "text",
                            "aria-label": "A name for this voice",
                            placeholder: "A name",
                            initial_value: "{name}",
                            onmounted: move |e| async move {
                                let _ = e.set_focus(true).await;
                            },
                            oninput: move |e: FormEvent| typed.set(e.value()),
                            onkeydown: {
                                let pid = page_id.clone();
                                move |e: KeyboardEvent| {
                                    if e.key() == Key::Escape {
                                        naming.set(None);
                                    } else if e.key() == Key::Enter {
                                        e.prevent_default();
                                        give_name(&pid);
                                    }
                                }
                            },
                            onblur: {
                                let pid = page_id.clone();
                                move |_| give_name(&pid)
                            },
                        }
                    } else {
                        button {
                            class: "band-name",
                            title: "Give this voice a name",
                            "aria-label": "{name}. Give this voice a name",
                            onclick: {
                                let name = name.clone();
                                move |_| {
                                    typed.set(name.clone());
                                    naming.set(Some(name.clone()));
                                }
                            },
                            span { class: "band-dot", "aria-hidden": "true" }
                            span { class: "band-name-text", "{name}" }
                        }
                    }
                    div { class: "band-track", "aria-hidden": "true",
                        for (n, (left, width)) in parts.into_iter().enumerate() {
                            span { key: "{n}", class: "band-part", style: "left: {left:.2}%; width: {width:.2}%;" }
                        }
                    }
                    span { class: "band-share", "{share}%" }
                }
            }
            div { class: "band-axis", "aria-hidden": "true",
                span { "0:00" }
                span { "{clock(length / 2.0)}" }
                span { "{clock(length)}" }
            }
        }
    }
}

/// The day a to-do is due, at the end of its line. A click opens the days to
/// choose from. A to-do without a day shows a small calendar there while the
/// pointer is on its line.
#[component]
fn DueChip(page_id: String, id: String, due: Option<chrono::NaiveDate>, #[props(default)] remind: Option<chrono::NaiveTime>, done: bool, open: bool) -> Element {
    let state = use_context::<AppState>();
    let today = crate::when::today();
    let late = !done && due.is_some_and(|d| d < today);
    let class = match (due, late) {
        (None, _) => "due-chip none",
        (Some(_), true) => "due-chip late",
        (Some(_), false) => "due-chip",
    };
    let at = due.zip(remind).map(|(d, t)| crate::when::at(d, t));
    let label = due.map(|d| match at {
        Some(at) => format!("{} {}", crate::when::due_label(d, today), crate::when::time_label(at)),
        None => crate::when::due_label(d, today),
    });
    let said = match (due, at) {
        (Some(d), Some(at)) => format!("Due {}, reminder at {}. Change the day or the reminder", d.format("%A, %B %-d"), crate::when::time_label(at)),
        (Some(d), None) => format!("Due {}. Change the day, or set a reminder", d.format("%A, %B %-d")),
        (None, _) => "Give it a day or a reminder".to_string(),
    };
    let time_value = remind.map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
    // The first day of the week after this one.
    let monday = crate::when::add_days(today, i64::from(7 - chrono::Datelike::weekday(&today).num_days_from_monday()));
    let choices = [("Today", today), ("Tomorrow", crate::when::next_day(today)), ("Next week", monday)];
    let value = due.map(crate::when::date_value).unwrap_or_default();
    let menu_id = id.clone();
    rsx! {
        div { class: "due",
            button {
                class: "{class}",
                "data-due": "{value}",
                title: "{said}",
                "aria-label": "{said}",
                "aria-haspopup": "menu",
                "aria-expanded": if open { "true" } else { "false" },
                onclick: move |_| state.set_popover((!open).then(|| Popover::Due { block_id: menu_id.clone() })),
                if let Some(l) = label {
                    if remind.is_some() {
                        Icon { name: "bell", size: 12 }
                    }
                    "{l}"
                } else {
                    Icon { name: "calendar", size: 14 }
                }
            }
            if open {
                div { class: "popover due-menu", role: "menu", onclick: move |e| e.stop_propagation(),
                    for (name, day) in choices {
                        button {
                            key: "{name}",
                            class: if due == Some(day) { "menu-item compact on" } else { "menu-item compact" },
                            role: "menuitem",
                            "data-due-pick": "{day}",
                            onclick: {
                                let (pid, bid) = (page_id.clone(), id.clone());
                                move |_| state.set_due(&pid, &bid, Some(day))
                            },
                            span { "{name}" }
                            span { class: "menu-shortcut", "{day.format(\"%a, %b %-d\")}" }
                        }
                    }
                    label { class: "due-other",
                        span { "Another day" }
                        input {
                            r#type: "date",
                            class: "ed-input",
                            "aria-label": "Another day",
                            value: "{value}",
                            onchange: {
                                let (pid, bid) = (page_id.clone(), id.clone());
                                move |e: FormEvent| {
                                    if let Some(day) = crate::when::parse_date(&e.value()) {
                                        state.set_due(&pid, &bid, Some(day));
                                    }
                                }
                            },
                        }
                    }
                    // A reminder: a Mac notification at a time of that day.
                    label { class: "due-other",
                        span { "Remind me at" }
                        input {
                            r#type: "time",
                            class: "ed-input",
                            "aria-label": "Remind me at",
                            "data-remind": "",
                            value: "{time_value}",
                            onchange: {
                                let (pid, bid) = (page_id.clone(), id.clone());
                                move |e: FormEvent| {
                                    if let Some(t) = crate::model::Block::remind_from_column(&e.value()) {
                                        state.set_reminder(&pid, &bid, Some(t));
                                    }
                                }
                            },
                        }
                    }
                    if remind.is_some() {
                        button {
                            class: "menu-item compact",
                            role: "menuitem",
                            "data-remind-off": "",
                            onclick: {
                                let (pid, bid) = (page_id.clone(), id.clone());
                                move |_| state.set_reminder(&pid, &bid, None)
                            },
                            span { "No reminder" }
                        }
                    }
                    if due.is_some() {
                        button {
                            class: "menu-item compact",
                            role: "menuitem",
                            "data-due-pick": "",
                            onclick: {
                                let (pid, bid) = (page_id.clone(), id.clone());
                                move |_| state.set_due(&pid, &bid, None)
                            },
                            span { "No day" }
                        }
                    }
                }
            }
        }
    }
}

/// Home's quick-note box: Return saves the line as a page.
#[component]
fn JotBox() -> Element {
    let state = use_context::<AppState>();
    let mut jot = use_signal(String::new);
    rsx! {
        div { class: "jot",
            input {
                id: "jot-input",
                class: "jot-input",
                placeholder: "Write a quick note",
                value: "{jot}",
                oninput: move |e: FormEvent| jot.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        let text = jot.read().clone();
                        state.capture_note(text);
                        jot.set(String::new());
                    }
                },
            }
            span { class: "jot-hint", "Return to save as a page" }
            button { class: "icon-btn small mic-btn", "data-dictate": "jot-input", title: "Speak a quick note", "aria-label": "Speak a quick note", Icon { name: "mic", size: 14 } }
        }
    }
}

#[component]
pub fn HomeView() -> Element {
    let state = use_context::<AppState>();
    // Every open to-do, from all pages (worked out again only when pages or
    // blocks change, and Home is redrawn only when the list itself does).
    let todos = use_memo(move || state.open_todos());
    let todo_count = todos.read().len();
    let first_todos: Vec<crate::state::OpenTodo> = todos.read().iter().take(6).cloned().collect();
    let ui = state.ui.read();
    let space_id = ui.space.clone();
    let recent_ids = ui.recent.clone();
    drop(ui);
    let pages = state.pages.read();
    let in_space = |p: &&Page| !p.trashed && p.space_id == space_id;
    // The pages last worked in: those opened last, then those changed last.
    let mut recent: Vec<Page> = recent_ids.iter().filter_map(|id| pages.get(id)).filter(in_space).cloned().collect();
    let mut edited: Vec<&Page> = pages.values().filter(in_space).collect();
    edited.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
    for p in edited {
        if recent.len() >= 3 {
            break;
        }
        if !recent.iter().any(|r| r.id == p.id) {
            recent.push(p.clone());
        }
    }
    recent.truncate(3);
    let no_pages = recent.is_empty();
    drop(pages);
    let cards: Vec<(Page, String)> = recent.into_iter().map(|p| {
        let snippet = state.page_snippet(&p.id);
        (p, snippet)
    }).collect();

    // The day: its events on a line, and what comes after it.
    let now = now_ms();
    let today = crate::when::today();
    let day_start = crate::when::midnight(today);
    let day_end = crate::when::midnight(crate::when::next_day(today));
    // Every space's events, as the calendar shows them (unless the person
    // chose to see only the space in front).
    let every_space = state.cal.read().all_spaces;
    let events = state.events.read();
    let mut of_today = crate::repeat::expand(events.values().filter(|e| every_space || e.space_id == space_id), day_start, day_end);
    of_today.extend(state.mac_events(&space_id, day_start, day_end));
    let ribbon = crate::state::day_ribbon(&of_today, day_start, now);
    let mut upcoming: Vec<crate::model::Event> = crate::repeat::expand(events.values().filter(|e| every_space || e.space_id == space_id), now, now + 14 * 24 * crate::when::HOUR);
    drop(events);
    upcoming.extend(state.mac_events(&space_id, now, now + 14 * 24 * crate::when::HOUR));
    upcoming.sort_by_key(|e| e.start);
    upcoming.truncate(5);
    // Each with the mark of its space, as in the calendar.
    let upcoming: Vec<(crate::model::Event, Option<crate::model::Space>)> = upcoming
        .into_iter()
        .map(|e| {
            let mark = super::calendar::space_mark(&state, &e);
            (e, mark)
        })
        .collect();
    let hours: Vec<String> = (ribbon.first_hour..=ribbon.last_hour).step_by(2).map(crate::when::hour_label).collect();
    let weekday = crate::when::local(now).format("%A").to_string();
    let date = crate::when::local(now).format("%B %-d").to_string();

    // What the last meeting left to do.
    let meeting = state.last_meeting().and_then(|id| state.pages.read().get(&id).cloned());
    let meeting_todos: Vec<crate::state::OpenTodo> = match &meeting {
        Some(m) => todos.read().iter().filter(|t| t.page == m.id).take(3).cloned().collect(),
        None => Vec::new(),
    };

    rsx! {
        div { class: "home today",
            div { class: "home-hero",
                div { class: "hero-day",
                    div { class: "hero-date", "{weekday}" }
                    h1 { class: "hero-title", "{date}" }
                }
                JotBox {}
            }
            // The line of the day.
            div { class: "day-line", id: "day-line", role: "group", "aria-label": "Today's events",
                if !ribbon.all_day.is_empty() {
                    div { class: "day-all",
                        span { "All day" }
                        for (id, title) in ribbon.all_day.clone() {
                            button { key: "{id}", class: "day-all-item", onclick: move |_| state.show_event(&id), "{title}" }
                        }
                    }
                }
                // One line for the day. A place with three names under each other is a little higher.
                div { class: "day-track", "data-rows": "{ribbon.rows}", "data-first": "{ribbon.first_hour}", "data-last": "{ribbon.last_hour}", style: "height: {66 + ribbon.rows.saturating_sub(2) * 18}px;",
                    div { class: "day-rail" }
                    for e in ribbon.events.clone() {
                        button {
                            key: "{e.id}",
                            class: format!(
                                "day-event{}{}{}",
                                if e.at_end { " at-end" } else { "" },
                                if e.together { " together" } else { "" },
                                if e.named { "" } else { " unnamed" }
                            ),
                            "data-event": "{e.id}",
                            "data-row": "{e.row}",
                            // Its box is the place its name has; its bar is at its own time in it.
                            style: "left: {e.box_left}%; width: {e.room}%; --bar-at: {((e.left - e.box_left) / e.room.max(0.1) * 100.0).clamp(0.0, 100.0)}%; --bar: {(e.width / e.room.max(0.1) * 100.0).min(100.0)}%; --row: {e.row};",
                            title: "{e.title}, {e.when}",
                            onclick: {
                                let id = e.id.clone();
                                move |_| state.show_event(&id)
                            },
                            span { class: "day-bar" }
                            if e.named {
                                span { class: "day-label",
                                    span { class: "day-name", "{e.title}" }
                                    span { class: "day-when", "{e.when}" }
                                    if e.more > 0 {
                                        span { class: "day-more", "+{e.more} more" }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(at) = ribbon.now {
                        div { class: "day-now", style: "left: {at}%;", span { "Now" } }
                    }
                    if ribbon.events.is_empty() {
                        div { class: "day-none", "Nothing on the calendar today" }
                    }
                }
                div { class: "day-hours", "aria-hidden": "true",
                    for h in hours {
                        span { "{h}" }
                    }
                }
            }
            div { class: "today-grid",
                div { class: "today-wide",
                    div { class: "today-label", "Pick up where you stopped" }
                    if no_pages {
                        div { class: "home-empty", "No pages in this space yet." }
                    }
                    div { class: "today-cards",
                        for (p, snippet) in cards {
                            {
                                let (_, color) = p.icon.as_deref().map(parse_icon).unwrap_or_else(|| ("file".into(), "gray".into()));
                                let id = p.id.clone();
                                rsx! {
                                    button {
                                        key: "{p.id}",
                                        class: "today-card",
                                        "data-page": "{p.id}",
                                        style: "--c: var(--icon-{color});",
                                        onclick: move |_| state.open_page(&id),
                                        span { class: "today-card-strip" }
                                        span { class: "today-card-title", "{p.display_title()}" }
                                        span { class: "today-card-text", "{snippet}" }
                                        span { class: "today-card-meta", "Edited {crate::when::ago(p.updated_at)}" }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "side-card", id: "home-todos",
                    div { class: "side-card-head",
                        span { "To do" }
                        if todo_count > first_todos.len() {
                            button { class: "ghost-btn small", id: "todos-all", onclick: move |_| state.set_modal(Some(crate::state::Modal::Todos)), "All {todo_count}" }
                        }
                    }
                    if first_todos.is_empty() {
                        div { class: "home-empty", "Nothing to do. To-dos from all your pages show here." }
                    }
                    for t in first_todos {
                        TodoRow { key: "{t.block}", todo: t }
                    }
                }
                if let Some(m) = meeting {
                    div { class: "today-wide", id: "last-meeting",
                        div { class: "today-label", "From the last meeting" }
                        div { class: "meeting-card",
                            div { class: "meeting-card-head",
                                button {
                                    class: "meeting-card-title",
                                    onclick: {
                                        let id = m.id.clone();
                                        move |_| state.open_page(&id)
                                    },
                                    "{m.display_title()}"
                                }
                                span { class: "meeting-card-meta", "{crate::when::ago(m.updated_at)}" }
                            }
                            div { class: "meeting-card-todos",
                                if meeting_todos.is_empty() {
                                    div { class: "home-empty", "Nothing left to do from it." }
                                }
                                for t in meeting_todos {
                                    TodoRow { key: "m-{t.block}", todo: t }
                                }
                            }
                        }
                    }
                }
                div { class: "side-card",
                    div { class: "side-card-head",
                        span { "Coming up" }
                        button { class: "ghost-btn small", onclick: move |_| state.navigate(Route::Calendar), "Open calendar" }
                    }
                    if upcoming.is_empty() {
                        div { class: "home-empty", "Nothing in the next two weeks. Click a time in the calendar to add something." }
                    }
                    for (e, mark) in upcoming {
                        div {
                            key: "{e.id}",
                            class: "up-row",
                            style: "--c: var(--icon-{e.color});",
                            onclick: {
                                let id = e.id;
                                move |_| state.show_event(&id)
                            },
                            span { class: "up-bar" }
                            div {
                                div { class: "up-title", "{e.display_title()}" }
                                div { class: "up-when",
                                    if e.all_day {
                                        "{crate::when::day_label(crate::when::date_of(e.start))} · all day"
                                    } else {
                                        "{crate::when::day_label(crate::when::date_of(e.start))} · {crate::when::time_label(e.start)}"
                                    }
                                }
                            }
                            if let Some(sp) = mark {
                                super::calendar::SpaceMark { space: sp }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn PageView(page_id: String) -> Element {
    let state = use_context::<AppState>();
    let Some(page) = state.pages.read().get(&page_id).cloned() else {
        return rsx! { div { class: "empty", "This page no longer exists." } };
    };
    let blocks = state.blocks.read().get(&page_id).cloned().unwrap_or_default();
    let slash = state.slash.read().clone().filter(|s| s.page_id == page_id);
    let ui = state.ui.read();
    let block_menu = match &ui.popover {
        Some(Popover::BlockMenu { block_id }) => Some(block_id.clone()),
        _ => None,
    };
    let due_menu = match &ui.popover {
        Some(Popover::Due { block_id }) => Some(block_id.clone()),
        _ => None,
    };
    let icon_picker = matches!(&ui.popover, Some(Popover::IconPicker { page_id: p }) if *p == page_id);
    let cover_picker = matches!(&ui.popover, Some(Popover::CoverPicker { page_id: p }) if *p == page_id);
    drop(ui);

    let visible = visibility(&blocks);
    let numbers = list_numbers(&blocks);
    // The blocks picked as a whole on this page.
    let picked: Vec<Id> = state.picked.read().as_ref().filter(|p| p.page == page_id).map(|p| p.ids.clone()).unwrap_or_default();
    let backlinks = crate::state::linked_from(&state.pages.read(), &state.blocks.read(), &page_id);
    // Children of a toggle that was just opened ease in, one after another.
    let reveal = state.reveal.read().clone().filter(|(p, _)| *p == page_id).and_then(|(_, t)| {
        let i = index_of(&blocks, &t)?;
        Some((i + 1, subtree_end(&blocks, i)))
    });
    let sides = side_lines(&blocks);
    let mut appear_n = 0usize;
    let rows: Vec<(Block, Option<usize>, bool, Option<usize>, (u8, bool))> = blocks
        .iter()
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(i, b)| {
            let appear = reveal.filter(|(a, z)| (*a..*z).contains(&i)).map(|_| {
                appear_n += 1;
                appear_n - 1
            });
            (b.clone(), numbers[i], has_children(&blocks, i), appear, sides[i])
        })
        .collect();

    let pid = page_id.clone();
    let title_id = format!("title-{page_id}");

    let has_cover = page.cover.is_some();
    // The line under the title: when the page was changed, how long it
    // takes to read, how many pages link to it. A page with nothing written
    // has no such line.
    let words: usize = blocks.iter().map(|b| b.plain_text().split_whitespace().count()).sum();
    let read = minutes_to_read(words);
    // "Edited today, 9:12 AM": the day's name is in the middle of a line here.
    let edited = {
        let at = crate::when::moment(state.edited_at(&page));
        match at.strip_prefix("Today").map(|rest| ("today", rest)).or_else(|| at.strip_prefix("Yesterday").map(|rest| ("yesterday", rest))) {
            Some((day, rest)) => format!("{day}{rest}"),
            None => at,
        }
    };
    let linked = backlinks.len();
    // The headings, for the outline at the right edge (two or more).
    let outline: Vec<(Id, u8, String)> = blocks
        .iter()
        .filter_map(|b| {
            let level = match b.kind {
                BlockKind::Heading1 => 1,
                BlockKind::Heading2 => 2,
                BlockKind::Heading3 => 3,
                _ => return None,
            };
            let name = b.plain_text().trim().to_string();
            (!name.is_empty()).then(|| (b.id.clone(), level, name))
        })
        .collect();
    // A meeting's page: who spoke when, from its transcript lines.
    let voices = crate::listen::voices(&crate::listen::spoken_in(&blocks));
    let page_class = format!(
        "page{}{}",
        if has_cover { " has-cover" } else { "" },
        match page.font.as_str() {
            "serif" => " font-serif",
            "mono" => " font-mono",
            _ => "",
        }
    );
    rsx! {
        if has_cover {
            PageCover {
                page_id: page_id.clone(),
                file: page.cover_file().map(String::from),
                css: page.cover_css().map(String::from),
                y: page.cover_y,
                picking: cover_picker,
            }
        }
        if outline.len() >= 2 {
            nav { class: "page-outline", "aria-label": "On this page",
                div { class: "page-outline-list",
                    for (id, level, name) in outline {
                        button { key: "{id}", class: "outline-item", "data-outline": "{id}", "data-level": "{level}", title: "{name}",
                            span { class: "outline-name", "{name}" }
                            span { class: "outline-bar" }
                        }
                    }
                }
            }
        }
        div { class: "{page_class}",
            div { class: "page-header",
                if page.icon.is_some() {
                    div {
                        class: "page-icon",
                        title: "Change icon",
                        onclick: {
                            let pid = pid.clone();
                            move |_| state.set_popover(Some(Popover::IconPicker { page_id: pid.clone() }))
                        },
                        PageIcon { icon: page.icon.clone(), large: true }
                    }
                }
                div { class: "page-controls",
                    if page.icon.is_none() {
                        button {
                            class: "ghost-btn",
                            onclick: {
                                let pid = pid.clone();
                                move |_| state.set_popover(Some(Popover::IconPicker { page_id: pid.clone() }))
                            },
                            Icon { name: "smile", size: 15 }
                            span { "Add icon" }
                        }
                    }
                    if !has_cover {
                        button {
                            class: "ghost-btn",
                            id: "add-cover",
                            onclick: {
                                let pid = pid.clone();
                                move |_| {
                                    // One of the app's own to start with; the covers open to change it.
                                    let presets = crate::model::covers::PRESETS;
                                    let pick = pid.bytes().map(usize::from).sum::<usize>() % 10;
                                    state.set_cover(&pid, Some(format!("preset:{}", presets[pick].0)));
                                    state.set_popover(Some(Popover::CoverPicker { page_id: pid.clone() }));
                                }
                            },
                            Icon { name: "image", size: 15 }
                            span { "Add cover" }
                        }
                    }
                    // The face of the title and the headings.
                    div { class: "face-pick", role: "group", "aria-label": "Title style",
                        for (font, name) in PAGE_FONTS {
                            button {
                                key: "{font}",
                                class: if page.font == font { "face on" } else { "face" },
                                "data-face": "{font}",
                                title: "{name} title",
                                "aria-label": "{name} title",
                                "aria-pressed": if page.font == font { "true" } else { "false" },
                                onclick: {
                                    let pid = pid.clone();
                                    move |_| state.set_page_font(&pid, font)
                                },
                                "Aa"
                            }
                        }
                    }
                }
                if icon_picker {
                    IconPicker { page_id: page_id.clone() }
                }
                textarea {
                    id: "{title_id}",
                    class: "title-input",
                    rows: "1",
                    placeholder: "New page",
                    spellcheck: false,
                    initial_value: "{page.title}",
                    oninput: {
                        let pid = pid.clone();
                        move |e: FormEvent| {
                            let value = e.value();
                            if value.contains('\n') {
                                let clean = value.replace('\n', "");
                                state.rename_page(&pid, clean.clone());
                                let js = format!(
                                    "(function(){{const t=document.getElementById({});if(t)t.value={};}})()",
                                    serde_json::to_string(&format!("title-{pid}")).unwrap_or_default(),
                                    serde_json::to_string(&clean).unwrap_or_default()
                                );
                                document::eval(&js);
                                state.title_enter(&pid);
                            } else {
                                state.rename_page(&pid, value);
                            }
                        }
                    },
                    onkeydown: {
                        let pid = pid.clone();
                        move |e: KeyboardEvent| {
                            if e.key() == Key::Enter || e.key() == Key::ArrowDown {
                                e.prevent_default();
                                state.title_enter(&pid);
                            }
                        }
                    },
                }
                if page.row_of.is_some() {
                    super::tables::RowValues { page_id: page_id.clone() }
                }
                if words > 0 {
                    div { class: "page-meta",
                        span { class: "meta-edited", "Edited {edited}" }
                        if let Some(minutes) = read {
                            span { class: "meta-read", "{minutes} min read" }
                        }
                        if linked > 0 {
                            span { class: "meta-links", if linked == 1 { "Linked from 1 page" } else { "Linked from {linked} pages" } }
                        }
                    }
                }
            }
            if let Some(v) = voices {
                MeetingBand { page_id: page_id.clone(), voices: v }
            }
            div { class: "blocks",
                for (block, number, kids, appear, (side, side_last)) in rows {
                    BlockRow {
                        key: "{block.id}",
                        appear,
                        side,
                        side_last,
                        page_id: page_id.clone(),
                        slash: slash.clone().filter(|s| s.block_id == block.id),
                        menu_open: block_menu.as_deref() == Some(block.id.as_str()),
                        due_open: due_menu.as_deref() == Some(block.id.as_str()),
                        picked: picked.contains(&block.id),
                        number,
                        has_children: kids,
                        block,
                    }
                }
            }
            // The pages that link to this one.
            if !backlinks.is_empty() {
                div { class: "backlinks", role: "navigation", "aria-label": "Pages that link to this page",
                    span { class: "backlinks-label", "Linked from" }
                    for p in backlinks {
                        button {
                            key: "{p.id}",
                            class: "backlink",
                            "data-backlink": "{p.id}",
                            onclick: {
                                let id = p.id.clone();
                                move |_| state.open_page(&id)
                            },
                            PageIcon { icon: p.icon.clone(), size: 14 }
                            span { "{p.display_title()}" }
                        }
                    }
                }
            }
            div {
                class: "page-tail",
                onclick: move |_| state.click_page_tail(&pid),
            }
        }
    }
}

/// The room between two tables that stand in one line.
const SIDE_GAP: usize = 44;

/// For each block: how many tables stand in its line (0 for a block that
/// has the line to itself), and whether it is the last of them. A table
/// with `beside` stands in the line of the table above it (three at most).
fn side_lines(blocks: &[Block]) -> Vec<(u8, bool)> {
    let mut out = vec![(0, false); blocks.len()];
    let mut i = 0;
    while i < blocks.len() {
        let mut end = i + 1;
        if blocks[i].kind == BlockKind::Table {
            while end < blocks.len() && end - i < 3 && blocks[end].kind == BlockKind::Table && blocks[end].place.beside {
                end += 1;
            }
        }
        if end - i > 1 {
            for (n, slot) in out[i..end].iter_mut().enumerate() {
                *slot = ((end - i) as u8, n + 1 == end - i);
            }
        }
        i = end;
    }
    out
}

fn bullet_for(indent: u8) -> &'static str {
    match indent % 3 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

fn number_label(n: usize, indent: u8) -> String {
    match indent % 3 {
        1 => {
            let letter = (b'a' + ((n - 1) % 26) as u8) as char;
            format!("{letter}.")
        }
        2 => format!("{}.", roman(n)),
        _ => format!("{n}."),
    }
}

fn roman(mut n: usize) -> String {
    let table = [(10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut out = String::new();
    for (value, s) in table {
        while n >= value {
            out.push_str(s);
            n -= value;
        }
    }
    out
}

const CODE_LANGUAGES: &[&str] = &[
    "Plain text", "Rust", "JavaScript", "TypeScript", "Python", "Go", "Swift", "Java", "C", "C++", "SQL",
    "Bash", "JSON", "YAML", "HTML", "CSS", "Markdown", "Mermaid",
];

/// A code block of this language is drawn as a diagram (see `drawDiagrams`
/// in bridge.js, and assets/mermaid).
fn is_diagram(language: Option<&str>) -> bool {
    language.is_some_and(|l| l.trim().eq_ignore_ascii_case("mermaid"))
}

#[component]
fn BlockRow(
    page_id: String,
    block: Block,
    number: Option<usize>,
    has_children: bool,
    slash: Option<SlashState>,
    menu_open: bool,
    #[props(default)] due_open: bool,
    #[props(default)] picked: bool,
    #[props(!optional)] appear: Option<usize>,
    /// How many tables stand in this block's line (0: it has the line to
    /// itself), and whether it is the last of them.
    #[props(default)]
    side: u8,
    #[props(default)] side_last: bool,
) -> Element {
    let state = use_context::<AppState>();
    // A diagram shows its code only while it is edited (an empty one always does).
    let mut diagram_code = use_signal(|| false);
    let id = block.id.clone();
    let kind = block.kind;
    // A free picture is at a side of the page; the lines after it run beside it.
    let free = block.floats() && crate::files::valid_name(&block.data);
    // A line that is only a YouTube link shows the video under it.
    let video = crate::video::in_block(kind, &block.text).filter(|_| block.data != crate::video::HIDDEN);
    let class = format!(
        "block-row kind-{}{}{}{}{}{}{}",
        kind.as_str(),
        if side > 0 { " side" } else { "" },
        if block.checked { " checked" } else { "" },
        if menu_open { " menu-open" } else { "" },
        if appear.is_some() { " appear" } else { "" },
        if picked { " picked" } else { "" },
        match (free, block.place.right) {
            (false, _) => "",
            (true, false) => " free left",
            (true, true) => " free right",
        }
    );
    let appear_i = appear.unwrap_or(0);
    // The style always names the same values: the framework keeps an inline
    // value that a new style no longer names, so a row that was a free
    // picture would keep its width and margins when it is back in line.
    let row_style = if free {
        // A free picture is never wider than its column, and never pushed
        // out of it at the side it is at (a narrow pane, a small window).
        // At its other side it leaves room: for the "+" and the handle of
        // the lines beside it at the left, a little air at the right.
        let (x, w) = (block.place.x, block.place.w.max(Place::NARROWEST));
        let at_side = format!("min({x}px, max(0px, 100% - {w}px))");
        let (left, right) = if block.place.right { ("24px".to_string(), at_side) } else { (at_side, "52px".to_string()) };
        format!("margin-left: {left}; margin-right: {right}; width: {w}px; padding-left: 0px; --i: {appear_i};")
    } else {
        // Nesting is room inside the row (not a margin), so that it still
        // shows when the row sits beside a picture.
        // Tables in one line share its width, with a gap between them.
        let (width, gap) = match side {
            0 => ("auto".to_string(), 0),
            n => (format!("calc((100% - {}px) / {n})", (n as usize - 1) * SIDE_GAP), if side_last { 0 } else { SIDE_GAP }),
        };
        format!("margin-left: 0px; margin-right: {gap}px; width: {width}; padding-left: {}px; --i: {appear_i};", block.indent as usize * 26)
    };
    let menu_flag = if slash.is_some() { "1" } else { "0" };
    let placeholder = if kind == BlockKind::Toggle && has_children { "" } else { kind.placeholder() };

    // A rich-text editor. Its contents are owned by assets/bridge.js: it fills
    // the element from `data-html` when the element is created, and Rust pushes
    // later changes made by code (see AppState::sync_blocks).
    let html = inline::to_html(kind, &block.text);
    let editor = rsx! {
        div {
            id: "blk-{id}",
            class: "blk",
            contenteditable: if kind == BlockKind::Code { "plaintext-only" } else { "true" },
            role: "textbox",
            "aria-multiline": "true",
            "data-id": "{id}",
            "data-page": "{page_id}",
            "data-kind": "{kind.as_str()}",
            "data-menu": "{menu_flag}",
            "data-placeholder": "{placeholder}",
            "data-html": "{html}",
            spellcheck: kind != BlockKind::Code,
        }
    };

    let (p1, b1) = (page_id.clone(), id.clone());
    let b2 = id.clone();
    let (p3, b3) = (page_id.clone(), id.clone());
    let (p4, b4) = (page_id.clone(), id.clone());
    let (p5, b5) = (page_id.clone(), id.clone());

    rsx! {
        div {
            class: "{class}",
            "data-id": "{id}",
            "data-page": "{page_id}",
            style: "{row_style}",
            div { class: "block-inner",
                div { class: "gutter",
                    button {
                        class: "g-btn",
                        title: "Click to add a block below",
                        onclick: move |_| state.add_below_with_menu(&p1, &b1),
                        Icon { name: "plus", size: 16 }
                    }
                    div {
                        class: "g-btn handle",
                        draggable: "true",
                        "data-drag": "{id}",
                        title: "Drag to move · Click for options",
                        onclick: move |_| {
                            let next = (!menu_open).then(|| Popover::BlockMenu { block_id: b2.clone() });
                            state.set_popover(next);
                        },
                        Icon { name: "grip", size: 16 }
                    }
                }
                match kind {
                    BlockKind::Bulleted => rsx! { span { class: "marker bullet", "{bullet_for(block.indent)}" } },
                    BlockKind::Numbered => rsx! { span { class: "marker number", "{number_label(number.unwrap_or(1), block.indent)}" } },
                    BlockKind::Todo => rsx! {
                        div {
                            class: "marker",
                            div {
                                class: if block.checked { "check on" } else { "check" },
                                onclick: move |_| state.toggle_check(&p3, &b3),
                                if block.checked { Icon { name: "check", size: 12 } }
                            }
                        }
                    },
                    BlockKind::Toggle => rsx! {
                        div { class: "marker",
                            button {
                                class: if block.collapsed { "toggle-arrow closed" } else { "toggle-arrow" },
                                onclick: move |_| state.toggle_collapsed(&p4, &b4),
                                Icon { name: "triangle-right", size: 12 }
                            }
                        }
                    },
                    BlockKind::Callout => rsx! {
                        button {
                            class: "callout-icon",
                            title: "Change icon",
                            onclick: move |_| state.cycle_callout_icon(&p5, &b5),
                            PageIcon { icon: Some(block.icon.clone().unwrap_or_else(|| CALLOUT_ICON.into())), size: 20 }
                        }
                    },
                    _ => rsx! {},
                }
                match kind {
                    BlockKind::Divider => rsx! { div { class: "divider", hr {} } },
                    BlockKind::PageLink => rsx! { PageLinkView { target: block.target.clone() } },
                    BlockKind::Drawing => rsx! { DrawingView { page_id: page_id.clone(), id: id.clone(), data: block.data.clone() } },
                    BlockKind::Image => rsx! { ImageView { page_id: page_id.clone(), id: id.clone(), name: block.data.clone(), place: block.place, caption: block.text.clone() } },
                    BlockKind::Table => rsx! { super::tables::TableView { page_id: page_id.clone(), id: id.clone(), data: block.data.clone() } },
                    BlockKind::Chart => rsx! { super::charts::ChartView { page_id: page_id.clone(), id: id.clone(), data: block.data.clone() } },
                    BlockKind::Code if is_diagram(block.language.as_deref()) => {
                        let empty = block.text.trim().is_empty();
                        let coding = diagram_code() || empty;
                        let source = block.text.clone();
                        rsx! {
                            div { class: if coding { "code-wrap diagram-wrap coding" } else { "code-wrap diagram-wrap" },
                                div { class: "diagram-tools",
                                    select {
                                        class: "code-lang",
                                        onchange: {
                                            let (pid, bid) = (page_id.clone(), id.clone());
                                            move |e: FormEvent| {
                                                let v = e.value();
                                                state.set_code_language(&pid, &bid, if v == "Plain text" { String::new() } else { v });
                                            }
                                        },
                                        for lang in CODE_LANGUAGES {
                                            option { value: "{lang}", selected: *lang == "Mermaid", "{lang}" }
                                        }
                                    }
                                    if !empty {
                                        button {
                                            class: "ghost-btn small diagram-toggle",
                                            "data-diagram-toggle": "",
                                            title: if coding { "Show the diagram" } else { "Change the diagram's code" },
                                            onclick: move |e| {
                                                e.stop_propagation();
                                                diagram_code.set(!diagram_code());
                                            },
                                            if coding { "Done" } else { "Edit" }
                                        }
                                    }
                                }
                                if !empty {
                                    div { class: "diagram", "data-diagram": "{source}", "aria-label": "Diagram", role: "img" }
                                }
                                div { class: "diagram-code", {editor} }
                            }
                        }
                    }
                    BlockKind::Code => rsx! {
                        div { class: "code-wrap",
                            select {
                                class: "code-lang",
                                onchange: {
                                    let (pid, bid) = (page_id.clone(), id.clone());
                                    move |e: FormEvent| {
                                        let v = e.value();
                                        state.set_code_language(&pid, &bid, if v == "Plain text" { String::new() } else { v });
                                    }
                                },
                                for lang in CODE_LANGUAGES {
                                    option {
                                        value: "{lang}",
                                        selected: block.language.as_deref().unwrap_or("Plain text").eq_ignore_ascii_case(lang),
                                        "{lang}"
                                    }
                                }
                            }
                            {editor}
                        }
                    },
                    BlockKind::Todo => rsx! {
                        {editor}
                        DueChip { page_id: page_id.clone(), id: id.clone(), due: block.due, remind: block.remind, done: block.checked, open: due_open }
                    },
                    _ => editor,
                }
            }
            if let Some(video) = video {
                for v in std::iter::once(video) {
                    VideoView { key: "{v.id}:{v.start}", id: v.id.clone(), start: v.start, page_id: page_id.clone(), block_id: id.clone() }
                }
            }
            if menu_open {
                BlockMenu { page_id: page_id.clone(), block: block }
            }
            if let Some(s) = slash {
                SlashMenu { slash: s }
            }
        }
    }
}

/// A YouTube video under the line that links to it: its still picture and a
/// play button. A click plays it in the small player at the corner of the
/// window (the page itself cannot hold YouTube's player: see player.rs).
#[component]
fn VideoView(id: String, start: u32, page_id: String, block_id: String) -> Element {
    let state = use_context::<AppState>();
    let video = crate::video::Video { id: id.clone(), start };
    let still = crate::video::thumbnail(&id);
    let style = if still.is_empty() { "background-image: none;".to_string() } else { format!("background-image: url('{still}');") };
    rsx! {
        div { class: "video-card",
            button {
                class: "video-embed",
                "data-video": "{id}",
                style: "{style}",
                title: "Play the video here",
                "aria-label": "Play the video here",
                "data-watch": "{crate::video::watch_url(&video)}",
                onclick: {
                    let (page, line) = (page_id.clone(), block_id.clone());
                    move |_| state.play_video_from(video.clone(), &page, &line)
                },
                span { class: "video-play", Icon { name: "triangle-right", size: 22 } }
            }
            // The picture can go; the link stays. (The menu of the line brings it back.)
            button {
                class: "video-hide",
                title: "Remove the picture. The link stays.",
                "aria-label": "Remove the picture of the video",
                onclick: move |_| state.show_video_under(&page_id, &block_id, false),
                Icon { name: "close", size: 14 }
            }
        }
    }
}

/// A picture in the page. Until one is chosen it is a quiet box to choose one
/// with (a paste or a drop on it works as well). A click shows the picture
/// as large as the window; a drag puts it anywhere on the page, free of the
/// lines; the corner makes it wider or narrower (see the page script).
#[component]
fn ImageView(page_id: String, id: String, name: String, place: Place, caption: String) -> Element {
    let state = use_context::<AppState>();
    // The caption's line shows when it has words, or was asked for.
    let mut captioning = use_signal(|| false);
    if !crate::files::valid_name(&name) {
        return rsx! {
            button {
                class: "image-empty",
                "data-image-empty": "{id}",
                "data-page": "{page_id}",
                onclick: move |_| state.pick_image(&page_id, &id),
                Icon { name: "image", size: 16 }
                span { "Choose a picture, or paste or drop one here" }
            }
        };
    }
    let src = crate::files::url(&name);
    let class = match (place.free, place.w > 0) {
        (true, _) => "image-block sized free",
        (false, true) => "image-block sized",
        (false, false) => "image-block",
    };
    // A free picture fills its row, which has the width. (The width is
    // always named: see the row's style.)
    let style = match (place.free, place.w) {
        (true, _) => "width: 100%;".to_string(),
        (false, 0) => "width: fit-content;".to_string(),
        (false, w) => format!("width: {w}px;"),
    };
    let side = match (place.free, place.right) {
        (false, _) => "line",
        (true, false) => "left",
        (true, true) => "right",
    };
    let (p1, b1) = (page_id.clone(), id.clone());
    let (p2, b2) = (page_id.clone(), id.clone());
    let (p3, b3) = (page_id.clone(), id.clone());
    let caption_id = format!("cap-{id}");
    let focus_caption = format!(
        "requestAnimationFrame(()=>requestAnimationFrame(()=>{{const c=document.getElementById({});if(c)c.focus();}}))",
        serde_json::to_string(&caption_id).unwrap_or_default()
    );
    let has_caption = !caption.trim().is_empty();
    rsx! {
        div {
            class: "{class}",
            style: "{style}",
            "data-image": "{id}",
            "data-page": "{page_id}",
            "data-place": "{place.free},{side},{place.x},{place.w}",
            img {
                src: "{src}",
                alt: "",
                draggable: "false",
                "loading": "lazy",
                title: "Click to see it large · Drag to put it anywhere",
            }
            // Every picture can be deleted here; a free one can go back in line.
            div { class: "image-tools",
                if place.free {
                    button {
                        class: "image-tool",
                        "data-image-op": "in-line",
                        title: "Put back in line",
                        onclick: move |_| state.image_in_line(&p1, &b1),
                        Icon { name: "text", size: 14 }
                    }
                }
                if !has_caption {
                    button {
                        class: "image-tool",
                        "data-image-op": "caption",
                        title: "Add a caption",
                        "aria-label": "Add a caption",
                        onclick: move |_| {
                            captioning.set(true);
                            document::eval(&focus_caption);
                        },
                        Icon { name: "quote", size: 14 }
                    }
                }
                button {
                    class: "image-tool",
                    "data-image-op": "delete",
                    title: "Delete",
                    onclick: move |_| state.delete_image(&p2, &b2),
                    Icon { name: "trash", size: 14 }
                }
            }
            span { class: "image-size", title: "Drag to change the size" }
        }
        // Under the picture, outside its box: the box is what is dragged and sized.
        if has_caption || captioning() {
            input {
                id: "{caption_id}",
                class: "image-caption",
                r#type: "text",
                placeholder: "Caption",
                "aria-label": "Caption of the picture",
                spellcheck: true,
                value: "{caption}",
                oninput: move |e: FormEvent| state.set_caption(&p3, &b3, &e.value()),
                onblur: move |_| captioning.set(false),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter || e.key() == Key::Escape {
                        e.prevent_default();
                        document::eval("document.activeElement && document.activeElement.blur()");
                    }
                },
            }
        }
    }
}

/// The band across the top of a page: one of the app's own colours or
/// gradients, or a picture of the person's. "Change" opens the covers to
/// choose from; a picture can be dragged up or down to show another part.
#[component]
fn PageCover(page_id: String, file: Option<String>, css: Option<String>, y: u8, picking: bool) -> Element {
    let state = use_context::<AppState>();
    let mut moving = use_signal(|| false);
    let paint = match (&file, &css) {
        (None, Some(css)) => format!("background: {css}"),
        _ => String::new(),
    };
    let has_picture = file.is_some();
    let pid = page_id.clone();
    rsx! {
        div { class: if moving() { "page-cover moving" } else { "page-cover" }, "data-cover": "{page_id}", style: "{paint}",
            if let Some(name) = &file {
                img { src: "{crate::files::url(name)}", alt: "", draggable: "false", style: "object-position: 50% {y}%" }
            }
            div { class: "cover-tools", onclick: move |e| e.stop_propagation(),
                if moving() {
                    span { class: "cover-hint", "Drag the picture up or down" }
                    button { class: "cover-btn", id: "cover-done", onclick: move |_| moving.set(false), "Done" }
                } else {
                    button {
                        class: "cover-btn",
                        id: "cover-change",
                        onclick: move |_| state.set_popover((!picking).then(|| Popover::CoverPicker { page_id: pid.clone() })),
                        "Change"
                    }
                    if has_picture {
                        button { class: "cover-btn", id: "cover-move", onclick: move |_| moving.set(true), "Reposition" }
                    }
                }
            }
            if picking {
                CoverPicker { page_id: page_id.clone() }
            }
        }
    }
}

/// The covers to choose from: the app's own, a picture file, or none.
#[component]
fn CoverPicker(page_id: String) -> Element {
    let state = use_context::<AppState>();
    let current = state.pages.read().get(&page_id).and_then(|p| p.cover.clone()).unwrap_or_default();
    let (remove_id, upload_id) = (page_id.clone(), page_id.clone());
    rsx! {
        div { class: "popover cover-picker", onclick: move |e| e.stop_propagation(),
            div { class: "picker-head",
                span { "Choose a cover" }
                button {
                    class: "ghost-btn",
                    id: "cover-remove",
                    onclick: move |_| {
                        state.set_cover(&remove_id, None);
                        state.set_popover(None);
                    },
                    "Remove"
                }
            }
            div { class: "cover-grid",
                for (name, label, css) in crate::model::covers::PRESETS {
                    button {
                        key: "{name}",
                        class: if current == format!("preset:{name}") { "cover-swatch on" } else { "cover-swatch" },
                        title: "{label}",
                        "aria-label": "{label}",
                        "data-cover-preset": "{name}",
                        style: "background: {css}",
                        onclick: {
                            let pid = page_id.clone();
                            move |_| state.set_cover(&pid, Some(format!("preset:{name}")))
                        },
                    }
                }
            }
            button { class: "secondary-btn with-icon cover-upload", id: "cover-upload", onclick: move |_| state.pick_cover(&upload_id),
                Icon { name: "image", size: 15 }
                span { "Use a picture of yours…" }
            }
        }
    }
}

/// An Excalidraw canvas in its own frame. Click it to start drawing (until
/// then the page scrolls past it); the button at its top right expands it.
#[component]
fn DrawingView(page_id: String, id: String, data: String) -> Element {
    let state = use_context::<AppState>();
    let window = state.win.key;
    let src = crate::drawing::frame_url(&id);
    let shown_id = id.clone();
    // Send the scene to the frame when this window hasn't shown it yet: on
    // first sight, and when it was changed in another window or by an agent.
    use_effect(use_reactive!(|(shown_id, data)| {
        if crate::drawing::needs_sending(window, &shown_id, &data) {
            document::eval(&crate::drawing::load_script(&shown_id, &data));
        }
    }));
    rsx! {
        div { class: "drawing", "data-drawing": "{id}", "data-page": "{page_id}",
            iframe { class: "drawing-frame", src: "{src}", title: "Drawing", allow: "clipboard-read; clipboard-write", "loading": "lazy" }
            div { class: "drawing-cover", span { "Click to draw" } }
        }
    }
}

#[component]
fn PageLinkView(target: Option<String>) -> Element {
    let state = use_context::<AppState>();
    let pages = state.pages.read();
    let page = target.as_ref().and_then(|t| pages.get(t)).filter(|p| !p.trashed).cloned();
    drop(pages);
    match page {
        Some(p) => {
            let id = p.id.clone();
            rsx! {
                div { class: "page-link", onclick: move |_| state.open_page(&id),
                    span { class: "pl-icon", PageIcon { icon: p.icon.clone(), size: 18 } }
                    span { class: "pl-title", "{p.display_title()}" }
                }
            }
        }
        None => rsx! {
            div { class: "page-link missing",
                span { class: "pl-icon", Icon { name: "trash", size: 18 } }
                span { class: "pl-title", "Page moved to Trash" }
            }
        },
    }
}

#[component]
fn SlashMenu(slash: SlashState) -> Element {
    let state = use_context::<AppState>();
    // After "@" or "[[": the pages to link to.
    if slash.pages {
        let (found, create) = state.mention_items(&slash);
        let rows: Vec<(usize, Page, String)> = found.into_iter().enumerate().map(|(i, p)| {
            let path = state.page_path(&p.id);
            (i, p, path)
        }).collect();
        let make_at = rows.len();
        return rsx! {
            div {
                class: "popover slash-menu page-links",
                onmousedown: move |e| e.prevent_default(),
                if rows.is_empty() && create.is_none() {
                    div { class: "menu-empty", "No page with that name" }
                }
                for (i, p, path) in rows {
                    div {
                        key: "{p.id}",
                        class: if i == slash.selected { "menu-item compact selected" } else { "menu-item compact" },
                        "data-link-page": "{p.id}",
                        onclick: move |_| state.apply_mention(i),
                        span { class: "menu-icon", PageIcon { icon: p.icon.clone(), size: 16 } }
                        span { class: "link-title", "{p.display_title()}" }
                        if !path.is_empty() {
                            span { class: "link-path", "{path}" }
                        }
                    }
                }
                if let Some(name) = create {
                    div {
                        class: if make_at == slash.selected { "menu-item compact selected" } else { "menu-item compact" },
                        "data-link-new": "1",
                        onclick: move |_| state.apply_mention(make_at),
                        span { class: "menu-icon", Icon { name: "plus", size: 16 } }
                        span { class: "link-title", "New page “{name}”" }
                        span { class: "link-path", "inside this page" }
                    }
                }
            }
        };
    }
    let items = filter_slash(&slash.query);
    rsx! {
        div {
            class: "popover slash-menu",
            onmousedown: move |e| e.prevent_default(),
            if items.is_empty() {
                div { class: "menu-empty", "No results" }
            }
            for (i, item) in items.iter().enumerate() {
                if i == 0 || items[i - 1].section != item.section {
                    div { class: "menu-section", "{item.section}" }
                }
                div {
                    class: if i == slash.selected { "menu-item selected" } else { "menu-item" },
                    onclick: move |_| state.apply_slash(i),
                    div { class: "menu-icon", Icon { name: item.icon, size: 20 } }
                    div { class: "menu-text",
                        div { "{item.label}" }
                        div { class: "menu-hint", "{item.hint}" }
                    }
                }
            }
        }
    }
}

#[component]
fn BlockMenu(page_id: String, block: Block) -> Element {
    let state = use_context::<AppState>();
    let id = block.id.clone();
    let editable = block.kind.is_editable();
    let has_text = !block.plain_text().trim().is_empty();
    let ai_actions = [
        AiAction::Improve,
        AiAction::FixGrammar,
        AiAction::Shorter,
        AiAction::Longer,
        AiAction::Translate("English".into()),
    ];
    let (p1, b1) = (page_id.clone(), id.clone());
    let (p2, b2) = (page_id.clone(), id.clone());
    // A line whose video picture was put away can show it again.
    let video_hidden = block.data == crate::video::HIDDEN && crate::video::in_block(block.kind, &block.text).is_some();
    let (p3, b3) = (page_id.clone(), id.clone());

    rsx! {
        div { class: "popover block-menu", onclick: move |e| e.stop_propagation(),
            if video_hidden {
                div { class: "menu-item compact", id: "show-video", onclick: move |_| state.show_video_under(&p3, &b3, true),
                    span { class: "menu-icon", Icon { name: "triangle-right", size: 16 } }
                    span { "Show the video" }
                }
            }
            div { class: "menu-item compact danger", onclick: move |_| state.delete_block(&p1, &b1),
                span { class: "menu-icon", Icon { name: "trash", size: 16 } }
                span { "Delete" }
                span { class: "menu-shortcut", "Del" }
            }
            div { class: "menu-item compact", onclick: move |_| state.duplicate_block(&p2, &b2),
                span { class: "menu-icon", Icon { name: "duplicate", size: 16 } }
                span { "Duplicate" }
                span { class: "menu-shortcut", "⌘D" }
            }
            if editable {
                div { class: "menu-section", "Turn into" }
                for kind in BlockKind::TURN_INTO {
                    div {
                        class: "menu-item compact",
                        onclick: {
                            let (pid, bid) = (page_id.clone(), id.clone());
                            move |_| state.turn_into(&pid, &bid, kind)
                        },
                        span { class: "menu-icon", Icon { name: kind.icon(), size: 16 } }
                        span { "{kind.label()}" }
                        if kind == block.kind {
                            span { class: "menu-shortcut", Icon { name: "check", size: 14 } }
                        }
                    }
                }
            }
            if editable && has_text && *state.ai_on.read() {
                div { class: "menu-section", "AI" }
                for action in ai_actions {
                    div {
                        class: "menu-item compact",
                        onclick: {
                            let (pid, bid, action) = (page_id.clone(), id.clone(), action);
                            move |_| state.ai_open(action.clone(), &pid, &bid)
                        },
                        span { class: "menu-icon", Icon { name: "sparkles", size: 16 } }
                        span { "{action.label()}" }
                    }
                }
            }
        }
    }
}

#[component]
fn IconPicker(page_id: String) -> Element {
    let state = use_context::<AppState>();
    let current = state.pages.read().get(&page_id).and_then(|p| p.icon.clone());
    let (cur_name, cur_color) = current.as_deref().map(parse_icon).unwrap_or_else(|| ("file".into(), "gray".into()));
    let remove_id = page_id.clone();
    rsx! {
        div { class: "popover icon-picker", onclick: move |e| e.stop_propagation(),
            div { class: "picker-head",
                span { "Choose an icon" }
                button { class: "ghost-btn", onclick: move |_| state.set_page_icon(&remove_id, None), "Remove" }
            }
            div { class: "ip-colors",
                for (color, label) in ICON_COLORS {
                    button {
                        class: if *color == cur_color { "ip-swatch on" } else { "ip-swatch" },
                        title: "{label}",
                        style: "--c: var(--icon-{color})",
                        onclick: {
                            let (pid, name) = (page_id.clone(), cur_name.clone());
                            move |_| state.set_page_icon(&pid, Some(format_icon(&name, color)))
                        },
                    }
                }
            }
            div { class: "ip-grid",
                for name in PAGE_ICONS {
                    button {
                        class: if *name == cur_name { "ip-cell on" } else { "ip-cell" },
                        title: "{name}",
                        onclick: {
                            let (pid, color) = (page_id.clone(), cur_color.clone());
                            move |_| state.set_page_icon(&pid, Some(format_icon(name, &color)))
                        },
                        PageIcon { icon: Some(format_icon(name, &cur_color)), size: 20 }
                    }
                }
            }
        }
    }
}

/// The AI at the foot of a pane: a small button that opens into a bar. What
/// the AI answers opens above the bar, over the page (never in its text).
#[component]
pub fn AiDock(route: Route, duplicate: bool) -> Element {
    let state = use_context::<AppState>();
    // A second pane with the same page only says where the page is.
    let page = match &route {
        Route::Page(id) if !duplicate => Some(id.clone()),
        _ => None,
    };
    // Open: a bar to type in. With an answer: a card that grows upward.
    let (open, answer) = match (&page, state.ai.read().as_ref()) {
        (Some(p), Some(s)) if &s.page_id == p => (true, s.status != AiStatus::Prompting),
        _ => (false, false),
    };
    let class = match (open, answer) {
        (true, true) => "ai-dock open answer",
        (true, false) => "ai-dock open",
        _ => "ai-dock",
    };
    // No AI tool is connected: nothing of the AI shows.
    if !*state.ai_on.read() && !open {
        return rsx! {};
    }
    // What the AI did here a moment ago (sub-pages made), with a way back.
    let done = state.ai_done.read().clone().filter(|d| page.as_deref() == Some(d.page.as_str()) && !open);
    rsx! {
        if let Some(done) = done {
            div { class: "ai-done", id: "ai-done", role: "status",
                Icon { name: "check", size: 14 }
                span { "{done.text}" }
                button { class: "secondary-btn small", id: "ai-undo", onclick: move |_| state.ai_undo_done(), "Undo" }
            }
        }
        div { class: "{class}",
            if open {
                AiPanel {}
            } else {
                button {
                    class: "ai-dock-btn",
                    title: "Ask AI (⌘J)",
                    "aria-label": "Ask AI",
                    onclick: move |_| state.ai_shortcut(),
                    Icon { name: "sparkles", size: 15 }
                }
            }
        }
    }
}

/// A picture that goes with a request to the AI: small, with a press to
/// take it out again.
#[component]
pub fn AiShot(id: String, src: String) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        span { class: "ai-shot",
            img { src: "{src}", alt: "A picture for the AI" }
            button {
                class: "ai-shot-x",
                title: "Take this picture out",
                "aria-label": "Take this picture out",
                onclick: move |e| {
                    e.stop_propagation();
                    state.ai_remove_picture(&id);
                },
                Icon { name: "close", size: 12 }
            }
        }
    }
}

/// How long a run of the AI goes on, as a small clock that counts.
#[component]
fn Elapsed(since: i64) -> Element {
    let mut now = use_signal(crate::model::now_ms);
    use_future(move || async move {
        loop {
            crate::ai::sleep_ms(1000).await;
            now.set(crate::model::now_ms());
        }
    });
    let seconds = if since > 0 { ((*now.read() - since).max(0) / 1000) as u64 } else { 0 };
    rsx! {
        span { class: "ai-clock", "aria-label": "Time so far", "{seconds / 60}:{seconds % 60:02}" }
    }
}

#[component]
fn AiPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(session) = state.ai.read().clone() else { return rsx! {} };
    let rewrite = session.action.rewrites_block();
    let can_send = !session.prompt.trim().is_empty();
    let recap = session.action == AiAction::Recap;
    let placeholder = if recap { "Say or type what happened in the meeting…" } else { "Ask anything about this page…" };
    let send_label = if recap { "Write the recap" } else { "Ask" };
    // Once it is sent, a quiet line says what was asked.
    let caption = match (&session.action, session.prompt.trim()) {
        (AiAction::Ask, asked) if !asked.is_empty() => asked.to_string(),
        (action, _) => action.label(),
    };
    let caption = if crate::ai::split_revision(&crate::ai::clean_reply(&session.output).0).is_some() { format!("The page, changed · {caption}") } else { caption };
    let caption = match session.pictures.len() {
        0 => caption,
        1 => format!("{caption} · 1 picture"),
        n => format!("{caption} · {n} pictures"),
    };
    // Model picker: every model of every installed tool, grouped by tool.
    let groups = use_hook(|| {
        let installed = crate::ai::local::installed_cached().unwrap_or_default();
        crate::ai::Provider::ALL
            .into_iter()
            .filter(|p| installed.is_empty() || installed.iter().any(|i| i.provider == *p))
            .map(|p| (p, crate::ai::model_choices(p)))
            .collect::<Vec<_>>()
    });
    let mut picked = use_signal(|| {
        let prefs = crate::ai::Prefs::load();
        let installed = crate::ai::local::installed_cached().unwrap_or_default();
        crate::ai::pick_provider(&prefs, &installed).map(|p| format!("{}:{}", p.id(), prefs.model(p))).unwrap_or_default()
    });
    let current = picked.read().clone();
    // The picker: closed it shows the strength as dots and the model's name;
    // open it is a menu in the style of Aven's (see style.css).
    let mut menu = use_signal(|| false);
    let mut effort = use_signal(|| crate::ai::Prefs::load().effort);
    let dots = crate::ai::effort_dots(&effort.read());
    let shown = groups
        .iter()
        .flat_map(|(p, choices)| choices.iter().map(move |(id, label)| (format!("{}:{}", p.id(), id), label.clone())))
        .find(|(key, _)| *key == current)
        .map(|(_, label)| label.strip_prefix("Default · ").map(str::to_string).unwrap_or(label))
        .unwrap_or_else(|| "Model".to_string());
    let (reply, invented) = crate::ai::clean_reply(&session.output);
    // A reply that is the whole page after a change: "Replace" puts it in.
    let (reply, revises) = match crate::ai::split_revision(&reply) {
        Some(body) => (body, true),
        None if crate::ai::REVISE_MARK.starts_with(reply.trim()) && !reply.trim().is_empty() => (String::new(), false),
        None => (reply, false),
    };
    let streaming = session.status == AiStatus::Streaming;
    // A reply that changes the page can ask for sub-pages: they are named
    // before anything is done, and the text shown is the page itself.
    let (reply, subpages) = if revises { crate::ai::split_pages(&reply) } else { (reply, Vec::new()) };
    let planned = !subpages.is_empty() && session.status == AiStatus::Done;
    // A plan for a large job: what it will build, its steps, and "Do it".
    let job = if session.status == AiStatus::Done && !revises { crate::ai::split_plan(&reply) } else { None };
    let reply = match &job {
        Some((about, _)) => about.clone(),
        None => reply,
    };
    // What the AI did with the tools of Rune in this run (a line for each
    // thing), and while it works, the step it is at.
    let did = if session.status == AiStatus::Streaming { Vec::new() } else { crate::mcp::describe(&session.did) };
    let acted = !did.is_empty();
    let more = did.len().saturating_sub(6);
    let working = session
        .working
        .clone()
        .or_else(|| session.steps.last().cloned())
        .unwrap_or_else(|| if session.output.trim().is_empty() { "Thinking" } else { "Writing" }.to_string());
    // The plan is short. The text of the pages shows when the person asks.
    let mut show_pages = use_signal(|| false);
    let reply = match (planned, *show_pages.read()) {
        (true, false) => String::new(),
        (true, true) => {
            let mut all = reply.trim_end().to_string();
            for (name, text) in &subpages {
                all.push_str(&format!("\n\n— {name} —\n{}", text.trim()));
            }
            all
        }
        _ => reply,
    };

    rsx! {
        div { class: if streaming { "ai-panel live" } else { "ai-panel" }, onclick: move |e| e.stop_propagation(),
          div { class: "ai-panel-in",
            if session.status == AiStatus::Prompting {
                // After Aven's message box: the text, and at its end the
                // model in quiet words, the microphone and a round button to
                // send (there once something is typed). Escape, or a press
                // outside while it is empty, puts the box away.
                div { class: "ai-compose",
                    for picture in session.pictures.iter() {
                        AiShot { key: "{picture.id}", id: picture.id.clone(), src: picture.data_url() }
                    }
                    input {
                        id: "ai-input",
                        class: "ai-input",
                        r#type: "text",
                        placeholder: "{placeholder}",
                        initial_value: "{session.prompt}",
                        onmounted: move |e: MountedEvent| async move {
                            let _ = e.set_focus(true).await;
                        },
                        oninput: move |e: FormEvent| state.ai_set_prompt(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Enter {
                                e.prevent_default();
                                state.ai_run();
                            } else if e.key() == Key::Escape {
                                state.ai_discard();
                            }
                        },
                    }
                    button {
                        id: "ai-model",
                        class: if *menu.read() { "ai-model open" } else { "ai-model" },
                        title: "Model and strength",
                        "aria-label": "Model and strength: {shown}",
                        "aria-haspopup": "menu",
                        "aria-expanded": if *menu.read() { "true" } else { "false" },
                        onclick: move |_| {
                            let open = *menu.peek();
                            menu.set(!open);
                        },
                        span { class: "ai-pips", "aria-hidden": "true",
                            for i in 0..4u8 {
                                i { "data-on": if i < dots { "1" } else { "0" } }
                            }
                        }
                        span { class: "ai-model-name", "{shown}" }
                    }
                    button { class: "ai-round mic-btn", "data-dictate": "ai-input", title: "Speak", "aria-label": "Speak", Icon { name: "mic", size: 15 } }
                    if can_send {
                        button {
                            id: "ai-send",
                            class: "ai-round ai-send",
                            title: "{send_label}",
                            "aria-label": "{send_label}",
                            onclick: move |_| state.ai_run(),
                            Icon { name: "arrow-up", size: 15 }
                        }
                    }
                }
            } else {
                div { class: "ai-caption", "{caption}" }
            }
            if let Some((_, steps)) = &job {
                div { class: "ai-plan", id: "ai-plan", role: "list", "aria-label": "What the AI will do",
                    for step in steps.iter() {
                        div { key: "{step}", class: "ai-plan-row", role: "listitem", Icon { name: "plus", size: 14 } span { "{step}" } }
                    }
                }
            }
            if planned {
                div { class: "ai-plan", id: "ai-plan", role: "list", "aria-label": "What the AI will do",
                    div { class: "ai-plan-row", role: "listitem", Icon { name: "file", size: 14 } span { "Change this page" } }
                    for (name, _) in subpages.iter() {
                        div { key: "{name}", class: "ai-plan-row", role: "listitem", "data-subpage": "{name}", Icon { name: "plus", size: 14 } span { "New sub-page: {name}" } }
                    }
                    div { class: "ai-plan-row", role: "listitem", Icon { name: "link", size: 14 } span { "Link the sub-pages in this page" } }
                }
            }
            if !reply.trim().is_empty() {
                div { class: "ai-output", "{reply}" }
            }
            if acted {
                div { class: "ai-plan ai-did", id: "ai-did", role: "list", "aria-label": "What the AI did",
                    for line in did.iter().take(6) {
                        div { key: "{line}", class: "ai-plan-row", role: "listitem", Icon { name: "check", size: 14 } span { "{line}" } }
                    }
                    if more > 0 {
                        div { class: "ai-plan-row", role: "listitem", span { class: "ai-more", "and {more} more" } }
                    }
                }
            }
            if invented {
                div { class: "ai-warning",
                    Icon { name: "warning", size: 14 }
                    span { "The AI tried to run commands it doesn't have, so parts of this reply may be made up. It can only see this page." }
                }
            }
            match session.status.clone() {
                AiStatus::Streaming => rsx! {
                    div { class: "ai-status",
                        span { class: "ai-writing", id: "ai-step", "{working}" span { class: "dots" } }
                        Elapsed { since: session.started }
                        button { class: "ghost-btn", onclick: move |_| state.ai_stop(), "Stop" }
                    }
                },
                AiStatus::Done => rsx! {
                    div { class: "ai-actions",
                        if job.is_some() {
                            button { class: "primary-btn", id: "ai-plan-go", title: "The AI does this now (one Undo takes it all back)", onclick: move |_| state.ai_do_plan(), "Do it" }
                            button { class: "secondary-btn", id: "ai-plan-change", onclick: move |_| state.ai_change_plan(), "Change the plan" }
                            button { class: "ghost-btn", onclick: move |_| state.ai_discard(), "Not this" }
                        } else if planned {
                            button { class: "primary-btn", id: "ai-replace", title: "Change the page and make the sub-pages (one Undo takes it all back)", onclick: move |_| state.ai_revise(), "Do it" }
                            button {
                                class: "secondary-btn",
                                id: "ai-show-pages",
                                onclick: move |_| {
                                    let on = *show_pages.peek();
                                    show_pages.set(!on);
                                },
                                if *show_pages.read() { "Hide the pages" } else { "Show the pages" }
                            }
                            button { class: "ghost-btn", onclick: move |_| state.ai_discard(), "Not this" }
                        } else if revises {
                            button { class: "primary-btn", id: "ai-replace", title: "Put this in place of the page (Undo brings the page back)", onclick: move |_| state.ai_revise(), "Replace" }
                            button { class: "secondary-btn", onclick: move |_| state.ai_accept(false), "Insert below" }
                        } else if acted {
                            // It is done already: keep it, or take all of it back.
                            button { class: "primary-btn", id: "ai-ok", onclick: move |_| state.ai_discard(), "Done" }
                            button { class: "secondary-btn", id: "ai-undo-run", title: "Take back all that the AI did", onclick: move |_| state.ai_undo_run(), "Undo" }
                        } else if rewrite {
                            button { class: "primary-btn", onclick: move |_| state.ai_accept(true), "Replace" }
                            button { class: "secondary-btn", onclick: move |_| state.ai_accept(false), "Insert below" }
                        } else {
                            button { class: "primary-btn", onclick: move |_| state.ai_accept(false), "Insert" }
                        }
                        if !planned && !acted && job.is_none() {
                            button { class: "ghost-btn", onclick: move |_| state.ai_run(), "Try again" }
                        }
                        if !planned && job.is_none() && (!acted || revises) {
                            button { class: "ghost-btn", onclick: move |_| state.ai_discard(), "Discard" }
                        }
                    }
                },
                AiStatus::Error(message) => rsx! {
                    div { class: "ai-error",
                        div { "{message}" }
                        div { class: "ai-actions",
                            button {
                                class: "secondary-btn",
                                onclick: move |_| state.set_modal(Some(crate::state::Modal::Settings)),
                                "Open Settings"
                            }
                            button { class: "ghost-btn", onclick: move |_| state.ai_run(), "Try again" }
                            button { class: "ghost-btn", onclick: move |_| state.ai_discard(), "Discard" }
                        }
                    }
                },
                AiStatus::Prompting => rsx! {},
            }
          }
            if *menu.read() && session.status == AiStatus::Prompting {
                // A press anywhere else puts the menu away.
                div { class: "ai-menu-shade", onclick: move |_| menu.set(false) }
                div {
                    class: "ai-menu",
                    role: "menu",
                    "aria-label": "Model and strength",
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Escape {
                            e.stop_propagation();
                            menu.set(false);
                        }
                    },
                    div { class: "ai-menu-label", "Strength" }
                    for (id, name, hint, n) in crate::ai::EFFORTS {
                        button {
                            key: "effort-{id}",
                            class: if *effort.read() == id { "ai-menu-row active" } else { "ai-menu-row" },
                            role: "menuitemradio",
                            "aria-checked": if *effort.read() == id { "true" } else { "false" },
                            "data-effort": "{id}",
                            onclick: move |_| {
                                crate::store::set_setting(crate::ai::EFFORT_SETTING, id);
                                effort.set(id.to_string());
                            },
                            span { class: "ai-menu-value",
                                span { class: "ai-pips", "aria-hidden": "true",
                                    for i in 0..4u8 {
                                        i { "data-on": if i < n { "1" } else { "0" } }
                                    }
                                }
                                "{name}"
                            }
                            span { class: "ai-menu-aside", "{hint}" }
                        }
                    }
                    div { class: "ai-menu-sep" }
                    for (p, choices) in groups.clone() {
                        div { class: "ai-menu-label", "{p.name()}" }
                        for (id, label) in choices {
                            {
                                let key = format!("{}:{}", p.id(), id);
                                let active = current == key;
                                let (name, aside) = match label.strip_prefix("Default · ") {
                                    Some(rest) => (rest.to_string(), "default"),
                                    None if id.is_empty() => (label.clone(), "default"),
                                    None => (label.clone(), ""),
                                };
                                rsx! {
                                    button {
                                        key: "{key}",
                                        class: if active { "ai-menu-row active" } else { "ai-menu-row" },
                                        role: "menuitemradio",
                                        "aria-checked": if active { "true" } else { "false" },
                                        "data-model": "{key}",
                                        onclick: move |_| {
                                            crate::store::set_setting(crate::ai::PROVIDER_SETTING, p.id());
                                            crate::store::set_setting(p.model_setting(), &id);
                                            picked.set(key.clone());
                                        },
                                        span { class: "ai-menu-value", "{name}" }
                                        span { class: "ai-menu-aside", "{aside}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
