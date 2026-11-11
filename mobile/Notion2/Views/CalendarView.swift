// Calendar: a week strip (or the whole month) and the events of the day
// that is selected. Repeating events show on every day their rule falls on.

import Notion2Core
import SwiftUI

struct CalendarView: View {
    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router

    @State private var selected: Day
    @State private var showsMonth = false
    /// The week in view: the ordinal of its first day.
    @State private var weekPage: Int?
    /// The month in view: year × 12 + month − 1.
    @State private var monthPage: Int?
    @State private var shownEvent: Event?
    /// A new event is being made on the day that is selected.
    @State private var adding = false

    init() {
        let today = When().today()
        _selected = State(initialValue: today)
        _weekPage = State(initialValue: CalendarView.weekStart(today).ordinal)
        _monthPage = State(initialValue: CalendarView.monthNumber(today))
    }

    private var when: When { model.when }

    /// How far the first day of the week is from Monday (0: Monday, 6: Sunday).
    private static var weekOffset: Int {
        (Calendar.current.firstWeekday + 5) % 7
    }

    private static func weekStart(_ day: Day) -> Day {
        day.adding(days: -((day.weekdayFromMonday - weekOffset + 7) % 7))
    }

    private static func monthNumber(_ day: Day) -> Int {
        day.year * 12 + day.month - 1
    }

    private func weekStart(_ day: Day) -> Day { CalendarView.weekStart(day) }

    private func monthNumber(_ day: Day) -> Int { CalendarView.monthNumber(day) }

    private func firstDay(ofMonth number: Int) -> Day {
        let year = Int((Double(number) / 12).rounded(.down))
        return Day(year: year, month: number - year * 12 + 1, day: 1) ?? selected
    }

    var body: some View {
        let today = when.today()
        let events = model.workspace.events(in: model.spaceID)
        VStack(alignment: .leading, spacing: 14) {
            header(today: today)
                .padding(.horizontal, 20)
            if showsMonth {
                monthPager(today: today, events: events)
            } else {
                weekPager(today: today, events: events)
            }
            TimelineView(.everyMinute) { timeline in
                DayAgenda(day: selected, events: events, now: timeline.date) { shownEvent = $0 }
            }
        }
        .padding(.top, 6)
        .safeAreaInset(edge: .bottom, spacing: 0) { TabBar() }
        .background { AppBackground() }
        .toolbar(.hidden, for: .navigationBar)
        .sheet(item: $shownEvent) { event in
            EventDetailView(event: event)
        }
        .sheet(isPresented: $adding) {
            EventEditor(original: nil, day: selected)
        }
        .onChange(of: selected) { _, day in
            let week = weekStart(day).ordinal
            if weekPage != week {
                weekPage = week
            }
            if monthPage != monthNumber(day) {
                monthPage = monthNumber(day)
            }
        }
        .onChange(of: weekPage) { _, page in
            // A swipe to another week keeps the day of the week.
            guard let page, !showsMonth, weekStart(selected).ordinal != page else { return }
            selected = Day(ordinal: page + selected.ordinal - weekStart(selected).ordinal)
        }
        .onChange(of: monthPage) { _, page in
            // A swipe to another month selects today in this month, else its first day.
            guard let page, showsMonth, monthNumber(selected) != page else { return }
            selected = monthNumber(today) == page ? today : firstDay(ofMonth: page)
        }
        .onChange(of: router.dayToShow, initial: true) { _, day in
            guard let day else { return }
            selected = day
            if let id = router.eventToShow {
                let from = when.midnight(day)
                let found = Repeat.expand(events, from: from, to: when.midnight(day.adding(days: 1)), when: when)
                shownEvent = found.first { $0.id == id } ?? found.first { $0.seriesID == Repeat.splitID(id).series }
            }
            router.dayToShow = nil
            router.eventToShow = nil
        }
    }

    // MARK: Header

    private func header(today: Day) -> some View {
        let shown = showsMonth ? firstDay(ofMonth: monthPage ?? monthNumber(selected)) : selected
        let date = Labels.date(shown, when)
        return HStack(alignment: .center, spacing: 12) {
            Button {
                withAnimation(.snappy(duration: 0.3)) {
                    monthPage = monthNumber(selected)
                    weekPage = weekStart(selected).ordinal
                    showsMonth.toggle()
                }
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text("\(Text(date.formatted(.dateTime.month(.wide))).fontWeight(.bold).foregroundStyle(Theme.text)) \(Text(date.formatted(.dateTime.year())).fontWeight(.medium).foregroundStyle(Theme.text2))")
                        .font(.system(size: 32))
                        .tracking(-0.8)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                    IconView("chevron-down", size: 18, weight: 2.1)
                        .foregroundStyle(Theme.text2)
                        .rotationEffect(.degrees(showsMonth ? 180 : 0))
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(date.formatted(.dateTime.month(.wide).year()))
            .accessibilityHint(showsMonth ? "Shows one week" : "Shows the whole month")

            Spacer(minLength: 0)

            if selected != today {
                Button("Today") {
                    withAnimation(.snappy(duration: 0.3)) { selected = today }
                }
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Theme.text)
                .buttonStyle(.glass)
                .transition(.opacity)
            }
            if model.canEdit {
                Button {
                    adding = true
                } label: {
                    IconView("plus", size: 18, weight: 2)
                        .foregroundStyle(Theme.text)
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.glass)
                .buttonBorderShape(.circle)
                .accessibilityLabel("New event")
            }
        }
        .frame(minHeight: 44)
    }

    // MARK: Week and month

    /// The colours of the events on each day from `first`, for `count` days.
    private func marks(from first: Day, count: Int, events: [Event]) -> [Day: [String]] {
        let from = when.midnight(first)
        let to = when.midnight(first.adding(days: count))
        var out: [Day: [String]] = [:]
        for event in Repeat.expand(events, from: from, to: to, when: when) {
            var day = max(when.day(of: event.start), first)
            let last = min(when.day(of: event.end - 1), first.adding(days: count - 1))
            while day <= last {
                if !(out[day] ?? []).contains(event.color) {
                    out[day, default: []].append(event.color)
                }
                day = day.adding(days: 1)
            }
        }
        return out
    }

    private func weekPager(today: Day, events: [Event]) -> some View {
        let base = weekStart(today).ordinal
        return ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach((-520...520).map { base + $0 * 7 }, id: \.self) { ordinal in
                    let start = Day(ordinal: ordinal)
                    WeekStrip(
                        days: (0..<7).map { start.adding(days: $0) },
                        selected: selected,
                        today: today,
                        marks: marks(from: start, count: 7, events: events),
                        select: { selected = $0 }
                    )
                    .padding(.horizontal, 20)
                    .containerRelativeFrame(.horizontal)
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.paging)
        .scrollPosition(id: $weekPage)
        .scrollIndicators(.hidden)
        .frame(height: 74)
    }

    private func monthPager(today: Day, events: [Event]) -> some View {
        let base = monthNumber(today)
        return ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach((-120...120).map { base + $0 }, id: \.self) { number in
                    let first = firstDay(ofMonth: number)
                    let gridStart = weekStart(first)
                    MonthGrid(
                        month: first,
                        gridStart: gridStart,
                        selected: selected,
                        today: today,
                        marks: marks(from: gridStart, count: 42, events: events),
                        select: { selected = $0 }
                    )
                    .padding(.horizontal, 20)
                    .containerRelativeFrame(.horizontal)
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.paging)
        .scrollPosition(id: $monthPage)
        .scrollIndicators(.hidden)
        .frame(height: MonthGrid.height)
    }
}

// MARK: - Week strip

private struct WeekStrip: View {
    let days: [Day]
    let selected: Day
    let today: Day
    let marks: [Day: [String]]
    let select: (Day) -> Void

    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 2) {
            ForEach(days, id: \.self) { day in
                let isSelected = day == selected
                let date = Labels.date(day, model.when)
                Button {
                    select(day)
                } label: {
                    VStack(spacing: 3) {
                        Text(date.formatted(.dateTime.weekday(.abbreviated)))
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(isSelected ? Theme.onInk : day == today ? Theme.now : Theme.text2)
                        Text("\(day.day)")
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(isSelected ? Theme.onInk : day == today ? Theme.now : Theme.text)
                        Circle()
                            .fill(dotColor(day, selected: isSelected))
                            .frame(width: 5, height: 5)
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 62)
                    .background(isSelected ? Theme.ink : .clear, in: RoundedRectangle(cornerRadius: 19, style: .continuous))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(date.formatted(.dateTime.weekday(.wide).month(.wide).day()))
                .accessibilityValue((marks[day] ?? []).isEmpty ? "" : "Has events")
                .accessibilityAddTraits(isSelected ? .isSelected : [])
            }
        }
        .padding(5)
        .card(cornerRadius: 24)
    }

    private func dotColor(_ day: Day, selected: Bool) -> Color {
        guard let first = marks[day]?.first else { return .clear }
        return selected ? Theme.onInk : Theme.tint(first).bar
    }
}

// MARK: - Month grid

private struct MonthGrid: View {
    let month: Day
    let gridStart: Day
    let selected: Day
    let today: Day
    let marks: [Day: [String]]
    let select: (Day) -> Void

    @Environment(AppModel.self) private var model

    static let rowHeight: CGFloat = 44
    static let height: CGFloat = 10 + 20 + 6 * rowHeight + 10

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                ForEach(0..<7, id: \.self) { column in
                    Text(Labels.date(gridStart.adding(days: column), model.when).formatted(.dateTime.weekday(.narrow)))
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Theme.text2)
                        .frame(maxWidth: .infinity)
                }
            }
            .frame(height: 20)
            .accessibilityHidden(true)
            ForEach(0..<6, id: \.self) { row in
                HStack(spacing: 0) {
                    ForEach(0..<7, id: \.self) { column in
                        cell(gridStart.adding(days: row * 7 + column))
                    }
                }
            }
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 6)
        .card(cornerRadius: 24)
    }

    private func cell(_ day: Day) -> some View {
        let isSelected = day == selected
        let inMonth = day.month == month.month
        let colors = marks[day] ?? []
        return Button {
            select(day)
        } label: {
            VStack(spacing: 3) {
                Text("\(day.day)")
                    .font(.system(size: 15, weight: isSelected || day == today ? .semibold : .regular))
                    .foregroundStyle(isSelected ? Theme.onInk : day == today ? Theme.now : Theme.text)
                    .frame(width: 32, height: 28)
                    .background(isSelected ? Theme.ink : .clear, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
                HStack(spacing: 3) {
                    ForEach(colors.prefix(3), id: \.self) { color in
                        Circle().fill(Theme.tint(color).bar).frame(width: 4, height: 4)
                    }
                }
                .frame(height: 4)
            }
            .frame(maxWidth: .infinity)
            .frame(height: MonthGrid.rowHeight)
            .opacity(inMonth ? 1 : 0.35)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Labels.date(day, model.when).formatted(.dateTime.weekday(.wide).month(.wide).day()))
        .accessibilityValue(colors.isEmpty ? "" : "Has events")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

// MARK: - The events of one day

private struct DayAgenda: View {
    let day: Day
    let events: [Event]
    let now: Date
    let open: (Event) -> Void

    @Environment(AppModel.self) private var model

    private enum Item: Identifiable {
        case event(Event)
        case now

        var id: String {
            switch self {
            case let .event(e): e.id
            case .now: "now"
            }
        }
    }

    var body: some View {
        let when = model.when
        let from = when.midnight(day)
        let to = when.midnight(day.adding(days: 1))
        let shown = Repeat.expand(events, from: from, to: to, when: when)
        // An event that covers the whole day shows with the all-day events.
        let allDay = shown.filter { $0.allDay || ($0.start <= from && $0.end >= to) }
        let timed = shown.filter { !($0.allDay || ($0.start <= from && $0.end >= to)) }
        let items = items(timed: timed, isToday: day == when.today(now: now))
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                ForEach(allDay) { event in
                    AllDayRow(event: event) { open(event) }
                }
                if shown.isEmpty {
                    Text("Nothing on this day.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text2)
                        .padding(.top, 6)
                        .padding(.leading, 4)
                }
                if !timed.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(items) { item in
                            HStack(alignment: .top, spacing: 0) {
                                Group {
                                    if case let .event(event) = item {
                                        Text(event.start >= from ? Labels.shortTime(event.start) : "")
                                    } else {
                                        Text("")
                                    }
                                }
                                .font(.system(size: 12))
                                .foregroundStyle(Theme.text2)
                                .frame(width: 52, alignment: .leading)
                                .padding(.top, 2)

                                VStack(spacing: 0) {
                                    RowSeparator()
                                    switch item {
                                    case let .event(event):
                                        EventCard(event: event) { open(event) }
                                            .padding(.top, 8)
                                            .padding(.bottom, 14)
                                    case .now:
                                        NowLine(now: now)
                                            .padding(.top, 8)
                                            .padding(.bottom, 14)
                                    }
                                }
                            }
                        }
                    }
                    .padding(.top, allDay.isEmpty ? 0 : 6)
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 2)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// The timed events in order, with the "now" line where the time is.
    private func items(timed: [Event], isToday: Bool) -> [Item] {
        var out = timed.map(Item.event)
        if isToday {
            let nowMS = When.ms(now)
            let at = timed.firstIndex { $0.start > nowMS } ?? timed.count
            out.insert(.now, at: at)
        }
        return out
    }
}

private struct AllDayRow: View {
    let event: Event
    let action: () -> Void

    var body: some View {
        let tint = Theme.tint(event.color)
        Button(action: action) {
            HStack(spacing: 10) {
                Text("All day")
                    .font(.system(size: 12, weight: .semibold))
                Text(event.displayTitle)
                    .font(.system(size: 15, weight: .medium))
                    .lineLimit(1)
                Spacer(minLength: 0)
            }
            .foregroundStyle(tint.eventText)
            .padding(.horizontal, 16)
            .frame(minHeight: 46)
            .eventSurface(tint)
        }
        .buttonStyle(.plain)
    }
}

private struct EventCard: View {
    let event: Event
    let action: () -> Void

    @Environment(AppModel.self) private var model

    var body: some View {
        let tint = Theme.tint(event.color)
        let notesPage = event.pageID.flatMap { model.workspace.pages[$0] }.flatMap { $0.trashed ? nil : $0 }
        Button(action: action) {
            VStack(alignment: .leading, spacing: 5) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(event.displayTitle)
                        .font(.system(size: 15, weight: .semibold))
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                    Spacer(minLength: 0)
                    if let remind = event.remind {
                        HStack(spacing: 4) {
                            IconView("bell", size: 13)
                            Text(Labels.reminder(remind))
                                .font(.system(size: 12, weight: .medium))
                                .lineLimit(1)
                        }
                    }
                }
                HStack(spacing: 6) {
                    Text(Labels.span(of: event, model.when))
                        .font(.system(size: 12.5))
                    if event.repeatRule != nil {
                        IconView("repeat", size: 12)
                            .accessibilityLabel("Repeats")
                    }
                }
                if let notesPage {
                    HStack(spacing: 6) {
                        IconView("file", size: 14)
                        Text("Notes: \(notesPage.displayTitle)")
                            .font(.system(size: 13, weight: .medium))
                            .lineLimit(1)
                    }
                    .padding(.top, 1)
                }
            }
            .foregroundStyle(tint.eventText)
            .padding(.vertical, 11)
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .eventSurface(tint)
        }
        .buttonStyle(.plain)
    }
}

private struct NowLine: View {
    let now: Date

    var body: some View {
        HStack(spacing: 8) {
            Capsule()
                .fill(Theme.now)
                .frame(height: 2)
                .overlay(alignment: .leading) {
                    Circle().fill(Theme.now).frame(width: 8, height: 8).offset(x: -4)
                }
                .padding(.leading, 4)
            Text("Now \(Labels.shortTime(When.ms(now)))")
                .font(.system(size: 12.5, weight: .semibold))
                .foregroundStyle(Theme.now)
        }
        .frame(height: 20)
        .accessibilityElement(children: .combine)
    }
}

private extension View {
    /// An event's card: a tinted fill with a bar of its colour at the edge.
    func eventSurface(_ tint: Theme.Tint) -> some View {
        background(tint.eventFill)
            .overlay(alignment: .leading) {
                Rectangle().fill(tint.bar).frame(width: 3)
            }
            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
    }
}

// MARK: - One event

struct EventDetailView: View {
    let event: Event

    @Environment(AppModel.self) private var model
    @Environment(Router.self) private var router
    @Environment(\.dismiss) private var dismiss
    @State private var editing = false

    var body: some View {
        let when = model.when
        let tint = Theme.tint(event.color)
        let stored = model.workspace.storedEvent(event.id)
        let notesPage = event.pageID.flatMap { model.workspace.pages[$0] }.flatMap { $0.trashed ? nil : $0 }
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(alignment: .top, spacing: 12) {
                    Capsule()
                        .fill(tint.bar)
                        .frame(width: 4, height: 28)
                    Text(event.displayTitle)
                        .font(.system(size: 22, weight: .bold))
                        .foregroundStyle(Theme.text)
                        .fixedSize(horizontal: false, vertical: true)
                }

                VStack(alignment: .leading, spacing: 12) {
                    line("clock") {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Labels.longDate(when.day(of: event.start), when))
                            Text(Labels.span(of: event, when))
                                .foregroundStyle(Theme.text2)
                        }
                    }
                    if let rule = event.repeatRule {
                        line("repeat") {
                            Text(rule.describe(first: when.day(of: stored?.start ?? event.start)))
                        }
                    }
                    if let remind = event.remind {
                        line("bell") {
                            Text(Labels.reminder(remind))
                        }
                    }
                    if let join = URL(string: event.join.trimmingCharacters(in: .whitespaces)), join.scheme?.hasPrefix("http") == true {
                        line("link") {
                            Link(destination: join) {
                                Text("Join the call")
                                    .fontWeight(.medium)
                            }
                            .foregroundStyle(Theme.accent)
                        }
                    }
                }

                if !event.notes.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Text(event.notes)
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.text)
                        .lineSpacing(4)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(14)
                        .background(Theme.field, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }

                if let notesPage {
                    Button {
                        dismiss()
                        router.openPage(notesPage.id)
                    } label: {
                        HStack(spacing: 10) {
                            PageIconView(icon: notesPage.icon, size: 20)
                            Text(notesPage.displayTitle)
                                .font(.system(size: 16, weight: .medium))
                                .foregroundStyle(Theme.text)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                            IconView("chevron-right", size: 15, weight: 1.9)
                                .foregroundStyle(Theme.text2)
                        }
                        .padding(.horizontal, 14)
                        .frame(height: 50)
                        .background(Theme.field, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint("Opens the notes page")
                }
            }
            .padding(.horizontal, 24)
            .padding(.top, 30)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .overlay(alignment: .topTrailing) {
            if model.canEdit, model.workspace.storedEvent(event.id) != nil {
                Button("Edit") { editing = true }
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.text)
                    .buttonStyle(.glass)
                    .padding(.top, 18)
                    .padding(.trailing, 18)
            }
        }
        .sheet(isPresented: $editing) {
            if let stored = model.workspace.storedEvent(event.id) {
                EventEditor(original: stored, day: model.when.day(of: stored.start)) { dismiss() }
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private func line(_ icon: String, @ViewBuilder content: () -> some View) -> some View {
        HStack(alignment: .top, spacing: 12) {
            IconView(icon, size: 18)
                .foregroundStyle(Theme.text2)
                .padding(.top, 1)
            content()
                .font(.system(size: 15))
                .foregroundStyle(Theme.text)
        }
    }
}
