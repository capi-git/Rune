// Charts: a block that draws the rows of a table. What it draws is kept in
// the block's `data` as JSON:
//
//     {"shape": "bars", "from": "Run log", "x": "Date", "by": "week", "y": "km", "add": "sum"}
//
// The table is the one above the chart on its page, or (when `from` names
// a page by its id or its title) the first table of that page: the app
// finds it, and passes it to the functions here. Bars and a line draw one
// amount for each row, day, week or month; a pie draws how much of each
// tag; a ring draws one amount against a goal; days draw which days of
// the last four weeks have a row. Mirrors `src/chart.rs` of the desktop app.
//
// How the app draws a chart:
//
//   let chart = Chart.parse(block.data)
//   chart.heading(table)                   // its title ("km per week")
//   switch chart.shape {
//   case .bars, .line: chart.points(table, today: today)    // oldest first; `current` holds today
//                      Chart.niceTop(max), chart.signed(table), chart.upsideDown(table)
//   case .pie:  chart.slices(table)                         // each with its tag's colour
//   case .ring: chart.ring(table), chart.goal               // the amount, and the goal it closes at
//   case .days: chart.days(table, today: today)             // 28 days, Monday to Sunday
//   }
//   chart.format(table, value)             // an amount written as its column writes it

import Foundation

/// How a chart draws.
public enum ChartShape: String, CaseIterable, Hashable, Sendable, Codable {
    case bars, line, ring, days
    /// Slices of a circle: how much of each tag (wins against losses).
    case pie

    public init(from decoder: Decoder) throws {
        self = ChartShape(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .bars
    }
}

/// What one bar or point stands for.
public enum ChartBy: String, CaseIterable, Hashable, Sendable, Codable {
    case row, day, week, month

    public init(from decoder: Decoder) throws {
        self = ChartBy(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .week
    }
}

/// How the amounts of one bar (or of a ring) are put together.
public enum ChartAdd: String, CaseIterable, Hashable, Sendable, Codable {
    case sum, average, highest, lowest
    /// How many rows, whatever is in them.
    case count

    /// The other words the Mac reads ("avg", "max"…).
    public init(from decoder: Decoder) throws {
        let word = try decoder.singleValueContainer().decode(String.self)
        self = switch word {
        case "avg", "mean": .average
        case "max": .highest
        case "min": .lowest
        default: ChartAdd(rawValue: word) ?? .sum
        }
    }

    func apply(_ values: [Double]) -> Double? {
        if values.isEmpty { return self == .count || self == .sum ? 0 : nil }
        switch self {
        case .sum: return values.reduce(0, +)
        case .average: return values.reduce(0, +) / Double(values.count)
        case .highest: return values.reduce(-Double.greatestFiniteMagnitude) { max($0, $1) }
        case .lowest: return values.reduce(Double.greatestFiniteMagnitude) { min($0, $1) }
        case .count: return Double(values.count)
        }
    }
}

/// One slice of a pie.
public struct ChartSlice: Hashable, Sendable {
    public let label: String
    public let value: Double
    public let color: TagColor
}

/// One bar or point.
public struct ChartPoint: Hashable, Sendable {
    public let label: String
    public let value: Double
    /// It holds today (this week, this month), or is the last row.
    public let current: Bool
}

/// One day of the days chart.
public struct ChartDay: Hashable, Sendable {
    public let date: Day
    public let hasRow: Bool
    public let today: Bool
    public let ahead: Bool
}

/// What a chart draws, and from where.
public struct Chart: Codable, Hashable, Sendable {
    public var shape: ChartShape = .bars
    /// The page whose first table it draws (its id or its title); empty
    /// for the table above the chart on its own page.
    public var from: String = ""
    /// The column along the bottom, usually a date ("Date").
    public var x: String = ""
    public var by: ChartBy = .week
    /// The column of the amounts ("km").
    public var y: String = ""
    public var add: ChartAdd = .sum
    public var goal: Double?
    public var title: String = ""
    /// Each bar or point adds to the ones before it (a curve of what was made).
    public var running: Bool = false

    public init(shape: ChartShape = .bars, from: String = "", x: String = "", by: ChartBy = .week, y: String = "", add: ChartAdd = .sum, goal: Double? = nil, title: String = "", running: Bool = false) {
        self.shape = shape
        self.from = from
        self.x = x
        self.by = by
        self.y = y
        self.add = add
        self.goal = goal
        self.title = title
        self.running = running
    }

    private enum CodingKeys: String, CodingKey { case shape, from, x, by, y, add, goal, title, running }

    /// Any name a field goes by on the Mac.
    private struct Name: CodingKey {
        let stringValue: String
        init(_ s: String) { stringValue = s }
        init?(stringValue: String) { self.stringValue = stringValue }
        var intValue: Int? { nil }
        init?(intValue: Int) { nil }
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Name.self)
        func read<T: Decodable>(_ type: T.Type, _ names: String...) -> T? {
            names.lazy.compactMap { c.lenient(T.self, Name($0)) }.first
        }
        shape = read(ChartShape.self, "shape", "kind", "type") ?? .bars
        from = read(String.self, "from", "table", "page") ?? ""
        x = read(String.self, "x", "across") ?? ""
        by = read(ChartBy.self, "by") ?? .week
        y = read(String.self, "y", "value", "column") ?? ""
        add = read(ChartAdd.self, "add", "aggregate", "agg") ?? .sum
        goal = read(Double.self, "goal")
        title = read(String.self, "title") ?? ""
        running = read(Bool.self, "running", "cumulative") ?? false
    }

    /// The chart kept in a block; an empty one (bars of the table above)
    /// when there is none yet or it cannot be read.
    public static func parse(_ data: String) -> Chart {
        (try? JSONDecoder().decode(Chart.self, from: Data(data.utf8))) ?? Chart()
    }

    /// The columns it draws: along the bottom (if any) and the amounts (if
    /// any). Unset, the first date column (for a pie, the first column of
    /// tags) and the first number, time, pace or formula column.
    public func columns(_ t: TableData) -> (x: Int?, y: Int?) {
        let x = t.findColumn(self.x) ?? (shape == .pie ? (0..<t.columns).first { t.kind($0).hasOptions } : t.firstOf(.date))
        let y = t.findColumn(self.y) ?? [ColumnKind.number, .time, .pace, .formula].lazy.compactMap { t.firstOf($0) }.first
        return (x, y)
    }

    /// Its title: the one it was given, or one that says what it draws
    /// ("km per week").
    public func heading(_ t: TableData) -> String {
        let given = title.trimmingCharacters(in: .whitespacesAndNewlines)
        if !given.isEmpty { return given }
        let (x, y) = columns(t)
        let amount = add == .count || y == nil ? "Rows" : t.heading(y!)
        let dated = x.map { t.kind($0) == .date } ?? false
        switch shape {
        case .days:
            return "Days with a row"
        case .ring:
            switch add {
            case .highest: return "Highest \(amount.lowercased())"
            case .lowest: return "Lowest \(amount.lowercased())"
            case .average: return "Average \(amount.lowercased())"
            case .sum, .count: return amount
            }
        case .pie:
            guard let x else { return amount }
            return add == .count ? "Rows by \(t.heading(x).lowercased())" : "\(amount) by \(t.heading(x).lowercased())"
        case .bars, .line:
            if running { return "\(amount), running total" }
            guard dated else { return amount }
            switch by {
            case .day: return "\(amount) per day"
            case .week: return "\(amount) per week"
            case .month: return "\(amount) per month"
            case .row: return amount
            }
        }
    }

    /// The amount of one row (1 when it counts rows).
    private func amount(_ t: TableData, _ r: Int, _ y: Int?) -> Double? {
        if add == .count { return 1 }
        guard let y else { return nil }
        return t.value(r, y)
    }

    /// The bars or points to draw, oldest first: the last few when there
    /// are many (30, or 26 periods of days), with a running total counted
    /// from the very first.
    public func points(_ t: TableData, today: Day) -> [ChartPoint] {
        var points = allPoints(t, today: today)
        if running {
            var sum = 0.0
            points = points.map { p in
                sum += p.value
                return ChartPoint(label: p.label, value: sum, current: p.current)
            }
        }
        let x = columns(t).x
        let most = by == .row || !(x.map { t.kind($0) == .date } ?? false) ? 30 : 26
        return Array(points.suffix(most))
    }

    /// The slices of a pie: for each tag of the column along the bottom, how
    /// many rows have it, or the sum of their amounts. In the column's order.
    public func slices(_ t: TableData) -> [ChartSlice] {
        let (x, y) = columns(t)
        guard let c = x else { return [] }
        let column = t.column(c)
        var out: [(label: String, value: Double, color: TagColor)] = column.options.map { ($0.name, 0, $0.color) }
        for r in t.filledRows {
            guard let amount = amount(t, r, y) else { continue }
            for name in Choice.names(in: t.shown(r, c), kind: column.kind) {
                if let i = out.firstIndex(where: { Money.asciiEqual($0.label, name) }) {
                    out[i].value += amount
                } else {
                    out.append((name, amount, .gray))
                }
            }
        }
        return out.filter { $0.value > 0 }.map { ChartSlice(label: $0.label, value: $0.value, color: $0.color) }
    }

    /// Amounts of this chart's column are coloured by their sign (a gain
    /// green and a loss red).
    public func signed(_ t: TableData) -> Bool {
        add != .count && (columns(t).y.map { t.column($0).signed } ?? false)
    }

    private func allPoints(_ t: TableData, today: Day) -> [ChartPoint] {
        let (x, y) = columns(t)
        let dated = x.flatMap { t.kind($0) == .date ? $0 : nil }
        guard by != .row, let xc = dated else {
            var rows = t.filledRows.filter { amount(t, $0, y) != nil }
            // Rows of a dated table go by their day, however they are kept.
            if let c = dated {
                rows = rows.enumerated().sorted { a, b in
                    let (da, db) = (t.day(a.element, c, today: today), t.day(b.element, c, today: today))
                    if da != db { return da == nil || (db != nil && da! < db!) }
                    return a.offset < b.offset
                }.map(\.element)
            }
            let last = rows.count - 1
            return rows.enumerated().map { i, r in
                let label = x.map { t.shown(r, $0) } ?? String(i + 1)
                return ChartPoint(label: label, value: amount(t, r, y) ?? 0, current: i == last)
            }
        }
        func key(_ d: Day) -> Day {
            switch by {
            case .day, .row: d
            case .week: d.adding(days: -d.weekdayFromMonday)
            case .month: d.firstOfMonth()
            }
        }
        func next(_ d: Day) -> Day {
            switch by {
            case .day, .row: d.adding(days: 1)
            case .week: d.adding(days: 7)
            case .month: d.firstOfMonth(adding: 1)
            }
        }
        var groups: [(Day, [Double])] = []
        for r in t.filledRows {
            guard let d = t.day(r, xc, today: today), let v = amount(t, r, y) else { continue }
            let k = key(d)
            if let i = groups.firstIndex(where: { $0.0 == k }) {
                groups[i].1.append(v)
            } else {
                groups.append((k, [v]))
            }
        }
        groups.sort { $0.0 < $1.0 }
        guard let first = groups.first?.0, let last = groups.last?.0 else { return [] }
        // Up to the period of today, when that is not far after the last row.
        let now = key(today)
        let end = now > last && now.ordinal - last.ordinal <= 62 ? now : last
        var out: [ChartPoint] = []
        var k = first
        while k <= end, out.count < 400 {
            let values = groups.first { $0.0 == k }?.1 ?? []
            let label = switch by {
            case .day, .row: "\(CellText.weekdayName(k)) \(k.day)"
            case .week: "\(k.day) \(CellText.monthName(k))"
            case .month: CellText.monthName(k)
            }
            out.append(ChartPoint(label: label, value: add.apply(values) ?? 0, current: k == now))
            k = next(k)
        }
        return out
    }

    /// The one amount of a ring.
    public func ring(_ t: TableData) -> Double? {
        let y = columns(t).y
        return add.apply(t.filledRows.compactMap { amount(t, $0, y) })
    }

    /// The last four weeks, Monday to Sunday, the week of today last.
    public func days(_ t: TableData, today: Day) -> [ChartDay] {
        let x = columns(t).x
        let dated = x.flatMap { t.kind($0) == .date ? $0 : nil }
        let withRows: Set<Day> = dated.map { c in Set(t.filledRows.compactMap { t.day($0, c, today: today) }) } ?? []
        let start = today.adding(days: -today.weekdayFromMonday - 21)
        return (0..<28).map { i in
            let date = start.adding(days: i)
            return ChartDay(date: date, hasRow: withRows.contains(date), today: date == today, ahead: date > today)
        }
    }

    /// An amount written as its column writes it.
    public func format(_ t: TableData, _ v: Double) -> String {
        guard add != .count, let c = columns(t).y else { return Money.fixed(v, 0) }
        return t.kind(c).isAmount ? t.format(c, v) : Money.fixed(v, t.places(c))
    }

    /// A pace is better when it is lower, so its line is drawn upside down.
    public func upsideDown(_ t: TableData) -> Bool {
        add != .count && (columns(t).y.map { t.kind($0) == .pace } ?? false)
    }

    /// A round number at or above `v`, for the top of a chart: 1, 2, 2.5 or
    /// 5 times a power of ten.
    public static func niceTop(_ v: Double) -> Double {
        if v <= 0 || !v.isFinite { return 1 }
        let power = pow(10, log10(v).rounded(.down))
        for step in [1.0, 2, 2.5, 5, 10] where step * power >= v - 1e-9 {
            return step * power
        }
        return 10 * power
    }
}
