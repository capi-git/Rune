//! Formulas for table columns. A formula belongs to one column and is worked
//! out once for each row, from the other cells of that row:
//!
//! ```text
//! [Unit price] * Qty * 1.2
//! if(Amount >= 100 and not Paid, "big", "")
//! ```
//!
//! A column is named by a bare word (`Qty`) or by any text in square brackets
//! (`[Unit price]`). Arithmetic with an empty cell gives nothing, not an
//! error, so a row with a missing cell shows an empty cell. The text is split
//! into tokens, the tokens are read into a tree, and the tree is worked out.

use std::cmp::Ordering;
use std::iter::Peekable;

/// The most characters a formula may have, and how deep it may nest.
const MOST_CHARACTERS: usize = 2000;
const MOST_LEVELS: usize = 64;

/// What a cell or a formula is worth.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Nothing there (an empty cell, a division by zero).
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
}

impl Value {
    /// As a number: a checkbox is 1 or 0, and a text counts when it is a plain
    /// number ("12", "-1.5"). Empty and any other text are not a number.
    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Empty => None,
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Text(text) => {
                let text = text.trim();
                let digits = text.strip_prefix('-').unwrap_or(text);
                // Only digits and one point: no "1e5", no "inf", no second minus.
                let plain = digits.chars().all(|c| c.is_ascii_digit() || c == '.');
                text.parse().ok().filter(|n: &f64| plain && n.is_finite())
            }
        }
    }

    /// True for a ticked checkbox, a number that is not 0 and a text that is
    /// not empty. Empty is false.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Empty => false,
            Value::Number(n) => *n != 0.0,
            Value::Text(text) => !text.is_empty(),
            Value::Bool(b) => *b,
        }
    }

    /// As text: nothing for Empty, and a whole number without `.0` at the end.
    fn text(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Number(n) => n.to_string(),
            Value::Text(text) => text.clone(),
            Value::Bool(b) => b.to_string(),
        }
    }
}

/// A formula read once, ready to be worked out for many rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Formula {
    root: Node,
}

/// One part of a formula, with the parts it is made of.
#[derive(Clone, Debug, PartialEq)]
enum Node {
    /// A number, a text, `true` or `false`.
    Literal(Value),
    Column(String),
    Not(Box<Node>),
    Negate(Box<Node>),
    /// A value and the operators after it, worked out from left to right:
    /// `a - b + c`. A list and not a deep tree, so a long sum can not use up
    /// the stack.
    Chain(Box<Node>, Vec<(Op, Node)>),
    Call(Func, Vec<Node>),
}

/// An operator between two values.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Op { Or, And, Eq, Ne, Lt, Le, Gt, Ge, Add, Sub, Mul, Div, Rem, Pow }

/// A function that a formula can call.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Func { If, Round, Floor, Ceil, Abs, Empty, Length, Min, Max, Sum, Average, Concat }

/// The functions: the name as written, then the least and the most values each takes.
const FUNCTIONS: [(&str, Func, usize, usize); 12] = [
    ("if", Func::If, 2, 3), ("round", Func::Round, 1, 2), ("floor", Func::Floor, 1, 1), ("ceil", Func::Ceil, 1, 1),
    ("abs", Func::Abs, 1, 1), ("empty", Func::Empty, 1, 1), ("length", Func::Length, 1, 1),
    ("min", Func::Min, 1, MANY), ("max", Func::Max, 1, MANY), ("sum", Func::Sum, 1, MANY),
    ("average", Func::Average, 1, MANY), ("concat", Func::Concat, 1, MANY),
];
const MANY: usize = usize::MAX;

impl Formula {
    /// Read a formula. `Err` holds one short sentence in plain words for the
    /// person, for example "A ) is missing.".
    pub fn parse(source: &str) -> Result<Formula, String> {
        if source.chars().count() > MOST_CHARACTERS {
            return Err("The formula is too long.".to_string());
        }
        let tokens = split(source)?;
        if tokens.is_empty() {
            return Err("The formula is empty.".to_string());
        }
        let mut parser = Parser { tokens: tokens.into_iter().peekable(), depth: 0 };
        let root = parser.either()?;
        match parser.tokens.next() {
            Some(extra) => Err(misplaced(&extra)),
            None => Ok(Formula { root }),
        }
    }

    /// The names of the columns it reads, as written, each once (without
    /// regard to case), in the order of first use.
    pub fn names(&self) -> Vec<String> {
        let mut names = Vec::new();
        collect(&self.root, &mut names);
        names
    }

    /// Work it out for one row. `cell` gives the value of a column by the name
    /// written in the formula; `None` means there is no such column, which is
    /// an error.
    pub fn eval(&self, cell: &dyn Fn(&str) -> Option<Value>) -> Result<Value, String> {
        work_out(&self.root, cell)
    }
}

// ----- tokens -----------------------------------------------------------------

/// One piece of a formula as written.
enum Token {
    /// A number, a text in quotes, `true` or `false`.
    Literal(Value),
    /// A bare word: a column, or a function when a `(` follows it.
    Word(String),
    /// A name in square brackets: always a column.
    Bracket(String),
    /// An operator, a round bracket or a comma. `and`, `or` and `not` are signs too.
    Sign(&'static str),
}

/// The signs, with the long ones first so that `<=` is not read as `<` and `=`.
const SIGNS: [&str; 17] = ["==", "!=", "<>", "<=", ">=", "=", "<", ">", "+", "-", "*", "/", "%", "^", "(", ")", ","];

/// Split a formula into tokens.
fn split(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        let token = if c.is_whitespace() {
            continue;
        } else if c.is_ascii_digit() || c == '.' {
            let mut digits = String::from(c);
            while let Some(more) = chars.next_if(|more| more.is_ascii_digit() || *more == '.') {
                digits.push(more);
            }
            // A point alone or a second point can not be read.
            Token::Literal(finite(digits.parse().map_err(|_| "The sign . does not belong here.")?))
        } else if c == '"' {
            Token::Literal(Value::Text(until(&mut chars, '"').ok_or("A closing \" is missing.")?))
        } else if c == '[' {
            Token::Bracket(until(&mut chars, ']').ok_or("A ] is missing.")?.trim().to_string())
        } else if c.is_alphabetic() || c == '_' {
            let mut word = String::from(c);
            while let Some(more) = chars.next_if(|more| more.is_alphanumeric() || *more == '_') {
                word.push(more);
            }
            match word.to_lowercase().as_str() {
                "and" => Token::Sign("and"),
                "or" => Token::Sign("or"),
                "not" => Token::Sign("not"),
                "true" => Token::Literal(Value::Bool(true)),
                "false" => Token::Literal(Value::Bool(false)),
                _ => Token::Word(word),
            }
        } else {
            // With the sign after it, to find a sign of two (`<=`) before a sign of one.
            let pair = format!("{c}{}", chars.peek().copied().unwrap_or(' '));
            let Some(sign) = SIGNS.iter().find(|sign| pair.starts_with(**sign)) else {
                return Err(format!("The sign {c} does not belong here."));
            };
            if sign.len() == 2 {
                chars.next();
            }
            Token::Sign(sign)
        };
        tokens.push(token);
    }
    Ok(tokens)
}

/// Read up to the closing sign `end`. `None` when it is missing.
fn until(chars: &mut Peekable<std::str::Chars>, end: char) -> Option<String> {
    let mut text = String::new();
    loop {
        match chars.next()? {
            c if c == end => return Some(text),
            // `\"` is a quote sign inside a text.
            '\\' if end == '"' && chars.next_if_eq(&'"').is_some() => text.push('"'),
            c => text.push(c),
        }
    }
}

/// The sentence for a token in a place where it can not be.
fn misplaced(token: &Token) -> String {
    let what = match token {
        Token::Sign(sign) if sign.chars().all(char::is_alphabetic) => format!("The word {sign}"),
        Token::Sign(sign) => format!("The sign {sign}"),
        Token::Word(word) => format!("The word {word}"),
        Token::Bracket(name) => format!("The name [{name}]"),
        Token::Literal(Value::Text(text)) => format!("The text \"{text}\""),
        Token::Literal(value) => format!("The value {}", value.text()),
    };
    format!("{what} does not belong here.")
}

// ----- the tree ---------------------------------------------------------------

/// Reads the tokens into a tree. Each level of binding has its own `fn`, from
/// `or` (the lowest) to a single value (the highest).
struct Parser {
    tokens: Peekable<std::vec::IntoIter<Token>>,
    /// How many round brackets, calls and prefix signs are open around this place.
    depth: usize,
}

impl Parser {
    /// `a or b`: the lowest level, so also a whole formula and the inside of round brackets.
    fn either(&mut self) -> Result<Node, String> {
        self.run(&[("or", Op::Or)], Parser::both)
    }

    /// `a and b`.
    fn both(&mut self) -> Result<Node, String> {
        self.run(&[("and", Op::And)], Parser::comparison)
    }

    /// `not a`, then `a < b`. Comparisons are not chained: `a < b < c` is an error.
    fn comparison(&mut self) -> Result<Node, String> {
        if self.take("not") {
            return Ok(Node::Not(Box::new(self.nested(Parser::comparison)?)));
        }
        let left = self.sum()?;
        let signs = [
            ("=", Op::Eq), ("==", Op::Eq), ("!=", Op::Ne), ("<>", Op::Ne),
            ("<", Op::Lt), ("<=", Op::Le), (">", Op::Gt), (">=", Op::Ge),
        ];
        match self.op(&signs) {
            Some(op) => Ok(Node::Chain(Box::new(left), vec![(op, self.sum()?)])),
            None => Ok(left),
        }
    }

    /// `a + b - c`.
    fn sum(&mut self) -> Result<Node, String> {
        self.run(&[("+", Op::Add), ("-", Op::Sub)], Parser::product)
    }

    /// `a * b / c % d`.
    fn product(&mut self) -> Result<Node, String> {
        self.run(&[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)], Parser::power)
    }

    /// `-a`, then `a ^ b`. The minus binds less than `^`, so `-2 ^ 2` is -4.
    /// `^` binds to the right, so `2 ^ 3 ^ 2` is 2 ^ 9.
    fn power(&mut self) -> Result<Node, String> {
        if self.take("-") {
            return Ok(Node::Negate(Box::new(self.nested(Parser::power)?)));
        }
        let base = self.value()?;
        match self.op(&[("^", Op::Pow)]) {
            Some(op) => Ok(Node::Chain(Box::new(base), vec![(op, self.nested(Parser::power)?)])),
            None => Ok(base),
        }
    }

    /// A single value: a number, a text, a column, a function call or round brackets.
    fn value(&mut self) -> Result<Node, String> {
        match self.tokens.next() {
            Some(Token::Literal(value)) => Ok(Node::Literal(value)),
            Some(Token::Bracket(name)) => Ok(Node::Column(name)),
            Some(Token::Word(word)) if self.take("(") => self.nested(|parser| parser.call(&word)),
            Some(Token::Word(word)) => Ok(Node::Column(word)),
            Some(Token::Sign("(")) => self.nested(|parser| {
                let inside = parser.either()?;
                parser.closing()?;
                Ok(inside)
            }),
            Some(other) => Err(misplaced(&other)),
            None => Err("A value is missing at the end.".to_string()),
        }
    }

    /// A function call, from after its `(` to its `)`.
    fn call(&mut self, word: &str) -> Result<Node, String> {
        let lower = word.to_lowercase();
        let Some(&(name, func, least, most)) = FUNCTIONS.iter().find(|known| known.0 == lower) else {
            return Err(format!("There is no function \"{word}\"."));
        };
        let mut values = Vec::new();
        if !self.take(")") {
            values.push(self.either()?);
            while self.take(",") {
                values.push(self.either()?);
            }
            self.closing()?;
        }
        if values.len() < least || values.len() > most {
            let needs = match (least, most) {
                (1, 1) => "one value",
                (1, 2) => "one or two values",
                (2, 3) => "two or three values",
                _ => "at least one value",
            };
            return Err(format!("{name} needs {needs}."));
        }
        Ok(Node::Call(func, values))
    }

    /// Operators of one level, worked out from left to right. `part` reads each value between them.
    fn run(&mut self, level: &[(&str, Op)], part: fn(&mut Parser) -> Result<Node, String>) -> Result<Node, String> {
        let first = part(self)?;
        let mut rest = Vec::new();
        while let Some(op) = self.op(level) {
            rest.push((op, part(self)?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    /// Read one level deeper, but never deeper than the limit, so that no formula can use up the stack.
    fn nested(&mut self, inner: impl FnOnce(&mut Parser) -> Result<Node, String>) -> Result<Node, String> {
        if self.depth >= MOST_LEVELS {
            return Err("The formula is nested too deep.".to_string());
        }
        self.depth += 1;
        let node = inner(self);
        self.depth -= 1;
        node
    }

    /// Take the next token if it is this sign.
    fn take(&mut self, sign: &str) -> bool {
        self.tokens.next_if(|token| matches!(token, Token::Sign(found) if *found == sign)).is_some()
    }

    /// Take the next token if it is one of these operators.
    fn op(&mut self, level: &[(&str, Op)]) -> Option<Op> {
        let Some(Token::Sign(sign)) = self.tokens.peek() else { return None };
        let op = level.iter().find(|(written, _)| written == sign)?.1;
        self.tokens.next();
        Some(op)
    }

    /// The next token must be a `)`.
    fn closing(&mut self) -> Result<(), String> {
        match self.tokens.next() {
            Some(Token::Sign(")")) => Ok(()),
            Some(other) => Err(misplaced(&other)),
            None => Err("A ) is missing.".to_string()),
        }
    }
}

/// Add the column names under a part of the tree, each once.
fn collect(node: &Node, names: &mut Vec<String>) {
    match node {
        Node::Literal(_) => {}
        Node::Column(name) if names.iter().any(|known| known.to_lowercase() == name.to_lowercase()) => {}
        Node::Column(name) => names.push(name.clone()),
        Node::Not(inner) | Node::Negate(inner) => collect(inner, names),
        Node::Chain(first, rest) => {
            collect(first, names);
            rest.iter().for_each(|(_, right)| collect(right, names));
        }
        Node::Call(_, values) => values.iter().for_each(|value| collect(value, names)),
    }
}

// ----- working out ------------------------------------------------------------

/// Work out one part of the tree for a row. The tree is never deeper than the
/// parser lets it be, so this can not use up the stack.
fn work_out(node: &Node, cell: &dyn Fn(&str) -> Option<Value>) -> Result<Value, String> {
    Ok(match node {
        Node::Literal(value) => value.clone(),
        Node::Column(name) => match cell(name) {
            Some(Value::Number(n)) => finite(n),
            Some(value) => value,
            None => return Err(format!("There is no column \"{name}\".")),
        },
        Node::Not(inner) => Value::Bool(!work_out(inner, cell)?.truthy()),
        Node::Negate(inner) => work_out(inner, cell)?.number().map_or(Value::Empty, |n| finite(-n)),
        Node::Chain(first, rest) => {
            let mut left = work_out(first, cell)?;
            for (op, right) in rest {
                left = match op {
                    // `and` and `or` stop early: when the left side decides, the right side is not worked out.
                    Op::And if !left.truthy() => Value::Bool(false),
                    Op::Or if left.truthy() => Value::Bool(true),
                    _ => apply(*op, &left, &work_out(right, cell)?),
                };
            }
            left
        }
        Node::Call(func, values) => call(*func, values, cell)?,
    })
}

/// A number as a value. One that is not finite (a division by zero, a number too big) is Empty.
fn finite(n: f64) -> Value {
    // `+ 0.0` turns a zero with a minus sign into a plain zero.
    if n.is_finite() { Value::Number(n + 0.0) } else { Value::Empty }
}

/// Work out one operator between two values.
fn apply(op: Op, left: &Value, right: &Value) -> Value {
    use Ordering::{Equal, Greater, Less};
    let is = |wanted: &[Ordering]| Value::Bool(order(left, right).is_some_and(|found| wanted.contains(&found)));
    // Arithmetic needs two numbers. With an empty cell or a word it gives nothing.
    let math = |work: fn(f64, f64) -> f64| match (left.number(), right.number()) {
        (Some(a), Some(b)) => finite(work(a, b)),
        _ => Value::Empty,
    };
    let words = |value: &Value| matches!(value, Value::Text(_)) && value.number().is_none();
    match op {
        Op::Or => Value::Bool(left.truthy() || right.truthy()),
        Op::And => Value::Bool(left.truthy() && right.truthy()),
        Op::Eq => is(&[Equal]),
        Op::Ne => Value::Bool(order(left, right) != Some(Equal)),
        Op::Lt => is(&[Less]),
        Op::Le => is(&[Less, Equal]),
        Op::Gt => is(&[Greater]),
        Op::Ge => is(&[Greater, Equal]),
        // `+` joins the two as text when a side is a text that is not a number.
        Op::Add if words(left) || words(right) => Value::Text(left.text() + &right.text()),
        Op::Add => math(|a, b| a + b),
        Op::Sub => math(|a, b| a - b),
        Op::Mul => math(|a, b| a * b),
        // A division or a remainder by zero is not finite, so it gives Empty.
        Op::Div => math(|a, b| a / b),
        Op::Rem => math(|a, b| a % b),
        Op::Pow => math(f64::powf),
    }
}

/// How two values compare: as numbers when both are numbers, else as text
/// without regard to case. `None` when only one side is empty: an empty side
/// is equal to another empty side, but never less or more than a value.
fn order(left: &Value, right: &Value) -> Option<Ordering> {
    if let (Some(a), Some(b)) = (left.number(), right.number()) {
        return a.partial_cmp(&b);
    }
    let (a, b) = (left.text().to_lowercase(), right.text().to_lowercase());
    if a.is_empty() != b.is_empty() { None } else { Some(a.cmp(&b)) }
}

/// Work out a function for a row.
fn call(func: Func, values: &[Node], cell: &dyn Fn(&str) -> Option<Value>) -> Result<Value, String> {
    // A value that was left out (the `else` of an `if`) counts as Empty.
    let value = |at: usize| values.get(at).map_or(Ok(Value::Empty), |node| work_out(node, cell));
    let one = |given: Value, work: fn(f64) -> f64| given.number().map_or(Value::Empty, |n| finite(work(n)));
    Ok(match func {
        // Only the chosen branch is worked out.
        Func::If => {
            if value(0)?.truthy() { value(1)? } else { value(2)? }
        }
        Func::Round => {
            let n = value(0)?.number();
            let places = if values.len() > 1 { value(1)?.number() } else { Some(0.0) };
            match (n, places) {
                (Some(n), Some(places)) => {
                    // At most 15 places after the point. Less than zero rounds to tens, hundreds and so on.
                    let places = places.clamp(-15.0, 15.0) as i32;
                    let step = 10f64.powi(places.abs());
                    finite(if places < 0 { (n / step).round() * step } else { (n * step).round() / step })
                }
                _ => Value::Empty,
            }
        }
        Func::Floor => one(value(0)?, f64::floor),
        Func::Ceil => one(value(0)?, f64::ceil),
        Func::Abs => one(value(0)?, f64::abs),
        Func::Empty => Value::Bool(value(0)?.text().trim().is_empty()),
        Func::Length => Value::Number(value(0)?.text().chars().count() as f64),
        Func::Min | Func::Max | Func::Sum | Func::Average => {
            // Empty cells and words are left out.
            let mut numbers = Vec::new();
            for node in values {
                numbers.extend(work_out(node, cell)?.number());
            }
            let sum: f64 = numbers.iter().sum();
            match func {
                _ if numbers.is_empty() => Value::Empty,
                Func::Min => finite(numbers.iter().copied().fold(f64::INFINITY, f64::min)),
                Func::Max => finite(numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                Func::Sum => finite(sum),
                _ => finite(sum / numbers.len() as f64),
            }
        }
        Func::Concat => {
            let mut joined = String::new();
            for node in values {
                joined.push_str(&work_out(node, cell)?.text());
            }
            Value::Text(joined)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const YES: Value = Value::Bool(true);
    const NO: Value = Value::Bool(false);
    const NONE: Value = Value::Empty;

    fn n(number: f64) -> Value {
        Value::Number(number)
    }

    fn t(text: &str) -> Value {
        Value::Text(text.to_string())
    }

    /// One row to work the formulas out for. Names match without regard to case.
    fn row(name: &str) -> Option<Value> {
        let cells = [("price", n(10.0)), ("qty", n(3.0)), ("unit price", n(2.5)), ("amount", n(150.0)), ("a", n(4.0)), ("b", n(9.0))];
        let more = [("größe", n(7.0)), ("_x1", n(1.0)), ("paid", YES), ("name", t("Ann")), ("blank", NONE)];
        cells.into_iter().chain(more).find(|(known, _)| *known == name.to_lowercase()).map(|(_, value)| value)
    }

    fn calc(source: &str) -> Result<Value, String> {
        Formula::parse(source)?.eval(&row)
    }

    /// Each formula must be worth the value next to it.
    fn all(cases: &[(&str, Value)]) {
        for (source, wanted) in cases {
            assert_eq!(calc(source).as_ref(), Ok(wanted), "{source}");
        }
    }

    /// Each formula must fail with the sentence next to it.
    fn fail(cases: &[(&str, &str)]) {
        for (source, sentence) in cases {
            assert_eq!(calc(source), Err(sentence.to_string()), "{source}");
        }
    }

    #[test]
    fn reads_numbers_text_and_the_examples() {
        all(&[
            ("12", n(12.0)), ("1.5", n(1.5)), (".5", n(0.5)), ("5.", n(5.0)), ("  7\n", n(7.0)), ("true", YES), ("FALSE", NO),
            ("\"big\"", t("big")), (r#""say \"hi\"""#, t("say \"hi\"")), (r#""a\b""#, t("a\\b")), ("\"[1 + 2]\"", t("[1 + 2]")),
            ("Price * Qty", n(30.0)), ("[Unit price] * Qty * 1.2", n(9.0)), ("if(Paid, 0, Amount)", n(0.0)),
            ("round(Amount / 3, 2)", n(50.0)), ("round(Price / 3, 2)", n(3.33)), ("max(A, B) - min(A, B)", n(5.0)),
            ("if(Amount >= 100 and not Paid, \"big\", \"\")", t("")), ("if(Amount >= 100 and Paid, \"big\", \"\")", t("big")),
        ]);
    }

    #[test]
    fn operators_bind_in_order() {
        all(&[
            ("1 + 2 * 3", n(7.0)), ("(1 + 2) * 3", n(9.0)), ("10 - 4 - 3", n(3.0)), ("8 / 2 / 2", n(2.0)), ("7 % 4", n(3.0)),
            ("2 * 7 % 4", n(2.0)), ("1 + 7 % 4", n(4.0)), ("2 * 3 ^ 2", n(18.0)), ("-2 ^ 2", n(-4.0)), ("(-2) ^ 2", n(4.0)),
            ("2 ^ 3 ^ 2", n(512.0)), ("2 ^ -1", n(0.5)), ("2 * -3", n(-6.0)), ("1 - -1", n(2.0)), ("--3", n(3.0)), ("1-1", n(0.0)),
            ("1 + 2 = 3", YES), ("2 * 3 > 5 + 0", YES), ("not 1 = 2", YES), ("not true or true", YES), ("not (true or true)", NO),
            ("true or false and false", YES), ("(true or false) and false", NO), ("1 < 2 and 2 < 3", YES),
            ("NOT not TRUE", YES), ("True AND False", NO), ("false Or true", YES),
        ]);
    }

    #[test]
    fn compares_numbers_and_text() {
        all(&[
            ("1 = 1", YES), ("1 == 2", NO), ("1 != 2", YES), ("1 <> 1", NO), ("1 < 2", YES), ("2 < 2", NO), ("2 <= 2", YES),
            ("1 > 2", NO), ("3 > 2", YES), ("2 >= 3", NO), ("3>=3", YES), ("1<>2", YES), ("\"10\" > \"9\"", YES),
            ("\"12\" = 12.0", YES), ("\"Big\" = \"big\"", YES), ("\"a\" < \"B\"", YES), ("\"b\" >= \"A\"", YES),
            ("\"b\" > \"abc\"", YES), ("Name != \"Bob\"", YES), ("Name = \"ANN\"", YES), ("Paid = true", YES), ("Paid = 1", YES),
            ("Paid = \"TRUE\"", YES), ("Blank = \"\"", YES), ("Blank = Blank", YES), ("Blank <= Blank", YES),
            ("Blank != Blank", NO), ("Blank < Blank", NO), ("Blank = 0", NO), ("Blank != 0", YES), ("Blank < 5", NO),
            ("Blank > 5", NO), ("Blank <= 5", NO), ("5 >= Blank", NO), ("Blank < \"a\"", NO), ("\"a\" > Blank", NO),
        ]);
        fail(&[("1 < 2 < 3", "The sign < does not belong here."), ("1 = 1 = 1", "The sign = does not belong here.")]);
    }

    #[test]
    fn joins_text_and_keeps_a_missing_cell_empty() {
        all(&[
            ("\"a\" + \"b\"", t("ab")), ("Name + \" \" + 1", t("Ann 1")), ("1.50 + Name", t("1.5Ann")), ("Name + Blank", t("Ann")),
            ("Blank + Name", t("Ann")), ("Name + Paid", t("Anntrue")), ("\"1\" + 2", n(3.0)), ("\" 2 \" * \"-.5\"", n(-1.0)),
            ("Paid + 1", n(2.0)), ("Blank + 1", NONE), ("1 - Blank", NONE), ("Blank * 2", NONE), ("2 / Blank", NONE),
            ("Blank % 2", NONE), ("Blank ^ 2", NONE), ("-Blank", NONE), ("Blank + Blank", NONE), ("Name * 2", NONE),
            ("Name - 1", NONE), ("-Name", NONE), ("1 / 0", NONE), ("5 % 0", NONE), ("0 / 0", NONE), ("1 / 0 + 1", NONE),
            ("(0 - 8) ^ 0.5", NONE), ("10 ^ 400", NONE), ("concat(0 * -1)", t("0")), ("0 - 7 % 4", n(-3.0)),
        ]);
        // A number that is not finite counts as empty, in a cell and as written.
        let formula = Formula::parse("A").unwrap();
        assert_eq!(formula.eval(&|_| Some(n(f64::NAN))), Ok(NONE));
        assert_eq!(formula.eval(&|_| Some(n(f64::INFINITY))), Ok(NONE));
        assert_eq!(calc(&"9".repeat(400)), Ok(NONE));
    }

    #[test]
    fn logic_uses_truth_and_stops_early() {
        all(&[
            ("1 and \"x\"", YES), ("0 or \"\"", NO), ("Blank or 2", YES), ("Blank and 2", NO), ("not Blank", YES), ("not Name", NO),
            ("not 0", YES), ("false and Missing", NO), ("true or Missing", YES), ("false and Missing or true", YES),
        ]);
        let missing = "There is no column \"Missing\".";
        fail(&[("true and Missing", missing), ("false or Missing", missing), ("not Missing", missing)]);
    }

    #[test]
    fn works_out_each_function() {
        all(&[
            ("if(1 > 2, \"a\", \"b\")", t("b")), ("if(Paid, 1)", n(1.0)), ("if(not Paid, 1)", NONE), ("if(Paid, 1, Missing)", n(1.0)),
            ("if(Blank, Missing, 2)", n(2.0)), ("IF(true, if(false, 1, 2), 3)", n(2.0)),
            ("round(2.5)", n(3.0)), ("round(-2.5)", n(-3.0)), ("round(2.4)", n(2.0)), ("round(2.346, 2)", n(2.35)),
            ("round(1234.5678, 1)", n(1234.6)), ("round(1250, -2)", n(1300.0)), ("round(1.29, 1.9)", n(1.3)),
            ("round(1.5, 99)", n(1.5)), ("round(Blank)", NONE), ("round(1.5, Blank)", NONE), ("round(Name, 1)", NONE),
            ("floor(1.9)", n(1.0)), ("floor(-1.1)", n(-2.0)), ("ceil(1.1)", n(2.0)), ("ceil(-1.9)", n(-1.0)), ("abs(-3)", n(3.0)),
            ("abs(3)", n(3.0)), ("abs(Blank)", NONE), ("floor(Name)", NONE), ("ceil(Blank)", NONE),
            ("min(3, 1, 2)", n(1.0)), ("max(3, 1, 2)", n(3.0)), ("sum(1, 2, 3)", n(6.0)), ("average(1, 2, 6)", n(3.0)),
            ("min(Blank, 5, Name)", n(5.0)), ("max(Blank, \"7\", 5)", n(7.0)), ("sum(Blank, 2, Name)", n(2.0)),
            ("average(Blank, 2, 4)", n(3.0)), ("sum(Paid, Paid)", n(2.0)), ("min(7)", n(7.0)), ("min(Blank)", NONE),
            ("max(Name, Blank)", NONE), ("sum(Blank, Blank)", NONE), ("average(Blank)", NONE),
            ("empty(Blank)", YES), ("empty(\"\")", YES), ("empty(\" \")", YES), ("empty(1 / 0)", YES), ("empty(0)", NO),
            ("empty(Name)", NO), ("empty(false)", NO), ("length(Name)", n(3.0)), ("length(12.50)", n(4.0)),
            ("length(Blank)", n(0.0)), ("length(\"Größe\")", n(5.0)), ("concat(Blank)", t("")),
            ("concat(Name, \" \", 3.0, Blank, 1.5, true)", t("Ann 31.5true")),
            ("ROUND(2.4)", n(2.0)), ("Max (1, 2)", n(2.0)), ("sum(1, sum(2, 3)) * 2", n(12.0)),
        ]);
    }

    #[test]
    fn reads_column_names() {
        all(&[
            ("[Unit price] * Qty", n(7.5)), ("[ unit PRICE ]", n(2.5)), ("Größe + 1", n(8.0)), ("[Größe]", n(7.0)),
            ("price", n(10.0)), ("_x1 + 1", n(2.0)), ("[A]+[B]", n(13.0)),
        ]);
        // The name goes to the caller as written, without the spaces around it.
        assert_eq!(Formula::parse("[ Unit price ]").unwrap().eval(&|name| Some(t(name))), Ok(t("Unit price")));
        assert_eq!(Formula::parse("[a-b (c), \"d\"]").unwrap().names(), ["a-b (c), \"d\""]);
        // A word is a function only when a ( follows it, and [and] is a column.
        let formula = Formula::parse("sum + if + [and]").unwrap();
        assert_eq!(formula.names(), ["sum", "if", "and"]);
        assert_eq!(formula.eval(&|_| Some(n(2.0))), Ok(n(6.0)));
        let formula = Formula::parse("[Unit price] * Qty + qty - if(Paid, 0, max(Amount, [ unit price ])) > 0 and not _x1").unwrap();
        assert_eq!(formula.names(), ["Unit price", "Qty", "Paid", "Amount", "_x1"]);
        assert!(Formula::parse("1 + 2").unwrap().names().is_empty());
        assert_eq!(Formula::parse("1+2*A"), Formula::parse(" 1 + 2 * A "));
        assert_eq!(formula.clone(), formula);
    }

    #[test]
    fn says_what_is_wrong() {
        let (end, one) = ("A value is missing at the end.", "needs one value.");
        fail(&[
            ("", "The formula is empty."), ("  \n ", "The formula is empty."), ("(1 + 2", "A ) is missing."),
            ("min(1, 2", "A ) is missing."), ("[x", "A ] is missing."), ("[", "A ] is missing."),
            ("\"abc", "A closing \" is missing."), ("\"abc\\\"", "A closing \" is missing."),
            ("1 +", end), ("if(", end), ("((((", end), ("not", end), ("-", end), ("2 ^", end), ("min(1,", end),
            ("1 + * 2", "The sign * does not belong here."), ("1 )", "The sign ) does not belong here."),
            (",", "The sign , does not belong here."), ("()", "The sign ) does not belong here."),
            ("(1, 2)", "The sign , does not belong here."), ("1 # 2", "The sign # does not belong here."),
            ("1 ! 2", "The sign ! does not belong here."), (".", "The sign . does not belong here."),
            ("1.2.3", "The sign . does not belong here."), ("]", "The sign ] does not belong here."),
            ("[x](1)", "The sign ( does not belong here."), ("true(1)", "The sign ( does not belong here."),
            ("= 1", "The sign = does not belong here."), ("1 2", "The value 2 does not belong here."),
            ("min(1 2)", "The value 2 does not belong here."), ("1 true", "The value true does not belong here."),
            ("1 \"a\"", "The text \"a\" does not belong here."), ("Unit price", "The word price does not belong here."),
            ("1 [x]", "The name [x] does not belong here."), ("1 and or 2", "The word or does not belong here."),
            ("1 + not 2", "The word not does not belong here."),
            ("foo(1)", "There is no function \"foo\"."), ("Größe(1)", "There is no function \"Größe\"."),
            ("min()", "min needs at least one value."), ("max()", "max needs at least one value."),
            ("sum()", "sum needs at least one value."), ("average()", "average needs at least one value."),
            ("CONCAT()", "concat needs at least one value."), ("floor()", &format!("floor {one}")),
            ("ceil()", &format!("ceil {one}")), ("abs(1, 2)", &format!("abs {one}")), ("empty()", &format!("empty {one}")),
            ("length(1, 2)", &format!("length {one}")), ("round()", "round needs one or two values."),
            ("round(1, 2, 3)", "round needs one or two values."), ("if(1)", "if needs two or three values."),
            ("if(1, 2, 3, 4)", "if needs two or three values."),
            ("Foo + 1", "There is no column \"Foo\"."), ("[No such]", "There is no column \"No such\"."),
            ("[]", "There is no column \"\"."),
        ]);
    }

    #[test]
    fn keeps_to_the_limits() {
        let brackets = |levels: usize| format!("{}1{}", "(".repeat(levels), ")".repeat(levels));
        let deep = Err("The formula is nested too deep.".to_string());
        assert_eq!(calc(&brackets(64)), Ok(n(1.0)));
        assert_eq!(calc(&brackets(65)), deep);
        assert_eq!(calc(&"(".repeat(1999)), deep);
        assert_eq!(calc(&format!("{}1", "-".repeat(64))), Ok(n(1.0)));
        assert_eq!(calc(&format!("{}1", "-".repeat(65))), deep);
        assert_eq!(calc(&format!("{}1", "not ".repeat(65))), deep);
        assert_eq!(calc(&format!("2{}", "^1".repeat(64))), Ok(n(2.0)));
        assert_eq!(calc(&format!("2{}", "^1".repeat(65))), deep);
        assert_eq!(calc(&format!("{}1{}", "abs(".repeat(65), ")".repeat(65))), deep);
        assert_eq!(calc(&"if(1,".repeat(399)), deep);
        // The deepest tree there can be: every kind of operator and a call on each of the 64 levels.
        let deepest = (0..64).fold("1".to_string(), |inside, _| format!("0 or 1 and 2=3+4*abs({inside})^2"));
        let formula = Formula::parse(&deepest).unwrap();
        assert_eq!((formula.eval(&row), formula.names().len(), formula.clone() == formula), (Ok(NO), 0, true));
        // A long formula that is not nested is no problem.
        assert_eq!(calc(&format!("1{}", "+1".repeat(999))), Ok(n(1000.0)));
        assert_eq!(calc(&format!("1{}", " or 0".repeat(399))), Ok(YES));
        // The limit of length counts characters, not bytes.
        let long = Err("The formula is too long.".to_string());
        assert_eq!(calc(&format!("{:>2000}", "1")), Ok(n(1.0)));
        assert_eq!(calc(&format!("{:>2001}", "1")), long);
        assert_eq!(calc(&"(".repeat(10_000)), long);
        assert_eq!(calc(&format!("length(\"{}\")", "ß".repeat(1900))), Ok(n(1900.0)));
    }

    #[test]
    fn hostile_input_fails_and_never_panics() {
        let hostile = [
            "((((", "\"abc", "1 +", "[x", "if(", ",", "))))", "]", "\\", "\"", "[", "!@#$%^&*", "= = =", "1 ..2", "..", "if(,)",
            "min(,", "min(1,)", "^", "*/%", "\0", "🙂", "1 +\u{0}", "a.b", "<>", "'x'", "and", "(,)", "[]]", "\"\"\"", "1e5",
            "1 = = 1", "“big”", "1 + + 2", "a b c", "if if(", "[a] [b]", "--", "not not", "()()",
        ];
        for source in hostile {
            let sentence = calc(source).expect_err(source);
            assert!(sentence.ends_with('.') && !sentence.contains('\n'), "{source}: {sentence}");
        }
        // Formulas made of pieces put together at random (the same ones each time).
        let pieces = [
            "(", ")", "[", "]", "\"", ",", "+", "-", "*", "/", "%", "^", "=", "<", ">", "!", "1", ".5", "0", "a", " ", "\\", "#", "ß",
            "if(", "min(", "round(", "concat(", "empty(", "not ", " and ", " or ", "true", "\"x\"", "[a]", "Blank",
        ];
        let mut seed: u64 = 42;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        let (mut good, mut bad) = (0, 0);
        for _ in 0..20_000 {
            let source: String = (0..1 + next() % 12).map(|_| pieces[next() % pieces.len()]).collect();
            match calc(&source) {
                Ok(_) => good += 1,
                Err(sentence) => {
                    assert!(sentence.ends_with('.'), "{source}: {sentence}");
                    bad += 1;
                }
            }
        }
        assert!(good > 100 && bad > 100, "{good} good, {bad} bad");
    }

    #[test]
    fn value_as_a_number_and_as_a_truth() {
        assert_eq!((n(1.5).number(), YES.number(), NO.number(), NONE.number()), (Some(1.5), Some(1.0), Some(0.0), None));
        let texts = [
            ("12", Some(12.0)), (" -1.5 ", Some(-1.5)), (".5", Some(0.5)), ("5.", Some(5.0)), ("-.5", Some(-0.5)), ("", None),
            ("-", None), (".", None), ("-.", None), ("1.2.3", None), ("--1", None), ("1e5", None), ("+1", None), ("1 000", None),
            ("1,5", None), ("inf", None), ("NaN", None), ("12 km", None), ("١٢", None),
        ];
        for (text, wanted) in texts {
            assert_eq!(t(text).number(), wanted, "{text}");
        }
        assert_eq!(t(&"9".repeat(400)).number(), None);
        assert!(YES.truthy() && n(-2.0).truthy() && t("x").truthy() && t("0").truthy());
        assert!(!NO.truthy() && !n(0.0).truthy() && !t("").truthy() && !NONE.truthy());
    }
}
