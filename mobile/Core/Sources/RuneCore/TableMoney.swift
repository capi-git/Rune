// Money and number formats of a table column, and how a number is read out
// of a cell. A cell stays text: a column with a format writes its numbers
// one way ("$3,500.00", "1,500", "15%"), and a number is read back out of
// that text. Mirrors `src/table/money.rs` (and the number reading of
// `src/table.rs`) of the desktop app.

import Foundation

/// The money and number formats a column can have.
public enum Money {
    /// One kind of money.
    public struct Currency: Hashable, Sendable {
        /// "USD": how a column's format names it.
        public let code: String
        /// What is written before the number: "$", or the code and a space.
        public let sign: String
        public let name: String
        /// Places after the point.
        public let places: Int
    }

    private static func c(_ code: String, _ sign: String, _ name: String, _ places: Int) -> Currency {
        Currency(code: code, sign: sign, name: name, places: places)
    }

    /// The kinds of money a column can be (the same list as the Mac's).
    public static let currencies: [Currency] = [
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
    ]

    /// Formats that are not money.
    public static let grouped = "grouped"
    public static let percent = "percent"

    public static func currency(_ code: String) -> Currency? {
        let code = code.trimmingCharacters(in: .whitespacesAndNewlines)
        return currencies.first { asciiEqual($0.code, code) }
    }

    /// The name of a format, for a menu: "US Dollar", "With separators".
    public static func formatLabel(_ format: String) -> String {
        switch format {
        case "": "Number"
        case grouped: "With separators"
        case percent: "Percent"
        default: currency(format)?.name ?? "Number"
        }
    }

    /// A format name as it is kept, or nil if it is none of them.
    public static func knownFormat(_ word: String) -> String? {
        let word = word.trimmingCharacters(in: .whitespacesAndNewlines)
        if asciiEqual(word, grouped) { return grouped }
        if asciiEqual(word, percent) { return percent }
        return currency(word)?.code
    }

    /// The money sign at the start of `text` (the longest that fits, so
    /// "CA$" is not read as "$"), and the text after it.
    static func leadingSign(_ text: String) -> (Currency, String)? {
        var best: (Currency, Int)?
        for cur in currencies {
            let sign = trimEnd(cur.sign)
            let n = sign.utf8.count
            guard hasASCIIPrefix(text, sign) else { continue }
            // (The last of equal lengths, as Rust's `max_by_key` keeps.)
            if best == nil || n >= best!.1 { best = (cur, n) }
        }
        guard let (cur, n) = best else { return nil }
        return (cur, trimStart(dropBytes(text, n)))
    }

    /// `text` without a minus sign and a money sign at its start, and
    /// whether there was a minus: "-$5" and "$-5" are both (true, "5").
    public static func stripSign(_ text: String) -> (minus: Bool, rest: String) {
        var text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        var minus = false
        if text.hasPrefix("-") {
            minus = true
            text = trimStart(String(text.dropFirst()))
        } else {
            if text.hasPrefix("+") { text = String(text.dropFirst()) }
            text = trimStart(text)
        }
        if let (_, rest) = leadingSign(text) { text = rest }
        if text.hasPrefix("-") {
            return (!minus, trimStart(String(text.dropFirst())))
        }
        return (minus, text)
    }

    /// These words are a money sign or the code of a currency and no more:
    /// "€", "eur".
    public static func isSign(_ words: String) -> Bool {
        let words = words.trimmingCharacters(in: .whitespacesAndNewlines)
        return !words.isEmpty && currencies.contains { asciiEqual(words, trimEnd($0.sign)) || asciiEqual(words, $0.code) }
    }

    /// The format that a typed cell asks for: "$3,500" is "USD", "20 €"
    /// and "20 eur" are "EUR", "15%" is "percent". Nothing for a plain
    /// number or for words.
    public static func typedFormat(_ text: String) -> String? {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        var body = text
        if body.hasPrefix("-") || body.hasPrefix("+") { body = String(body.dropFirst()) }
        body = trimStart(body)
        func digits(_ s: String) -> Bool {
            guard let first = s.unicodeScalars.first, first.isASCIIDigit || first == "." else { return false }
            return s.unicodeScalars.allSatisfy { $0.isASCIIDigit || $0 == "." || $0 == "," || $0 == " " }
        }
        if let (cur, rest) = leadingSign(body) {
            var rest = rest
            if rest.hasPrefix("-") { rest = String(rest.dropFirst()) }
            return digits(rest.trimmingCharacters(in: .whitespacesAndNewlines)) ? cur.code : nil
        }
        if body.hasSuffix("%") {
            return digits(String(body.dropLast()).trimmingCharacters(in: .whitespacesAndNewlines)) ? percent : nil
        }
        // The sign or the code after the number: "20 €", "20 EUR".
        let bytes = Array(body.utf8)
        for cur in currencies {
            for end in [trimEnd(cur.sign), cur.code] {
                let tail = Array(end.utf8)
                guard bytes.count > tail.count, asciiLower(Array(bytes.suffix(tail.count))) == asciiLower(tail),
                      let head = String(bytes: bytes.prefix(bytes.count - tail.count), encoding: .utf8)
                else { continue }
                // (The first ending that fits decides, as on the Mac.)
                if digits(head.trimmingCharacters(in: .whitespacesAndNewlines)) { return cur.code }
                break
            }
        }
        return nil
    }

    /// A number with a comma between each three digits and `places` after
    /// the point: 3500.5 is "3,500.50".
    public static func grouped(_ value: Double, places: Int) -> String {
        let plain = fixed(abs(value), places)
        let split = plain.split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        let whole = Array(split[0])
        let part = split.count > 1 ? String(split[1]) : ""
        var out = ""
        // (A rounded "-0.00" is zero: no minus sign.)
        if value < 0, plain.contains(where: { $0.isASCII && $0.isNumber && $0 != "0" }) {
            out.append("-")
        }
        for (i, ch) in whole.enumerated() {
            if i > 0, (whole.count - i) % 3 == 0 { out.append(",") }
            out.append(ch)
        }
        if !part.isEmpty {
            out.append(".")
            out.append(part)
        }
        return out
    }

    /// A number as a format writes it. `places` is how many places after
    /// the point a format with no number of its own uses.
    public static func write(_ format: String, _ value: Double, places: Int?) -> String {
        switch format {
        case "": return fixed(value, places ?? 0)
        case grouped: return Money.grouped(value, places: places ?? 0)
        case percent: return fixed(value, places ?? 0) + "%"
        default:
            guard let cur = currency(format) else { return fixed(value, places ?? 0) }
            let number = Money.grouped(value, places: places ?? cur.places)
            if number.hasPrefix("-") {
                return "-" + cur.sign + number.dropFirst()
            }
            return cur.sign + number
        }
    }

    /// `value` with `places` after the point, rounded as the Mac rounds.
    static func fixed(_ value: Double, _ places: Int) -> String {
        guard value.isFinite else { return value.isNaN ? "NaN" : (value < 0 ? "-inf" : "inf") }
        return String(format: "%.\(max(0, min(places, 300)))f", value)
    }

    // ----- reading a number out of a cell -----------------------------------

    /// A number read from the start of a cell.
    struct Read {
        var value: Double
        /// How many places after the point it was typed with, and how many
        /// of them it needs (those before its last zeros): "3.50" has two
        /// and needs one, "6,5" one and one, "1,500" none.
        var typed: Int
        var needed: Int
        /// What comes after it in the cell: " km" of "6.5 km".
        var rest: String
    }

    /// The number at the start of a cell, after a minus and a money sign:
    /// "6", "6.5 km", "6,5", "$3,500.00", "-€20". The thousands can have a
    /// comma, a point or a space between them, and the last of a point and
    /// a comma is the decimal point: "1,500", "1.234,56" and "1 234,56" are
    /// read the way they were meant.
    static func read(_ text: String) -> Read? {
        let (minus, text) = stripSign(text)
        let scalars = Array(text.unicodeScalars)
        func threeDigits(_ from: Int) -> Bool {
            guard from + 3 <= scalars.count, scalars[from..<from + 3].allSatisfy(\.isASCIIDigit) else { return false }
            return !(from + 3 < scalars.count && scalars[from + 3].isASCIIDigit)
        }
        var token: [Unicode.Scalar] = []
        var end = 0
        for (i, ch) in scalars.enumerated() {
            if ch.isASCIIDigit || ch == "." || ch == "," {
                token.append(ch)
                end = i + 1
            } else if !([" ", "\u{a0}", "\u{202f}"].contains(ch) && !token.isEmpty && token.allSatisfy(\.isASCIIDigit) && threeDigits(i + 1)) {
                // (A space is part of the number only between the thousands: "1 500".)
                break
            }
        }
        var rest = String.UnicodeScalarView()
        rest.append(contentsOf: scalars[end...])
        while let last = token.last, last == "," || last == "." { token.removeLast() }
        let points = token.filter { $0 == "." }.count
        let commas = token.filter { $0 == "," }.count
        let lastPoint = token.lastIndex(of: "."), lastComma = token.lastIndex(of: ",")
        // Each part between the signs of the thousands has three digits.
        func inThrees(_ sign: Unicode.Scalar) -> Bool {
            token.split(separator: sign, omittingEmptySubsequences: false).enumerated().allSatisfy { i, part in
                i == 0 ? !part.isEmpty && part.count <= 3 : part.count == 3
            }
        }
        let decimal: Unicode.Scalar?
        if points == 0, commas == 0 {
            decimal = nil
        } else if points == 1, commas > 0, (lastPoint ?? -1) > (lastComma ?? -1) {
            // Both: the one that comes last, and it comes once.
            decimal = "."
        } else if commas == 1, points > 0, (lastComma ?? -1) > (lastPoint ?? -1) {
            decimal = ","
        } else if points > 0, commas > 0 {
            return nil
        } else if points == 1 {
            decimal = "."
        } else if commas == 0 {
            // "1.234.567"; "1.2.3" is no number.
            guard inThrees(".") else { return nil }
            decimal = nil
        } else if commas == 1, token.count - (token.firstIndex(of: ",") ?? 0) - 1 != 3 {
            // One comma with three digits after it is between the thousands
            // ("1,500"), as are several commas; another is the decimal ("6,5").
            decimal = ","
        } else {
            decimal = nil
        }
        var whole = token[...], part = token[token.endIndex...]
        if let sign = decimal, let at = token.lastIndex(of: sign) {
            whole = token[..<at]
            part = token[(at + 1)...]
        }
        let digits = String(String.UnicodeScalarView(whole.filter(\.isASCIIDigit)))
        let fraction = String(String.UnicodeScalarView(part))
        if digits.isEmpty, fraction.isEmpty { return nil }
        guard let value = Double(digits + "." + fraction + "0") else { return nil }
        let needed = fraction.count - fraction.reversed().prefix(while: { $0 == "0" }).count
        return Read(value: minus ? -value : value, typed: fraction.count, needed: needed, rest: String(rest))
    }

    /// The number a cell is, when the cell is one number and no more: with
    /// a money sign or a percent sign, before it or after it, or with
    /// `unit` (the unit of its column) after it. "2 bags" and "3/10" start
    /// with a number, and are words.
    static func wholeNumber(_ text: String, unit: String) -> Read? {
        guard let read = read(text) else { return nil }
        let rest = read.rest.trimmingCharacters(in: .whitespacesAndNewlines)
        let ok = rest.isEmpty || rest == "%" || (!unit.isEmpty && asciiEqual(rest, unit)) || isSign(rest)
        return ok ? read : nil
    }

    // ----- small helpers -----------------------------------------------------

    static func asciiLower(_ bytes: [UInt8]) -> [UInt8] {
        bytes.map { (65...90).contains($0) ? $0 + 32 : $0 }
    }

    /// Equal but for the case of ASCII letters (Rust's `eq_ignore_ascii_case`).
    static func asciiEqual(_ a: String, _ b: String) -> Bool {
        asciiLower(Array(a.utf8)) == asciiLower(Array(b.utf8))
    }

    static func hasASCIIPrefix(_ text: String, _ prefix: String) -> Bool {
        let t = Array(text.utf8), p = Array(prefix.utf8)
        return t.count >= p.count && asciiLower(Array(t.prefix(p.count))) == asciiLower(p)
    }

    static func dropBytes(_ text: String, _ n: Int) -> String {
        String(decoding: Array(text.utf8).dropFirst(n), as: UTF8.self)
    }

    static func trimStart(_ s: String) -> String {
        String(s.drop { $0.isWhitespace })
    }

    static func trimEnd(_ s: String) -> String {
        var s = Substring(s)
        while let last = s.last, last.isWhitespace { s = s.dropLast() }
        return String(s)
    }
}

extension Unicode.Scalar {
    var isASCIIDigit: Bool { ("0"..."9").contains(self) }
}
