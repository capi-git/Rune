// Formulas for table columns. A formula belongs to one column and is worked
// out once for each row, from the other cells of that row:
//
//     [Unit price] * Qty * 1.2
//     if(Amount >= 100 and not Paid, "big", "")
//
// A column is named by a bare word (`Qty`) or by any text in square
// brackets (`[Unit price]`). Arithmetic with an empty cell gives nothing,
// not an error, so a row with a missing cell shows an empty cell. Mirrors
// `src/formula.rs` of the desktop app; the sentences of its errors are the
// same word for word.

import Foundation

/// What a cell or a formula is worth.
public enum FormulaValue: Hashable, Sendable {
    /// Nothing there (an empty cell, a division by zero).
    case empty
    case number(Double)
    case text(String)
    case bool(Bool)

    /// As a number: a checkbox is 1 or 0, and a text counts when it is a
    /// plain number ("12", "-1.5"). Empty and any other text are not.
    public var number: Double? {
        switch self {
        case .empty: return nil
        case .number(let n): return n
        case .bool(let b): return b ? 1 : 0
        case .text(let text):
            let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
            let digits = text.hasPrefix("-") ? text.dropFirst() : Substring(text)
            // Only digits and one point: no "1e5", no "inf", no second minus.
            guard digits.unicodeScalars.allSatisfy({ $0.isASCIIDigit || $0 == "." }),
                  let n = Formula.parseNumber(text), n.isFinite
            else { return nil }
            return n
        }
    }

    /// True for a ticked checkbox, a number that is not 0 and a text that
    /// is not empty. Empty is false.
    public var truthy: Bool {
        switch self {
        case .empty: false
        case .number(let n): n != 0
        case .text(let text): !text.isEmpty
        case .bool(let b): b
        }
    }

    /// As text: nothing for Empty, and a whole number without ".0" at the
    /// end (as Rust writes a number).
    public var text: String {
        switch self {
        case .empty: ""
        case .number(let n): Formula.write(n)
        case .text(let text): text
        case .bool(let b): b ? "true" : "false"
        }
    }
}

/// A formula read once, ready to be worked out for many rows.
public struct Formula: Hashable, Sendable {
    /// The most characters a formula may have, and how deep it may nest.
    static let mostCharacters = 2000
    static let mostLevels = 64

    /// Brackets, calls and signs nested deeper than this are read and
    /// worked out on a thread with a large stack (see `onLargeStack`).
    static let shallow = 8

    fileprivate let root: Node
    /// It nests deeper than `shallow`.
    fileprivate let deep: Bool

    /// Read a formula. On failure, one short sentence in plain words for the
    /// person, for example "A ) is missing.".
    public static func parse(_ source: String) -> Result<Formula, FormulaError> {
        if source.unicodeScalars.count > mostCharacters {
            return .failure(FormulaError("The formula is too long."))
        }
        // (What may open a level: a bracket, a minus, a power, a "not".)
        let openers = source.unicodeScalars.filter { $0 == "(" || $0 == "-" || $0 == "^" }.count + source.lowercased().components(separatedBy: "not").count - 1
        return openers > shallow ? onLargeStack { read(source) } : read(source)
    }

    private static func read(_ source: String) -> Result<Formula, FormulaError> {
        do {
            let tokens = try split(source)
            if tokens.isEmpty { throw FormulaError("The formula is empty.") }
            var parser = Parser(tokens: tokens)
            let root = try parser.either()
            if let extra = parser.next() { throw FormulaError(misplaced(extra)) }
            return .success(Formula(root: root, deep: parser.deepest > shallow))
        } catch let error as FormulaError {
            return .failure(error)
        } catch {
            return .failure(FormulaError("The formula cannot be read."))
        }
    }

    /// The names of the columns it reads, as written, each once (without
    /// regard to case), in the order of first use.
    public var names: [String] {
        func all() -> [String] {
            var names: [String] = []
            collect(root, &names)
            return names
        }
        return deep ? onLargeStack(all) : all()
    }

    /// Work it out for one row. `cell` gives the value of a column by the
    /// name written in the formula; nil means there is no such column,
    /// which is an error.
    public func eval(_ cell: (String) -> FormulaValue?) -> Result<FormulaValue, FormulaError> {
        deep ? onLargeStack { work(cell) } : work(cell)
    }

    private func work(_ cell: (String) -> FormulaValue?) -> Result<FormulaValue, FormulaError> {
        do {
            return .success(try workOut(root, cell))
        } catch let error as FormulaError {
            return .failure(error)
        } catch {
            return .failure(FormulaError("The formula cannot be worked out."))
        }
    }

    // ----- numbers as Rust reads and writes them ------------------------------

    /// A number written with digits and one point ("12", ".5", "5."). Too
    /// many digits is infinity, as in Rust (and so Empty, once worked out).
    static func parseNumber(_ text: String) -> Double? {
        let body = text.hasPrefix("-") ? text.dropFirst() : Substring(text)
        guard !body.isEmpty, body.unicodeScalars.allSatisfy({ $0.isASCIIDigit || $0 == "." }),
              body.filter({ $0 == "." }).count <= 1, body.contains(where: \.isNumber)
        else { return nil }
        if let n = Double(text) { return n }
        return text.hasPrefix("-") ? -.infinity : .infinity
    }

    /// A number as Rust's `to_string` writes it: as few digits as tell it
    /// apart, never with an exponent, and no ".0" on a whole number.
    static func write(_ n: Double) -> String {
        guard n.isFinite else { return n.isNaN ? "NaN" : (n < 0 ? "-inf" : "inf") }
        var s = "\(n)"
        if let e = s.firstIndex(where: { $0 == "e" || $0 == "E" }) {
            let exponent = Int(s[s.index(after: e)...]) ?? 0
            var mantissa = String(s[..<e])
            let minus = mantissa.hasPrefix("-")
            if minus { mantissa.removeFirst() }
            let point = mantissa.firstIndex(of: ".").map { mantissa.distance(from: mantissa.startIndex, to: $0) } ?? mantissa.count
            var digits = mantissa.replacingOccurrences(of: ".", with: "")
            var at = point + exponent
            if at <= 0 {
                digits = String(repeating: "0", count: 1 - at) + digits
                at = 1
            }
            if at >= digits.count {
                digits += String(repeating: "0", count: at - digits.count)
                s = digits
            } else {
                s = String(digits.prefix(at)) + "." + String(digits.dropFirst(at))
            }
            while s.contains("."), s.hasSuffix("0") { s.removeLast() }
            if s.hasSuffix(".") { s.removeLast() }
            if minus { s = "-" + s }
            return s
        }
        if s.hasSuffix(".0") { s.removeLast(2) }
        return s
    }
}

/// Do `work` on a thread of its own with a large stack, and wait for it. A
/// formula may nest 64 levels deep, as on the Mac; reading or working out
/// such a one takes more of the stack than a thread of the phone has (half
/// a megabyte), so the few that nest deeply are done here.
func onLargeStack<T>(_ work: () -> T) -> T {
    withoutActuallyEscaping(work) { work in
        let job = LargeStackJob(work)
        let done = DispatchSemaphore(value: 0)
        let thread = Thread {
            job.result = job.work?()
            // (Let go of the work before the caller goes on: it may not
            // outlive the call.)
            job.work = nil
            done.signal()
        }
        thread.stackSize = 16 << 20
        thread.start()
        done.wait()
        return job.result!
    }
}

/// The work and its result, handed to the other thread and back. (Only one
/// thread uses it at a time: the caller waits.)
private final class LargeStackJob<T>: @unchecked Sendable {
    var work: (() -> T)?
    var result: T?
    init(_ work: @escaping () -> T) { self.work = work }
}

/// What is wrong with a formula, in one sentence.
public struct FormulaError: Error, Hashable, Sendable, CustomStringConvertible {
    public let sentence: String
    init(_ sentence: String) { self.sentence = sentence }
    public var description: String { sentence }
}

/// One part of a formula, with the parts it is made of.
fileprivate indirect enum Node: Hashable, Sendable {
    /// A number, a text, `true` or `false`.
    case literal(FormulaValue)
    case column(String)
    case not(Node)
    case negate(Node)
    /// A value and the operators after it, worked out from left to right:
    /// `a - b + c`. A list and not a deep tree, so a long sum cannot use up
    /// the stack.
    case chain(Node, [Link])
    case call(Func, [Node])
}

fileprivate struct Link: Hashable, Sendable {
    let op: Op
    let node: Node
}

/// An operator between two values.
fileprivate enum Op: Hashable, Sendable { case or, and, eq, ne, lt, le, gt, ge, add, sub, mul, div, rem, pow }

/// A function that a formula can call.
fileprivate enum Func: Hashable, Sendable { case choose, round, floor, ceil, abs, empty, length, min, max, sum, average, concat }

/// The functions: the name as written, then the least and the most values each takes.
fileprivate let functions: [(String, Func, Int, Int)] = [
    ("if", .choose, 2, 3), ("round", .round, 1, 2), ("floor", .floor, 1, 1), ("ceil", .ceil, 1, 1),
    ("abs", .abs, 1, 1), ("empty", .empty, 1, 1), ("length", .length, 1, 1),
    ("min", .min, 1, .max), ("max", .max, 1, .max), ("sum", .sum, 1, .max),
    ("average", .average, 1, .max), ("concat", .concat, 1, .max),
]

// ----- tokens -------------------------------------------------------------------

/// One piece of a formula as written.
fileprivate enum Token {
    /// A number, a text in quotes, `true` or `false`.
    case literal(FormulaValue)
    /// A bare word: a column, or a function when a `(` follows it.
    case word(String)
    /// A name in square brackets: always a column.
    case bracket(String)
    /// An operator, a round bracket or a comma. `and`, `or` and `not` are signs too.
    case sign(String)
}

/// The signs, with the long ones first so that `<=` is not read as `<` and `=`.
fileprivate let signs = ["==", "!=", "<>", "<=", ">=", "=", "<", ">", "+", "-", "*", "/", "%", "^", "(", ")", ","]

/// A number as a value. One that is not finite (a division by zero, a
/// number too big) is Empty. (`+ 0` turns a zero with a minus into a plain zero.)
fileprivate func finite(_ n: Double) -> FormulaValue {
    n.isFinite ? .number(n + 0) : .empty
}

fileprivate func isAlphabetic(_ c: Unicode.Scalar) -> Bool { c.properties.isAlphabetic }
fileprivate func isAlphanumeric(_ c: Unicode.Scalar) -> Bool { c.properties.isAlphabetic || c.properties.numericType != nil }

/// Split a formula into tokens.
fileprivate func split(_ source: String) throws -> [Token] {
    var tokens: [Token] = []
    let chars = Array(source.unicodeScalars)
    var i = 0
    while i < chars.count {
        let c = chars[i]
        i += 1
        if c.properties.isWhitespace { continue }
        if c.isASCIIDigit || c == "." {
            var digits = String(c)
            while i < chars.count, chars[i].isASCIIDigit || chars[i] == "." {
                digits.unicodeScalars.append(chars[i])
                i += 1
            }
            // A point alone or a second point cannot be read.
            guard let n = Formula.parseNumber(digits) else { throw FormulaError("The sign . does not belong here.") }
            tokens.append(.literal(finite(n)))
        } else if c == "\"" {
            guard let text = until(chars, &i, "\"") else { throw FormulaError("A closing \" is missing.") }
            tokens.append(.literal(.text(text)))
        } else if c == "[" {
            guard let name = until(chars, &i, "]") else { throw FormulaError("A ] is missing.") }
            tokens.append(.bracket(name.trimmingCharacters(in: .whitespacesAndNewlines)))
        } else if isAlphabetic(c) || c == "_" {
            var word = String(c)
            while i < chars.count, isAlphanumeric(chars[i]) || chars[i] == "_" {
                word.unicodeScalars.append(chars[i])
                i += 1
            }
            switch word.lowercased() {
            case "and": tokens.append(.sign("and"))
            case "or": tokens.append(.sign("or"))
            case "not": tokens.append(.sign("not"))
            case "true": tokens.append(.literal(.bool(true)))
            case "false": tokens.append(.literal(.bool(false)))
            default: tokens.append(.word(word))
            }
        } else {
            // With the sign after it, to find a sign of two (`<=`) before a sign of one.
            var pair = String(c)
            pair.unicodeScalars.append(i < chars.count ? chars[i] : " ")
            guard let sign = signs.first(where: { pair.unicodeScalars.starts(with: $0.unicodeScalars) }) else {
                throw FormulaError("The sign \(Character(c)) does not belong here.")
            }
            if sign.unicodeScalars.count == 2 { i += 1 }
            tokens.append(.sign(sign))
        }
    }
    return tokens
}

/// Read up to the closing sign `end`. Nil when it is missing.
fileprivate func until(_ chars: [Unicode.Scalar], _ i: inout Int, _ end: Unicode.Scalar) -> String? {
    var text = String.UnicodeScalarView()
    while i < chars.count {
        let c = chars[i]
        i += 1
        if c == end { return String(text) }
        // `\"` is a quote sign inside a text.
        if c == "\\", end == "\"", i < chars.count, chars[i] == "\"" {
            text.append("\"")
            i += 1
        } else {
            text.append(c)
        }
    }
    return nil
}

/// The sentence for a token in a place where it cannot be.
fileprivate func misplaced(_ token: Token) -> String {
    let what: String
    switch token {
    case .sign(let sign) where sign.unicodeScalars.allSatisfy(isAlphabetic): what = "The word \(sign)"
    case .sign(let sign): what = "The sign \(sign)"
    case .word(let word): what = "The word \(word)"
    case .bracket(let name): what = "The name [\(name)]"
    case .literal(.text(let text)): what = "The text \"\(text)\""
    case .literal(let value): what = "The value \(value.text)"
    }
    return "\(what) does not belong here."
}

// ----- the tree -------------------------------------------------------------------

/// Reads the tokens into a tree. Each level of binding has its own method,
/// from `or` (the lowest) to a single value (the highest). (Written with
/// plain loops and no closures, so that each level of nesting takes few
/// frames of the stack: a thread of the phone has little of it.)
fileprivate struct Parser {
    let tokens: [Token]
    var at = 0
    /// How many round brackets, calls and prefix signs are open around this place.
    var depth = 0
    /// The most there were.
    var deepest = 0

    init(tokens: [Token]) { self.tokens = tokens }

    mutating func next() -> Token? {
        guard at < tokens.count else { return nil }
        defer { at += 1 }
        return tokens[at]
    }

    /// `a or b`: the lowest level, so also a whole formula and the inside of round brackets.
    mutating func either() throws -> Node {
        let first = try both()
        var rest: [Link] = []
        while take("or") { rest.append(Link(op: .or, node: try both())) }
        return rest.isEmpty ? first : .chain(first, rest)
    }

    /// `a and b`.
    mutating func both() throws -> Node {
        let first = try comparison()
        var rest: [Link] = []
        while take("and") { rest.append(Link(op: .and, node: try comparison())) }
        return rest.isEmpty ? first : .chain(first, rest)
    }

    /// `not a`, then `a < b`. Comparisons are not chained: `a < b < c` is an error.
    mutating func comparison() throws -> Node {
        if take("not") {
            try enter()
            defer { depth -= 1 }
            return .not(try comparison())
        }
        let left = try sum()
        let ops: [(String, Op)] = [("=", .eq), ("==", .eq), ("!=", .ne), ("<>", .ne), ("<", .lt), ("<=", .le), (">", .gt), (">=", .ge)]
        if let found = op(ops) {
            return .chain(left, [Link(op: found, node: try sum())])
        }
        return left
    }

    /// `a + b - c`.
    mutating func sum() throws -> Node {
        let first = try product()
        var rest: [Link] = []
        while let found = op([("+", .add), ("-", .sub)]) { rest.append(Link(op: found, node: try product())) }
        return rest.isEmpty ? first : .chain(first, rest)
    }

    /// `a * b / c % d`.
    mutating func product() throws -> Node {
        let first = try power()
        var rest: [Link] = []
        while let found = op([("*", .mul), ("/", .div), ("%", .rem)]) { rest.append(Link(op: found, node: try power())) }
        return rest.isEmpty ? first : .chain(first, rest)
    }

    /// `-a`, then `a ^ b`. The minus binds less than `^`, so `-2 ^ 2` is -4.
    /// `^` binds to the right, so `2 ^ 3 ^ 2` is 2 ^ 9.
    mutating func power() throws -> Node {
        if take("-") {
            try enter()
            defer { depth -= 1 }
            return .negate(try power())
        }
        let base = try value()
        if take("^") {
            try enter()
            defer { depth -= 1 }
            return .chain(base, [Link(op: .pow, node: try power())])
        }
        return base
    }

    /// A single value: a number, a text, a column, a function call or round brackets.
    mutating func value() throws -> Node {
        switch next() {
        case .literal(let value)?: return .literal(value)
        case .bracket(let name)?: return .column(name)
        case .word(let word)?:
            guard take("(") else { return .column(word) }
            try enter()
            defer { depth -= 1 }
            return try call(word)
        case .sign("(")?:
            try enter()
            defer { depth -= 1 }
            let inside = try either()
            try closing()
            return inside
        case let other?: throw FormulaError(misplaced(other))
        case nil: throw FormulaError("A value is missing at the end.")
        }
    }

    /// A function call, from after its `(` to its `)`.
    mutating func call(_ word: String) throws -> Node {
        let lower = word.lowercased()
        guard let (name, function, least, most) = functions.first(where: { $0.0 == lower }) else {
            throw FormulaError("There is no function \"\(word)\".")
        }
        var values: [Node] = []
        if !take(")") {
            values.append(try either())
            while take(",") {
                values.append(try either())
            }
            try closing()
        }
        if values.count < least || values.count > most {
            let needs = switch (least, most) {
            case (1, 1): "one value"
            case (1, 2): "one or two values"
            case (2, 3): "two or three values"
            default: "at least one value"
            }
            throw FormulaError("\(name) needs \(needs).")
        }
        return .call(function, values)
    }

    /// One level deeper, but never deeper than the limit, so that no formula can use up the stack.
    mutating func enter() throws {
        if depth >= Formula.mostLevels {
            throw FormulaError("The formula is nested too deep.")
        }
        depth += 1
        deepest = max(deepest, depth)
    }

    /// Take the next token if it is this sign.
    mutating func take(_ sign: String) -> Bool {
        guard at < tokens.count, case .sign(let found) = tokens[at], found == sign else { return false }
        at += 1
        return true
    }

    /// Take the next token if it is one of these operators.
    mutating func op(_ level: [(String, Op)]) -> Op? {
        guard at < tokens.count, case .sign(let sign) = tokens[at], let op = level.first(where: { $0.0 == sign })?.1 else { return nil }
        at += 1
        return op
    }

    /// The next token must be a `)`.
    mutating func closing() throws {
        switch next() {
        case .sign(")")?: return
        case let other?: throw FormulaError(misplaced(other))
        case nil: throw FormulaError("A ) is missing.")
        }
    }
}

/// Add the column names under a part of the tree, each once.
fileprivate func collect(_ node: Node, _ names: inout [String]) {
    switch node {
    case .literal: break
    case .column(let name):
        if !names.contains(where: { $0.lowercased() == name.lowercased() }) { names.append(name) }
    case .not(let inner), .negate(let inner): collect(inner, &names)
    case .chain(let first, let rest):
        collect(first, &names)
        for link in rest { collect(link.node, &names) }
    case .call(_, let values):
        for value in values { collect(value, &names) }
    }
}

// ----- working out ----------------------------------------------------------------

/// Work out one part of the tree for a row. The tree is never deeper than
/// the parser lets it be, so this cannot use up the stack. (Each case is a
/// function of its own, so that each frame of the stack is small.)
fileprivate func workOut(_ node: Node, _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    switch node {
    case .literal(let value): return value
    case .column(let name): return try columnValue(name, cell)
    case .not(let inner): return try notValue(inner, cell)
    case .negate(let inner): return try negateValue(inner, cell)
    case .chain(let first, let rest): return try chainValue(first, rest, cell)
    case .call(let function, let values): return try call(function, values, cell)
    }
}

@inline(never)
fileprivate func columnValue(_ name: String, _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    switch cell(name) {
    case .number(let n)?: return finite(n)
    case let value?: return value
    case nil: throw FormulaError("There is no column \"\(name)\".")
    }
}

@inline(never)
fileprivate func notValue(_ inner: Node, _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    .bool(!(try workOut(inner, cell).truthy))
}

@inline(never)
fileprivate func negateValue(_ inner: Node, _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    guard let n = try workOut(inner, cell).number else { return .empty }
    return finite(-n)
}

@inline(never)
fileprivate func chainValue(_ first: Node, _ rest: [Link], _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    var left = try workOut(first, cell)
    for link in rest {
        // `and` and `or` stop early: when the left side decides, the right side is not worked out.
        if link.op == .and, !left.truthy {
            left = .bool(false)
        } else if link.op == .or, left.truthy {
            left = .bool(true)
        } else {
            left = apply(link.op, left, try workOut(link.node, cell))
        }
    }
    return left
}

/// Work out one operator between two values.
fileprivate func apply(_ op: Op, _ left: FormulaValue, _ right: FormulaValue) -> FormulaValue {
    func isOrder(_ wanted: [ComparisonResult]) -> FormulaValue {
        .bool(order(left, right).map(wanted.contains) ?? false)
    }
    // Arithmetic needs two numbers. With an empty cell or a word it gives nothing.
    func math(_ work: (Double, Double) -> Double) -> FormulaValue {
        guard let a = left.number, let b = right.number else { return .empty }
        return finite(work(a, b))
    }
    func words(_ value: FormulaValue) -> Bool {
        if case .text = value { return value.number == nil }
        return false
    }
    switch op {
    case .or: return .bool(left.truthy || right.truthy)
    case .and: return .bool(left.truthy && right.truthy)
    case .eq: return isOrder([.orderedSame])
    case .ne: return .bool(order(left, right) != .orderedSame)
    case .lt: return isOrder([.orderedAscending])
    case .le: return isOrder([.orderedAscending, .orderedSame])
    case .gt: return isOrder([.orderedDescending])
    case .ge: return isOrder([.orderedDescending, .orderedSame])
    // `+` joins the two as text when a side is a text that is not a number.
    case .add where words(left) || words(right): return .text(left.text + right.text)
    case .add: return math(+)
    case .sub: return math(-)
    case .mul: return math(*)
    // A division or a remainder by zero is not finite, so it gives Empty.
    case .div: return math(/)
    case .rem: return math { $0.truncatingRemainder(dividingBy: $1) }
    case .pow: return math { Foundation.pow($0, $1) }
    }
}

/// How two values compare: as numbers when both are numbers, else as text
/// without regard to case. Nil when only one side is empty: an empty side
/// is equal to another empty side, but never less or more than a value.
fileprivate func order(_ left: FormulaValue, _ right: FormulaValue) -> ComparisonResult? {
    if let a = left.number, let b = right.number {
        return a < b ? .orderedAscending : (a > b ? .orderedDescending : .orderedSame)
    }
    let a = left.text.lowercased(), b = right.text.lowercased()
    if a.isEmpty != b.isEmpty { return nil }
    return byteOrder(a, b)
}

/// Text in the order of its bytes, as Rust orders strings.
func byteOrder(_ a: String, _ b: String) -> ComparisonResult {
    let x = Array(a.utf8), y = Array(b.utf8)
    if x == y { return .orderedSame }
    return x.lexicographicallyPrecedes(y) ? .orderedAscending : .orderedDescending
}

/// Work out a function for a row.
@inline(never)
fileprivate func call(_ function: Func, _ values: [Node], _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    switch function {
    // Only the chosen branch is worked out; a value that was left out (the
    // `else` of an `if`) counts as Empty.
    case .choose:
        let at = try workOut(values[0], cell).truthy ? 1 : 2
        return at < values.count ? try workOut(values[at], cell) : .empty
    case .round: return try round(values, cell)
    case .floor: return try one(values[0], cell) { $0.rounded(.down) }
    case .ceil: return try one(values[0], cell) { $0.rounded(.up) }
    case .abs: return try one(values[0], cell) { Swift.abs($0) }
    case .empty: return .bool(try workOut(values[0], cell).text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    case .length: return .number(Double(try workOut(values[0], cell).text.unicodeScalars.count))
    case .min, .max, .sum, .average: return try numbers(function, values, cell)
    case .concat:
        var joined = ""
        for node in values { joined += try workOut(node, cell).text }
        return .text(joined)
    }
}

@inline(never)
fileprivate func one(_ node: Node, _ cell: (String) -> FormulaValue?, _ work: (Double) -> Double) throws -> FormulaValue {
    guard let n = try workOut(node, cell).number else { return .empty }
    return finite(work(n))
}

@inline(never)
fileprivate func round(_ values: [Node], _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    let n = try workOut(values[0], cell).number
    let places = values.count > 1 ? try workOut(values[1], cell).number : 0
    guard let n, let places else { return .empty }
    // At most 15 places after the point. Less than zero rounds to tens, hundreds and so on.
    let p = Int(Swift.min(Swift.max(places, -15), 15))
    let step = Foundation.pow(10.0, Double(Swift.abs(p)))
    return finite(p < 0 ? (n / step).rounded() * step : (n * step).rounded() / step)
}

@inline(never)
fileprivate func numbers(_ function: Func, _ values: [Node], _ cell: (String) -> FormulaValue?) throws -> FormulaValue {
    // Empty cells and words are left out.
    var numbers: [Double] = []
    for node in values {
        if let n = try workOut(node, cell).number { numbers.append(n) }
    }
    if numbers.isEmpty { return .empty }
    let sum = numbers.reduce(0, +)
    switch function {
    case .min: return finite(numbers.reduce(.infinity) { Swift.min($0, $1) })
    case .max: return finite(numbers.reduce(-.infinity) { Swift.max($0, $1) })
    case .sum: return finite(sum)
    default: return finite(sum / Double(numbers.count))
    }
}
