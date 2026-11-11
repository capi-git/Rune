// The open to-dos from all pages of the space, as on the Mac's Home and its
// "To do" list. The iPhone cannot tick a to-do yet: a row opens its page.

import Notion2Core
import SwiftUI

/// A card of to-dos. Each row opens the page the to-do is on.
struct TodoList: View {
    /// How many to-dos Home shows before "All" (as on the Mac).
    static let shownOnHome = 7

    let todos: [OpenTodo]
    /// Show the page's name under each to-do.
    let showsPage: Bool

    @Environment(AppModel.self) private var model
    /// Ticked here a moment ago: shown done before the row goes.
    @State private var ticked: Set<ID> = []

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(todos.enumerated()), id: \.element.id) { index, todo in
                if index > 0 {
                    RowSeparator()
                }
                NavigationLink(value: Route.page(todo.pageID)) {
                    HStack(alignment: .top, spacing: 12) {
                        Group {
                            if ticked.contains(todo.blockID) {
                                RoundedRectangle(cornerRadius: 6, style: .continuous)
                                    .fill(Theme.check)
                                    .overlay { IconView("check", size: 14, weight: 2.6).foregroundStyle(Theme.checkMark) }
                            } else {
                                RoundedRectangle(cornerRadius: 6, style: .continuous)
                                    .strokeBorder(Theme.checkEmpty, lineWidth: 1.75)
                            }
                        }
                        .frame(width: 20, height: 20)
                        .padding(.top, 1)
                        .contentShape(Rectangle().inset(by: -10))
                        .onTapGesture { tick(todo) }
                        .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(todo.text)
                                .font(.system(size: 15))
                                .foregroundStyle(Theme.text)
                                .lineLimit(3)
                                .multilineTextAlignment(.leading)
                            if showsPage {
                                Text(todo.pageTitle)
                                    .font(.system(size: 12.5))
                                    .foregroundStyle(Theme.text2)
                                    .lineLimit(1)
                            }
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(showsPage ? "To-do: \(todo.text), on \(todo.pageTitle)" : "To-do: \(todo.text)")
                .accessibilityHint("Opens the page")
            }
        }
        .card()
    }

    /// Tick a to-do from the list: it shows done, then leaves the list.
    private func tick(_ todo: OpenTodo) {
        guard model.canEdit, !ticked.contains(todo.blockID) else { return }
        withAnimation(.snappy(duration: 0.15)) { _ = ticked.insert(todo.blockID) }
        Task {
            try? await Task.sleep(for: .milliseconds(450))
            await model.setChecked(true, block: todo.blockID, page: todo.pageID)
            ticked.remove(todo.blockID)
        }
    }
}

/// Every open to-do of the space, grouped by the page it is on (the pages
/// edited last first).
struct TodosView: View {
    @Environment(AppModel.self) private var model

    private struct PageGroup: Identifiable {
        let id: ID
        let title: String
        var todos: [OpenTodo]
    }

    private func groups(_ todos: [OpenTodo]) -> [PageGroup] {
        var out: [PageGroup] = []
        for todo in todos {
            if let last = out.indices.last, out[last].id == todo.pageID {
                out[last].todos.append(todo)
            } else {
                out.append(PageGroup(id: todo.pageID, title: todo.pageTitle, todos: [todo]))
            }
        }
        return out
    }

    var body: some View {
        let todos = model.workspace.openTodos(in: model.spaceID)
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("To do").screenTitle()
                    Text(todos.count == 1 ? "1 open to-do, from all your pages" : "\(todos.count) open to-dos, from all your pages")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Theme.text2)
                }
                if todos.isEmpty {
                    Text("Nothing to do.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text2)
                }
                ForEach(groups(todos)) { group in
                    VStack(alignment: .leading, spacing: 8) {
                        NavigationLink(value: Route.page(group.id)) {
                            HStack(spacing: 8) {
                                PageIconView(icon: model.workspace.pages[group.id]?.icon, size: 16)
                                Text(group.title)
                                    .font(.system(size: 13, weight: .semibold))
                                    .foregroundStyle(Theme.text2)
                                    .lineLimit(1)
                            }
                            .padding(.leading, 4)
                            .frame(minHeight: 24)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(.isHeader)
                        TodoList(todos: group.todos, showsPage: false)
                    }
                }
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background { AppBackground() }
        .navigationTitle("To do")
        .hidesBarTitle()
    }
}
