// Pages: the spaces, the favourites and the tree of pages inside pages.

import Notion2Core
import SwiftUI

struct PagesView: View {
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router
    /// Pages whose sub-pages show. Starts as on the Mac.
    @State private var expanded: Set<ID>?

    var body: some View {
        let workspace = model.workspace
        let space = model.spaceID
        let favorites = workspace.favorites(in: space)
        let rows = treeRows(workspace, space: space)
        let trashCount = workspace.trashed(in: space).count
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .center) {
                    Text("Pages").screenTitle()
                    Spacer(minLength: 0)
                    if model.canEdit {
                        Button {
                            Task {
                                if let id = await model.newPage() {
                                    router.openPage(id)
                                }
                            }
                        } label: {
                            IconView("plus", size: 18, weight: 2)
                                .foregroundStyle(Theme.text)
                                .frame(width: 22, height: 22)
                        }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.circle)
                        .accessibilityLabel("New page")
                    }
                }

                if workspace.spaces.count > 1 {
                    ScrollView(.horizontal) {
                        HStack(spacing: 8) {
                            ForEach(workspace.spaces) { other in
                                SpaceChip(space: other, selected: other.id == space) {
                                    model.spaceID = other.id
                                }
                            }
                        }
                        .padding(.horizontal, 20)
                    }
                    .scrollIndicators(.hidden)
                    .scrollClipDisabled()
                    .padding(.horizontal, -20)
                }

                if !favorites.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Favorites").sectionHeading()
                        PageList(pages: favorites)
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text(model.space?.displayName ?? "Pages").sectionHeading()
                    VStack(spacing: 0) {
                        ForEach(Array(rows.enumerated()), id: \.element.page.id) { index, row in
                            if index > 0 && row.depth == 0 {
                                RowSeparator()
                            }
                            TreeRow(row: row) { toggle(row.page.id) }
                        }
                        if rows.isEmpty {
                            Text("No pages")
                                .font(.system(size: 15))
                                .foregroundStyle(Theme.text2)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, 16)
                                .frame(height: 50)
                        }
                        if trashCount > 0 {
                            RowSeparator()
                            NavigationLink(value: Route.trash) {
                                HStack(spacing: 10) {
                                    IconView("trash", size: 20)
                                    Text("Trash")
                                        .font(.system(size: 15))
                                    Spacer(minLength: 0)
                                }
                                .foregroundStyle(Theme.text2)
                                .padding(.leading, 38)
                                .padding(.trailing, 16)
                                .frame(height: 50)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .card()
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 6)
            .padding(.bottom, 24)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { TabBar() }
        .background { AppBackground() }
        .toolbar(.hidden, for: .navigationBar)
    }

    private func isExpanded(_ id: ID) -> Bool {
        (expanded ?? initiallyExpanded).contains(id)
    }

    private func toggle(_ id: ID) {
        var set = expanded ?? initiallyExpanded
        if !set.insert(id).inserted {
            set.remove(id)
        }
        withAnimation(.snappy(duration: 0.22)) {
            expanded = set
        }
    }

    /// The pages that are open in the sidebar of the desktop app.
    private var initiallyExpanded: Set<ID> {
        guard let stored = model.workspace.settings["expanded"],
              let data = stored.data(using: .utf8),
              let ids = try? JSONDecoder().decode([String].self, from: data)
        else { return [] }
        return Set(ids.filter { !$0.contains(":") })
    }

    struct Row {
        let page: Page
        let depth: Int
        let hasChildren: Bool
        let expanded: Bool
    }

    /// The visible rows of the tree, top down.
    private func treeRows(_ workspace: Workspace, space: ID) -> [Row] {
        var rows: [Row] = []
        func add(_ page: Page, depth: Int) {
            let children = workspace.children(of: page.id)
            let open = !children.isEmpty && isExpanded(page.id)
            rows.append(Row(page: page, depth: depth, hasChildren: !children.isEmpty, expanded: open))
            if open && depth < 32 {
                for child in children {
                    add(child, depth: depth + 1)
                }
            }
        }
        for root in workspace.roots(in: space) {
            add(root, depth: 0)
        }
        return rows
    }
}

private struct TreeRow: View {
    let row: PagesView.Row
    let toggle: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            if row.hasChildren {
                Button(action: toggle) {
                    IconView("chevron-right", size: 16, weight: 1.9)
                        .foregroundStyle(Theme.text2)
                        .rotationEffect(.degrees(row.expanded ? 90 : 0))
                        .frame(width: 34, height: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(row.expanded ? "Collapse \(row.page.displayTitle)" : "Expand \(row.page.displayTitle)")
            } else {
                Color.clear.frame(width: 34, height: 44)
            }
            NavigationLink(value: Route.page(row.page.id)) {
                HStack(spacing: 10) {
                    PageIconView(icon: row.page.icon, size: row.depth == 0 ? 20 : 18)
                    Text(row.page.displayTitle)
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .frame(height: row.depth == 0 ? 50 : 46)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.leading, 4 + CGFloat(min(row.depth, 6)) * 20)
        .padding(.trailing, 16)
    }
}

private struct SpaceChip: View {
    let space: Space
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                SpaceBadge(space: space, size: 26)
                Text(space.displayName)
                    .font(.system(size: 14, weight: selected ? .semibold : .medium))
                    .lineLimit(1)
            }
            .foregroundStyle(selected ? Theme.onInk : Theme.text)
            .padding(.leading, 6)
            .padding(.trailing, 14)
            .frame(height: 38)
            .background(selected ? Theme.ink : Theme.card, in: Capsule())
            .overlay {
                if !selected {
                    Capsule().strokeBorder(Theme.edge, lineWidth: 1)
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// The pages in the trash of the current space. They can be read here; the
/// Mac restores or removes them.
struct TrashView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let pages = model.workspace.trashed(in: model.spaceID)
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Trash").screenTitle()
                if pages.isEmpty {
                    Text("The trash is empty.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text2)
                } else {
                    PageList(pages: pages)
                }
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background { AppBackground() }
        .navigationTitle("Trash")
        .hidesBarTitle()
    }
}
