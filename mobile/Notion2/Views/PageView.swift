// A page: its icon and title, then its blocks. Notes that came from sync
// can be changed here: a tap on a line types in it (Return makes the next
// line, as on the Mac), a tap on the title renames the page, and a tap
// below the last line writes a new one.

import Notion2Core
import SwiftUI

struct PageView: View {
    let pageID: ID
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router
    /// Toggles opened or closed here (the file is not changed).
    @State private var flipped: Set<ID> = []
    /// The line being typed in, and what it holds now.
    @State private var editing: ID?
    @State private var draft = ""
    /// The draft is the line's plain words (it has no formatting): it is
    /// saved with the marks the Mac reads escaped. Else it is the line's
    /// inline Markdown as it is stored.
    @State private var draftIsPlain = true
    @State private var editingTitle = false
    /// Typing moves to a line that is not drawn yet: the field that loses
    /// the keyboard meanwhile is not left (nothing is saved for it).
    @State private var moving = false
    @State private var titleDraft = ""
    @FocusState private var focus: Focus?

    private enum Focus: Hashable {
        case line(ID)
        case title
    }

    var body: some View {
        Group {
            if let page = model.workspace.pages[pageID] {
                content(page)
            } else {
                Text("This page is not in this copy of your notes.")
                    .font(.system(size: 15))
                    .foregroundStyle(Theme.text2)
                    .padding(24)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background {
            ZStack {
                AppBackground()
                Theme.paper.ignoresSafeArea()
            }
        }
        .hidesBarTitle()
    }

    /// The height of a page's cover, from the top of the screen.
    private static let coverHeight: CGFloat = 210

    private struct Row: Identifiable {
        let block: Block
        let number: Int?
        let open: Bool
        let spacing: CGFloat
        var id: ID { block.id }
    }

    private func rows(_ blocks: [Block]) -> [Row] {
        // Empty lines at the end of a page are only room to type in.
        var blocks = blocks
        while let last = blocks.last, last.kind == .text, last.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, last.id != editing {
            blocks.removeLast()
        }
        func isClosed(_ b: Block) -> Bool { b.collapsed != flipped.contains(b.id) }
        let visible = Outline.visibility(blocks, collapsed: isClosed)
        let numbers = Outline.listNumbers(blocks)
        var out: [Row] = []
        var previous: BlockKind?
        for (i, block) in blocks.enumerated() where visible[i] {
            out.append(Row(
                block: block,
                number: numbers[i],
                open: !isClosed(block),
                spacing: BlockView.spacing(before: block.kind, after: previous)
            ))
            previous = block.kind
        }
        return out
    }

    private func content(_ page: Page) -> some View {
        let workspace = model.workspace
        let blocks = workspace.blocks[page.id] ?? []
        let rows = rows(blocks)
        let path = workspace.path(of: page.id)
        let linked = Set(blocks.compactMap { $0.kind == .pageLink ? $0.target : nil })
        let unlinked = workspace.children(of: page.id).filter { !linked.contains($0.id) }
        let cover = Cover(stored: page.cover)
        return ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if let cover {
                    // The band runs to the edges and up under the top bar.
                    CoverBand(cover: cover, positionY: page.coverY)
                        .frame(height: PageView.coverHeight)
                        .padding(.horizontal, -24)
                        .padding(.bottom, 16)
                }
                VStack(alignment: .leading, spacing: 10) {
                    if !path.isEmpty {
                        Text(path)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(Theme.text2)
                            .lineLimit(1)
                            .truncationMode(.head)
                    }
                    HStack(alignment: .center, spacing: 12) {
                        if !(page.icon ?? "").isEmpty {
                            PageIconView(icon: page.icon, size: 46, tile: true)
                        }
                        if editingTitle {
                            TextField("Untitled", text: $titleDraft, axis: .vertical)
                                .font(.system(size: 28, weight: .bold, design: PageFace.design(page.font)))
                                .tracking(-0.5)
                                .foregroundStyle(Theme.text)
                                .focused($focus, equals: .title)
                                .onChange(of: titleDraft) { _, new in
                                    // Return ends the title.
                                    if new.contains("\n") {
                                        titleDraft = new.replacingOccurrences(of: "\n", with: "")
                                        focus = nil
                                    }
                                }
                        } else {
                            Text(page.displayTitle)
                                .font(.system(size: 28, weight: .bold, design: PageFace.design(page.font)))
                                .tracking(-0.5)
                                .foregroundStyle(Theme.text)
                                .accessibilityAddTraits(.isHeader)
                                .onTapGesture {
                                    guard model.canEdit else { return }
                                    commitLine()
                                    titleDraft = page.title
                                    editingTitle = true
                                    focus = .title
                                }
                        }
                    }
                    if page.trashed {
                        Text("This page is in the trash.")
                            .font(.system(size: 13))
                            .foregroundStyle(Theme.text2)
                    }
                    if let rowOf = page.rowOf {
                        RowFacts(parentID: page.parentID, rowOf: rowOf)
                    }
                }
                .padding(.bottom, 6)

                ForEach(rows) { row in
                    Group {
                        if editing == row.block.id {
                            LineEditor(block: row.block, number: row.number, face: PageFace.design(page.font), draft: $draft) {
                                // Return from a hardware keyboard ends the field
                                // ("submit"): it makes the next line too.
                                moving = true
                                draft += "\n"
                            }
                            .focused($focus, equals: .line(row.block.id))
                        } else {
                            BlockView(pageID: pageID, block: row.block, number: row.number, open: row.open) {
                                withAnimation(.snappy(duration: 0.2)) {
                                    if !flipped.insert(row.block.id).inserted {
                                        flipped.remove(row.block.id)
                                    }
                                }
                            }
                            .contentShape(Rectangle())
                            .onTapGesture {
                                if model.canEdit, PageView.typesIn(row.block.kind), !row.block.text.contains("\n") {
                                    begin(row.block)
                                }
                            }
                        }
                    }
                    .padding(.top, row.spacing)
                }

                if model.canEdit && editing == nil && !editingTitle {
                    // Room below the last line: a tap writes a new one.
                    Color.clear
                        .frame(maxWidth: .infinity)
                        .frame(height: 120)
                        .contentShape(Rectangle())
                        .onTapGesture { appendLine(blocks) }
                        .accessibilityLabel("Write a new line")
                        .accessibilityAddTraits(.isButton)
                }

                if !unlinked.isEmpty {
                    Text("Pages inside")
                        .sectionHeading()
                        .padding(.top, 28)
                        .padding(.bottom, 8)
                    PageList(pages: unlinked)
                }
            }
            .padding(.horizontal, 24)
            .padding(.top, cover == nil ? 4 : 0)
            .padding(.bottom, 48)
            .textSelection(.enabled)
        }
        .scrollDismissesKeyboard(.interactively)
        .onChange(of: draft) { _, new in splitAtReturn(new) }
        .onChange(of: focus) { old, new in
            // Leaving a line or the title saves it.
            if moving {
                return
            }
            if case .line = old, new != old {
                commitLine()
            }
            if old == .title, new != .title {
                commitTitle()
            }
        }
        .onDisappear {
            commitLine()
            commitTitle()
        }
        .onAppear {
            // A new page: its name first.
            if model.canEdit, page.title.isEmpty, blocks.allSatisfy({ $0.text.isEmpty }) {
                titleDraft = ""
                editingTitle = true
                focus = .title
            }
        }
        .ignoresSafeArea(.container, edges: cover == nil ? [] : .top)
        .navigationTitle(page.displayTitle)
        .opensLinks(with: router, in: workspace)
    }

    // MARK: Changing the page

    /// The kinds of line a tap types in. A code block keeps its own lines;
    /// a picture, a table or a drawing is not text.
    static func typesIn(_ kind: BlockKind) -> Bool {
        kind.hasText && kind != .code
    }

    private var page: Page? { model.workspace.pages[pageID] }
    private var blocks: [Block] { model.workspace.blocks[pageID] ?? [] }

    private func begin(_ block: Block) {
        // (A Return that did not make a line must not hold the next one.)
        moving = false
        commitLine()
        let plain = Inline.plain(block.text)
        draftIsPlain = Inline.escape(plain) == block.text
        draft = draftIsPlain ? plain : block.text
        editing = block.id
        focus = .line(block.id)
    }

    /// The stored text of what is typed.
    private func stored(_ typed: String) -> String {
        draftIsPlain ? Inline.escape(typed) : typed
    }

    /// Save the line being typed in. A line left empty goes (not the only
    /// line of the page).
    private func commitLine() {
        guard let id = editing, let page else { return }
        editing = nil
        var all = blocks
        guard let i = all.firstIndex(where: { $0.id == id }) else { return }
        let text = stored(draft.trimmingCharacters(in: .newlines))
        if text.trimmingCharacters(in: .whitespaces).isEmpty && all.count > 1 && all[i].kind != .heading1 {
            all.remove(at: i)
        } else if all[i].text != text {
            all[i].text = text
        } else {
            return
        }
        let blocks = all
        Task { await model.save(page: page, blocks: blocks) }
    }

    private func commitTitle() {
        guard editingTitle, let page else { return }
        editingTitle = false
        let title = titleDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard title != page.title else { return }
        var changed = page
        changed.title = title
        let blocks = self.blocks
        Task { await model.save(page: changed, blocks: blocks) }
    }

    /// Return was typed: the words after it go to a new line below, of the
    /// same kind for a list (as on the Mac). Return in an empty list item
    /// makes it plain text.
    private func splitAtReturn(_ typed: String) {
        guard let id = editing, let page, let newline = typed.firstIndex(of: "\n") else { return }
        var all = blocks
        guard let i = all.firstIndex(where: { $0.id == id }) else { return }
        let before = String(typed[..<newline])
        let after = String(typed[typed.index(after: newline)...])
        let lists: Set<BlockKind> = [.bulleted, .numbered, .todo]
        if before.isEmpty && after.isEmpty && lists.contains(all[i].kind) {
            all[i].kind = .text
            all[i].indent = 0
            draft = ""
            let blocks = all
            Task { await model.save(page: page, blocks: blocks) }
            return
        }
        all[i].text = stored(before)
        let kind: BlockKind = lists.contains(all[i].kind) ? all[i].kind : .text
        let next = Block(id: SyncFormat.newRecordID(), kind: kind, text: stored(after), indent: lists.contains(kind) ? all[i].indent : 0)
        all.insert(next, at: i + 1)
        let blocks = all
        moving = true
        editing = next.id
        draft = after
        Task {
            await model.save(page: page, blocks: blocks)
            focus = .line(next.id)
            moving = false
        }
    }

    /// A new empty line at the end of the page, to type in.
    private func appendLine(_ current: [Block]) {
        guard let page else { return }
        commitLine()
        var all = current
        // An empty line already at the end is the one to type in.
        if let last = all.last, last.kind == .text, last.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            begin(last)
            return
        }
        let line = Block(id: SyncFormat.newRecordID(), kind: .text)
        all.append(line)
        let blocks = all
        draftIsPlain = true
        draft = ""
        moving = true
        editing = line.id
        Task {
            await model.save(page: page, blocks: blocks)
            focus = .line(line.id)
            moving = false
        }
    }
}

/// The cells of the row a page belongs to, above its text: one quiet line
/// for each column, its heading in grey and then what the cell shows (as
/// the Mac shows a row opened at the side).
private struct RowFacts: View {
    let parentID: ID?
    let rowOf: String

    @Environment(AppModel.self) private var model

    var body: some View {
        let parts = rowOf.split(separator: "/", maxSplits: 1).map(String.init)
        if parts.count == 2,
           let parentID,
           let block = model.workspace.blocks[parentID]?.first(where: { $0.id == parts[0] && $0.kind == .table }),
           let table = TableData.parse(block.data),
           let r = table.rowWithID(parts[1]) {
            let fulls = table.meterFulls()
            VStack(alignment: .leading, spacing: 0) {
                ForEach(table.shownColumns.filter { $0 != 0 }, id: \.self) { c in
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(table.heading(c))
                            .font(.system(size: 14))
                            .foregroundStyle(Theme.text2)
                            .lineLimit(1)
                            .frame(width: 112, alignment: .leading)
                        CellView(display: table.display(r, c, fulls: fulls), size: 14.5, leading: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(minHeight: 34)
                }
            }
            .padding(.top, 4)
            .padding(.bottom, 8)
            .overlay(alignment: .bottom) {
                Rectangle().fill(Theme.separator).frame(height: 1)
            }
        }
    }
}

/// The line being typed in: the same size and mark as the line it is, so
/// that typing does not move the page.
private struct LineEditor: View {
    let block: Block
    let number: Int?
    let face: Font.Design
    @Binding var draft: String
    var onReturn: () -> Void = {}

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            switch block.kind {
            case .bulleted:
                Text("\u{2022}").font(.system(size: 16)).foregroundStyle(Theme.text)
            case .numbered:
                Text("\(number ?? 1).").font(.system(size: 16)).monospacedDigit().foregroundStyle(Theme.text)
            case .todo:
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .strokeBorder(Theme.checkEmpty, lineWidth: 1.75)
                    .frame(width: 20, height: 20)
                    .alignmentGuide(.firstTextBaseline) { $0[.bottom] - 4 }
            case .quote:
                RoundedRectangle(cornerRadius: 1.5).fill(Theme.text).frame(width: 3, height: 20)
            default:
                EmptyView()
            }
            TextField(placeholder, text: $draft, axis: .vertical)
                .onSubmit(onReturn)
                .font(font)
                .foregroundStyle(Theme.text)
                .tint(Theme.accent)
        }
        .padding(.leading, CGFloat(block.indent) * 26)
    }

    private var placeholder: String {
        switch block.kind {
        case .heading1, .heading2, .heading3: "Heading"
        case .todo: "To-do"
        default: "Write something"
        }
    }

    private var font: Font {
        switch block.kind {
        case .heading1: .system(size: 25, weight: .bold, design: face)
        case .heading2: .system(size: 20, weight: .semibold, design: face)
        case .heading3: .system(size: 17, weight: .semibold, design: face)
        default: .system(size: 16)
        }
    }
}
