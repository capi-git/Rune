// Home: the pages changed last, what is next on the calendar, the open
// to-dos from all pages, favourites.

import RuneCore
import SwiftUI

struct HomeView: View {
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router
    @State private var showsSettings = false

    var body: some View {
        TimelineView(.everyMinute) { timeline in
            content(now: timeline.date)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { TabBar() }
        .background { AppBackground() }
        .navigationTitle("Home")
        .hidesBarTitle()
        .toolbar {
            // With one space there is nothing to switch: its name is plain
            // text, without the glass of a control.
            ToolbarItem(placement: .topBarLeading) {
                SpaceSwitcher()
            }
            .sharedBackgroundVisibility(model.workspace.spaces.count > 1 ? .automatic : .hidden)
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    showsSettings = true
                } label: {
                    IconView("settings", size: 22)
                        .foregroundStyle(Theme.text)
                }
                .accessibilityLabel("Settings")
            }
        }
        .sheet(isPresented: $showsSettings) {
            SettingsView()
        }
    }

    private func content(now: Date) -> some View {
        let space = model.spaceID
        let today = model.when.today(now: now)
        let recent = model.workspace.recent(in: space, limit: 6)
        let upcoming = upcomingEvents(now: now)
        let favorites = model.workspace.favorites(in: space)
        let todos = model.workspace.openTodos(in: space)
        return ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(Labels.longDate(today, model.when))
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Theme.text2)
                    Text(Labels.greeting(now: now, model.when))
                        .screenTitle()
                }
                .padding(.horizontal, 20)

                if !recent.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Jump back in").sectionHeading().padding(.leading, 20)
                        ScrollView(.horizontal) {
                            HStack(spacing: 10) {
                                ForEach(recent) { page in
                                    RecentCard(page: page, now: now)
                                }
                            }
                            .padding(.horizontal, 20)
                        }
                        .scrollIndicators(.hidden)
                        .scrollClipDisabled()
                    }
                }

                if !upcoming.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Upcoming").sectionHeading()
                        VStack(spacing: 0) {
                            ForEach(Array(upcoming.enumerated()), id: \.element.id) { index, event in
                                if index > 0 {
                                    RowSeparator()
                                }
                                UpcomingRow(event: event, today: today)
                            }
                        }
                        .card()
                    }
                    .padding(.horizontal, 20)
                }

                if !todos.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack(alignment: .firstTextBaseline) {
                            Text("To do").sectionHeading()
                            Spacer(minLength: 8)
                            // The first few show here; the rest are one tap away.
                            if todos.count > TodoList.shownOnHome {
                                NavigationLink(value: Route.todos) {
                                    Text("All \(todos.count)")
                                        .font(.system(size: 13, weight: .semibold))
                                        .foregroundStyle(Theme.accent)
                                        .padding(.trailing, 4)
                                        .frame(minHeight: 24)
                                        .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel("All \(todos.count) to-dos")
                            }
                        }
                        TodoList(todos: Array(todos.prefix(TodoList.shownOnHome)), showsPage: true)
                    }
                    .padding(.horizontal, 20)
                }

                if !favorites.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Favorites").sectionHeading()
                        PageList(pages: favorites)
                    }
                    .padding(.horizontal, 20)
                }

                if recent.isEmpty && upcoming.isEmpty && favorites.isEmpty && todos.isEmpty {
                    Text("This space has no pages yet.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text2)
                        .padding(.horizontal, 20)
                }
            }
            .padding(.top, 8)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// The next few events, from now, in the next two weeks.
    private func upcomingEvents(now: Date) -> [Event] {
        let from = When.ms(now)
        let events = Repeat.expand(model.workspace.events(in: model.spaceID), from: from, to: from + 14 * 24 * When.hour, when: model.when)
        return Array(events.prefix(3))
    }
}

/// The space's badge and name; a menu of the spaces when there are several.
private struct SpaceSwitcher: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let space = model.space {
            if model.workspace.spaces.count > 1 {
                Menu {
                    ForEach(model.workspace.spaces) { other in
                        Button {
                            model.spaceID = other.id
                        } label: {
                            if other.id == space.id {
                                Label(other.displayName, systemImage: "checkmark")
                            } else {
                                Text(other.displayName)
                            }
                        }
                    }
                } label: {
                    label(space, chevron: true)
                }
                .accessibilityLabel("Space: \(space.displayName)")
            } else {
                label(space, chevron: false)
            }
        }
    }

    private func label(_ space: Space, chevron: Bool) -> some View {
        HStack(spacing: 8) {
            SpaceBadge(space: space, size: 26)
            Text(space.displayName)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(Theme.text)
                .lineLimit(1)
            if chevron {
                IconView("chevron-down", size: 14, weight: 2)
                    .foregroundStyle(Theme.text2)
            }
        }
        .padding(.horizontal, 4)
        .fixedSize()
    }
}

/// The round badge of a space: its first letter on its colour.
struct SpaceBadge: View {
    let space: Space
    var size: CGFloat = 26

    var body: some View {
        Text(space.initial)
            .font(.system(size: size * 0.46, weight: .semibold))
            .foregroundStyle(Theme.onInk)
            .frame(width: size, height: size)
            .background(Theme.tint(space.color).bar, in: Circle())
            .accessibilityHidden(true)
    }
}

private struct RecentCard: View {
    let page: Page
    let now: Date
    @Environment(AppModel.self) private var model

    var body: some View {
        NavigationLink(value: Route.page(page.id)) {
            VStack(alignment: .leading, spacing: 0) {
                PageIconView(icon: page.icon, size: 34, tile: true)
                Spacer(minLength: 8)
                Text(page.displayTitle)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Theme.text)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                Text("Edited \(Labels.ago(page.updatedAt, now: now, model.when))")
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.text2)
                    .lineLimit(1)
                    .padding(.top, 2)
            }
            .padding(12)
            .frame(width: 132, height: 120, alignment: .leading)
            .card()
        }
        .buttonStyle(.plain)
    }
}

private struct UpcomingRow: View {
    let event: Event
    let today: Day
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router

    var body: some View {
        let day = max(model.when.day(of: event.start), today)
        Button {
            router.showCalendar(day: day, event: event.id)
        } label: {
            HStack(spacing: 12) {
                Capsule()
                    .fill(Theme.tint(event.color).bar)
                    .frame(width: 4, height: 32)
                VStack(alignment: .leading, spacing: 2) {
                    Text(event.displayTitle)
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(Theme.text)
                        .lineLimit(1)
                    Text(subtitle(day: day))
                        .font(.system(size: 12.5))
                        .foregroundStyle(Theme.text2)
                        .lineLimit(1)
                }
                Spacer(minLength: 8)
                if event.remind != nil {
                    IconView("bell", size: 18)
                        .foregroundStyle(Theme.text2)
                }
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 58)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func subtitle(day: Day) -> String {
        let name = Labels.dayName(day, today: today, model.when)
        if event.allDay {
            return "\(name) · All day"
        }
        if model.when.day(of: event.start) == model.when.day(of: event.end - 1) {
            return "\(name) · \(Labels.timeRange(event.start, event.end))"
        }
        return "\(name) · \(Labels.time(event.start))"
    }
}

/// A card of page rows.
struct PageList: View {
    let pages: [Page]

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(pages.enumerated()), id: \.element.id) { index, page in
                if index > 0 {
                    RowSeparator()
                }
                NavigationLink(value: Route.page(page.id)) {
                    HStack(spacing: 12) {
                        PageIconView(icon: page.icon, size: 20)
                        Text(page.displayTitle)
                            .font(.system(size: 15))
                            .foregroundStyle(Theme.text)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 16)
                    .frame(height: 50)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .card()
    }
}
