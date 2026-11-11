//! Money and number formats of a table column.
//!
//! A cell stays text. A column with a format writes its numbers one way
//! ("$3,500.00", "1,500", "15%"), and a number is read back out of that
//! text. A sign typed in a cell ("$", "€", "%") is enough to know the format.

/// One kind of money.
#[derive(Debug, PartialEq, Eq)]
pub struct Currency {
    /// "USD": how a column's format names it.
    pub code: &'static str,
    /// What is written before the number: "$", or the code and a space.
    pub sign: &'static str,
    pub name: &'static str,
    /// Places after the point.
    pub places: u8,
}

const fn c(code: &'static str, sign: &'static str, name: &'static str, places: u8) -> Currency {
    Currency { code, sign, name, places }
}

/// The kinds of money a column can be. The first four are in the short menu.
pub const CURRENCIES: &[Currency] = &[
    c("USD", "$", "US Dollar", 2),
    c("EUR", "€", "Euro", 2),
    c("GBP", "£", "Pound", 2),
    c("JPY", "¥", "Yen", 0),
    c("AUD", "A$", "Australian dollar", 2),
    c("CAD", "CA$", "Canadian dollar", 2),
    c("NZD", "NZ$", "New Zealand dollar", 2),
    c("SGD", "S$", "Singapore dollar", 2),
    c("HKD", "HK$", "Hong Kong dollar", 2),
    c("MXN", "MX$", "Mexican peso", 2),
    c("BRL", "R$", "Real", 2),
    c("CHF", "CHF ", "Franc", 2),
    c("CNY", "CN¥", "Yuan", 2),
    c("INR", "₹", "Rupee", 2),
    c("KRW", "₩", "Won", 0),
    c("SEK", "SEK ", "Swedish krona", 2),
    c("NOK", "NOK ", "Norwegian krone", 2),
    c("DKK", "DKK ", "Danish krone", 2),
    c("PLN", "PLN ", "Złoty", 2),
    c("TRY", "₺", "Lira", 2),
    c("ILS", "₪", "Shekel", 2),
    c("THB", "฿", "Baht", 2),
    c("PHP", "₱", "Philippine peso", 2),
    c("VND", "₫", "Dong", 0),
    c("ZAR", "ZAR ", "Rand", 2),
    c("AED", "AED ", "Dirham", 2),
    c("NGN", "₦", "Naira", 2),
];

/// Formats that are not money.
pub const GROUPED: &str = "grouped";
pub const PERCENT: &str = "percent";

pub fn currency(code: &str) -> Option<&'static Currency> {
    CURRENCIES.iter().find(|c| c.code.eq_ignore_ascii_case(code.trim()))
}

/// The name of a format, for a menu: "US Dollar", "With separators".
pub fn format_label(format: &str) -> &'static str {
    match format {
        "" => "Number",
        GROUPED => "With separators",
        PERCENT => "Percent",
        code => currency(code).map(|c| c.name).unwrap_or("Number"),
    }
}

/// A format name as it is kept, or `None` if it is none of them.
pub fn known_format(word: &str) -> Option<&'static str> {
    let word = word.trim();
    if word.eq_ignore_ascii_case(GROUPED) {
        Some(GROUPED)
    } else if word.eq_ignore_ascii_case(PERCENT) {
        Some(PERCENT)
    } else {
        currency(word).map(|c| c.code)
    }
}

/// The money sign at the start of `text` (the longest that fits, so "CA$"
/// is not read as "$"), and the text after it.
fn leading_sign(text: &str) -> Option<(&'static Currency, &str)> {
    CURRENCIES
        .iter()
        .filter(|c| {
            let sign = c.sign.trim_end();
            text.get(..sign.len()).is_some_and(|start| start.eq_ignore_ascii_case(sign))
        })
        .max_by_key(|c| c.sign.trim_end().len())
        .map(|c| (c, text[c.sign.trim_end().len()..].trim_start()))
}

/// `text` without a minus sign and a money sign at its start, and whether
/// there was a minus: "-$5" and "$-5" are both (true, "5").
pub fn strip_sign(text: &str) -> (bool, &str) {
    let text = text.trim();
    let (minus, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text.strip_prefix('+').unwrap_or(text).trim_start()),
    };
    let text = leading_sign(text).map(|(_, rest)| rest).unwrap_or(text);
    match text.strip_prefix('-') {
        Some(rest) => (!minus, rest.trim_start()),
        None => (minus, text),
    }
}

/// These words are a money sign or the code of a currency and no more:
/// "€", "eur".
pub fn is_sign(words: &str) -> bool {
    let words = words.trim();
    !words.is_empty() && CURRENCIES.iter().any(|c| words.eq_ignore_ascii_case(c.sign.trim_end()) || words.eq_ignore_ascii_case(c.code))
}

/// The format that a typed cell asks for: "$3,500" is "USD", "20 €" and
/// "20 eur" are "EUR", "15%" is "percent". Nothing for a plain number or
/// for words.
pub fn typed_format(text: &str) -> Option<&'static str> {
    let text = text.trim();
    let body = text.strip_prefix(['-', '+']).unwrap_or(text).trim_start();
    let digits = |s: &str| !s.is_empty() && s.chars().next().is_some_and(|ch| ch.is_ascii_digit() || ch == '.') && s.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ' '));
    if let Some((cur, rest)) = leading_sign(body) {
        let rest = rest.strip_prefix('-').unwrap_or(rest).trim();
        return digits(rest).then_some(cur.code);
    }
    if let Some(number) = body.strip_suffix('%') {
        return digits(number.trim()).then_some(PERCENT);
    }
    // The sign or the code after the number: "20 €", "20 EUR".
    CURRENCIES.iter().find_map(|cur| {
        let tail = |end: &str| body.len() > end.len() && body.is_char_boundary(body.len() - end.len()) && body[body.len() - end.len()..].eq_ignore_ascii_case(end);
        [cur.sign.trim_end(), cur.code].into_iter().find(|end| tail(end)).filter(|end| digits(body[..body.len() - end.len()].trim())).map(|_| cur.code)
    })
}

/// A number with a comma between each three digits and `places` after the
/// point: 3500.5 is "3,500.50".
pub fn grouped(value: f64, places: usize) -> String {
    let plain = format!("{:.*}", places, value.abs());
    let (whole, part) = plain.split_once('.').unwrap_or((&plain, ""));
    let mut out = String::with_capacity(plain.len() + whole.len() / 3 + 1);
    // (A rounded "-0.00" is zero: no minus sign.)
    if value < 0.0 && plain.chars().any(|ch| ch.is_ascii_digit() && ch != '0') {
        out.push('-');
    }
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if !part.is_empty() {
        out.push('.');
        out.push_str(part);
    }
    out
}

/// A number as a format writes it. `places` is how many places after the
/// point a format with no number of its own uses.
pub fn write(format: &str, value: f64, places: Option<usize>) -> String {
    match format {
        "" => format!("{:.*}", places.unwrap_or(0), value),
        GROUPED => grouped(value, places.unwrap_or(0)),
        PERCENT => format!("{:.*}%", places.unwrap_or(0), value),
        code => match currency(code) {
            Some(cur) => {
                let number = grouped(value, places.unwrap_or(cur.places as usize));
                match number.strip_prefix('-') {
                    Some(rest) => format!("-{}{rest}", cur.sign),
                    None => format!("{}{number}", cur.sign),
                }
            }
            None => format!("{:.*}", places.unwrap_or(0), value),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_by_their_format() {
        assert_eq!(write("USD", 3500.0, None), "$3,500.00");
        assert_eq!(write("USD", -65.5, None), "-$65.50");
        assert_eq!(write("JPY", 1500.4, None), "¥1,500");
        assert_eq!(write("CHF", 20.0, None), "CHF 20.00");
        assert_eq!(write("EUR", 1234567.891, Some(1)), "€1,234,567.9");
        assert_eq!(write(GROUPED, 1500.0, None), "1,500");
        assert_eq!(write(GROUPED, 999.0, Some(2)), "999.00");
        assert_eq!(write(PERCENT, 15.0, None), "15%");
        assert_eq!(write("", 6.25, Some(1)), "6.2");
        assert_eq!(write("USD", -0.001, None), "$0.00");
        assert_eq!(write("???", 2.0, None), "2");
    }

    #[test]
    fn a_typed_sign_names_the_format() {
        assert_eq!(typed_format("$3,500"), Some("USD"));
        assert_eq!(typed_format(" -$65.00 "), Some("USD"));
        assert_eq!(typed_format("$-65"), Some("USD"));
        assert_eq!(typed_format("CA$ 12"), Some("CAD"));
        assert_eq!(typed_format("€20"), Some("EUR"));
        assert_eq!(typed_format("20 €"), Some("EUR"));
        assert_eq!(typed_format("20 eur"), Some("EUR"));
        assert_eq!(typed_format("£4.50"), Some("GBP"));
        assert_eq!(typed_format("¥1500"), Some("JPY"));
        assert_eq!(typed_format("15%"), Some(PERCENT));
        assert_eq!(typed_format("15 %"), Some(PERCENT));
        // Not money: a plain number, words, a sign alone, a sign in the middle.
        for text in ["3500", "", "$", "%", "$ a lot", "about $5", "5 dollars", "A$AP", "100% sure", "eur", "€€5"] {
            assert_eq!(typed_format(text), None, "{text}");
        }
    }

    #[test]
    fn signs_come_off_before_a_number_is_read() {
        assert_eq!(strip_sign("$3,500.00"), (false, "3,500.00"));
        assert_eq!(strip_sign("-$65"), (true, "65"));
        assert_eq!(strip_sign("$-65"), (true, "65"));
        assert_eq!(strip_sign("CHF 20"), (false, "20"));
        assert_eq!(strip_sign("  6.5 km"), (false, "6.5 km"));
        assert_eq!(strip_sign("+4"), (false, "4"));
        assert_eq!(strip_sign("words"), (false, "words"));
        assert_eq!(known_format("usd"), Some("USD"));
        assert_eq!(known_format("Percent"), Some(PERCENT));
        assert_eq!(known_format("km"), None);
        assert_eq!(format_label("EUR"), "Euro");
        assert_eq!(format_label(""), "Number");
    }
}
