//! Charts: a block that draws the rows of a table. The table is the one
//! above the chart on its page, or the first table of another page. What it
//! draws is kept in the block's `data` as JSON, which is also how it is
//! written in Markdown (a code block of the language `chart`):
//!
//! ```text
//! ```chart
//! {"shape": "bars", "from": "Run log", "x": "Date", "by": "week", "y": "km", "add": "sum"}
//! ```
//! ```
//!
//! Bars and a line draw one amount for each row, day, week or month; a ring
//! draws one amount against a goal; days draw which days of the last four
//! weeks have a row.

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::table::{Kind, Table};

/// How a chart draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Bars,
    Line,
    Ring,
    Days,
}

impl Shape {
    pub const ALL: [Shape; 4] = [Shape::Bars, Shape::Line, Shape::Ring, Shape::Days];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Bars => "Bars",
            Shape::Line => "Line",
            Shape::Ring => "Ring",
            Shape::Days => "Days",
        }
    }

    /// Name of its icon (see `ui/icons.rs`).
    pub fn icon(self) -> &'static str {
        match self {
            Shape::Bars => "chart-bars",
            Shape::Line => "chart",
            Shape::Ring => "ring",
            Shape::Days => "days",
        }
    }
}

/// What one bar or point stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Row,
    Day,
    #[default]
    Week,
    Month,
}

impl By {
    pub const ALL: [By; 4] = [By::Row, By::Day, By::Week, By::Month];

    pub fn label(self) -> &'static str {
        match self {
            By::Row => "Each row",
            By::Day => "By day",
            By::Week => "By week",
            By::Month => "By month",
        }
    }
}

/// How the amounts of one bar (or of a ring) are put together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Add {
    #[default]
    Sum,
    #[serde(alias = "avg", alias = "mean")]
    Average,
    #[serde(alias = "max")]
    Highest,
    #[serde(alias = "min")]
    Lowest,
    /// How many rows, whatever is in them.
    Count,
}

impl Add {
    pub fn label(self) -> &'static str {
        match self {
            Add::Sum => "Sum",
            Add::Average => "Average",
            Add::Highest => "Highest",
            Add::Lowest => "Lowest",
            Add::Count => "Number of rows",
        }
    }

    fn apply(self, values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return (self == Add::Count || self == Add::Sum).then_some(0.0);
        }
        Some(match self {
            Add::Sum => values.iter().sum(),
            Add::Average => values.iter().sum::<f64>() / values.len() as f64,
            Add::Highest => values.iter().copied().fold(f64::MIN, f64::max),
            Add::Lowest => values.iter().copied().fold(f64::MAX, f64::min),
            Add::Count => values.len() as f64,
        })
    }
}

/// What a chart draws, and from where.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    #[serde(default, alias = "kind", alias = "type")]
    pub shape: Shape,
    /// The page whose first table it draws (its id or its title); empty
    /// for the table above the chart on its own page.
    #[serde(default, alias = "table", alias = "page", skip_serializing_if = "String::is_empty")]
    pub from: String,
    /// The column along the bottom, usually a date ("Date").
    #[serde(default, alias = "across", skip_serializing_if = "String::is_empty")]
    pub x: String,
    #[serde(default)]
    pub by: By,
    /// The column of the amounts ("km").
    #[serde(default, alias = "value", alias = "column", skip_serializing_if = "String::is_empty")]
    pub y: String,
    #[serde(default, alias = "aggregate", alias = "agg")]
    pub add: Add,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}

/// One bar or point.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub label: String,
    pub value: f64,
    /// It holds today (this week, this month).
    pub current: bool,
}

/// One day of the days chart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub has_row: bool,
    pub today: bool,
    pub ahead: bool,
}

impl Chart {
    /// The chart kept in a block; an empty one (bars of the table above)
    /// when there is none yet or it cannot be read.
    pub fn parse(data: &str) -> Chart {
        serde_json::from_str(data).unwrap_or_default()
    }

    /// The chart written in a Markdown code block, if it is one.
    pub fn from_markdown(text: &str) -> Option<Chart> {
        let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
        value.is_object().then(|| serde_json::from_value(value).ok()).flatten()
    }

    pub fn to_data(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// As a Markdown code block.
    pub fn to_markdown(&self) -> String {
        format!("```chart\n{}\n```", self.to_data())
    }

    /// The columns it draws: along the bottom (if any) and the amounts (if
    /// any). Unset, the first date column and the first number, time or
    /// pace column.
    pub fn columns(&self, t: &Table) -> (Option<usize>, Option<usize>) {
        let x = t.find_column(&self.x).or_else(|| t.first_of(Kind::Date));
        let y = t.find_column(&self.y).or_else(|| [Kind::Number, Kind::Time, Kind::Pace].into_iter().find_map(|k| t.first_of(k)));
        (x, y)
    }

    /// Its title: the one it was given, or one that says what it draws
    /// ("km per week").
    pub fn heading(&self, t: &Table) -> String {
        if !self.title.trim().is_empty() {
            return self.title.trim().to_string();
        }
        let (x, y) = self.columns(t);
        let amount = match (self.add, y) {
            (Add::Count, _) | (_, None) => "Rows".to_string(),
            (_, Some(c)) => t.heading(c),
        };
        let dated = x.is_some_and(|c| t.kind(c) == Kind::Date);
        match self.shape {
            Shape::Days => "Days with a row".to_string(),
            Shape::Ring => match self.add {
                Add::Highest => format!("Highest {}", amount.to_lowercase()),
                Add::Lowest => format!("Lowest {}", amount.to_lowercase()),
                Add::Average => format!("Average {}", amount.to_lowercase()),
                Add::Sum | Add::Count => amount,
            },
            Shape::Bars | Shape::Line => match (self.by, dated) {
                (By::Day, true) => format!("{amount} per day"),
                (By::Week, true) => format!("{amount} per week"),
                (By::Month, true) => format!("{amount} per month"),
                _ => amount,
            },
        }
    }

    /// The amount of one row (1 when it counts rows).
    fn amount(&self, t: &Table, r: usize, y: Option<usize>) -> Option<f64> {
        match (self.add, y) {
            (Add::Count, _) => Some(1.0),
            (_, Some(c)) => t.value(r, c),
            (_, None) => None,
        }
    }

    /// The bars or points to draw, oldest first.
    pub fn points(&self, t: &Table, today: NaiveDate) -> Vec<Point> {
        let (x, y) = self.columns(t);
        let dated = x.filter(|&c| t.kind(c) == Kind::Date);
        match (self.by, dated) {
            (By::Row, _) | (_, None) => {
                let rows: Vec<usize> = t.body().filter(|&r| self.amount(t, r, y).is_some()).collect();
                let from = rows.len().saturating_sub(30);
                let last = rows.len().saturating_sub(1);
                rows[from..]
                    .iter()
                    .enumerate()
                    .map(|(i, &r)| {
                        let label = match x {
                            Some(c) => t.shown(r, c),
                            None => (from + i + 1).to_string(),
                        };
                        Point { label, value: self.amount(t, r, y).unwrap_or(0.0), current: from + i == last }
                    })
                    .collect()
            }
            (by, Some(xc)) => {
                let key = |d: NaiveDate| match by {
                    By::Day | By::Row => d,
                    By::Week => d - Duration::days(i64::from(d.weekday().num_days_from_monday())),
                    By::Month => d.with_day(1).unwrap_or(d),
                };
                let next = |d: NaiveDate| match by {
                    By::Day | By::Row => d + Duration::days(1),
                    By::Week => d + Duration::days(7),
                    By::Month => {
                        let (y, m) = if d.month() == 12 { (d.year() + 1, 1) } else { (d.year(), d.month() + 1) };
                        NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(d + Duration::days(31))
                    }
                };
                let mut groups: Vec<(NaiveDate, Vec<f64>)> = Vec::new();
                for r in t.body() {
                    let (Some(d), Some(v)) = (t.day(r, xc, today), self.amount(t, r, y)) else { continue };
                    let k = key(d);
                    match groups.iter_mut().find(|(g, _)| *g == k) {
                        Some((_, values)) => values.push(v),
                        None => groups.push((k, vec![v])),
                    }
                }
                groups.sort_by_key(|(k, _)| *k);
                let (Some(first), Some(last)) = (groups.first().map(|g| g.0), groups.last().map(|g| g.0)) else { return Vec::new() };
                // Up to the period of today, when that is not far after the last row.
                let now = key(today);
                let end = if now > last && (now - last).num_days() <= 62 { now } else { last };
                let mut out = Vec::new();
                let mut k = first;
                while k <= end && out.len() < 400 {
                    let values = groups.iter().find(|(g, _)| *g == k).map(|(_, v)| v.as_slice()).unwrap_or(&[]);
                    let label = match by {
                        By::Day | By::Row => k.format("%a %-d").to_string(),
                        By::Week => k.format("%-d %b").to_string(),
                        By::Month => k.format("%b").to_string(),
                    };
                    out.push(Point { label, value: self.add.apply(values).unwrap_or(0.0), current: k == now });
                    k = next(k);
                }
                let from = out.len().saturating_sub(26);
                out.split_off(from)
            }
        }
    }

    /// The one amount of a ring.
    pub fn ring(&self, t: &Table) -> Option<f64> {
        let (_, y) = self.columns(t);
        let values: Vec<f64> = t.body().filter_map(|r| self.amount(t, r, y)).collect();
        self.add.apply(&values)
    }

    /// The last four weeks, Monday to Sunday, the week of today last.
    pub fn days(&self, t: &Table, today: NaiveDate) -> Vec<Day> {
        let (x, _) = self.columns(t);
        let dated = x.filter(|&c| t.kind(c) == Kind::Date);
        let with_rows: Vec<NaiveDate> = match dated {
            Some(c) => t.body().filter_map(|r| t.day(r, c, today)).collect(),
            None => Vec::new(),
        };
        let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        let start = monday - Duration::days(21);
        (0..28)
            .map(|i| {
                let date = start + Duration::days(i);
                Day { date, has_row: with_rows.contains(&date), today: date == today, ahead: date > today }
            })
            .collect()
    }

    /// An amount written as its column writes it.
    pub fn format(&self, t: &Table, v: f64) -> String {
        match (self.add, self.columns(t).1) {
            (Add::Count, _) | (_, None) => format!("{v:.0}"),
            (_, Some(c)) if t.kind(c).is_amount() => t.format(c, v),
            (_, Some(c)) => format!("{:.*}", t.places(c), v),
        }
    }

    /// A pace is better when it is lower, so its line is drawn upside down.
    pub fn upside_down(&self, t: &Table) -> bool {
        self.add != Add::Count && self.columns(t).1.is_some_and(|c| t.kind(c) == Kind::Pace)
    }
}

/// A round number at or above `v`, for the top of a chart: 1, 2, 2.5 or 5
/// times a power of ten.
pub fn nice_top(v: f64) -> f64 {
    if v <= 0.0 || !v.is_finite() {
        return 1.0;
    }
    let power = 10f64.powf(v.log10().floor());
    for step in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if step * power >= v - 1e-9 {
            return step * power;
        }
    }
    10.0 * power
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Total;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// Four weeks of runs, as in the mocks.
    fn log() -> Table {
        let rows: Vec<Vec<String>> = [
            ["Date", "km", "Time", "Pace"],
            ["Tue 10 Nov", "3.0", "19:30", ""],
            ["Thu 12 Nov", "3.0", "19:12", ""],
            ["Sun 15 Nov", "3.5", "23:05", ""],
            ["Tue 17 Nov", "3.5", "22:10", ""],
            ["Thu 19 Nov", "4.0", "24:48", ""],
            ["Sun 22 Nov", "4.9", "31:30", ""],
            ["Tue 24 Nov", "4.0", "24:40", ""],
            ["Thu 26 Nov", "4.1", "24:36", ""],
            ["Sun 29 Nov", "5.0", "31:15", ""],
            ["Tue 1 Dec", "6.0", "34:00", ""],
        ]
        .iter()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect();
        let mut t = Table { rows, columns: Vec::new() };
        t.set_kind(0, Kind::Date);
        t.set_kind(1, Kind::Number);
        t.set_kind(2, Kind::Time);
        t.set_kind(3, Kind::Pace);
        t.set_total(1, Some(Total::Sum));
        t
    }

    #[test]
    fn km_per_week() {
        let today = d(2026, 12, 1);
        let chart = Chart::default();
        let t = log();
        assert_eq!(chart.columns(&t), (Some(0), Some(1)));
        assert_eq!(chart.heading(&t), "km per week");
        let points = chart.points(&t, today);
        let got: Vec<(&str, String, bool)> = points.iter().map(|p| (p.label.as_str(), format!("{:.1}", p.value), p.current)).collect();
        assert_eq!(
            got,
            vec![
                ("9 Nov", "9.5".to_string(), false),
                ("16 Nov", "12.4".to_string(), false),
                ("23 Nov", "13.1".to_string(), false),
                ("30 Nov", "6.0".to_string(), true),
            ]
        );
        assert_eq!(chart.format(&t, 12.4), "12.4");
    }

    #[test]
    fn a_week_with_no_runs_is_drawn_empty_and_today_is_reached() {
        let today = d(2026, 12, 15);
        let mut t = log();
        t.remove_row(10);
        let points = Chart::default().points(&t, today);
        let labels: Vec<&str> = points.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["9 Nov", "16 Nov", "23 Nov", "30 Nov", "7 Dec", "14 Dec"]);
        assert_eq!(points[5].value, 0.0);
        assert!(points[5].current);
    }

    #[test]
    fn pace_for_each_run_is_drawn_upside_down() {
        let today = d(2026, 12, 1);
        let t = log();
        let chart = Chart { shape: Shape::Line, y: "Pace".into(), by: By::Row, add: Add::Average, ..Chart::default() };
        assert!(chart.upside_down(&t));
        let points = chart.points(&t, today);
        assert_eq!(points.len(), 10);
        assert_eq!(points[0].label, "Tue 10 Nov");
        assert_eq!(chart.format(&t, points[9].value), "5:40");
        assert!(points[9].current && !points[8].current);
        assert_eq!(chart.heading(&t), "Pace");
    }

    #[test]
    fn a_ring_and_the_days() {
        let today = d(2026, 12, 1);
        let t = log();
        let ring = Chart { shape: Shape::Ring, add: Add::Highest, goal: Some(10.0), ..Chart::default() };
        assert_eq!(ring.ring(&t), Some(6.0));
        assert_eq!(ring.heading(&t), "Highest km");
        let days = Chart { shape: Shape::Days, ..Chart::default() }.days(&t, today);
        assert_eq!(days.len(), 28);
        assert_eq!(days[0].date, d(2026, 11, 9));
        let marked: Vec<u32> = days.iter().filter(|x| x.has_row).map(|x| x.date.day()).collect();
        assert_eq!(marked, [10, 12, 15, 17, 19, 22, 24, 26, 29, 1]);
        assert!(days[22].today && days[23].ahead);
        let count = Chart { add: Add::Count, ..Chart::default() };
        assert_eq!(count.points(&t, today)[0].value, 3.0);
        assert_eq!(count.heading(&t), "Rows per week");
    }

    #[test]
    fn written_in_markdown() {
        let md = r#"{"kind": "line", "table": "Run log", "y": "Pace", "by": "row", "agg": "avg", "title": "Pace per run"}"#;
        let chart = Chart::from_markdown(md).unwrap();
        assert_eq!((chart.shape, chart.from.as_str(), chart.by, chart.add), (Shape::Line, "Run log", By::Row, Add::Average));
        assert_eq!(Chart::parse(&chart.to_data()), chart);
        assert!(chart.to_markdown().starts_with("```chart\n{\"shape\":\"line\""));
        assert_eq!(Chart::from_markdown("not a chart"), None);
        assert_eq!(Chart::from_markdown("[1, 2]"), None);
        assert_eq!(Chart::parse(""), Chart::default());
    }

    #[test]
    fn round_tops() {
        assert_eq!(nice_top(13.1), 20.0);
        assert_eq!(nice_top(19.5), 20.0);
        assert_eq!(nice_top(21.0), 25.0);
        assert_eq!(nice_top(2040.0), 2500.0);
        assert_eq!(nice_top(0.0), 1.0);
    }
}
