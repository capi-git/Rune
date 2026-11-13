// Tables: rows of cells, with the first row as the header. A table is one
// block; its cells, the kinds of its columns, its name and its view are in
// the block's `data` as JSON. Mirrors `src/table.rs` of the desktop app
// (reading and showing only: the iPhone does not change tables).
//
// How the app shows a table (row numbers count the header as row 0, as
// `rows` does; column numbers start at 0):
//
//   let table = TableData.parse(block.data)          // nil: show "cannot be read"
//   let today = When().today()
//   table.name, table.icon                           // shown above it ("" for none; icon is "name:color")
//   table.view.layout                                // .table, .board, .list, .gallery or .calendar
//   table.shownColumns                               // the columns that show, in order (hidden ones left out)
//   table.view.frozen                                // how many columns at the left stay put when it scrolls
//   table.order(today: today)                        // the rows that show, in order (filters, sorts, nesting, folding)
//   table.shownIndent(r)                             // how deep row r shows nested (0 while a filter is on)
//   table.hasChildren(r), table.info(r).closed       // a row with rows nested in it, and whether they are folded away
//   table.display(r, c)                              // what a cell shows: words, an amount, a box, tags, a link…
//   table.meterFulls()                               // once per table, to pass to display(r, c, fulls:) for bars and rings
//   table.rowColor(r, today: today)                  // the colour a rule paints row r (nil for none)
//   table.total(c, today: today)?.text               // what shows at the foot of column c ("Sum $12.00")
//   table.summary(today: today)                      // the totals in one line, for a layout without columns
//   table.boardColumn, table.groups(rows, c)         // a board's column and its groups of rows
//   table.groupLabel(c, value)                       // the name of a group ("No Status" for the empty one)
//   table.rowsByDay(rows, today: today)              // a calendar's rows on their days
//   table.picture(r), table.facts(r, but:, most:)    // a card's picture and its few words
//   table.nameOf(r)                                  // a row's name ("Untitled" for none)
//
// Charts are in Chart.swift; the rules of the view in TableView.swift;
// money in TableMoney.swift; tags and their colours in TableChoice.swift;
// formulas in Formula.swift.

import Foundation

/// How the cells of a column are read.
public enum ColumnKind: String, CaseIterable, Hashable, Sendable, Codable {
    case text, number, date
    /// A length of time: "34:00", "1:02:30", "34 min".
    case time
    /// Time for each unit of distance, worked out from the first number
    /// column and the first time column of its row.
    case pace
    case checkbox
    /// One of a few words ("Easy", "Long"), shown as a quiet tag.
    case choice
    /// Some of a few words ("Home, Food"), each a tag.
    case multi
    /// Where something stands: one word of a few, with a dot in its tag.
    case status
    /// A web address.
    case link
    case email, phone
    /// Pictures kept with the notes; the cell holds their names.
    case file
    /// Worked out from other cells of the row by `TableColumn.formula`.
    case formula
    /// Rows of another table (`TableColumn.from`), by their names.
    case relation

    /// An unknown kind (from a newer Mac) is read as text.
    public init(from decoder: Decoder) throws {
        self = ColumnKind(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .text
    }

    public var label: String {
        switch self {
        case .text: "Text"
        case .number: "Number"
        case .date: "Date"
        case .time: "Time"
        case .pace: "Pace"
        case .checkbox: "Checkbox"
        case .choice: "Choice"
        case .multi: "Several choices"
        case .status: "Status"
        case .link: "Link"
        case .email: "Email"
        case .phone: "Phone"
        case .file: "Picture"
        case .formula: "Formula"
        case .relation: "Row of another table"
        }
    }

    /// The name of its icon in the Mac's icon set.
    public var icon: String {
        switch self {
        case .text: "text"
        case .number: "hash"
        case .date: "calendar"
        case .time: "clock"
        case .pace: "gauge"
        case .checkbox: "checkbox"
        case .choice: "choice"
        case .multi: "choices"
        case .status: "status"
        case .link: "link"
        case .email: "mail"
        case .phone: "phone"
        case .file: "image"
        case .formula: "sigma"
        case .relation: "arrow-up-right"
        }
    }

    /// Its cells hold words from a list of options with colours.
    public var hasOptions: Bool { self == .choice || self == .multi || self == .status }

    /// Its cells are amounts, which charts can draw and totals add up.
    public var isAmount: Bool { self == .number || self == .time || self == .pace || self == .formula }

    /// Its cells are worked out, not typed.
    public var isWorkedOut: Bool { self == .pace || self == .formula }
}

/// What shows at the foot of a column.
public enum ColumnTotal: String, CaseIterable, Hashable, Sendable, Codable {
    case count, sum, average
    /// The middle amount.
    case median
    case highest, lowest
    /// From the lowest to the highest.
    case range
    /// How many different values there are.
    case unique
    /// How many cells have nothing in them.
    case empty
    /// How many boxes are ticked, of how many rows.
    case checked

    /// The short word written before the total at the foot.
    public var short: String {
        switch self {
        case .sum: "Sum"
        case .average: "Avg"
        case .median: "Med"
        case .highest: "Max"
        case .lowest: "Min"
        case .range: "Range"
        case .count, .unique, .empty, .checked: ""
        }
    }
}

/// How a number shows in its cell.
public enum ColumnShow: String, CaseIterable, Hashable, Sendable, Codable {
    case number
    /// The number, and a bar that is full at the column's goal.
    case bar
    /// The number, and a ring that is closed at the column's goal.
    case ring

    public init(from decoder: Decoder) throws {
        self = ColumnShow(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .number
    }
}

/// How one column is read, and what shows at its foot.
public struct TableColumn: Codable, Hashable, Sendable {
    public var kind: ColumnKind = .text
    /// Written after the heading's name ("km").
    public var unit: String?
    public var total: ColumnTotal?
    /// How its numbers are written: "" as typed, "grouped", "percent", or
    /// the code of a kind of money ("USD"). See `Money`.
    public var format: String = ""
    /// Places after the point, when that is not the format's own number.
    public var places: Int?
    public var show: ColumnShow = .number
    /// The amount at which a bar is full or a ring closed (else the highest
    /// amount of the column, or 100 for a percent).
    public var goal: Double?
    /// The words a choice column offers, in their order.
    public var options: [Choice] = []
    /// What a formula column works out ("Price * Qty").
    public var formula: String = ""
    /// The table (its block's id) whose rows a relation column names.
    public var from: String = ""
    /// Amounts above zero show green and below it red (a gain and a loss).
    public var signed: Bool = false

    public init(kind: ColumnKind = .text, unit: String? = nil, total: ColumnTotal? = nil, format: String = "", places: Int? = nil, show: ColumnShow = .number, goal: Double? = nil, options: [Choice] = [], formula: String = "", from: String = "", signed: Bool = false) {
        self.kind = kind
        self.unit = unit
        self.total = total
        self.format = format
        self.places = places
        self.show = show
        self.goal = goal
        self.options = options
        self.formula = formula
        self.from = from
        self.signed = signed
    }

    private enum CodingKeys: String, CodingKey { case kind, unit, total, format, places, show, goal, options, formula, from, signed }

    /// Every field may be missing; a field that cannot be read is left at
    /// its default.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = c.lenient(ColumnKind.self, .kind) ?? .text
        unit = c.lenient(String.self, .unit)
        total = c.lenient(ColumnTotal.self, .total)
        format = c.lenient(String.self, .format) ?? ""
        places = c.index(.places).map { min($0, 255) }
        show = c.lenient(ColumnShow.self, .show) ?? .number
        goal = c.lenient(Double.self, .goal)
        options = c.lossy(Choice.self, .options)
        formula = c.lenient(String.self, .formula) ?? ""
        from = c.lenient(String.self, .from) ?? ""
        signed = c.lenient(Bool.self, .signed) ?? false
    }
}

/// What a row has besides its cells.
public struct TableRowInfo: Codable, Hashable, Sendable {
    /// What the row goes by, whatever its place among the rows ("" until a
    /// row of the table was opened as a page).
    public var id: String = ""
    /// How deep it is nested under the rows above it (0: not at all).
    public var indent: Int = 0
    /// The rows nested in it are folded away.
    public var closed: Bool = false

    public init(id: String = "", indent: Int = 0, closed: Bool = false) {
        self.id = id
        self.indent = indent
        self.closed = closed
    }

    private enum CodingKeys: String, CodingKey { case id, indent, closed }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = c.lenient(String.self, .id) ?? ""
        indent = c.index(.indent).map { min($0, 255) } ?? 0
        closed = c.lenient(Bool.self, .closed) ?? false
    }
}

/// What a new row starts with (read only, to be complete).
public struct TableTemplate: Codable, Hashable, Sendable {
    /// One for each column; an empty one leaves the cell empty.
    public var cells: [String] = []

    public init(cells: [String] = []) { self.cells = cells }

    private enum CodingKeys: String, CodingKey { case cells }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        cells = c.lenient([String].self, .cells) ?? []
    }
}

/// What shows at the foot of a column: a short word ("Sum", or "") and the amount.
public struct TableTotal: Hashable, Sendable {
    public let word: String
    public let value: String

    /// The two in one: "Sum $12.00", "3 rows".
    public var text: String { word.isEmpty ? value : word + " " + value }
}

public struct TableData: Codable, Hashable, Sendable {
    /// The most rows and columns a table may have.
    public static let mostRows = 200
    public static let mostColumns = 12
    /// How deep a row can be nested under other rows.
    public static let mostNesting = 3

    /// Every row has the same number of cells; the first row is the header.
    public var rows: [[String]]
    /// How each column is read, one for each column, or none at all when
    /// every column is plain text. (Kept as "columns" in the data.)
    public var columnInfo: [TableColumn] {
        didSet { formulas = Self.readFormulas(columnInfo) }
    }
    /// Its name, shown above it ("" for none).
    public var name: String
    /// The icon beside its name ("name:color", see `Icons`).
    public var icon: String
    /// One for each row (the header too), or none at all when no row has a
    /// page or is nested.
    public var info: [TableRowInfo]
    /// Which rows show, in what order, in which layout (see TableView.swift).
    public var view: TableViewRules
    public var template: TableTemplate?
    /// The formulas of the columns, read once. (Worked out from `columns`.)
    private var formulas: [Formula?] = []

    public init(rows: [[String]], columnInfo: [TableColumn] = [], name: String = "", icon: String = "", info: [TableRowInfo] = [], view: TableViewRules = TableViewRules(), template: TableTemplate? = nil) {
        self.rows = rows
        self.columnInfo = columnInfo
        self.name = name
        self.icon = icon
        self.info = info
        self.view = view
        self.template = template
        square()
    }

    private enum CodingKeys: String, CodingKey {
        case rows, columnInfo = "columns", name, icon, info, view, template
    }

    /// The rows must be there (as lists of words); anything else that
    /// cannot be read is left at its default, and fields this app does not
    /// know are passed over.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        rows = try c.decode([[String]].self, forKey: .rows)
        columnInfo = c.lossy(TableColumn.self, .columnInfo)
        name = c.lenient(String.self, .name) ?? ""
        icon = c.lenient(String.self, .icon) ?? ""
        info = c.lossy(TableRowInfo.self, .info)
        view = c.lenient(TableViewRules.self, .view) ?? TableViewRules()
        template = c.lenient(TableTemplate.self, .template)
        square()
    }

    /// The table kept in a block, or nil when the data cannot be read.
    /// (The Mac shows a new empty table then; the iPhone says so instead.)
    public static func parse(_ data: String) -> TableData? {
        try? JSONDecoder().decode(TableData.self, from: Data(data.utf8))
    }

    /// How many columns the table has.
    public var columns: Int {
        rows.first?.count ?? 0
    }

    /// The rows under the header.
    public var body: ArraySlice<[String]> {
        rows.dropFirst()
    }

    /// The words of the table, for search: its name, then its rows.
    public var plainText: String {
        ((name.isEmpty ? [] : [name]) + rows.map { $0.joined(separator: " ") }).joined(separator: "\n")
    }

    private static func readFormulas(_ columnInfo: [TableColumn]) -> [Formula?] {
        columnInfo.map { $0.kind == .formula ? (try? Formula.parse($0.formula).get()) : nil }
    }

    /// Make every row as wide as the widest, with at least one cell at all;
    /// keep one description for each column and each row (or none), and
    /// nothing that names a column or a nesting that is not there.
    private mutating func square() {
        if rows.count > TableData.mostRows {
            rows.removeSubrange(TableData.mostRows...)
        }
        if rows.isEmpty {
            rows = [[]]
        }
        let width = min(max(rows.map(\.count).max() ?? 0, 1), TableData.mostColumns)
        rows = rows.map { row in
            Array(row.prefix(width)) + Array(repeating: "", count: max(width - row.count, 0))
        }
        if columnInfo.allSatisfy({ $0 == TableColumn() }) {
            columnInfo = []
        } else {
            columnInfo = Array(columnInfo.prefix(width)) + Array(repeating: TableColumn(), count: max(width - columnInfo.count, 0))
        }
        if !info.isEmpty {
            info = Array(info.prefix(rows.count)) + Array(repeating: TableRowInfo(), count: max(rows.count - info.count, 0))
            // The header and the first row are not nested, and no row is
            // more than one step deeper than the row above it.
            var above = 0
            for r in info.indices {
                let most = r <= 1 ? 0 : min(above + 1, TableData.mostNesting)
                info[r].indent = max(0, min(info[r].indent, most))
                above = info[r].indent
            }
            // (The header is no row to open.)
            info[0].id = ""
            if info.allSatisfy({ $0 == TableRowInfo() }) {
                info = []
            }
        }
        if var t = template {
            t.cells = Array(t.cells.prefix(width)) + Array(repeating: "", count: max(width - t.cells.count, 0))
            template = t
        }
        view.tidy(width: width)
        syncOptions()
        formulas = Self.readFormulas(columnInfo)
    }

    // ----- columns ------------------------------------------------------------

    /// How column `c` is read.
    public func column(_ c: Int) -> TableColumn {
        c >= 0 && c < columnInfo.count ? columnInfo[c] : TableColumn()
    }

    public func kind(_ c: Int) -> ColumnKind {
        column(c).kind
    }

    /// Some column has a kind, a unit or a total.
    public var isTyped: Bool { !columnInfo.isEmpty }

    /// The heading of column `c`, or "Column 3" when it has none.
    public func heading(_ c: Int) -> String {
        let h = rows.first.flatMap { c >= 0 && c < $0.count ? $0[c] : nil }?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return h.isEmpty ? "Column \(c + 1)" : h
    }

    /// The first column of a kind.
    public func firstOf(_ kind: ColumnKind) -> Int? {
        (0..<columns).first { self.kind($0) == kind }
    }

    /// The column with this heading (any case), or whose unit is these
    /// words, or whose heading holds them ("km" finds "Distance (km)"), or
    /// the column with this number (from 1).
    public func findColumn(_ name: String) -> Int? {
        let want = name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if want.isEmpty { return nil }
        let heads = (rows.first ?? []).map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
        if let c = heads.firstIndex(of: want) { return c }
        if let c = heads.indices.first(where: { column($0).unit?.lowercased() == want }) { return c }
        if let c = heads.firstIndex(where: { !$0.isEmpty && ($0.contains(want) || want.contains($0)) }) { return c }
        if let n = Int(want), n >= 1, n <= heads.count { return n - 1 }
        return nil
    }

    /// The column with exactly this heading (any case).
    public func findHeading(_ name: String) -> Int? {
        let want = name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !want.isEmpty else { return nil }
        return rows.first?.firstIndex { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == want }
    }

    /// What is wrong with the formula of column `c`, in one sentence.
    public func formulaError(_ c: Int) -> String? {
        let col = column(c)
        guard col.kind == .formula, !col.formula.isEmpty else { return nil }
        switch Formula.parse(col.formula) {
        case .failure(let why): return why.sentence
        case .success(let formula):
            return formula.names.first { name in findHeading(name).map { $0 == c } ?? true }.map { "There is no column \"\($0)\"." }
        }
    }

    // ----- rows -----------------------------------------------------------------

    /// What a row has besides its cells.
    public func info(_ r: Int) -> TableRowInfo {
        r >= 0 && r < info.count ? info[r] : TableRowInfo()
    }

    /// How deep a row is nested.
    public func indent(_ r: Int) -> Int {
        info(r).indent
    }

    /// The row after `r` and the rows nested in it.
    public func subtreeEnd(_ r: Int) -> Int {
        let level = indent(r)
        var end = r + 1
        while end < rows.count, indent(end) > level { end += 1 }
        return end
    }

    /// Rows are nested in this row.
    public func hasChildren(_ r: Int) -> Bool {
        r >= 1 && r + 1 < rows.count && indent(r + 1) > indent(r)
    }

    /// What a row goes by, if the rows of the table have such names yet
    /// (a row's page finds its row by this).
    public func rowID(_ r: Int) -> String? {
        let id = info(r).id
        return id.isEmpty ? nil : id
    }

    /// The row that goes by `id`.
    public func rowWithID(_ id: String) -> Int? {
        id.isEmpty ? nil : info.firstIndex { $0.id == id }
    }

    /// The name of a row: its first cell.
    public func title(_ r: Int) -> String {
        cell(r, 0).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// The name of a row as a card shows it.
    public func nameOf(_ r: Int) -> String {
        let t = title(r)
        return t.isEmpty ? "Untitled" : t
    }

    // ----- values ---------------------------------------------------------------

    /// The rows under the header that have something in them.
    public var filledRows: [Int] {
        rows.indices.dropFirst().filter { !isBlank($0) }
    }

    /// A row with nothing in any cell.
    public func isBlank(_ r: Int) -> Bool {
        guard r >= 0, r < rows.count else { return true }
        return rows[r].allSatisfy { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }

    public func cell(_ r: Int, _ c: Int) -> String {
        guard r >= 0, r < rows.count, c >= 0, c < rows[r].count else { return "" }
        return rows[r][c]
    }

    /// What a cell is worth to a formula. A formula that reads itself, or
    /// formulas that read each other in a ring, are worth nothing.
    func worth(_ r: Int, _ c: Int, depth: Int) -> FormulaValue {
        let text = cell(r, c).trimmingCharacters(in: .whitespacesAndNewlines)
        func amount(_ v: Double?) -> FormulaValue { v.map { .number($0) } ?? .empty }
        switch kind(c) {
        case .formula:
            guard c < formulas.count, let formula = formulas[c], depth < 8 else { return .empty }
            let found = formula.eval { name in
                findHeading(name).flatMap { $0 == c ? nil : worth(r, $0, depth: depth + 1) }
            }
            return (try? found.get()) ?? .empty
        case .checkbox: return .bool(CellText.checked(text))
        case .number: return amount(CellText.number(text))
        case .time: return amount(CellText.seconds(text))
        case .pace: return amount(pace(r))
        default: return text.isEmpty ? .empty : .text(text)
        }
    }

    /// The amount in a cell: a number, a time in seconds, a pace in seconds
    /// for each unit, 1 or 0 for a checkbox, what a formula works out.
    public func value(_ r: Int, _ c: Int) -> Double? {
        switch kind(c) {
        case .time: CellText.seconds(cell(r, c))
        case .pace: pace(r)
        case .checkbox: CellText.checked(cell(r, c)) ? 1 : 0
        case .formula: worth(r, c, depth: 0).number.flatMap { $0.isFinite ? $0 : nil }
        default: CellText.number(cell(r, c))
        }
    }

    /// The pace of a row: its first time column over its first number column.
    public func pace(_ r: Int) -> Double? {
        guard let n = firstOf(.number), let distance = CellText.number(cell(r, n)),
              let t = firstOf(.time), let time = CellText.seconds(cell(r, t)), distance > 0
        else { return nil }
        return time / distance
    }

    /// The day in a cell of a date column.
    public func day(_ r: Int, _ c: Int, today: Day) -> Day? {
        CellText.day(cell(r, c), today: today)
    }

    /// What a cell shows as words: its text, or what is worked out for it.
    public func shown(_ r: Int, _ c: Int) -> String {
        switch kind(c) {
        case .pace where r > 0:
            return pace(r).map(CellText.clock) ?? ""
        case .formula where r > 0:
            switch worth(r, c, depth: 0) {
            case .number(let v) where v.isFinite: return format(c, v)
            case .text(let text): return text
            case .bool(let yes): return yes ? "Yes" : "No"
            default: return ""
            }
        default:
            return cell(r, c)
        }
    }

    /// For a row with rows nested in it and nothing typed in an amount
    /// cell: the sum of that column over the nested rows (shown greyed).
    public func rolledUp(_ r: Int, _ c: Int) -> String? {
        guard kind(c) == .number || kind(c) == .time,
              cell(r, c).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, hasChildren(r)
        else { return nil }
        let values = (r + 1..<subtreeEnd(r)).compactMap { value($0, c) }
        return values.isEmpty ? nil : format(c, values.reduce(0, +))
    }

    /// How many places after the point the numbers of a column use: its
    /// own number of places, or as many as its cells were typed with (two
    /// at the most).
    public func places(_ c: Int) -> Int {
        if let places = column(c).places { return places }
        return min(filledRows.compactMap { Money.read(cell($0, c))?.typed }.max() ?? 0, 2)
    }

    /// An amount of column `c` written as the column writes it.
    public func format(_ c: Int, _ v: Double) -> String {
        let col = column(c)
        if col.kind == .time || col.kind == .pace { return CellText.clock(v) }
        if Money.currency(col.format) != nil { return Money.write(col.format, v, places: col.places) }
        if col.kind == .formula, col.places == nil {
            // What a formula works out has no typed places: as few as the
            // number needs, two at the most.
            let cents = (v * 100).rounded()
            let places = cents.truncatingRemainder(dividingBy: 100) == 0 ? 0 : cents.truncatingRemainder(dividingBy: 10) == 0 ? 1 : 2
            return Money.write(col.format, v, places: places)
        }
        return Money.write(col.format, v, places: places(c))
    }

    /// The amount at which a bar or a ring of column `c` is full: the
    /// column's goal, or 100 for a percent, or the highest amount it has.
    public func fullAt(_ c: Int) -> Double {
        let col = column(c)
        if let goal = col.goal { return goal }
        if col.format == Money.percent { return 100 }
        return filledRows.compactMap { value($0, c) }.reduce(0) { max($0, $1) }
    }

    /// What shows at the foot of column `c`. Only the rows that pass the
    /// filters count.
    public func total(_ c: Int, today: Day = When().today()) -> TableTotal? {
        guard let total = column(c).total else { return nil }
        let rows = counted(today: today)
        // (A table with nothing in its rows yet has nothing to add up.)
        if rows.allSatisfy({ isBlank($0) }) { return nil }
        func filled(_ r: Int) -> Bool { !shown(r, c).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        var values = rows.compactMap { value($0, c) }
        let low = values.min(), high = values.max()
        let text: String
        switch total {
        case .count:
            let n = rows.filter(filled).count
            text = view.filter.isEmpty ? "\(n) \(n == 1 ? "row" : "rows")" : "\(n) of \(filledRows.count) rows"
        case .unique:
            let seen = Set(rows.map { shown($0, c).trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }.filter { !$0.isEmpty })
            text = "\(seen.count) different"
        case .empty:
            text = "\(rows.filter { !filled($0) }.count) empty"
        case .checked:
            text = "\(rows.filter { CellText.checked(cell($0, c)) }.count) of \(rows.count)"
        case .sum:
            guard !values.isEmpty else { return nil }
            text = format(c, values.reduce(0, +))
        case .average:
            guard !values.isEmpty else { return nil }
            text = format(c, values.reduce(0, +) / Double(values.count))
        case .median:
            guard !values.isEmpty else { return nil }
            values.sort()
            let mid = values.count / 2
            text = format(c, values.count % 2 == 1 ? values[mid] : (values[mid - 1] + values[mid]) / 2)
        case .highest:
            guard let high else { return nil }
            text = format(c, high)
        case .lowest:
            guard let low else { return nil }
            text = format(c, low)
        case .range:
            guard let high, let low else { return nil }
            text = format(c, high - low)
        }
        return TableTotal(word: total.short, value: text)
    }

    /// The totals of a table in one line, for a layout that has no columns
    /// to put them under (a board, a list, a gallery, a calendar). The
    /// number of rows comes first.
    public func summary(today: Day = When().today()) -> [String] {
        var totals = shownColumns.filter { kind($0) != .checkbox }.compactMap { total($0, today: today)?.text }
        let n = counted(today: today).filter { !isBlank($0) }.count
        if n > 0, !totals.contains(where: { $0.hasSuffix(" row") || $0.hasSuffix(" rows") }) {
            totals.insert(n == 1 ? "1 row" : "\(n) rows", at: 0)
        }
        return totals
    }

    // ----- what a cell shows ----------------------------------------------------

    /// For each column, the amount at which its bars or rings are full (0
    /// for a column that shows plain numbers). Work it out once for a
    /// table and pass it to `display(_:_:fulls:)`.
    public func meterFulls() -> [Double] {
        (0..<columns).map { column($0).show == .number ? 0 : fullAt($0) }
    }

    /// What a cell shows, by the kind of its column. `fulls` is
    /// `meterFulls()`, worked out once for the table (it is worked out here
    /// when it is not given).
    public func display(_ r: Int, _ c: Int, fulls: [Double]? = nil) -> CellDisplay {
        let col = column(c)
        let text = cell(r, c)
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        switch col.kind {
        case .checkbox:
            return .checkbox(CellText.checked(text))
        case .choice, .multi, .status:
            let tags = col.names(in: text).map { CellTag(name: $0, color: col.color(of: $0)) }
            return .tags(tags, dots: col.kind == .status)
        case .link, .email, .phone:
            guard !trimmed.isEmpty else { return .link(text: "", url: nil) }
            let url: String = switch col.kind {
            case .email: "mailto:" + trimmed
            case .phone: "tel:" + trimmed.filter { $0.isNumber || $0 == "+" }
            default: trimmed.contains("://") ? trimmed : "https://" + trimmed
            }
            return .link(text: trimmed, url: url)
        case .file:
            return .pictures(CellText.parts(text).filter(CellText.isPictureName))
        case .relation:
            return .rows(CellText.parts(text))
        case .number, .time, .pace, .formula:
            let shown = shown(r, c)
            var words = col.kind.isWorkedOut ? shown : text
            var rolled = false
            if !col.kind.isWorkedOut, trimmed.isEmpty, let sum = rolledUp(r, c) {
                words = sum
                rolled = true
            }
            // A gain green and a loss red, in a column that asks for it.
            let amount = col.kind.isWorkedOut ? CellText.number(shown) : (CellText.number(text) ?? CellText.number(shown))
            let sign = col.signed ? (amount.map { $0 > 0 ? 1 : $0 < 0 ? -1 : 0 } ?? 0) : 0
            var meter: Double?
            if col.show != .number, col.kind != .time {
                let full = fulls.flatMap { c < $0.count ? $0[c] : nil } ?? fullAt(c)
                if full > 0, let v = value(r, c) { meter = min(max(v / full, 0), 1) }
            }
            return .amount(CellAmount(text: words, rolledUp: rolled, sign: sign, meter: meter, show: col.show))
        case .text, .date:
            return .text(text)
        }
    }

    /// The colour a paint rule gives a row (nil for none).
    public func rowColor(_ r: Int, today: Day = When().today()) -> TagColor? {
        color(r, today: today).flatMap(TagColor.parse)
    }

    /// The name of a group of a board or of grouped rows: the value, or
    /// "No Status" for the rows with nothing in the column, or "Ticked".
    public func groupLabel(_ c: Int, _ value: String) -> String {
        if kind(c) == .checkbox { return CellText.checked(value) ? "Ticked" : "Not ticked" }
        return value.isEmpty ? "No \(heading(c))" : value
    }

    /// A calendar's rows: each of `rows` on the day of the first date column.
    public func rowsByDay(_ rows: [Int], today: Day = When().today()) -> [Day: [Int]] {
        guard let dc = firstOf(.date) else { return [:] }
        var out: [Day: [Int]] = [:]
        for r in rows {
            if let d = day(r, dc, today: today) { out[d, default: []].append(r) }
        }
        return out
    }

    /// The picture of a gallery card: the first picture of the first
    /// picture column.
    public func picture(_ r: Int) -> String? {
        guard let c = firstOf(.file) else { return nil }
        return CellText.parts(cell(r, c)).first(where: CellText.isPictureName)
    }

    /// What a row says besides its name, in a few short words: what the
    /// columns that show have for it (not the names, not the column `but`,
    /// not pictures; a ticked box says the name of its column).
    public func facts(_ r: Int, but: Int? = nil, most: Int = 2) -> [String] {
        var out: [String] = []
        for c in shownColumns where c != 0 && c != but {
            let text: String = switch kind(c) {
            case .file: ""
            case .checkbox: CellText.checked(cell(r, c)) ? heading(c) : ""
            default: shown(r, c).trimmingCharacters(in: .whitespacesAndNewlines)
            }
            if !text.isEmpty { out.append(text) }
            if out.count == most { break }
        }
        return out
    }
}

/// What a cell shows.
public enum CellDisplay: Hashable, Sendable {
    /// Words as typed (text and date columns). May be "".
    case text(String)
    /// A number, a time, a pace or what a formula works out.
    case amount(CellAmount)
    case checkbox(Bool)
    /// The tags of a choice, several choices or a status column (a status
    /// tag has a dot in it). Empty for an empty cell.
    case tags([CellTag], dots: Bool)
    /// A link, an email address or a phone number, and the address that
    /// opens it (nil for an empty cell).
    case link(text: String, url: String?)
    /// The names of the pictures in a cell (files kept with the notes).
    case pictures([String])
    /// The names of rows of another table (`TableColumn.from`).
    case rows([String])
}

/// An amount as a cell shows it.
public struct CellAmount: Hashable, Sendable {
    /// What to write: as typed for a number or a time, worked out for a
    /// pace or a formula. May be "".
    public let text: String
    /// The text is the sum of the rows nested in this row (nothing is
    /// typed in it): show it greyed.
    public let rolledUp: Bool
    /// 1 for a gain (green), -1 for a loss (red), 0 for neither (or a
    /// column that is not signed).
    public let sign: Int
    /// How full the bar or ring is, from 0 to 1 (nil: draw none).
    public let meter: Double?
    /// A bar or a ring (when `meter` is not nil).
    public let show: ColumnShow
}

/// One tag of a cell.
public struct CellTag: Hashable, Sendable {
    public let name: String
    public let color: TagColor

    public init(name: String, color: TagColor) {
        self.name = name
        self.color = color
    }
}

// ----- reading cells -----------------------------------------------------------

/// How the words of a cell are read: numbers, times, ticks, days.
public enum CellText {
    /// The number at the start of a cell, its unit left out: "6", "6.5 km",
    /// "6,5", "1,500" (a thousand and five hundred), "$3,500.00", "-€20", "15%".
    public static func number(_ text: String) -> Double? {
        Money.read(text)?.value
    }

    /// The words of a cell that holds several: "Home, Food" is two.
    public static func parts(_ text: String) -> [String] {
        text.split(separator: ",", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    /// A cell that holds several has these words among them (a name with a
    /// comma in it, "Smith, John", is two parts in a row).
    public static func holdsPart(_ cell: String, _ part: String) -> Bool {
        let all = parts(cell).map { $0.lowercased() }, want = parts(part).map { $0.lowercased() }
        guard !want.isEmpty, want.count <= all.count else { return false }
        return (0...all.count - want.count).contains { Array(all[$0..<$0 + want.count]) == want }
    }

    /// A length of time in seconds: "34:00", "1:02:30", "34 min", "1h 5m",
    /// "90s"; a bare number is minutes.
    public static func seconds(_ text: String) -> Double? {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if text.isEmpty { return nil }
        if text.contains(":") {
            let parts = text.split(separator: ":", omittingEmptySubsequences: false).map { Double($0.trimmingCharacters(in: .whitespacesAndNewlines)) }
            guard parts.allSatisfy({ $0 != nil }) else { return nil }
            let p = parts.map { $0! }
            switch p.count {
            case 2: return p[0] * 60 + p[1]
            case 3: return p[0] * 3600 + p[1] * 60 + p[2]
            default: return nil
            }
        }
        var total = 0.0
        var found = false
        var rest = Substring.UnicodeScalarView(text.unicodeScalars)
        while true {
            rest = rest.drop { $0.properties.isWhitespace }
            if rest.isEmpty { break }
            let number = rest.prefix { $0.isASCIIDigit || $0 == "." }
            if number.isEmpty { return nil }
            guard let n = Double(String(number)) else { return nil }
            rest = rest.dropFirst(number.count).drop { $0.properties.isWhitespace }
            let unit = String(rest.prefix { $0.properties.isAlphabetic })
            rest = rest.dropFirst(unit.unicodeScalars.count)
            let scale: Double
            switch unit {
            case "h", "hr", "hrs", "hour", "hours": scale = 3600
            case "", "m", "min", "mins", "minute", "minutes": scale = 60
            case "s", "sec", "secs", "second", "seconds": scale = 1
            default: return nil
            }
            total += n * scale
            found = true
        }
        return found ? total : nil
    }

    /// A time in seconds as "34:00" or "1:02:30".
    public static func clock(_ secs: Double) -> String {
        let s = secs.isFinite ? Int(max(min(secs.rounded(), 9e15), 0)) : 0
        if s >= 3600 {
            return "\(s / 3600):" + String(format: "%02d:%02d", (s % 3600) / 60, s % 60)
        }
        return "\(s / 60):" + String(format: "%02d", s % 60)
    }

    /// A ticked box: "x", "yes", "✓", "done", "true", "1".
    public static func checked(_ text: String) -> Bool {
        ["x", "yes", "y", "true", "1", "✓", "✔", "done", "[x]"].contains(text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased())
    }

    private static let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]

    private static func words(_ text: String) -> [String] {
        text.split { $0.isWhitespace || $0 == "," || $0 == "/" || $0 == "." }.map(String.init)
    }

    /// A day written in a cell: "2026-12-01", "Tue 1 Dec", "1 December",
    /// "Dec 1, 2026", "today", "yesterday". With no year: the nearest such
    /// day ("3 January" in December is next month; a day ahead counts as
    /// twice as far, since a log is mostly of days gone by).
    public static func day(_ text: String, today: Day) -> Day? {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        switch text {
        case "": return nil
        case "today": return today
        case "yesterday": return today.adding(days: -1)
        case "tomorrow": return today.adding(days: 1)
        default: break
        }
        if let d = Day.parse(text) { return d }
        var month: Int?, dom: Int?, year: Int?
        for word in words(text) {
            if word.utf8.count >= 3, let m = months.firstIndex(where: { word.hasPrefix($0) }) {
                month = m + 1
            } else if let n = Int(String(word.unicodeScalars.reversed().drop { $0.properties.isAlphabetic }.reversed().map(Character.init))) {
                if (1...31).contains(n), dom == nil {
                    dom = n
                } else if (1900...2999).contains(n) {
                    year = n
                }
            }
        }
        guard let month, let dom else { return nil }
        if let year { return Day(year: year, month: month, day: dom) }
        var best: (Day, Int)?
        for y in today.year - 1...today.year + 1 {
            guard let d = Day(year: y, month: month, day: dom) else { continue }
            let days = d.ordinal - today.ordinal
            let far = days > 0 ? days * 2 : -days
            if best == nil || far < best!.1 { best = (d, far) }
        }
        return best?.0
    }

    /// The day a cell says, when the cell says a day and no more: each of
    /// its words is the name of a month or of a day of the week, a number,
    /// or "of" or "the". "Mark 3 done" is no day.
    public static func wholeDay(_ text: String, today: Day) -> Day? {
        let names = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december",
                     "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
        func known(_ word: String) -> Bool {
            let number = String(word.unicodeScalars.reversed().drop { $0.isASCII && $0.properties.isAlphabetic }.reversed().map(Character.init))
            let ending = String(word.dropFirst(number.count))
            return (word.utf8.count >= 3 && names.contains { $0.hasPrefix(word) })
                || ["of", "the", "today", "yesterday", "tomorrow"].contains(word)
                || Day.parse(word) != nil
                || (!number.isEmpty && number.unicodeScalars.allSatisfy(\.isASCIIDigit) && ["", "st", "nd", "rd", "th"].contains(ending))
        }
        let low = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return words(low).allSatisfy(known) ? day(text, today: today) : nil
    }

    private static let weekdayNames = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    private static let monthNames = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

    /// "Tue", the short name of a day's weekday (in English, as on the Mac).
    public static func weekdayName(_ d: Day) -> String { weekdayNames[d.weekdayFromMonday] }
    /// "Dec", the short name of a day's month.
    public static func monthName(_ d: Day) -> String { monthNames[d.month - 1] }

    /// A day as a date column writes it: "Tue 1 Dec", with the year when it
    /// is not this year's.
    public static func formatDay(_ d: Day, today: Day) -> String {
        let s = "\(weekdayName(d)) \(d.day) \(monthName(d))"
        return d.year == today.year ? s : "\(s) \(d.year)"
    }

    /// A name of a picture kept with the notes: letters and digits, one dot,
    /// a known ending. (Nothing else is ever read from the folder.)
    public static func isPictureName(_ name: String) -> Bool {
        let split = name.split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        guard split.count == 2 else { return false }
        let (stem, ending) = (split[0], split[1])
        return !stem.isEmpty && stem.utf8.count <= 40 && stem.unicodeScalars.allSatisfy { $0.isASCII && $0.properties.isAlphabetic || $0.isASCIIDigit }
            && ["png", "jpg", "gif", "webp", "heic", "tiff", "bmp"].contains(String(ending))
    }
}

// ----- reading JSON as the Mac's serde does, and a little more kindly ---------------

/// One element of a list that is passed over when it cannot be read.
struct Lossy<T: Decodable>: Decodable {
    let value: T?
    init(from decoder: Decoder) throws {
        value = try? T(from: decoder)
    }
}

extension KeyedDecodingContainer {
    /// The value, or nil when it is missing or cannot be read.
    func lenient<T: Decodable>(_ type: T.Type, _ key: Key) -> T? {
        (try? decodeIfPresent(T.self, forKey: key)) ?? nil
    }

    /// A list, without the elements that cannot be read.
    func lossy<T: Decodable>(_ type: T.Type, _ key: Key) -> [T] {
        (lenient([Lossy<T>].self, key) ?? []).compactMap(\.value)
    }

    /// A count or a place (not below zero). A number too big for an Int is
    /// the most there can be, so that it is cut down where it is used.
    func index(_ key: Key) -> Int? {
        if let n = lenient(Int.self, key) { return n >= 0 ? n : nil }
        if let d = lenient(Double.self, key), d >= 0, d == d.rounded() { return Int(min(d, 1e15)) }
        return nil
    }
}
