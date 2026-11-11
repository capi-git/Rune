// Search: page names, the text inside pages, and events. The field is in
// the bar at the bottom (see `SearchBar`).

import Notion2Core
import SwiftUI

struct SearchView: View {
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router
    @State private var results = SearchResults()
    /// The query the results are for.
    @State private var answered = ""
    /// Search every space, not only the current one.
    @State private var allSpaces = true

    var body: some View {
        let query = router.query.trimmingCharacters(in: .whitespacesAndNewlines)
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Search").screenTitle()

                if model.workspace.spaces.count > 1 {
                    HStack(spacing: 8) {
                        ScopeChip(title: "All spaces", selected: allSpaces) { allSpaces = true }
                        ScopeChip(title: model.space?.displayName ?? "This space", selected: !allSpaces) { allSpaces = false }
                    }
                }

                if query.isEmpty {
                    Text("Search page names, the text in pages, and events.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text2)
                } else if results.isEmpty && answered == query {
                    Text("Nothing found for “\(query)”.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text2)
                } else {
                    sections(query: answered)
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 6)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollDismissesKeyboard(.immediately)
        .safeAreaInset(edge: .bottom, spacing: 0) { SearchBar() }
        .background { AppBackground() }
        .toolbar(.hidden, for: .navigationBar)
        .task(id: SearchKey(query: query, allSpaces: allSpaces, space: model.spaceID)) {
            await search(query)
        }
    }

    private struct SearchKey: Hashable {
        let query: String
        let allSpaces: Bool
        let space: ID
    }

    /// Search a moment after the last key, off the main thread.
    private func search(_ query: String) async {
        guard !query.isEmpty else {
            results = SearchResults()
            answered = ""
            return
        }
        try? await Task.sleep(for: .milliseconds(120))
        if Task.isCancelled {
            return
        }
        let index = model.index
        let space = allSpaces ? nil : model.spaceID
        let found = await Task.detached(priority: .userInitiated) {
            index.search(query, space: space)
        }.value
        if Task.isCancelled {
            return
        }
        results = found
        answered = query
    }

    @ViewBuilder
    private func sections(query: String) -> some View {
        if !results.names.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("Page names").sectionHeading()
                VStack(spacing: 0) {
                    ForEach(Array(results.names.enumerated()), id: \.element.id) { index, hit in
                        if index > 0 {
                            RowSeparator()
                        }
                        NameRow(hit: hit, query: query)
                    }
                }
                .card()
            }
        }
        if !results.inside.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("Inside pages").sectionHeading()
                VStack(spacing: 0) {
                    ForEach(Array(results.inside.enumerated()), id: \.element.id) { index, hit in
                        if index > 0 {
                            RowSeparator()
                        }
                        InsideRow(hit: hit, query: query)
                    }
                }
                .card()
            }
        }
        if !results.events.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("Events").sectionHeading()
                VStack(spacing: 0) {
                    ForEach(Array(results.events.enumerated()), id: \.element.id) { index, hit in
                        if index > 0 {
                            RowSeparator()
                        }
                        EventRow(hit: hit, query: query)
                    }
                }
                .card()
            }
        }
    }
}

private struct NameRow: View {
    let hit: SearchHit
    let query: String

    var body: some View {
        if case let .page(id) = hit.target {
            NavigationLink(value: Route.page(id)) {
                HStack(spacing: 12) {
                    PageIconView(icon: hit.icon, size: 34, tile: true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(InlineText.highlighted(hit.title, query: query))
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(Theme.text)
                            .lineLimit(1)
                        if !hit.path.isEmpty {
                            Text(hit.path)
                                .font(.system(size: 12.5))
                                .foregroundStyle(Theme.text2)
                                .lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .frame(minHeight: 62)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }
}

private struct InsideRow: View {
    let hit: SearchHit
    let query: String

    var body: some View {
        if case let .page(id) = hit.target {
            NavigationLink(value: Route.page(id)) {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(hit.title)
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(Theme.text)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(hit.path)
                            .font(.system(size: 12))
                            .foregroundStyle(Theme.text2)
                            .lineLimit(1)
                            .layoutPriority(-1)
                    }
                    Text(InlineText.highlighted(hit.snippet ?? "", query: query))
                        .font(.system(size: 13.5))
                        .foregroundStyle(Theme.text2)
                        .lineSpacing(2)
                        .lineLimit(3)
                        .multilineTextAlignment(.leading)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }
}

private struct EventRow: View {
    let hit: SearchHit
    let query: String
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router

    var body: some View {
        if case let .event(id) = hit.target, let event = model.workspace.storedEvent(id) {
            Button {
                let day = nextDay(of: event)
                let shown = event.repeatRule == nil ? event.id : Repeat.instanceID(series: event.id, day: day)
                router.query = ""
                router.showCalendar(day: day, event: shown)
            } label: {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(InlineText.highlighted(hit.title, query: query))
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(Theme.text)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(dateLabel(event))
                            .font(.system(size: 12))
                            .foregroundStyle(Theme.text2)
                            .lineLimit(1)
                    }
                    Text(detail(event))
                        .font(.system(size: 13.5))
                        .foregroundStyle(Theme.text2)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }

    /// The day to show: the next time a repeating event happens, else its day.
    private func nextDay(of event: Event) -> Day {
        let first = model.when.day(of: event.start)
        guard let rule = event.repeatRule else { return first }
        let today = model.when.today()
        return Repeat.days(first: first, rule: rule, from: today, to: today.adding(days: 800)).first ?? first
    }

    private func dateLabel(_ event: Event) -> String {
        Labels.dayName(nextDay(of: event), today: model.when.today(), model.when)
    }

    private func detail(_ event: Event) -> AttributedString {
        if let snippet = hit.snippet {
            return InlineText.highlighted(snippet, query: query)
        }
        var parts = [event.allDay ? "All day" : Labels.timeRange(event.start, event.end)]
        if let rule = event.repeatRule {
            parts.append(rule.describe(first: model.when.day(of: event.start)))
        }
        return AttributedString(parts.joined(separator: " · "))
    }
}

private struct ScopeChip: View {
    let title: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 13, weight: selected ? .semibold : .medium))
                .foregroundStyle(selected ? Theme.onInk : Theme.text)
                .lineLimit(1)
                .padding(.horizontal, 14)
                .frame(height: 34)
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
