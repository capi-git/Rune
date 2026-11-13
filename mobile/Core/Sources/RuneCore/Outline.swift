// The shape of a page (which blocks a toggle hides, the numbers of a numbered
// list) and of the page tree. Mirrors the helpers in `src/model.rs` and
// `src/state.rs` of the desktop app.

import Foundation

public enum Outline {
    /// Exclusive end of the block at `index` plus all of its nested children.
    public static func subtreeEnd(_ blocks: [Block], _ index: Int) -> Int {
        let level = blocks[index].indent
        var end = index + 1
        while end < blocks.count && blocks[end].indent > level {
            end += 1
        }
        return end
    }

    /// Whether the block at `index` has nested children.
    public static func hasChildren(_ blocks: [Block], _ index: Int) -> Bool {
        subtreeEnd(blocks, index) > index + 1
    }

    /// Which blocks are visible (not hidden inside a collapsed toggle).
    /// `collapsed` says whether a toggle is closed; by default, as stored.
    public static func visibility(_ blocks: [Block], collapsed: (Block) -> Bool = { $0.collapsed }) -> [Bool] {
        var out: [Bool] = []
        out.reserveCapacity(blocks.count)
        var hiddenBelow: Int?
        for b in blocks {
            if let level = hiddenBelow {
                if b.indent > level {
                    out.append(false)
                    continue
                }
                hiddenBelow = nil
            }
            out.append(true)
            if b.kind == .toggle && collapsed(b) {
                hiddenBelow = b.indent
            }
        }
        return out
    }

    /// The number shown on each numbered-list block (nil for other blocks).
    public static func listNumbers(_ blocks: [Block]) -> [Int?] {
        var counters = [Int](repeating: 0, count: maxIndent + 1)
        return blocks.map { b in
            let level = min(max(b.indent, 0), maxIndent)
            for i in counters.indices where i > level {
                counters[i] = 0
            }
            if b.kind == .numbered {
                counters[level] += 1
                return counters[level]
            }
            counters[level] = 0
            return nil
        }
    }
}

/// A to-do that is not done yet, with the page it is written on.
public struct OpenTodo: Identifiable, Hashable, Sendable {
    public var pageID: ID
    public var pageTitle: String
    public var blockID: ID
    /// The to-do's text, without formatting marks.
    public var text: String

    public var id: ID { blockID }
}

extension Workspace {
    /// Every to-do not yet done in a space: from the pages edited last first,
    /// and in page order inside a page. Pages in the trash and to-dos with no
    /// text are left out. The same list as the desktop app's
    /// (`AppState::open_todos` in `src/state.rs`).
    public func openTodos(in space: ID) -> [OpenTodo] {
        pages.values
            .filter { $0.spaceID == space && !$0.trashed }
            .sorted { ($0.updatedAt, $0.id) > ($1.updatedAt, $1.id) }
            .flatMap { page in
                (blocks[page.id] ?? [])
                    .filter { $0.kind == .todo && !$0.checked && !$0.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                    .map { OpenTodo(pageID: page.id, pageTitle: page.displayTitle, blockID: $0.id, text: $0.plainText) }
            }
    }

    private static func inOrder(_ a: Page, _ b: Page) -> Bool {
        if a.position != b.position {
            return a.position < b.position
        }
        if a.createdAt != b.createdAt {
            return a.createdAt < b.createdAt
        }
        return a.id < b.id
    }

    /// Pages under `parent` (or at the top level), not trashed, in order.
    /// The page of a table's row is not one of them: it opens from its row
    /// (as in the desktop's sidebar).
    public func children(of parent: ID?) -> [Page] {
        pages.values.filter { !$0.trashed && !$0.isRowPage && $0.parentID == parent }.sorted(by: Workspace.inOrder)
    }

    /// The page of a row of a table (`rowID` from the table's `info`), if
    /// one was made.
    public func rowPage(table blockID: ID, row rowID: String) -> Page? {
        guard !rowID.isEmpty else { return nil }
        let key = "\(blockID)/\(rowID)"
        return pages.values.first { !$0.trashed && $0.rowOf == key }
    }

    /// Top-level pages of a space, not trashed, in order.
    public func roots(in space: ID) -> [Page] {
        children(of: nil).filter { $0.spaceID == space }
    }

    /// Ancestors from the top level down to (not including) the page.
    public func ancestors(of id: ID) -> [Page] {
        var out: [Page] = []
        var current = pages[id]?.parentID
        while let pid = current, let p = pages[pid], out.count < 32 {
            current = p.parentID
            out.append(p)
        }
        return out.reversed()
    }

    /// The titles of a page's ancestors, "Projects / Mobile app".
    public func path(of id: ID) -> String {
        ancestors(of: id).map(\.displayTitle).joined(separator: " / ")
    }

    public func favorites(in space: ID) -> [Page] {
        pages.values.filter { $0.favorite && !$0.trashed && $0.spaceID == space }.sorted(by: Workspace.inOrder)
    }

    /// Pages in the trash, most recently changed first.
    public func trashed(in space: ID) -> [Page] {
        pages.values.filter { $0.trashed && $0.spaceID == space }.sorted { $0.updatedAt > $1.updatedAt }
    }

    /// Pages of a space, most recently changed first.
    public func recent(in space: ID, limit: Int) -> [Page] {
        Array(
            pages.values
                .filter { !$0.trashed && !$0.isRowPage && $0.spaceID == space }
                .sorted { ($0.updatedAt, $0.id) > ($1.updatedAt, $1.id) }
                .prefix(limit)
        )
    }

    public func events(in space: ID) -> [Event] {
        events.filter { $0.spaceID == space }
    }

    /// The stored event an id (of an event or of one repeat) belongs to.
    public func storedEvent(_ id: ID) -> Event? {
        let series = Repeat.splitID(id).series
        return events.first { $0.id == series }
    }
}
