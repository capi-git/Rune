//! A chart in a page: bars, a line, a ring or days, drawn from a table, and
//! the small panel that says what it draws. Grey, with the ink of the text
//! for what is now (this week, the last run); no colours of its own.

use super::icons::Icon;
use crate::chart::{nice_top, Add, By, Chart, Day, Point, Shape};
use crate::state::{AppState, ChartSource, Popover};
use crate::table::{Kind, Table};
use dioxus::prelude::*;

/// The width and height the drawings are made at (they scale to the page).
const W: f64 = 640.0;
const H: f64 = 168.0;

#[component]
pub fn ChartView(page_id: String, id: String, data: String) -> Element {
    let state = use_context::<AppState>();
    let chart = Chart::parse(&data);
    let source = state.chart_source(&page_id, &id, &chart.from);
    let open = matches!(state.ui.read().popover.clone(), Some(Popover::Chart { block_id }) if block_id == id);
    // A chart just made from the "/" menu opens its panel.
    let fresh = data.trim().is_empty();
    let first_id = id.clone();
    use_effect(move || {
        if fresh {
            state.set_popover(Some(Popover::Chart { block_id: first_id.clone() }));
        }
    });
    let today = crate::when::today();
    let title = source.as_ref().map(|s| chart.heading(&s.table)).unwrap_or_else(|| "Chart".to_string());
    let from = source.as_ref().filter(|s| s.elsewhere).map(|s| format!("from {}", s.title));
    let note = source.as_ref().map(|s| note(&chart, &s.table, today)).unwrap_or_default();
    let toggle_id = id.clone();
    rsx! {
        figure { class: if open { "chart-block open" } else { "chart-block" }, "data-chart": "{id}",
            figcaption { class: "chart-head",
                span { class: "chart-title", "{title}" }
                if let Some(from) = from {
                    span { class: "chart-from", "{from}" }
                }
                span { class: "chart-note", "{note}" }
                button {
                    class: if open { "chart-settings open" } else { "chart-settings" },
                    title: "What the chart shows",
                    "aria-label": "What the chart shows",
                    "aria-haspopup": "dialog",
                    "aria-expanded": if open { "true" } else { "false" },
                    onclick: move |e| {
                        e.stop_propagation();
                        state.set_popover((!open).then(|| Popover::Chart { block_id: toggle_id.clone() }));
                    },
                    Icon { name: "sliders", size: 14 }
                }
            }
            match &source {
                None => rsx! { div { class: "chart-empty", "No table to draw yet." } },
                Some(src) => match chart.shape {
                    Shape::Bars => rsx! { Bars { chart: chart.clone(), table: src.table.clone(), points: chart.points(&src.table, today) } },
                    Shape::Line => rsx! { Line { chart: chart.clone(), table: src.table.clone(), points: chart.points(&src.table, today) } },
                    Shape::Ring => rsx! { Ring { chart: chart.clone(), table: src.table.clone() } },
                    Shape::Days => rsx! { Days { days: chart.days(&src.table, today) } },
                },
            }
            if open {
                ChartPanel { page_id: page_id.clone(), id: id.clone(), chart: chart.clone(), source: source.clone() }
            }
        }
    }
}

/// The few words at the right of the title: all of it, or the goal.
fn note(chart: &Chart, t: &Table, today: chrono::NaiveDate) -> String {
    match chart.shape {
        Shape::Ring => chart.goal.map(|g| format!("Goal {}", chart.format(t, g))).unwrap_or_default(),
        Shape::Days => {
            let n = chart.days(t, today).iter().filter(|d| d.has_row).count();
            format!("{n} {} in 4 weeks", if n == 1 { "day" } else { "days" })
        }
        Shape::Bars | Shape::Line if chart.add == Add::Sum || chart.add == Add::Count => {
            let points = chart.points(t, today);
            if points.is_empty() {
                return String::new();
            }
            format!("{} in all", chart.format(t, points.iter().map(|p| p.value).sum()))
        }
        Shape::Bars | Shape::Line => String::new(),
    }
}

/// The labels under the bars or points: all when there are few, else
/// every so many, and always the one of now.
fn shows_label(i: usize, n: usize, current: bool) -> bool {
    let every = n.div_ceil(10).max(1);
    current || i % every == 0 || i + 1 == n
}

#[component]
fn Bars(chart: Chart, table: Table, points: Vec<Point>) -> Element {
    if points.is_empty() {
        return rsx! { div { class: "chart-empty", "No rows to draw yet." } };
    }
    let (left, right, top_y, bottom) = (34.0, W - 4.0, 12.0, H - 30.0);
    let most = points.iter().map(|p| p.value).fold(chart.goal.unwrap_or(0.0), f64::max);
    let top = nice_top(most);
    let y_of = |v: f64| bottom - (v / top).clamp(0.0, 1.0) * (bottom - top_y);
    let n = points.len();
    let band = (right - left) / n as f64;
    let bw = (band * 0.56).clamp(3.0, 40.0);
    let bars: Vec<(usize, f64, f64, f64, &Point)> = points.iter().enumerate().map(|(i, p)| (i, left + band * i as f64 + (band - bw) / 2.0, y_of(p.value), (bottom - y_of(p.value)).max(if p.value > 0.0 { 1.0 } else { 0.0 }), p)).collect();
    let ticks = [0.0, top / 2.0, top].map(|v| (y_of(v), chart.format(&table, v)));
    let goal = chart.goal.map(|g| (y_of(g), chart.format(&table, g)));
    let said = points.iter().map(|p| format!("{} {}", p.label, chart.format(&table, p.value))).collect::<Vec<_>>().join(", ");
    rsx! {
        svg { class: "chart-svg", view_box: "0 0 {W} {H}", role: "img", "aria-label": "{said}",
            for (i, (y, label)) in ticks.iter().enumerate() {
                g { key: "t{i}",
                    line { class: if i == 0 { "chart-base" } else { "chart-grid" }, x1: "{left}", x2: "{right}", y1: "{y}", y2: "{y}" }
                    text { class: "chart-tick", x: "{left - 6.0}", y: "{y + 3.5}", text_anchor: "end", "{label}" }
                }
            }
            if let Some((y, label)) = goal {
                line { class: "chart-goal", x1: "{left}", x2: "{right}", y1: "{y}", y2: "{y}" }
                text { class: "chart-tick", x: "{right}", y: "{y - 5.0}", text_anchor: "end", "Goal {label}" }
            }
            for (i, x, y, h, p) in bars {
                g { key: "b{i}",
                    rect { class: if p.current { "chart-bar current" } else { "chart-bar" }, x: "{x}", y: "{y}", width: "{bw}", height: "{h}", rx: "3" }
                    if p.current && p.value > 0.0 {
                        text { class: "chart-value", x: "{x + bw / 2.0}", y: "{y - 6.0}", text_anchor: "middle", "{chart.format(&table, p.value)}" }
                    }
                    if shows_label(i, n, p.current) {
                        text { class: if p.current { "chart-label current" } else { "chart-label" }, x: "{x + bw / 2.0}", y: "{H - 12.0}", text_anchor: "middle", "{p.label}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Line(chart: Chart, table: Table, points: Vec<Point>) -> Element {
    if points.is_empty() {
        return rsx! { div { class: "chart-empty", "No rows to draw yet." } };
    }
    let (left, right, top_y, bottom) = (40.0, W - 12.0, 16.0, H - 30.0);
    let lo = points.iter().map(|p| p.value).fold(f64::MAX, f64::min);
    let hi = points.iter().map(|p| p.value).fold(f64::MIN, f64::max);
    let pad = ((hi - lo) * 0.12).max(hi.abs() * 0.02).max(0.5);
    let (lo, hi) = (lo - pad, hi + pad);
    // A pace is better when lower: lower is drawn higher.
    let flip = chart.upside_down(&table);
    let y_of = |v: f64| {
        let f = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        if flip {
            top_y + f * (bottom - top_y)
        } else {
            bottom - f * (bottom - top_y)
        }
    };
    let n = points.len();
    let x_of = |i: usize| if n == 1 { (left + right) / 2.0 } else { left + (right - left) * i as f64 / (n - 1) as f64 };
    let path = points.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", x_of(i), y_of(p.value))).collect::<Vec<_>>().join(" ");
    let ticks = [lo + pad, (lo + hi) / 2.0, hi - pad].map(|v| (y_of(v), chart.format(&table, v)));
    let dots: Vec<(usize, f64, f64, &Point)> = points.iter().enumerate().map(|(i, p)| (i, x_of(i), y_of(p.value), p)).collect();
    let said = points.iter().map(|p| format!("{} {}", p.label, chart.format(&table, p.value))).collect::<Vec<_>>().join(", ");
    rsx! {
        svg { class: "chart-svg", view_box: "0 0 {W} {H}", role: "img", "aria-label": "{said}",
            for (i, (y, label)) in ticks.iter().enumerate() {
                g { key: "t{i}",
                    line { class: "chart-grid", x1: "{left}", x2: "{right}", y1: "{y}", y2: "{y}" }
                    text { class: "chart-tick", x: "{left - 6.0}", y: "{y + 3.5}", text_anchor: "end", "{label}" }
                }
            }
            polyline { class: "chart-line", points: "{path}" }
            for (i, x, y, p) in dots {
                g { key: "d{i}",
                    circle { class: if p.current { "chart-dot current" } else { "chart-dot" }, cx: "{x}", cy: "{y}", r: if p.current { "4" } else { "2.5" } }
                    if p.current {
                        text { class: "chart-value", x: "{x - 8.0}", y: "{y - 7.0}", text_anchor: "end", "{chart.format(&table, p.value)}" }
                    }
                    if shows_label(i, n, p.current) {
                        text { class: if p.current { "chart-label current" } else { "chart-label" }, x: "{x}", y: "{H - 12.0}", text_anchor: if i == 0 && n > 1 { "start" } else if i + 1 == n && n > 1 { "end" } else { "middle" }, "{p.label}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Ring(chart: Chart, table: Table) -> Element {
    let value = chart.ring(&table);
    let shown = value.map(|v| chart.format(&table, v)).unwrap_or_else(|| "–".into());
    let share = match (value, chart.goal) {
        (Some(v), Some(g)) if g > 0.0 => Some((v / g).clamp(0.0, 1.0)),
        _ => None,
    };
    let round = 2.0 * std::f64::consts::PI * 34.0;
    let arc = share.map(|s| s * round).unwrap_or(0.0);
    let percent = share.map(|s| format!("{:.0}%", s * 100.0)).unwrap_or_default();
    let under = match chart.goal {
        Some(g) => format!("of {}", chart.format(&table, g)),
        None => "No goal".to_string(),
    };
    rsx! {
        div { class: "chart-ring",
            svg { view_box: "0 0 84 84", width: "84", height: "84", role: "img", "aria-label": "{shown} {under}",
                circle { class: "ring-track", cx: "42", cy: "42", r: "34" }
                if arc > 0.0 {
                    circle { class: "ring-arc", cx: "42", cy: "42", r: "34", stroke_dasharray: "{arc:.1} {round:.1}", transform: "rotate(-90 42 42)" }
                }
                text { class: "ring-percent", x: "42", y: "46", text_anchor: "middle", "{percent}" }
            }
            div { class: "ring-words",
                span { class: "ring-value", "{shown}" }
                span { class: "ring-under", "{under}" }
            }
        }
    }
}

#[component]
fn Days(days: Vec<Day>) -> Element {
    let weeks: Vec<Vec<Day>> = days.chunks(7).map(|w| w.to_vec()).collect();
    rsx! {
        div { class: "chart-days", role: "img", "aria-label": "Days with a row in the last four weeks",
            span {}
            for (i, wd) in ["M", "T", "W", "T", "F", "S", "S"].into_iter().enumerate() {
                span { key: "h{i}", class: "days-head", "{wd}" }
            }
            for (w, week) in weeks.into_iter().enumerate() {
                span { key: "w{w}", class: "days-week", "{week[0].date.format(\"%-d %b\")}" }
                for (i, d) in week.into_iter().enumerate() {
                    span {
                        key: "{w}-{i}",
                        class: match (d.has_row, d.today, d.ahead) {
                            (true, true, _) => "day on today",
                            (true, false, _) => "day on",
                            (false, true, _) => "day today",
                            (false, false, true) => "day ahead",
                            (false, false, false) => "day",
                        },
                        title: "{d.date.format(\"%a %-d %b\")}",
                    }
                }
            }
        }
    }
}

/// The panel that says what a chart draws: its shape, its table, what goes
/// along the bottom, which amounts, and a goal. Each field opens a list of
/// plain rows, Rune's own menu, in place.
#[component]
fn ChartPanel(page_id: String, id: String, chart: Chart, source: Option<ChartSource>) -> Element {
    let state = use_context::<AppState>();
    let mut list = use_signal(|| None::<&'static str>);
    let save = {
        let (page_id, id) = (page_id.clone(), id.clone());
        move |c: Chart| {
            state.set_chart(&page_id, &id, &c);
            state.set_popover(Some(Popover::Chart { block_id: id.clone() }));
            let mut list = list;
            list.set(None);
        }
    };
    let table = source.as_ref().map(|s| s.table.clone());
    let (x, y) = table.as_ref().map(|t| chart.columns(t)).unwrap_or((None, None));
    let from_label = match &source {
        Some(s) if s.elsewhere => s.title.clone(),
        Some(_) => "Table on this page".to_string(),
        None => "No table".to_string(),
    };
    let heading = |c: Option<usize>| match (&table, c) {
        (Some(t), Some(c)) => t.heading(c),
        _ => "None".to_string(),
    };
    let across_label = match chart.by {
        By::Row => format!("{}, each row", heading(x)),
        by => format!("{}, {}", heading(x), by.label().to_lowercase()),
    };
    let value_label = match chart.add {
        Add::Count => "Number of rows".to_string(),
        add => format!("{}, {}", heading(y), add.label().to_lowercase()),
    };
    let goal_text = chart.goal.map(|g| format!("{g}")).unwrap_or_default();
    let others = state.pages_with_tables(&page_id);
    let columns: Vec<(usize, String, Kind)> = table.as_ref().map(|t| (0..t.columns()).map(|c| (c, t.heading(c), t.kind(c))).collect()).unwrap_or_default();
    let open = *list.read();
    let shapes_chart = chart.clone();
    let save_shape = save.clone();
    rsx! {
        div { class: "popover chart-panel", role: "dialog", "aria-label": "What the chart shows", onclick: move |e| e.stop_propagation(),
            div { class: "chart-shapes",
                for shape in Shape::ALL {
                    button {
                        key: "{shape.label()}",
                        class: if chart.shape == shape { "chart-shape on" } else { "chart-shape" },
                        "aria-pressed": if chart.shape == shape { "true" } else { "false" },
                        onclick: {
                            let mut c = shapes_chart.clone();
                            let save = save_shape.clone();
                            move |_| {
                                c.shape = shape;
                                save(c.clone());
                            }
                        },
                        Icon { name: shape.icon(), size: 18 }
                        span { "{shape.label()}" }
                    }
                }
            }
            div { class: "ai-menu-sep" }
            PanelField { name: "From", icon: "table", value: from_label, open: open == Some("from"), onpick: move |_| list.set(if open == Some("from") { None } else { Some("from") }) }
            if open == Some("from") {
                div { class: "panel-list",
                    button {
                        class: if chart.from.is_empty() { "ai-menu-row active" } else { "ai-menu-row" },
                        onclick: {
                            let mut c = chart.clone();
                            let save = save.clone();
                            move |_| {
                                c.from = String::new();
                                save(c.clone());
                            }
                        },
                        span { class: "ai-menu-value", "Table on this page" }
                    }
                    for (pid, title) in others {
                        button {
                            key: "{pid}",
                            class: if chart.from == pid || chart.from.eq_ignore_ascii_case(&title) { "ai-menu-row active" } else { "ai-menu-row" },
                            onclick: {
                                let mut c = chart.clone();
                                let save = save.clone();
                                let pid = pid.clone();
                                move |_| {
                                    c.from = pid.clone();
                                    c.x = String::new();
                                    c.y = String::new();
                                    save(c.clone());
                                }
                            },
                            span { class: "ai-menu-value", Icon { name: "file", size: 14 } "{title}" }
                        }
                    }
                }
            }
            if table.is_some() && chart.shape != Shape::Ring {
                PanelField { name: "Across", icon: "calendar", value: across_label, open: open == Some("across"), onpick: move |_| list.set(if open == Some("across") { None } else { Some("across") }) }
                if open == Some("across") {
                    div { class: "panel-list",
                        for (c, name, kind) in columns.iter().cloned().filter(|(_, _, k)| matches!(k, Kind::Date | Kind::Text | Kind::Choice)) {
                            button {
                                key: "x{c}",
                                class: if x == Some(c) { "ai-menu-row active" } else { "ai-menu-row" },
                                onclick: {
                                    let mut ch = chart.clone();
                                    let save = save.clone();
                                    let name = name.clone();
                                    move |_| {
                                        ch.x = name.clone();
                                        if kind != Kind::Date {
                                            ch.by = By::Row;
                                        }
                                        save(ch.clone());
                                    }
                                },
                                span { class: "ai-menu-value", Icon { name: kind.icon(), size: 14 } "{name}" }
                            }
                        }
                        if x.and_then(|c| columns.get(c)).is_some_and(|(_, _, k)| *k == Kind::Date) {
                            div { class: "ai-menu-sep" }
                            for by in By::ALL {
                                button {
                                    key: "{by.label()}",
                                    class: if chart.by == by { "ai-menu-row active" } else { "ai-menu-row" },
                                    onclick: {
                                        let mut ch = chart.clone();
                                        let save = save.clone();
                                        move |_| {
                                            ch.by = by;
                                            save(ch.clone());
                                        }
                                    },
                                    span { class: "ai-menu-value", "{by.label()}" }
                                }
                            }
                        }
                    }
                }
            }
            if table.is_some() && chart.shape != Shape::Days {
                PanelField { name: "Value", icon: "hash", value: value_label, open: open == Some("value"), onpick: move |_| list.set(if open == Some("value") { None } else { Some("value") }) }
                if open == Some("value") {
                    div { class: "panel-list",
                        div { class: "ai-menu-label", "Column" }
                        for (c, name, kind) in columns.iter().cloned().filter(|(_, _, k)| k.is_amount()) {
                            button {
                                key: "y{c}",
                                class: if chart.add != Add::Count && y == Some(c) { "ai-menu-row active" } else { "ai-menu-row" },
                                onclick: {
                                    let mut ch = chart.clone();
                                    let save = save.clone();
                                    let name = name.clone();
                                    move |_| {
                                        ch.y = name.clone();
                                        if ch.add == Add::Count {
                                            ch.add = if kind == Kind::Pace { Add::Average } else { Add::Sum };
                                        }
                                        save(ch.clone());
                                    }
                                },
                                span { class: "ai-menu-value", Icon { name: kind.icon(), size: 14 } "{name}" }
                            }
                        }
                        button {
                            class: if chart.add == Add::Count { "ai-menu-row active" } else { "ai-menu-row" },
                            onclick: {
                                let mut ch = chart.clone();
                                let save = save.clone();
                                move |_| {
                                    ch.add = Add::Count;
                                    save(ch.clone());
                                }
                            },
                            span { class: "ai-menu-value", "Number of rows" }
                        }
                        if chart.add != Add::Count {
                            div { class: "ai-menu-sep" }
                            div { class: "ai-menu-label", "Add up" }
                            for add in [Add::Sum, Add::Average, Add::Highest, Add::Lowest] {
                                button {
                                    key: "{add.label()}",
                                    class: if chart.add == add { "ai-menu-row active" } else { "ai-menu-row" },
                                    onclick: {
                                        let mut ch = chart.clone();
                                        let save = save.clone();
                                        move |_| {
                                            ch.add = add;
                                            save(ch.clone());
                                        }
                                    },
                                    span { class: "ai-menu-value", "{add.label()}" }
                                }
                            }
                        }
                    }
                }
            }
            if table.is_some() && chart.shape != Shape::Days {
                label { class: "panel-field panel-input",
                    span { class: "panel-name", "Goal" }
                    input {
                        r#type: "text",
                        inputmode: "decimal",
                        placeholder: "None",
                        "aria-label": "Goal",
                        value: "{goal_text}",
                        onchange: {
                            let mut ch = chart.clone();
                            let save = save.clone();
                            move |e: FormEvent| {
                                let v = e.value();
                                ch.goal = crate::table::number(&v).or_else(|| crate::table::seconds(&v).filter(|_| v.contains(':')));
                                save(ch.clone());
                            }
                        },
                    }
                }
            }
            label { class: "panel-field panel-input",
                span { class: "panel-name", "Title" }
                input {
                    r#type: "text",
                    placeholder: "Says what it draws",
                    "aria-label": "Title",
                    value: "{chart.title}",
                    onchange: {
                        let mut ch = chart.clone();
                        let save = save.clone();
                        move |e: FormEvent| {
                            ch.title = e.value();
                            save(ch.clone());
                        }
                    },
                }
            }
        }
    }
}

/// One field of the panel: its name, and a quiet button with what it is now.
#[component]
fn PanelField(name: &'static str, icon: &'static str, value: String, open: bool, onpick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        div { class: "panel-field",
            span { class: "panel-name", "{name}" }
            button {
                class: if open { "panel-pick open" } else { "panel-pick" },
                "aria-expanded": if open { "true" } else { "false" },
                onclick: move |e| onpick.call(e),
                Icon { name: icon, size: 14 }
                span { class: "panel-value", "{value}" }
                Icon { name: "chevron-down", size: 12 }
            }
        }
    }
}
