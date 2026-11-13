// How a table is shown: which rows (filters), in what order (sorts), in
// which layout, with which columns, in which colours. None of this changes
// the rows as they are kept. Mirrors `src/table/view.rs` of the desktop app.

import Foundation

/// The shape the rows are shown in.
public enum TableLayout: String, CaseIterable, Hashable, Sendable, Codable {
    case table
    /// A column of cards for each value of one column.
    case board
    /// One line for each row.
    case list
    /// A card for each row.
    case gallery
    /// A month, with each row on its day.
    case calendar

    /// A layout this app does not know is shown as a table.
    public init(from decoder: Decoder) throws {
        self = TableLayout(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .table
    }
}

/// Rows in the order of one column.
public struct TableSort: Codable, Hashable, Sendable {
    public var column: Int
    /// From high to low (Z to A, newest first).
    public var down: Bool = false

    public init(column: Int, down: Bool = false) {
        self.column = column
        self.down = down
    }

    private enum CodingKeys: String, CodingKey { case column, down }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        guard let column = c.index(.column) else {
            throw DecodingError.dataCorruptedError(forKey: .column, in: c, debugDescription: "A sort names no column.")
        }
        self.column = column
        down = c.lenient(Bool.self, .down) ?? false
    }
}

/// What a rule asks of a cell.
public enum TableRuleOp: String, CaseIterable, Hashable, Sendable, Codable {
    case `is`, isNot = "isnot", has, hasNot = "hasnot", above, below, atLeast = "atleast", atMost = "atmost", empty, filled, ticked, notTicked = "notticked"

    /// It compares the cell with some words (else it needs none).
    public var needsText: Bool {
        !(self == .empty || self == .filled || self == .ticked || self == .notTicked)
    }
}

/// One thing asked of the rows: a filter (rows that do not pass are not
/// shown), or with a colour a paint rule (rows that pass get the colour).
public struct TableRule: Codable, Hashable, Sendable {
    public var column: Int
    public var op: TableRuleOp
    public var text: String = ""
    /// A colour name ("" for a filter); see `TagColor`.
    public var color: String = ""

    public init(column: Int, op: TableRuleOp, text: String = "", color: String = "") {
        self.column = column
        self.op = op
        self.text = text
        self.color = color
    }

    private enum CodingKeys: String, CodingKey { case column, op, text, color }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        column = try c.index(.column) ?? { throw DecodingError.dataCorruptedError(forKey: .column, in: c, debugDescription: "A rule names no column.") }()
        op = try c.decode(TableRuleOp.self, forKey: .op)
        text = c.lenient(String.self, .text) ?? ""
        color = c.lenient(String.self, .color) ?? ""
    }
}

/// The view of a table (the Mac's `View`).
public struct TableViewRules: Codable, Hashable, Sendable {
    public var layout: TableLayout = .table
    /// The first decides; the next ones order the rows it leaves equal.
    public var sort: [TableSort] = []
    /// A row shows when it passes every one.
    public var filter: [TableRule] = []
    /// The column whose values the rows are grouped by (in a board: the
    /// column of cards for each value).
    public var group: Int?
    /// Columns that are not shown. The first column always is.
    public var hidden: [Int] = []
    /// How many columns at the left stay in place when the table is
    /// scrolled sideways.
    public var frozen: Int = 0
    /// The first rule a row passes gives the row its colour.
    public var paint: [TableRule] = []
    /// Nothing in the table can be changed by a slip of the hand.
    public var locked: Bool = false
    /// A row opens as a full page (else at the side).
    public var full: Bool = false

    public init(layout: TableLayout = .table, sort: [TableSort] = [], filter: [TableRule] = [], group: Int? = nil, hidden: [Int] = [], frozen: Int = 0, paint: [TableRule] = [], locked: Bool = false, full: Bool = false) {
        self.layout = layout
        self.sort = sort
        self.filter = filter
        self.group = group
        self.hidden = hidden
        self.frozen = frozen
        self.paint = paint
        self.locked = locked
        self.full = full
    }

    private enum CodingKeys: String, CodingKey { case layout, sort, filter, group, hidden, frozen, paint, locked, full }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        layout = c.lenient(TableLayout.self, .layout) ?? .table
        sort = c.lossy(TableSort.self, .sort)
        filter = c.lossy(TableRule.self, .filter)
        group = c.index(.group)
        hidden = (c.lenient([Lossy<Double>].self, .hidden) ?? []).compactMap { $0.value.flatMap { $0 >= 0 && $0 == $0.rounded() ? Int(min($0, 1e15)) : nil } }
        frozen = c.index(.frozen) ?? 0
        paint = c.lossy(TableRule.self, .paint)
        locked = c.lenient(Bool.self, .locked) ?? false
        full = c.lenient(Bool.self, .full) ?? false
    }

    public var isPlain: Bool { self == TableViewRules() }

    /// Columns came, went or moved: `to` gives the new number of a column,
    /// or nil for a column that is gone. What named a gone column goes.
    mutating func remap(_ to: (Int) -> Int?) {
        sort = sort.compactMap { s in to(s.column).map { TableSort(column: $0, down: s.down) } }
        filter = filter.compactMap { r in to(r.column).map { TableRule(column: $0, op: r.op, text: r.text, color: r.color) } }
        paint = paint.compactMap { r in to(r.column).map { TableRule(column: $0, op: r.op, text: r.text, color: r.color) } }
        group = group.flatMap(to)
        hidden = hidden.compactMap(to)
        // (No more columns than a table can have: a number from a file that
        // was written wrong must not be counted up to.)
        frozen = (0..<min(frozen, TableData.mostColumns)).compactMap(to).map { $0 + 1 }.max() ?? 0
    }

    /// Nothing that names a column a table of this width does not have, no
    /// column sorted or hidden twice, and the first column in view.
    mutating func tidy(width: Int) {
        if isPlain { return }
        remap { $0 < width ? $0 : nil }
        var seen: Set<Int> = []
        sort = sort.filter { seen.insert($0.column).inserted }
        hidden = Array(Set(hidden)).sorted().filter { $0 != 0 }
        frozen = min(frozen, max(width - 1, 0))
    }
}

/// What a row is ordered by. Cells with nothing in them come last.
private enum Key {
    case none
    case amount(Double)
    case words(String)

    func order(_ other: Key, down: Bool) -> ComparisonResult {
        func turned(_ o: ComparisonResult) -> ComparisonResult {
            guard down else { return o }
            return o == .orderedAscending ? .orderedDescending : o == .orderedDescending ? .orderedAscending : .orderedSame
        }
        switch (self, other) {
        case (.none, .none): return .orderedSame
        case (.none, _): return .orderedDescending
        case (_, .none): return .orderedAscending
        case let (.amount(a), .amount(b)): return turned(a < b ? .orderedAscending : a > b ? .orderedDescending : .orderedSame)
        case let (.words(a), .words(b)): return turned(byteOrder(a, b))
        case (.amount, .words): return turned(.orderedAscending)
        case (.words, .amount): return turned(.orderedDescending)
        }
    }
}

extension TableData {
    /// The columns that show, in order.
    public var shownColumns: [Int] {
        (0..<columns).filter { !view.hidden.contains($0) }
    }

    /// Whether a row passes a rule. A rule that still waits for its words
    /// lets every row pass.
    public func passes(_ r: Int, _ rule: TableRule, today: Day = When().today()) -> Bool {
        let kind = kind(rule.column)
        let text = shown(r, rule.column).trimmingCharacters(in: .whitespacesAndNewlines)
        let want = rule.text.trimmingCharacters(in: .whitespacesAndNewlines)
        if rule.op.needsText, want.isEmpty { return true }
        // Amounts and days are compared as what they are, not as letters.
        let measured = kind.isAmount || kind == .date
        func amount(_ t: String) -> Double? {
            switch kind {
            case .date: CellText.day(t, today: today).map { Double($0.ordinal) }
            case .time, .pace: CellText.seconds(t)
            default: CellText.number(t)
            }
        }
        func compare(_ ok: (Double, Double) -> Bool) -> Bool {
            guard let a = amount(text), let b = amount(want) else { return false }
            return ok(a, b)
        }
        func same() -> Bool {
            if kind == .multi { return CellText.parts(text).contains { $0.lowercased() == want.lowercased() } }
            if measured { return compare(==) }
            return text.lowercased() == want.lowercased()
        }
        func has() -> Bool { text.lowercased().contains(want.lowercased()) }
        switch rule.op {
        case .empty: return text.isEmpty
        case .filled: return !text.isEmpty
        case .ticked: return CellText.checked(text)
        case .notTicked: return !CellText.checked(text)
        case .is: return same()
        case .isNot: return !same()
        case .has: return has()
        case .hasNot: return !has()
        case .above: return compare(>)
        case .below: return compare(<)
        case .atLeast: return compare(>=)
        case .atMost: return compare(<=)
        }
    }

    private func passesFilters(_ r: Int, today: Day) -> Bool {
        view.filter.allSatisfy { passes(r, $0, today: today) }
    }

    /// The rows that count for the totals: those with something in them
    /// that pass every filter.
    public func counted(today: Day = When().today()) -> [Int] {
        filledRows.filter { passesFilters($0, today: today) }
    }

    private func key(_ r: Int, _ c: Int, today: Day) -> Key {
        let kind = kind(c)
        let text = shown(r, c).trimmingCharacters(in: .whitespacesAndNewlines)
        let words = Key.words(text.lowercased())
        if kind == .checkbox { return .amount(CellText.checked(text) ? 1 : 0) }
        if text.isEmpty { return .none }
        switch kind {
        case .date:
            return CellText.day(text, today: today).map { .amount(Double($0.ordinal)) } ?? words
        case .choice, .status:
            // (In the order of its options, as Notion sorts them.)
            return optionPlace(c, text).map { .amount(Double($0)) } ?? words
        case _ where kind.isAmount:
            return value(r, c).map { .amount($0) } ?? words
        default:
            return words
        }
    }

    /// The rows to show, in order: those that pass the filters (a row with
    /// nothing in it yet always does), ordered by the sorts, with rows that
    /// have nothing in them last. Without a filter, nested rows stay under
    /// their row (each level is ordered by itself), and the rows nested in
    /// a closed row are left out.
    public func order(today: Day = When().today()) -> [Int] {
        let sorts = view.sort
        let keys: [[Key]] = sorts.isEmpty ? [] : rows.indices.map { r in sorts.map { key(r, $0.column, today: today) } }
        let blank = rows.indices.map { isBlank($0) }
        func compare(_ a: Int, _ b: Int) -> ComparisonResult {
            if blank[a] != blank[b] { return blank[a] ? .orderedDescending : .orderedAscending }
            for (i, s) in sorts.enumerated() {
                let o = keys[a][i].order(keys[b][i], down: s.down)
                if o != .orderedSame { return o }
            }
            return .orderedSame
        }
        // A stable sort: rows the sorts leave equal keep their places.
        func sorted(_ list: [Int]) -> [Int] {
            guard !sorts.isEmpty else { return list }
            return list.enumerated().sorted { x, y in
                let o = compare(x.element, y.element)
                return o == .orderedSame ? x.offset < y.offset : o == .orderedAscending
            }.map(\.element)
        }
        if !view.filter.isEmpty || info.allSatisfy({ $0.indent == 0 }) {
            return sorted(rows.indices.dropFirst().filter { blank[$0] || passesFilters($0, today: today) })
        }
        var out: [Int] = []
        func walk(_ from: Int, _ to: Int) {
            var level: [Int] = []
            var r = from
            while r < to {
                level.append(r)
                r = subtreeEnd(r)
            }
            for r in sorted(level) {
                out.append(r)
                if !info(r).closed {
                    walk(r + 1, min(subtreeEnd(r), to))
                }
            }
        }
        walk(1, rows.count)
        return out
    }

    /// How deep a row shows nested (not at all while a filter is on: then
    /// the rows are a plain list).
    public func shownIndent(_ r: Int) -> Int {
        view.filter.isEmpty ? indent(r) : 0
    }

    /// `rows` in groups by what column `c` says, each with its value. Every
    /// value the column has makes a group, in the order of the rows as they
    /// are kept: a group stays in its place when the rows are sorted, and
    /// stays (empty) when a filter takes its rows. A choice column has a
    /// group for each option, in their order. The rows with nothing in the
    /// column are the last group, with "" for its value.
    public func groups(_ rows: [Int], _ c: Int) -> [(value: String, rows: [Int])] {
        func value(_ r: Int) -> String { shown(r, c).trimmingCharacters(in: .whitespacesAndNewlines) }
        var out: [(value: String, rows: [Int])] = kind(c) == .choice || kind(c) == .status ? column(c).options.map { ($0.name, []) } : []
        for r in filledRows {
            let v = value(r)
            if !v.isEmpty, !out.contains(where: { $0.value.lowercased() == v.lowercased() }) {
                out.append((v, []))
            }
        }
        var none: [Int] = []
        for r in rows {
            let v = value(r).lowercased()
            if let i = out.firstIndex(where: { $0.value.lowercased() == v }) {
                out[i].rows.append(r)
            } else {
                none.append(r)
            }
        }
        if !none.isEmpty { out.append(("", none)) }
        return out
    }

    /// The column a board makes its columns of cards from: the group
    /// column, or else the first choice column (or status column).
    public var boardColumn: Int? {
        view.group ?? firstOf(.choice) ?? firstOf(.status)
    }

    /// The colour of a row, by name: that of the first paint rule it passes.
    public func color(_ r: Int, today: Day = When().today()) -> String? {
        if isBlank(r) { return nil }
        return view.paint.first { p in
            !p.color.isEmpty && (!p.op.needsText || !p.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) && passes(r, p, today: today)
        }?.color
    }
}
