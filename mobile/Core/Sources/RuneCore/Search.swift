// Search over page titles, the text inside pages, and events. The same
// ranking as the desktop quick find (`src/ui/modals.rs`): titles that start
// with the query, then titles that contain it, then text inside pages.

import Foundation

public struct SearchHit: Identifiable, Hashable, Sendable {
    public enum Target: Hashable, Sendable {
        case page(ID)
        case event(ID)
    }

    public var target: Target
    public var title: String
    /// Where the page is ("Projects / Mobile app"), or the space's name.
    public var path: String
    /// The text around the match, for a match inside a page.
    public var snippet: String?
    /// The page's stored icon.
    public var icon: String?
    /// For an event: its start.
    public var start: Int64?

    public var id: String {
        switch target {
        case let .page(id): "page:\(id)"
        case let .event(id): "event:\(id)"
        }
    }
}

public struct SearchResults: Hashable, Sendable {
    /// Pages whose title matches.
    public var names: [SearchHit] = []
    /// Pages with the words inside them (and not in the title).
    public var inside: [SearchHit] = []
    public var events: [SearchHit] = []

    public var isEmpty: Bool {
        names.isEmpty && inside.isEmpty && events.isEmpty
    }

    public init() {}
}

/// The text of a workspace, made ready once so that each search is quick.
public struct SearchIndex: Sendable {
    struct Entry: Sendable {
        var page: Page
        var path: String
        var texts: [String]
    }

    var entries: [Entry]
    var events: [Event]
    var spaceNames: [ID: String]

    public static let empty = SearchIndex(Workspace.empty)

    public init(_ workspace: Workspace) {
        entries = workspace.pages.values
            .filter { !$0.trashed }
            .map { page in
                let texts = (workspace.blocks[page.id] ?? [])
                    .map(\.searchText)
                    .filter { !$0.isEmpty }
                return Entry(page: page, path: workspace.path(of: page.id), texts: texts)
            }
        events = workspace.events
        spaceNames = Dictionary(uniqueKeysWithValues: workspace.spaces.map { ($0.id, $0.displayName) })
    }

    static let options: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]

    /// Where `query` is found in `text`, ignoring case and accents.
    public static func ranges(of query: String, in text: String) -> [Range<String.Index>] {
        guard !query.isEmpty else { return [] }
        var out: [Range<String.Index>] = []
        var from = text.startIndex
        while from < text.endIndex, let r = text.range(of: query, options: options, range: from..<text.endIndex) {
            out.append(r)
            from = r.isEmpty ? text.index(after: r.lowerBound) : r.upperBound
        }
        return out
    }

    /// The text around the first match: about 30 characters before it and 50
    /// after, on one line.
    static func snippet(around match: Range<String.Index>, in text: String) -> String {
        let start = text.index(match.lowerBound, offsetBy: -30, limitedBy: text.startIndex) ?? text.startIndex
        let end = text.index(match.upperBound, offsetBy: 50, limitedBy: text.endIndex) ?? text.endIndex
        var s = String(text[start..<end]).replacingOccurrences(of: "\n", with: " ")
        if start > text.startIndex {
            s = "…" + s
        }
        if end < text.endIndex {
            s += "…"
        }
        return s
    }

    /// Search in one space (`space`), or in all of them (nil).
    public func search(_ query: String, space: ID? = nil, limit: Int = 30) -> SearchResults {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        var results = SearchResults()
        guard !q.isEmpty else { return results }

        var names: [(rank: Int, hit: SearchHit)] = []
        var inside: [SearchHit] = []
        for entry in entries where space == nil || entry.page.spaceID == space {
            let title = entry.page.displayTitle
            let path = entry.path.isEmpty ? (spaceNames[entry.page.spaceID] ?? "") : entry.path
            if let found = title.range(of: q, options: SearchIndex.options) {
                let rank = found.lowerBound == title.startIndex ? 0 : 1
                names.append((rank, SearchHit(target: .page(entry.page.id), title: title, path: path, snippet: nil, icon: entry.page.icon)))
                continue
            }
            for text in entry.texts {
                if let found = text.range(of: q, options: SearchIndex.options) {
                    inside.append(SearchHit(
                        target: .page(entry.page.id),
                        title: title,
                        path: path,
                        snippet: SearchIndex.snippet(around: found, in: text),
                        icon: entry.page.icon
                    ))
                    break
                }
            }
        }
        let byTitle: (SearchHit, SearchHit) -> Bool = {
            let order = $0.title.localizedCaseInsensitiveCompare($1.title)
            return order == .orderedSame ? $0.id < $1.id : order == .orderedAscending
        }
        results.names = names
            .sorted { $0.rank != $1.rank ? $0.rank < $1.rank : byTitle($0.hit, $1.hit) }
            .prefix(limit)
            .map(\.hit)
        results.inside = Array(inside.sorted(by: byTitle).prefix(limit))

        results.events = events
            .filter { space == nil || $0.spaceID == space }
            .compactMap { e -> SearchHit? in
                let title = e.displayTitle
                if title.range(of: q, options: SearchIndex.options) != nil {
                    return SearchHit(target: .event(e.id), title: title, path: "", snippet: nil, icon: nil, start: e.start)
                }
                if let found = e.notes.range(of: q, options: SearchIndex.options) {
                    return SearchHit(
                        target: .event(e.id),
                        title: title,
                        path: "",
                        snippet: SearchIndex.snippet(around: found, in: e.notes),
                        icon: nil,
                        start: e.start
                    )
                }
                return nil
            }
            .sorted { ($0.start ?? 0) > ($1.start ?? 0) }
            .prefix(limit)
            .map { $0 }
        return results
    }
}
