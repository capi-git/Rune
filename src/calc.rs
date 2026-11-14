//! Math in a line. A line that ends with `=` and holds a calculation shows
//! its answer at its end, in grey; a line that says `sum`, `total` or
//! `average` under lines of amounts shows their total:
//!
//! ```text
//! 12 × 4.5 =            54
//! Rent: 1200 * 12 =     14,400
//! 80 + 15% =            92
//! 2h 30m + 45m =        3h 15m
//! - Coffee $4.50
//! - Lunch $12
//! sum                   = $16.50
//! ```
//!
//! The answer is worked out from the line's text each time the page is
//! drawn, and is never kept: it is not part of the line. Numbers are read
//! the way a table reads them (`table::number`): "1,500" and "1.234,56" are
//! what they were meant to be, and an answer to numbers written with a
//! decimal comma is written with one. Anything that is no calculation (a
//! word, a mark the parser does not know, a division by zero) gives nothing,
//! so a line that only happens to end with `=` stays as it is.

use crate::model::{Block, BlockKind};
use crate::table::money;

/// The most tokens a line is read with, and how deep brackets may nest.
const MOST_TOKENS: usize = 400;
const MOST_LEVELS: usize = 32;

/// The answer to show at the end of each block (`None` for nearly all).
pub fn answers(blocks: &[Block]) -> Vec<Option<String>> {
    (0..blocks.len()).map(|i| answer_at(blocks, i)).collect()
}

/// The kinds of line that can show an answer: not headings, code, tables
/// or titles.
fn can_show(kind: BlockKind) -> bool {
    matches!(kind, BlockKind::Text | BlockKind::Bulleted | BlockKind::Numbered | BlockKind::Todo)
}

fn answer_at(blocks: &[Block], i: usize) -> Option<String> {
    let block = &blocks[i];
    if !can_show(block.kind) {
        return None;
    }
    // (Cheap first: most lines end with no `=` and are longer than a total word.)
    let raw = block.text.trim_end();
    if !raw.ends_with('=') && raw.len() > "average =".len() {
        return None;
    }
    let text = block.plain_text();
    if let Some(how) = total_word(&text) {
        let amounts = amounts_above(blocks, i);
        let answer = total(how, &amounts)?;
        // "sum" shows "= 54"; "sum =" and "Total:" show "54".
        let bare = text.trim_end().ends_with(|c: char| c.is_alphabetic());
        return Some(if bare { format!("= {answer}") } else { answer });
    }
    line(&text)
}

// ----- a calculation in a line ---------------------------------------------

/// The answer to a line that ends with `=`: the calculation is the text
/// before it, from the start of the line or from after the last `:` or `=`
/// ("Rent: 1200 * 12 =").
pub fn line(text: &str) -> Option<String> {
    let body = text.trim_end().strip_suffix('=')?;
    let body = body.rsplit([':', '=']).next().unwrap_or(body);
    calculate(body)
}

/// The answer to a calculation, written to be read: "54", "$36", "92",
/// "3h 15m". Nothing when it is no calculation, or has nothing to work out
/// (a number alone).
pub fn calculate(text: &str) -> Option<String> {
    let tokens = tokens(text)?;
    let mut p = Parser { tokens: &tokens, at: 0, ops: 0, depth: 0 };
    let value = p.sum()?;
    if p.at != tokens.len() || p.ops == 0 || !value.n.is_finite() {
        return None;
    }
    let comma = tokens.iter().any(|t| matches!(t, Tok::Num(n) if n.comma));
    write(value, comma, 0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Plain,
    /// A share, kept as a fraction: 15% is 0.15.
    Percent,
    /// An amount of money, with its sign: "$".
    Money(&'static str),
    /// A length of time, in seconds.
    Time,
}

#[derive(Clone, Copy, Debug)]
struct Value {
    n: f64,
    kind: Kind,
}

/// A number as it was typed: its value, how many places it was typed with,
/// and whether its decimal sign was a comma.
#[derive(Clone, Copy, Debug)]
struct Typed {
    value: f64,
    places: usize,
    comma: bool,
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Num(Typed),
    /// A length of time, in seconds: "2h", "30m".
    Time(f64),
    Money(&'static str),
    Op(char),
    Percent,
    Of,
    Open,
    Close,
    /// Between the two parts of `round(x; 2)`.
    Next,
    Sqrt,
    Round,
}

/// The money signs a calculation knows.
const SIGNS: [(char, &str); 4] = [('$', "$"), ('€', "€"), ('£', "£"), ('¥', "¥")];

/// A number at the start of `text`: its digits, with `.` and `,` between
/// them, read the way a table reads a number. And how many bytes it took.
fn read_number(text: &str) -> Option<(Typed, usize)> {
    let b = text.as_bytes();
    let mut end = 0;
    while end < b.len() {
        let c = b[end];
        if c.is_ascii_digit() || ((c == b'.' || c == b',') && b.get(end + 1).is_some_and(u8::is_ascii_digit)) {
            end += 1;
        } else {
            break;
        }
    }
    let token = &text[..end];
    if !token.bytes().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let value = crate::table::number(token)?;
    // A sign is the decimal when the number is another without it:
    // "6,5" (not 65), "1.5"; "1,500" and "1.234.567" group the thousands.
    let is_decimal = |sign: char| token.contains(sign) && crate::table::number(&token.replace(sign, "")) != Some(value);
    let (comma, places) = if is_decimal(',') {
        (true, token.rsplit(',').next().map_or(0, str::len))
    } else if is_decimal('.') {
        (false, token.rsplit('.').next().map_or(0, str::len))
    } else {
        (false, 0)
    };
    Some((Typed { value, places, comma }, end))
}

/// How many seconds a unit of time is: "h", "min".
fn time_unit(word: &str) -> Option<f64> {
    match word.to_lowercase().as_str() {
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(3600.0),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(60.0),
        "s" | "sec" | "secs" | "second" | "seconds" => Some(1.0),
        _ => None,
    }
}

/// The text as tokens, or nothing when it holds a word or a mark that is
/// no part of a calculation.
fn tokens(text: &str) -> Option<Vec<Tok>> {
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    // After a number, a closing bracket or a percent: an `x` there multiplies.
    let ends_value = |out: &Vec<Tok>| matches!(out.last(), Some(Tok::Num(_) | Tok::Time(_) | Tok::Close | Tok::Percent | Tok::Money(_)));
    while i < text.len() {
        if out.len() > MOST_TOKENS {
            return None;
        }
        let rest = &text[i..];
        let ch = rest.chars().next()?;
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        if let Some((typed, len)) = read_number(rest) {
            i += len;
            // A unit of time right after it ("30m"), or an `x` ("2x3").
            let word: String = text[i..].chars().take_while(|c| c.is_alphabetic()).collect();
            match time_unit(&word) {
                Some(seconds) if !matches!(out.last(), Some(Tok::Money(_))) => {
                    out.push(Tok::Time(typed.value * seconds));
                    i += word.len();
                }
                _ if word.is_empty() => out.push(Tok::Num(typed)),
                _ if word.eq_ignore_ascii_case("x") => {
                    out.push(Tok::Num(typed));
                    out.push(Tok::Op('*'));
                    i += word.len();
                }
                _ => return None,
            }
            continue;
        }
        if ch.is_alphabetic() {
            let word: String = rest.chars().take_while(|c| c.is_alphabetic()).collect();
            // A unit of time after a space: "90 min".
            if let (Some(seconds), Some(Tok::Num(t))) = (time_unit(&word), out.last().copied()) {
                if !matches!(out.len().checked_sub(2).map(|j| out[j]), Some(Tok::Money(_))) {
                    *out.last_mut()? = Tok::Time(t.value * seconds);
                    i += word.len();
                    continue;
                }
            }
            let tok = match word.to_lowercase().as_str() {
                "sqrt" => Tok::Sqrt,
                "round" => Tok::Round,
                "of" => Tok::Of,
                "x" if ends_value(&out) => Tok::Op('*'),
                _ => return None,
            };
            out.push(tok);
            i += word.len();
            continue;
        }
        let tok = match ch {
            '+' => Tok::Op('+'),
            '-' | '\u{2212}' | '\u{2013}' => Tok::Op('-'),
            '*' | '×' | '·' | '∙' => Tok::Op('*'),
            '/' | '÷' => Tok::Op('/'),
            '^' => Tok::Op('^'),
            '%' => Tok::Percent,
            '(' => Tok::Open,
            ')' => Tok::Close,
            ';' | ',' => Tok::Next,
            '√' => Tok::Sqrt,
            _ => match SIGNS.iter().find(|(c, _)| *c == ch) {
                Some((_, sign)) => Tok::Money(sign),
                None => return None,
            },
        };
        out.push(tok);
        i += ch.len_utf8();
    }
    Some(out)
}

struct Parser<'a> {
    tokens: &'a [Tok],
    at: usize,
    /// How many operations it has: a number alone is nothing to work out.
    ops: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Tok> {
        self.tokens.get(self.at).copied()
    }

    fn next(&mut self) -> Option<Tok> {
        let tok = self.peek();
        self.at += 1;
        tok
    }

    /// Terms with `+` and `-` between them.
    fn sum(&mut self) -> Option<Value> {
        let mut a = self.product()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek() {
            self.at += 1;
            self.ops += 1;
            let b = self.product()?;
            let sign = if op == '-' { -1.0 } else { 1.0 };
            a = match (a.kind, b.kind) {
                // "80 + 15%" is 80 and 15% of it.
                (kind, Kind::Percent) if kind != Kind::Percent => Value { n: a.n * (1.0 + sign * b.n), kind },
                (x, y) => Value { n: a.n + sign * b.n, kind: same(x, y)? },
            };
        }
        Some(a)
    }

    /// Factors with `*`, `/` and `of` between them, and lengths of time side
    /// by side ("2h 30m").
    fn product(&mut self) -> Option<Value> {
        let mut a = self.unary()?;
        loop {
            match self.peek() {
                Some(Tok::Op(op @ ('*' | '/'))) => {
                    self.at += 1;
                    self.ops += 1;
                    let b = self.unary()?;
                    a = if op == '*' { times(a, b)? } else { divided(a, b)? };
                }
                // "15% of 80".
                Some(Tok::Of) if a.kind == Kind::Percent => {
                    self.at += 1;
                    self.ops += 1;
                    let b = self.unary()?;
                    a = Value { n: a.n * b.n, kind: if b.kind == Kind::Percent { Kind::Percent } else { b.kind } };
                }
                Some(Tok::Time(_)) if a.kind == Kind::Time => {
                    let b = self.unary()?;
                    a.n += b.n;
                }
                _ => return Some(a),
            }
        }
    }

    fn unary(&mut self) -> Option<Value> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.at += 1;
                let v = self.unary()?;
                Some(Value { n: -v.n, ..v })
            }
            Some(Tok::Op('+')) => {
                self.at += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// `2 ^ 10`, read from the right: `2 ^ 3 ^ 2` is `2 ^ 9`.
    fn power(&mut self) -> Option<Value> {
        let base = self.percent()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.at += 1;
            self.ops += 1;
            let exp = self.unary()?;
            if base.kind != Kind::Plain || exp.kind != Kind::Plain {
                return None;
            }
            return Some(Value { n: base.n.powf(exp.n), kind: Kind::Plain });
        }
        Some(base)
    }

    fn percent(&mut self) -> Option<Value> {
        let v = self.atom()?;
        if let Some(Tok::Percent) = self.peek() {
            self.at += 1;
            return (v.kind == Kind::Plain).then_some(Value { n: v.n / 100.0, kind: Kind::Percent });
        }
        Some(v)
    }

    fn atom(&mut self) -> Option<Value> {
        match self.next()? {
            // "$12", and "12 €".
            Tok::Money(sign) => match self.next()? {
                Tok::Num(t) => Some(Value { n: t.value, kind: Kind::Money(sign) }),
                _ => None,
            },
            Tok::Num(t) => match self.peek() {
                Some(Tok::Money(sign)) => {
                    self.at += 1;
                    Some(Value { n: t.value, kind: Kind::Money(sign) })
                }
                _ => Some(Value { n: t.value, kind: Kind::Plain }),
            },
            Tok::Time(seconds) => Some(Value { n: seconds, kind: Kind::Time }),
            Tok::Open => {
                let v = self.inside()?;
                matches!(self.next()?, Tok::Close).then_some(v)
            }
            func @ (Tok::Sqrt | Tok::Round) => {
                self.ops += 1;
                if !matches!(self.next()?, Tok::Open) {
                    return None;
                }
                let v = self.inside()?;
                let places = match self.peek() {
                    Some(Tok::Next) if matches!(func, Tok::Round) => {
                        self.at += 1;
                        let p = self.inside()?;
                        (p.kind == Kind::Plain && p.n.fract() == 0.0 && (0.0..=10.0).contains(&p.n)).then_some(p.n as i32)?
                    }
                    _ => 0,
                };
                if !matches!(self.next()?, Tok::Close) {
                    return None;
                }
                match func {
                    Tok::Sqrt => (v.kind == Kind::Plain && v.n >= 0.0).then(|| Value { n: v.n.sqrt(), kind: Kind::Plain }),
                    _ => {
                        let scale = 10f64.powi(places);
                        Some(Value { n: (v.n * scale).round() / scale, ..v })
                    }
                }
            }
            _ => None,
        }
    }

    /// What is between brackets.
    fn inside(&mut self) -> Option<Value> {
        self.depth += 1;
        if self.depth > MOST_LEVELS {
            return None;
        }
        let v = self.sum();
        self.depth -= 1;
        v
    }
}

/// What an amount and another added are: money with plain numbers stays
/// money; dollars and euros, or time and a number, are nothing.
fn same(x: Kind, y: Kind) -> Option<Kind> {
    match (x, y) {
        (x, y) if x == y => Some(x),
        (Kind::Money(s), Kind::Plain) | (Kind::Plain, Kind::Money(s)) => Some(Kind::Money(s)),
        _ => None,
    }
}

fn times(a: Value, b: Value) -> Option<Value> {
    let kind = match (a.kind, b.kind) {
        (Kind::Percent, Kind::Percent) => Kind::Percent,
        // "80 * 15%" is 12, not a share.
        (Kind::Plain, Kind::Percent) | (Kind::Percent, Kind::Plain) => Kind::Plain,
        (Kind::Percent | Kind::Plain, kind) | (kind, Kind::Percent | Kind::Plain) => kind,
        _ => return None,
    };
    Some(Value { n: a.n * b.n, kind })
}

fn divided(a: Value, b: Value) -> Option<Value> {
    if b.n == 0.0 {
        return None;
    }
    let kind = match (a.kind, b.kind) {
        (kind, Kind::Plain | Kind::Percent) => kind,
        // "$120 / $40" is 3; "1h / 15m" is 4.
        (x, y) if x == y => Kind::Plain,
        _ => return None,
    };
    Some(Value { n: a.n / b.n, kind })
}

// ----- totals of the lines above -------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum How {
    Sum,
    Average,
}

/// "sum", "Total:", "average =" (alone on its line, in any letter case).
fn total_word(text: &str) -> Option<How> {
    let word = text.trim().trim_end_matches('=').trim_end().trim_end_matches(':').trim_end();
    match word.to_lowercase().as_str() {
        "sum" | "total" => Some(How::Sum),
        "average" => Some(How::Average),
        _ => None,
    }
}

/// One amount on a line of a list: its number, typed, and its money sign.
#[derive(Clone, Copy, Debug)]
struct Amount {
    typed: Typed,
    sign: Option<&'static str>,
}

/// The amounts of the lines right above line `i`, from the nearest up: up
/// to a line with no number (or more than one), the start of its list, or
/// a line less indented. Lines indented more are passed over.
fn amounts_above(blocks: &[Block], i: usize) -> Vec<Amount> {
    let line = &blocks[i];
    // A total in a list adds up its list; a total under a list, that list.
    let mut kind = (line.kind != BlockKind::Text).then_some(line.kind);
    let mut out = Vec::new();
    for above in blocks[..i].iter().rev() {
        // (Not across into another column: see columns.rs.)
        if above.place.col != line.place.col {
            break;
        }
        if above.indent > line.indent && can_show(above.kind) {
            continue;
        }
        let kind = *kind.get_or_insert(above.kind);
        if above.indent < line.indent || above.kind != kind || !can_show(kind) {
            break;
        }
        match one_amount(&above.plain_text()) {
            Some(amount) => out.push(amount),
            None => break,
        }
    }
    out
}

/// The one number in a line of words: "Coffee 4.50", "- Rent $1,200",
/// "Lunch 12 €". Nothing for a line with no number or with two.
fn one_amount(text: &str) -> Option<Amount> {
    let mut found: Option<Amount> = None;
    let mut i = 0;
    // The character before `at`.
    let before = |at: usize| text[..at].chars().next_back();
    while i < text.len() {
        let rest = &text[i..];
        let ch = rest.chars().next()?;
        // Digits stuck to the end of a word are part of it: "Q3", "B2B".
        if before(i).is_some_and(|c| c.is_alphanumeric()) || !ch.is_ascii_digit() && ch != '.' {
            i += ch.len_utf8();
            continue;
        }
        let Some((mut typed, len)) = read_number(rest) else {
            i += ch.len_utf8();
            continue;
        };
        if found.is_some() {
            return None;
        }
        // A money sign right before it (after a space, maybe) or right after it.
        let head = text[..i].trim_end_matches(' ');
        let mut start = i;
        let mut sign = SIGNS.iter().find(|(c, _)| head.ends_with(*c)).map(|(c, s)| {
            start = head.len() - c.len_utf8();
            *s
        });
        let tail = text[i + len..].trim_start_matches(' ');
        if sign.is_none() {
            sign = SIGNS.iter().find(|(c, _)| tail.starts_with(*c)).map(|(_, s)| *s);
        }
        // A minus right before it (or before its sign): "-5", "-$5".
        if text[..start].ends_with(['-', '\u{2212}']) {
            let at = text[..start].trim_end_matches(['-', '\u{2212}']).len();
            if text[..at].chars().next_back().is_none_or(char::is_whitespace) {
                typed.value = -typed.value;
            }
        }
        found = Some(Amount { typed, sign });
        i += len;
    }
    found
}

/// The total of some amounts, written to be read; their money sign is kept
/// when they all have the same one (and dollars and euros add up to
/// nothing).
fn total(how: How, amounts: &[Amount]) -> Option<String> {
    if amounts.is_empty() {
        return None;
    }
    let mut signs = amounts.iter().filter_map(|a| a.sign);
    let sign = signs.next();
    if signs.any(|s| Some(s) != sign) {
        return None;
    }
    let sum: f64 = amounts.iter().map(|a| a.typed.value).sum();
    let n = match how {
        How::Sum => sum,
        How::Average => sum / amounts.len() as f64,
    };
    let kind = sign.map_or(Kind::Plain, Kind::Money);
    let places = amounts.iter().map(|a| a.typed.places).max().unwrap_or(0);
    let comma = amounts.iter().any(|a| a.typed.comma);
    write(Value { n, kind }, comma, places)
}

// ----- writing an answer ---------------------------------------------------

/// An answer as it shows: at most ten digits that mean something, grouped
/// in threes ("14,400"), with at least `places` after the point; a decimal
/// comma when the numbers were typed with one.
fn write(v: Value, comma: bool, places: usize) -> Option<String> {
    if !v.n.is_finite() {
        return None;
    }
    let text = match v.kind {
        Kind::Plain => number(v.n, places)?,
        Kind::Percent => format!("{}%", number(v.n * 100.0, 0)?),
        Kind::Money(sign) => {
            // Cents when there are any: "$36", "$7.50", "$3.33"; the yen has none.
            let cents = if sign == "¥" { 0 } else { 2 };
            let scale = 10f64.powi(cents);
            let rounded = (v.n * scale).round() / scale;
            let places = if rounded.fract() == 0.0 { 0 } else { cents as usize };
            let digits = number(rounded.abs(), places)?;
            if rounded < 0.0 { format!("-{sign}{digits}") } else { format!("{sign}{digits}") }
        }
        Kind::Time => return Some(duration(v.n)),
    };
    // (Swap the two signs: "1,234.5" is "1.234,5".)
    Some(if comma { text.chars().map(|c| match c { ',' => '.', '.' => ',', c => c }).collect() } else { text })
}

/// A number with at most ten digits that mean something: 0.1 + 0.2 is 0.3,
/// 1/3 is 0.3333333333, 2^40 is 1,099,511,628,000. Very large and very
/// small numbers are written with a power of ten: 1.844674407e19.
fn number(n: f64, places: usize) -> Option<String> {
    if !n.is_finite() {
        return None;
    }
    if n == 0.0 {
        return Some(money::grouped(0.0, places));
    }
    let size = n.abs().log10().floor() as i32;
    if !(-6..15).contains(&size) {
        let text = format!("{n:.9e}");
        let (digits, power) = text.split_once('e')?;
        let digits = if digits.contains('.') { digits.trim_end_matches('0').trim_end_matches('.') } else { digits };
        return Some(format!("{digits}e{power}"));
    }
    // Ten digits that mean something, then no zeros at the end.
    let rounded: f64 = format!("{n:.9e}").parse().ok()?;
    let most = (9 - size).max(0) as usize;
    let plain = format!("{rounded:.most$}");
    let typed = match plain.split_once('.') {
        Some((_, part)) => part.trim_end_matches('0').len(),
        None => 0,
    };
    Some(money::grouped(rounded, typed.max(places)))
}

/// A length of time: "3h 15m", "45m", "1h 20s".
fn duration(seconds: f64) -> String {
    let all = seconds.round() as i64;
    let (h, m, s) = (all.abs() / 3600, all.abs() % 3600 / 60, all.abs() % 60);
    let mut parts: Vec<String> = Vec::new();
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if s > 0 {
        parts.push(format!("{s}s"));
    }
    if parts.is_empty() {
        return "0m".to_string();
    }
    let text = parts.join(" ");
    if all < 0 { format!("-{text}") } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(text: &str) -> Option<String> {
        line(text)
    }

    #[test]
    fn a_line_that_ends_with_equals_shows_its_answer() {
        for (text, answer) in [
            ("12 × 4.5 =", "54"),
            ("12 * 4.5=", "54"),
            ("12 x 4.5 =", "54"),
            ("12x3 =", "36"),
            ("10 ÷ 4 =", "2.5"),
            ("10 / 4 =", "2.5"),
            ("2 + 3 * 4 =", "14"),
            ("(2 + 3) * 4 =", "20"),
            ("2 ^ 10 =", "1,024"),
            ("2^3^2 =", "512"),
            ("-3 + 5 =", "2"),
            ("10 - -2 =", "12"),
            ("Rent: 1200 * 12 =", "14,400"),
            ("Rent: 1,200 * 12 =", "14,400"),
            ("a = 3 * 4 =", "12"),
            ("sqrt(16) =", "4"),
            ("sqrt(2) =", "1.414213562"),
            ("round(2.567) =", "3"),
            ("round(2.567; 2) =", "2.57"),
            ("round(2.567, 1) =", "2.6"),
            ("√(81) + 1 =", "10"),
            ("1 − 3 =", "-2"),
        ] {
            assert_eq!(calc(text).as_deref(), Some(answer), "{text}");
        }
    }

    #[test]
    fn answers_have_no_float_noise_and_are_grouped() {
        for (text, answer) in [
            ("0.1 + 0.2 =", "0.3"),
            ("3 * 1.1 =", "3.3"),
            ("1 / 3 =", "0.3333333333"),
            ("2 / 3 =", "0.6666666667"),
            ("1000000 * 1000 =", "1,000,000,000"),
            ("123456789 * 10 =", "1,234,567,890"),
            ("2^40 =", "1,099,511,628,000"),
            ("2^64 =", "1.844674407e19"),
            ("1 / 3000000000 =", "3.333333333e-10"),
            ("0.5 - 0.5 =", "0"),
            ("1.5 * 2 =", "3"),
        ] {
            assert_eq!(calc(text).as_deref(), Some(answer), "{text}");
        }
    }

    #[test]
    fn percents_work_the_way_people_say_them() {
        for (text, answer) in [
            ("15% of 80 =", "12"),
            ("80 + 15% =", "92"),
            ("200 - 10% =", "180"),
            ("80 * 15% =", "12"),
            ("10% + 5% =", "15%"),
            ("$80 + 15% =", "$92"),
            ("15% of $80 =", "$12"),
        ] {
            assert_eq!(calc(text).as_deref(), Some(answer), "{text}");
        }
    }

    #[test]
    fn money_keeps_its_sign() {
        for (text, answer) in [
            ("$12 × 3 =", "$36"),
            ("$4.50 + $3 =", "$7.50"),
            ("$10 / 3 =", "$3.33"),
            ("€1.234,50 + €10 =", "€1.244,50"),
            ("12 € * 2 =", "€24"),
            ("£5 - £7 =", "-£2"),
            ("¥1000 / 3 =", "¥333"),
            ("$120 / $40 =", "3"),
            ("$1,500 * 12 =", "$18,000"),
        ] {
            assert_eq!(calc(text).as_deref(), Some(answer), "{text}");
        }
        // Dollars and euros do not add up, and there are no rates to change them.
        for text in ["$5 + €5 =", "€120 in USD =", "$5 * $5 ="] {
            assert_eq!(calc(text), None, "{text}");
        }
    }

    #[test]
    fn a_decimal_comma_gives_a_decimal_comma() {
        assert_eq!(calc("6,5 + 1 =").as_deref(), Some("7,5"));
        assert_eq!(calc("1.234,5 * 2 =").as_deref(), Some("2.469"));
        assert_eq!(calc("1.234,5 * 3 =").as_deref(), Some("3.703,5"));
        // A comma between the thousands is no decimal comma.
        assert_eq!(calc("1,500 + 0.5 =").as_deref(), Some("1,500.5"));
    }

    #[test]
    fn lengths_of_time_add_up() {
        for (text, answer) in [
            ("2h 30m + 45m =", "3h 15m"),
            ("45m + 30m =", "1h 15m"),
            ("1h - 20m =", "40m"),
            ("1h30m * 3 =", "4h 30m"),
            ("10m / 3 =", "3m 20s"),
            ("2h / 30m =", "4"),
            ("90 min + 1 hour =", "2h 30m"),
        ] {
            assert_eq!(calc(text).as_deref(), Some(answer), "{text}");
        }
        assert_eq!(calc("2h + 5 ="), None);
    }

    #[test]
    fn a_line_that_is_no_calculation_shows_nothing() {
        for text in [
            "x =",
            "=",
            " = ",
            "5 =",
            "$1200 =",
            "15% =",
            "Rent 1200 * 12 =",
            "a == b",
            "a == b =",
            "1 / 0 =",
            "5 % 0 =",
            "sqrt(-1) =",
            "2(3 + 4) =",
            "(2 + 3 =",
            "2 + =",
            "Total cost = price * qty =",
            "x = y",
            "Let me think =",
            "5km + 3km =",
            "1e5 + 1 =",
            "192.168.1.1 + 1 =",
            "2 x =",
            "x 2 =",
            "12 × 4.5",
            "10^400 =",
        ] {
            assert_eq!(calc(text), None, "{text}");
        }
        // Brackets nested too deep are no calculation, and do not overflow.
        let deep = format!("{}1{} =", "(".repeat(5000), ")".repeat(5000));
        assert_eq!(calc(&deep), None);
    }

    fn page(lines: &[(BlockKind, u8, &str)]) -> Vec<Block> {
        lines.iter().map(|(kind, indent, text)| Block::new(*kind, crate::inline::escape(text)).with_indent(*indent)).collect()
    }

    #[test]
    fn sum_adds_up_the_lines_above() {
        use BlockKind::{Bulleted as B, Heading2 as H, Numbered as N, Text as T, Todo as D};
        let blocks = page(&[
            (H, 0, "Week 45 spending"),
            (B, 0, "Coffee $4.50"),
            (B, 0, "Lunch $12"),
            (B, 0, "-$2 refund"),
            (T, 0, "sum"),
            (T, 0, "Groceries"),
            (N, 0, "Milk 2.5"),
            (N, 0, "Bread 3"),
            (N, 0, "Total:"),
            (T, 0, "12 × 4.5 ="),
            (T, 0, "Rent 1,200"),
            (T, 0, "Food 300"),
            (T, 0, "AVERAGE ="),
        ]);
        let got = answers(&blocks);
        assert_eq!(got[4].as_deref(), Some("= $14.50"));
        assert_eq!(got[8].as_deref(), Some("5.5"));
        assert_eq!(got[9].as_deref(), Some("54"));
        assert_eq!(got[12].as_deref(), Some("750"));
        assert!(got.iter().enumerate().all(|(i, a)| [4, 8, 9, 12].contains(&i) || a.is_none()), "{got:?}");
        // To-dos, the deeper lines of a list passed over, and a line with no number stops it.
        let blocks = page(&[
            (D, 0, "Pay 99"),
            (D, 0, "Call Ann"),
            (D, 0, "Book 40"),
            (D, 1, "Hotel 25"),
            (D, 1, "Train 15"),
            (D, 0, "Taxi 10"),
            (D, 0, "sum"),
        ]);
        assert_eq!(answers(&blocks)[6].as_deref(), Some("= 50"));
        // Mixed money is nothing; a line with two numbers ends the run.
        let blocks = page(&[(T, 0, "a $5"), (T, 0, "b €5"), (T, 0, "sum")]);
        assert_eq!(answers(&blocks)[2], None);
        let blocks = page(&[(T, 0, "9:30 call"), (T, 0, "b 5"), (T, 0, "Total")]);
        assert_eq!(answers(&blocks)[2].as_deref(), Some("= 5"));
        // Nothing above: nothing to show. A heading that says "Total" shows nothing.
        let blocks = page(&[(T, 0, "sum"), (T, 0, "a 1"), (H, 0, "Total")]);
        assert_eq!(answers(&blocks), vec![None, None, None]);
        // Q3 is a word; a decimal comma stays one.
        let blocks = page(&[(B, 0, "Q3 sales 6,5"), (B, 0, "Q4 sales 1,25"), (B, 0, "sum =")]);
        assert_eq!(answers(&blocks)[2].as_deref(), Some("7,75"));
    }

    #[test]
    fn only_lines_of_text_and_lists_show_an_answer() {
        use BlockKind::*;
        for kind in [Heading1, Heading2, Heading3, Code, Quote, Callout, Toggle] {
            assert_eq!(answers(&page(&[(kind, 0, "2 + 2 =")])), vec![None], "{kind:?}");
        }
        for kind in [Text, Bulleted, Numbered, Todo] {
            assert_eq!(answers(&page(&[(kind, 0, "2 + 2 =")])), vec![Some("4".to_string())], "{kind:?}");
        }
        // Formatting in the line is no part of the calculation.
        let blocks = vec![Block::text("**12** \\* 2 =")];
        assert_eq!(answers(&blocks), vec![Some("24".to_string())]);
    }
}
