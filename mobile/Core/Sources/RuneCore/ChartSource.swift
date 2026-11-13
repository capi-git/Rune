// Which table a chart draws, and the short line beside its title. Mirrors
// `AppState::chart_source` (src/state/tables.rs) and `note` (src/ui/charts.rs)
// of the desktop app.

import Foundation

public struct ChartSource: Sendable {
    /// The page the table is on.
    public let pageID: ID
    /// That page's title, shown as "from <title>" when it is another page.
    public let title: String
    /// The table is on another page than the chart.
    public let elsewhere: Bool
    public let table: TableData
}

extension Workspace {
    /// The table the chart `blockID` on page `pageID` draws: the table
    /// above it (else the first table of its page), or, when `from` names
    /// another page by its id or its title, that page's first table.
    public func chartSource(page pageID: ID, block blockID: ID, from: String) -> ChartSource? {
        let from = from.trimmingCharacters(in: .whitespacesAndNewlines)
        let target: ID
        if from.isEmpty || from == pageID {
            target = pageID
        } else if pages[from] != nil {
            target = from
        } else {
            let space = pages[pageID]?.spaceID
            let want = from.lowercased()
            let found = pages.values
                .filter { !$0.trashed && $0.title.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == want }
                .sorted { a, b in
                    // Its own space first, then the page changed last.
                    let (ia, ib) = (a.spaceID == space, b.spaceID == space)
                    return ia != ib ? ia : a.updatedAt > b.updatedAt
                }
            guard let first = found.first else { return nil }
            target = first.id
        }
        guard let lines = blocks[target] else { return nil }
        let tableBlock: Block?
        if target == pageID {
            let at = lines.firstIndex { $0.id == blockID } ?? lines.count
            tableBlock = lines[..<at].last { $0.kind == .table } ?? lines.first { $0.kind == .table }
        } else {
            tableBlock = lines.first { $0.kind == .table }
        }
        guard let tableBlock, let table = TableData.parse(tableBlock.data) else { return nil }
        return ChartSource(pageID: target, title: pages[target]?.displayTitle ?? "", elsewhere: target != pageID, table: table)
    }
}

extension Chart {
    /// The few words beside a chart's title: the goal of a ring, how many
    /// days of four weeks have a row, what the bars come to.
    public func note(_ table: TableData, today: Day = When().today()) -> String {
        switch shape {
        case .ring:
            return goal.map { "Goal \(format(table, $0))" } ?? ""
        case .days:
            let n = days(table, today: today).filter(\.hasRow).count
            return "\(n) \(n == 1 ? "day" : "days") in 4 weeks"
        case .pie:
            return ""
        case .bars, .line:
            let points = points(table, today: today)
            if running {
                return points.last.map { "\(format(table, $0.value)) now" } ?? ""
            }
            guard add == .sum || add == .count, !points.isEmpty else { return "" }
            return "\(format(table, points.map(\.value).reduce(0, +))) in all"
        }
    }
}
