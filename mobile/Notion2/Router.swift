// Where the user is: which tab, and which pages are open on top of it.

import Notion2Core
import Observation
import SwiftUI

enum AppTab: String, CaseIterable, Identifiable {
    case home
    case pages
    case search
    case calendar

    var id: String { rawValue }

    var title: String {
        switch self {
        case .home: "Home"
        case .pages: "Pages"
        case .search: "Search"
        case .calendar: "Calendar"
        }
    }

    var icon: String {
        switch self {
        case .home: "home"
        case .pages: "file"
        case .search: "search"
        case .calendar: "calendar"
        }
    }
}

enum Route: Hashable {
    case page(ID)
    case trash
    /// Every open to-do of the space.
    case todos
}

@Observable
final class Router {
    var tab: AppTab = .home {
        didSet {
            if oldValue != .search && tab == .search {
                tabBeforeSearch = oldValue
            }
        }
    }

    /// The pages open on top of each tab.
    var paths: [AppTab: [Route]] = [:]
    /// What is typed in the search field.
    var query = ""
    /// A day the calendar is asked to show.
    var dayToShow: Day?
    /// An event the calendar is asked to show.
    var eventToShow: ID?

    private var tabBeforeSearch: AppTab = .home

    func path(for tab: AppTab) -> Binding<[Route]> {
        Binding(
            get: { self.paths[tab] ?? [] },
            set: { self.paths[tab] = $0 }
        )
    }

    func open(_ route: Route) {
        paths[tab, default: []].append(route)
    }

    func openPage(_ id: ID) {
        open(.page(id))
    }

    func closeSearch() {
        query = ""
        tab = tabBeforeSearch
    }

    func showCalendar(day: Day, event: ID? = nil) {
        dayToShow = day
        eventToShow = event
        paths[.calendar] = []
        tab = .calendar
    }

    /// Back to the first screen everywhere (after another copy is loaded).
    func reset() {
        paths = [:]
        query = ""
        if tab == .search {
            tab = .home
        }
    }
}
