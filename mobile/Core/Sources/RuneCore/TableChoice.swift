// The words a choice column (or a column of several choices, or a status
// column) offers, each with a colour: a list that belongs to the column,
// as Notion's options do. A cell still holds the words themselves. Mirrors
// `src/table/choice.rs` of the desktop app.

import Foundation

/// The colour of a tag, or of a row painted by a rule. Named `TagColor`
/// and not `Color` so that it does not clash with SwiftUI's.
public enum TagColor: String, CaseIterable, Hashable, Sendable, Codable {
    case `default`, gray, brown, orange, yellow, green, blue, purple, pink, red

    /// The colours new options take, one after the other.
    static let next: [TagColor] = [.blue, .green, .orange, .purple, .pink, .red, .yellow, .brown, .gray]

    /// The colour the option after `count` others takes. (Callers add the
    /// number of the column, so that columns do not all start alike.)
    public static func next(_ count: Int) -> TagColor {
        next[((count % next.count) + next.count) % next.count]
    }

    public var label: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }

    /// A colour by its name, in any case ("grey" too).
    public static func parse(_ s: String) -> TagColor? {
        let s = s.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return TagColor(rawValue: s) ?? (s == "grey" ? .gray : nil)
    }

    /// An unknown colour is the plain one (the Mac would refuse the table).
    public init(from decoder: Decoder) throws {
        let name = try decoder.singleValueContainer().decode(String.self)
        self = TagColor(rawValue: name) ?? .default
    }
}

/// One word a column offers.
public struct Choice: Hashable, Sendable, Codable {
    public var name: String
    public var color: TagColor

    public init(name: String, color: TagColor = .default) {
        self.name = name
        self.color = color
    }

    private enum CodingKeys: String, CodingKey { case name, color }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        color = c.lenient(TagColor.self, .color) ?? .default
    }

    /// The three words a status column starts with.
    static var statusOptions: [Choice] {
        [Choice(name: "Not started", color: .gray), Choice(name: "In progress", color: .blue), Choice(name: "Done", color: .green)]
    }

    /// The words in a cell: for several choices those between the commas,
    /// else the whole cell.
    public static func names(in cell: String, kind: ColumnKind) -> [String] {
        switch kind {
        case .multi:
            return cell.split(separator: ",", omittingEmptySubsequences: false)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        default:
            let one = cell.trimmingCharacters(in: .whitespacesAndNewlines)
            return one.isEmpty ? [] : [one]
        }
    }
}

extension TableColumn {
    /// The option with this name (any case).
    public func option(_ name: String) -> Choice? {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return options.first { Money.asciiEqual($0.name, name) }
    }

    /// The colour of a word of this column.
    public func color(of name: String) -> TagColor {
        option(name)?.color ?? .default
    }

    public func names(in cell: String) -> [String] {
        Choice.names(in: cell, kind: kind)
    }
}

extension TableData {
    /// Every word in a cell of a column with options is one of them. A
    /// word that is not yet takes the next colour. (The Mac does this each
    /// time it reads a table, so the colours here are the ones it shows.)
    mutating func syncOptions() {
        for c in columnInfo.indices {
            let kind = columnInfo[c].kind
            guard kind.hasOptions else { continue }
            var col = columnInfo[c]
            if kind == .status, col.options.isEmpty {
                col.options = Choice.statusOptions
            }
            for r in rows.indices.dropFirst() {
                for name in Choice.names(in: rows[r][c], kind: kind) where col.option(name) == nil {
                    col.options.append(Choice(name: name, color: .next(col.options.count + c)))
                }
            }
            if col != columnInfo[c] { columnInfo[c] = col }
        }
    }

    /// Where an option stands in its column's list (rows sort by it).
    func optionPlace(_ c: Int, _ name: String) -> Int? {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return column(c).options.firstIndex { Money.asciiEqual($0.name, name) }
    }
}
