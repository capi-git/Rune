// A table block, as the Mac shows it (src/ui/table*.rs): its name and icon,
// then its rows in the layout of its view (a table, a board, a list, a
// gallery or a calendar). Cells show by the kind of their column: money in
// green or red, tags in their colours, a box for a checkbox, bars and rings.
// The rules are in Notion2Core (Table.swift, TableView.swift); this file only
// draws. Read-only: a row with a page of its own opens that page.

import Notion2Core
import SwiftUI

struct TableBlockView: View {
    let pageID: ID
    let blockID: ID
    let table: TableData

    @Environment(AppModel.self) private var model

    var body: some View {
        let today = model.when.today()
        VStack(alignment: .leading, spacing: 8) {
            if !table.name.isEmpty || !table.icon.isEmpty {
                HStack(spacing: 7) {
                    if !table.icon.isEmpty {
                        PageIconView(icon: table.icon, size: 17)
                    }
                    Text(table.name.isEmpty ? "Table" : table.name)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(Theme.text)
                }
                .accessibilityAddTraits(.isHeader)
            }
            switch table.view.layout {
            case .table:
                TableGrid(blockID: blockID, table: table, today: today)
            case .board:
                TableBoard(blockID: blockID, table: table, today: today)
            case .list:
                TableList(blockID: blockID, table: table, today: today)
            case .gallery:
                TableGallery(blockID: blockID, table: table, today: today)
            case .calendar:
                TableCalendar(blockID: blockID, table: table, today: today)
            }
            if table.view.layout != .table {
                let summary = table.summary(today: today)
                if !summary.isEmpty {
                    Text(summary.joined(separator: "  ·  "))
                        .font(.system(size: 12.5))
                        .foregroundStyle(Theme.text2)
                }
            }
        }
    }
}

// MARK: - Shared parts

/// The colours of a tag, a painted row and a bar: the Mac's own
/// (`--tag-*-bg`, `--tag-ink`, `--gain`, `--loss` in assets/tables.css), so
/// a tag looks the same on both.
enum TagLook {
    private static let light: [TagColor: UInt32] = [
        .gray: 0xE3E2E0, .brown: 0xEEE0DA, .orange: 0xFADEC9, .yellow: 0xFDECC8, .green: 0xDBEDDB,
        .blue: 0xD3E5EF, .purple: 0xE8DEEE, .pink: 0xF5E0E9, .red: 0xFFE2DD,
    ]
    private static let dark: [TagColor: UInt32] = [
        .gray: 0x545454, .brown: 0x603B2C, .orange: 0x854C1D, .yellow: 0x89632A, .green: 0x2B593F,
        .blue: 0x28456C, .purple: 0x492F64, .pink: 0x69314C, .red: 0x6E3630,
    ]

    static func fill(_ color: TagColor) -> Color {
        guard let l = light[color], let d = dark[color] else {
            return .dynamic(light: Color(hex: 0xCECDCA, opacity: 0.5), dark: Color(hex: 0x373737))
        }
        return .dynamic(light: Color(hex: l), dark: Color(hex: d))
    }

    static func ink(_ color: TagColor) -> Color {
        .dynamic(light: Color(hex: 0x32302C), dark: .white.opacity(0.88))
    }

    /// The dot of a status tag, a slice of a pie.
    static func dot(_ color: TagColor) -> Color {
        color == .default ? Theme.text2 : Theme.tint(color.rawValue).bar
    }

    /// A row a colour rule paints: its tag colour, softer.
    static func row(_ color: TagColor) -> Color {
        fill(color).opacity(0.55)
    }

    /// A gain and a loss.
    static let gain = Color.dynamic(light: Color(hex: 0x287850), dark: Color(hex: 0x5FC88C))
    static let loss = Color.dynamic(light: Color(hex: 0xBE3C32), dark: Color(hex: 0xF0786E))
}

/// Opens the page of a row when it has one.
private struct RowLink<Label: View>: View {
    let blockID: ID
    let table: TableData
    let row: Int
    @ViewBuilder let label: () -> Label

    @Environment(AppModel.self) private var model

    var body: some View {
        if let id = table.rowID(row), let page = model.workspace.rowPage(table: blockID, row: id) {
            NavigationLink(value: Route.page(page.id)) {
                label().contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        } else {
            label()
        }
    }
}

/// What one cell shows.
struct CellView: View {
    let display: CellDisplay
    /// The cell may run onto more lines.
    var wraps = true
    var size: CGFloat = 15
    /// Amounts at the start of the line (else at its end, as in a column).
    var leading = false

    var body: some View {
        switch display {
        case let .text(text):
            Text(text.isEmpty ? " " : text)
                .font(.system(size: size))
                .foregroundStyle(Theme.text)
                .lineLimit(wraps ? nil : 1)
        case let .amount(amount):
            AmountView(amount: amount, size: size, leading: leading)
        case let .checkbox(on):
            CheckBox(on: on)
        case let .tags(tags, dots):
            TagsView(tags: tags, dots: dots, wraps: wraps)
        case let .link(text, url):
            if let url, let address = URL(string: url) {
                Link(destination: address) {
                    Text(text)
                        .font(.system(size: size))
                        .underline()
                        .lineLimit(1)
                }
                .foregroundStyle(Theme.text)
            } else {
                Text(text.isEmpty ? " " : text)
                    .font(.system(size: size))
                    .foregroundStyle(Theme.text)
            }
        case let .pictures(names):
            HStack(spacing: 4) {
                ForEach(names.prefix(3), id: \.self) { name in
                    PictureLoader(name: name) { image in
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFill()
                            .frame(width: 26, height: 26)
                            .clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
                    } missing: {
                        RoundedRectangle(cornerRadius: 5, style: .continuous)
                            .fill(Theme.field)
                            .frame(width: 26, height: 26)
                    }
                }
            }
            .accessibilityLabel(names.count == 1 ? "1 picture" : "\(names.count) pictures")
        case let .rows(names):
            TagsView(tags: names.map { CellTag(name: $0, color: .default) }, dots: false, wraps: wraps)
        }
    }
}

private struct AmountView: View {
    let amount: CellAmount
    let size: CGFloat
    let leading: Bool

    var body: some View {
        let ink: Color = amount.sign > 0 ? TagLook.gain : amount.sign < 0 ? TagLook.loss : Theme.text
        HStack(spacing: 7) {
            if amount.show == .ring, let meter = amount.meter {
                ZStack {
                    Circle().stroke(Theme.field, lineWidth: 3)
                    Circle()
                        .trim(from: 0, to: min(max(meter, 0), 1))
                        .stroke(Theme.ink, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                }
                .frame(width: 15, height: 15)
            }
            Text(amount.text.isEmpty ? " " : amount.text)
                .font(.system(size: size, weight: amount.sign != 0 ? .medium : .regular))
                .monospacedDigit()
                .foregroundStyle(amount.rolledUp ? Theme.text2 : ink)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: leading || amount.show == .ring ? .leading : .trailing)
        .overlay(alignment: .bottom) {
            if amount.show == .bar, let meter = amount.meter {
                GeometryReader { box in
                    Capsule().fill(Theme.field)
                        .overlay(alignment: .leading) {
                            Capsule().fill(Theme.ink).frame(width: box.size.width * min(max(meter, 0), 1))
                        }
                }
                .frame(height: 3)
                .offset(y: 7)
            }
        }
    }
}

private struct CheckBox: View {
    let on: Bool

    var body: some View {
        RoundedRectangle(cornerRadius: 4.5, style: .continuous)
            .fill(on ? Theme.check : .clear)
            .strokeBorder(on ? .clear : Theme.checkEmpty, lineWidth: 1.5)
            .frame(width: 17, height: 17)
            .overlay {
                if on {
                    IconView("check", size: 12, weight: 2.4)
                        .foregroundStyle(Theme.checkMark)
                }
            }
            .accessibilityLabel(on ? "Ticked" : "Not ticked")
    }
}

struct TagsView: View {
    let tags: [CellTag]
    let dots: Bool
    var wraps = true

    var body: some View {
        if tags.isEmpty {
            Text(" ").font(.system(size: 13))
        } else if wraps {
            FlowLayout(spacing: 4) { chips }
        } else {
            HStack(spacing: 4) { chips }
        }
    }

    @ViewBuilder
    private var chips: some View {
        ForEach(Array(tags.enumerated()), id: \.offset) { _, tag in
            HStack(spacing: 5) {
                if dots {
                    Circle().fill(TagLook.dot(tag.color)).frame(width: 6, height: 6)
                }
                Text(tag.name)
                    .font(.system(size: 13, weight: .medium))
                    .lineLimit(1)
            }
            .foregroundStyle(TagLook.ink(tag.color))
            .padding(.horizontal, 7)
            .frame(height: 22)
            .background(TagLook.fill(tag.color), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
    }
}

/// Lays its children out in lines, as words are.
struct FlowLayout: Layout {
    var spacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width {
                y += line + spacing
                x = 0
                line = 0
            }
            x += size.width + spacing
            line = max(line, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: min(widest, width), height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, line: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                y += line + spacing
                x = bounds.minX
                line = 0
            }
            view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            line = max(line, size.height)
        }
    }
}

// MARK: - Table

private struct TableGrid: View {
    let blockID: ID
    let table: TableData
    let today: Day

    @State private var available: CGFloat = 0

    private static let fontSize: CGFloat = 15
    private static let padding: CGFloat = 11
    private static let narrowest: CGFloat = 64
    private static let widest: CGFloat = 244
    private static let lineHeight: CGFloat = 40

    var body: some View {
        let columns = table.shownColumns
        let rows = table.order(today: today)
        let fulls = table.meterFulls()
        let cells = Dictionary(uniqueKeysWithValues: rows.map { r in (r, columns.map { table.display(r, $0, fulls: fulls) }) })
        let totals = columns.map { table.total($0, today: today)?.text ?? "" }
        let widths = naturalWidths(columns, rows, cells, totals)
        let (drawn, fits) = fit(widths)
        let frozen = fits ? 0 : min(table.view.frozen, columns.count - 1)
        VStack(alignment: .leading, spacing: 0) {
            Color.clear
                .frame(maxWidth: .infinity)
                .frame(height: 0)
                .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { available = $0 }
            Group {
                if fits {
                    grid(Array(columns.indices), columns, drawn, rows, cells, totals, wraps: true)
                } else {
                    HStack(alignment: .top, spacing: 0) {
                        if frozen > 0 {
                            grid(Array(0..<frozen), columns, drawn, rows, cells, totals, wraps: false)
                                .overlay(alignment: .trailing) {
                                    Rectangle().fill(Theme.separator).frame(width: 1)
                                }
                        }
                        ScrollView(.horizontal) {
                            grid(Array(frozen..<columns.count), columns, drawn, rows, cells, totals, wraps: false)
                        }
                        .scrollIndicators(.hidden)
                    }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.separator, lineWidth: 1)
            }
            if rows.isEmpty && table.body.count > 0 {
                Text("No row passes the filter.")
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.text2)
                    .padding(.top, 6)
            }
        }
    }

    /// The columns `shown` (indices into `columns`) as one block of rows:
    /// the header, the rows, and the totals at the foot.
    private func grid(
        _ shown: [Int], _ columns: [Int], _ widths: [CGFloat], _ rows: [Int],
        _ cells: [Int: [CellDisplay]], _ totals: [String], wraps: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            line(header: true) {
                ForEach(shown, id: \.self) { i in
                    Text(table.heading(columns[i]))
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Theme.text2)
                        .lineLimit(1)
                        .frame(width: widths[i] - 2 * Self.padding, alignment: alignment(columns[i]))
                        .padding(.horizontal, Self.padding)
                }
            }
            ForEach(rows, id: \.self) { r in
                let painted = table.rowColor(r, today: today)
                RowLink(blockID: blockID, table: table, row: r) {
                    line(header: false, wraps: wraps) {
                        ForEach(shown, id: \.self) { i in
                            HStack(spacing: 4) {
                                if i == 0 && columns[i] == 0 {
                                    let depth = table.shownIndent(r)
                                    if depth > 0 {
                                        Color.clear.frame(width: CGFloat(depth) * 14)
                                    }
                                    if table.hasChildren(r) {
                                        IconView(table.info(r).closed ? "chevron-right" : "chevron-down", size: 12, weight: 2)
                                            .foregroundStyle(Theme.text2)
                                    }
                                }
                                CellView(display: cells[r]?[i] ?? .text(""), wraps: wraps)
                                    .fontWeight(columns[i] == 0 ? .medium : nil)
                            }
                            .frame(width: widths[i] - 2 * Self.padding, alignment: alignment(columns[i]))
                            .padding(.horizontal, Self.padding)
                        }
                    }
                    .background(painted.map { TagLook.row($0) } ?? .clear)
                }
            }
            if totals.contains(where: { !$0.isEmpty }) {
                line(header: true) {
                    ForEach(shown, id: \.self) { i in
                        Text(totals[i])
                            .font(.system(size: 12.5, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(Theme.text2)
                            .lineLimit(1)
                            .frame(width: widths[i] - 2 * Self.padding, alignment: alignment(columns[i]))
                            .padding(.horizontal, Self.padding)
                    }
                }
            }
        }
    }

    private func line(header: Bool, wraps: Bool = false, @ViewBuilder _ content: () -> some View) -> some View {
        HStack(alignment: .center, spacing: 0) {
            content()
        }
        .padding(.vertical, wraps ? 9 : 0)
        .frame(minHeight: header ? 34 : Self.lineHeight)
        .frame(height: wraps ? nil : (header ? 34 : Self.lineHeight))
        .fixedSize(horizontal: false, vertical: wraps)
        .background(header ? Theme.field.opacity(0.6) : .clear)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Theme.separator).frame(height: 1)
        }
    }

    private func alignment(_ c: Int) -> Alignment {
        table.kind(c).isAmount ? .trailing : .leading
    }

    /// The width each shown column needs for its longest content, on one line.
    private func naturalWidths(_ columns: [Int], _ rows: [Int], _ cells: [Int: [CellDisplay]], _ totals: [String]) -> [CGFloat] {
        let bold = UIFont.systemFont(ofSize: 13, weight: .semibold)
        let body = UIFont.systemFont(ofSize: Self.fontSize)
        let tag = UIFont.systemFont(ofSize: 13, weight: .medium)
        func width(_ s: String, _ font: UIFont) -> CGFloat {
            (s as NSString).size(withAttributes: [.font: font]).width
        }
        return columns.indices.map { i in
            var most = max(width(table.heading(columns[i]), bold), width(totals[i], tag))
            for r in rows {
                let need: CGFloat
                switch cells[r]?[i] ?? .text("") {
                case let .text(text): need = width(text, body)
                case let .amount(a): need = width(a.text, body) + (a.show == .ring ? 22 : 0)
                case .checkbox: need = 18
                case let .tags(tags, dots): need = tags.map { width($0.name, tag) + 14 + (dots ? 11 : 0) }.reduce(0) { $0 + $1 + 4 }
                case let .link(text, _): need = width(text, body)
                case let .pictures(names): need = CGFloat(min(names.count, 3)) * 30
                case let .rows(names): need = names.map { width($0, tag) + 14 }.reduce(0) { $0 + $1 + 4 }
                }
                most = max(most, need + (i == 0 ? CGFloat(table.shownIndent(r)) * 14 + (table.hasChildren(r) ? 16 : 0) : 0))
            }
            return min(max(most.rounded(.up) + 2 * Self.padding + 1, Self.narrowest), Self.widest)
        }
    }

    /// The widths to draw with, and whether the table fits the page. A
    /// table that is a little too wide gets narrower long-text columns (the
    /// text goes onto more lines) before it has to move sideways; a table
    /// that fits is spread over the whole width.
    private func fit(_ natural: [CGFloat]) -> ([CGFloat], Bool) {
        var widths = natural
        guard available > 0 else { return (widths, false) }
        var total = widths.reduce(0, +)
        if total > available {
            let comfortable: CGFloat = 132
            let spare = widths.map { max($0 - comfortable, 0) }
            let over = total - available
            let give = spare.reduce(0, +)
            if give >= over, give > 0 {
                widths = zip(widths, spare).map { $0 - $1 * over / give }
                total = available
            }
        }
        guard total <= available, total > 0 else { return (widths, false) }
        return (widths.map { ($0 * available / total).rounded(.down) }, true)
    }
}

// MARK: - Board, list, gallery, calendar

/// A row as a card: its name, then a few of its cells.
private struct RowCard: View {
    let blockID: ID
    let table: TableData
    let row: Int
    var leaveOut: Int?
    var picture = false

    var body: some View {
        RowLink(blockID: blockID, table: table, row: row) {
            VStack(alignment: .leading, spacing: 6) {
                if picture, let name = table.picture(row) {
                    PictureLoader(name: name) { image in
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFill()
                            .frame(height: 96)
                            .frame(maxWidth: .infinity)
                            .clipped()
                    } missing: {
                        Theme.field.frame(height: 96)
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                Text(table.nameOf(row))
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.text)
                    .lineLimit(2)
                ForEach(cardColumns, id: \.self) { c in
                    CellView(display: table.display(row, c), wraps: false, size: 13.5, leading: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(11)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.card, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.separator, lineWidth: 1)
            }
        }
    }

    /// Up to three filled columns besides the name (and the board's own).
    private var cardColumns: [Int] {
        Array(table.shownColumns.filter { c in
            c != 0 && c != leaveOut && {
                if case let .text(t) = table.display(row, c) { return !t.isEmpty }
                if case let .tags(tags, _) = table.display(row, c) { return !tags.isEmpty }
                return !table.cell(row, c).isEmpty
            }()
        }.prefix(3))
    }
}

private struct TableBoard: View {
    let blockID: ID
    let table: TableData
    let today: Day

    var body: some View {
        let rows = table.order(today: today)
        if let c = table.boardColumn {
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: 10) {
                    ForEach(Array(table.groups(rows, c).enumerated()), id: \.offset) { _, group in
                        VStack(alignment: .leading, spacing: 8) {
                            HStack(spacing: 6) {
                                let tag = table.display(group.rows.first ?? 0, c)
                                if case let .tags(tags, dots) = tag, !group.value.isEmpty, let first = tags.first {
                                    TagsView(tags: [first], dots: dots, wraps: false)
                                } else {
                                    Text(table.groupLabel(c, group.value))
                                        .font(.system(size: 13, weight: .semibold))
                                        .foregroundStyle(Theme.text2)
                                }
                                Text("\(group.rows.count)")
                                    .font(.system(size: 12.5))
                                    .foregroundStyle(Theme.text2)
                            }
                            ForEach(group.rows, id: \.self) { r in
                                RowCard(blockID: blockID, table: table, row: r, leaveOut: c)
                            }
                        }
                        .frame(width: 232, alignment: .top)
                    }
                }
            }
            .scrollIndicators(.hidden)
        } else {
            TableList(blockID: blockID, table: table, today: today)
        }
    }
}

private struct TableList: View {
    let blockID: ID
    let table: TableData
    let today: Day

    var body: some View {
        VStack(spacing: 0) {
            ForEach(table.order(today: today), id: \.self) { r in
                RowLink(blockID: blockID, table: table, row: r) {
                    HStack(spacing: 10) {
                        Text(table.nameOf(r))
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(Theme.text)
                            .lineLimit(1)
                            .padding(.leading, CGFloat(table.shownIndent(r)) * 14)
                        Spacer(minLength: 8)
                        Text(table.facts(r).joined(separator: "  ·  "))
                            .font(.system(size: 13))
                            .foregroundStyle(Theme.text2)
                            .lineLimit(1)
                    }
                    .frame(height: 42)
                    .overlay(alignment: .bottom) {
                        Rectangle().fill(Theme.separator).frame(height: 1)
                    }
                    .background(table.rowColor(r, today: today).map { TagLook.row($0) } ?? .clear)
                }
            }
        }
    }
}

private struct TableGallery: View {
    let blockID: ID
    let table: TableData
    let today: Day

    var body: some View {
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 10), GridItem(.flexible(), spacing: 10)], spacing: 10) {
            ForEach(table.order(today: today), id: \.self) { r in
                RowCard(blockID: blockID, table: table, row: r, picture: true)
            }
        }
    }
}

/// The rows on their days, the days in order (a month grid does not fit a
/// phone; the days with rows do).
private struct TableCalendar: View {
    let blockID: ID
    let table: TableData
    let today: Day

    var body: some View {
        let byDay = table.rowsByDay(table.order(today: today), today: today)
        VStack(alignment: .leading, spacing: 12) {
            if byDay.isEmpty {
                Text("No row has a date.")
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.text2)
            }
            ForEach(byDay.keys.sorted(by: >), id: \.self) { day in
                VStack(alignment: .leading, spacing: 6) {
                    Text(CellText.formatDay(day, today: today))
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(day == today ? Theme.now : Theme.text2)
                    ForEach(byDay[day] ?? [], id: \.self) { r in
                        RowCard(blockID: blockID, table: table, row: r)
                    }
                }
            }
        }
    }
}
