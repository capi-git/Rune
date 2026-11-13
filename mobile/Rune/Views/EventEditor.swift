// Making and changing an event on the phone, in the quiet form of the Mac's
// event panel: the name large at the top, then one row for each thing (a
// grey icon, then the value to tap), colour dots on one line, and a grey bin
// at the foot. A change to a repeating event changes every repeat.

import RuneCore
import SwiftUI

struct EventEditor: View {
    /// The stored event to change, or nil for a new one.
    let original: Event?
    /// For a new event: its day.
    let day: Day
    /// After a save or a delete (the screen under it may close too).
    var onDone: () -> Void = {}

    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    @State private var title = ""
    @State private var allDay = false
    @State private var start = Date()
    @State private var end = Date()
    @State private var every: Repeat.Every?
    @State private var remind: Int?
    @State private var color = "blue"
    @State private var notes = ""
    @State private var asksDelete = false
    /// The form holds the event: from now on a new start moves the end too.
    @State private var filled = false
    @FocusState private var titleFocused: Bool

    private static let colors = ["blue", "green", "orange", "red", "purple", "pink", "yellow", "brown", "gray"]
    private static let reminders: [Int?] = [nil, 0, 5, 10, 15, 30, 60, 24 * 60]
    private static let repeats: [(Repeat.Every?, String)] = [
        (nil, "Does not repeat"), (.day, "Every day"), (.weekday, "Every weekday"), (.week, "Every week"),
        (.month, "Every month"), (.year, "Every year"),
    ]

    init(original: Event?, day: Day, onDone: @escaping () -> Void = {}) {
        self.original = original
        self.day = day
        self.onDone = onDone
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    HStack(alignment: .center, spacing: 12) {
                        Capsule()
                            .fill(Theme.tint(color).bar)
                            .frame(width: 4, height: 28)
                        TextField("New event", text: $title, axis: .vertical)
                            .font(.system(size: 22, weight: .bold))
                            .foregroundStyle(Theme.text)
                            .focused($titleFocused)
                            .submitLabel(.done)
                    }
                    .padding(.bottom, 18)

                    group {
                        row("clock") {
                            Toggle("All day", isOn: $allDay.animation(.snappy(duration: 0.2)))
                                .tint(Theme.ink)
                        }
                        row("calendar") {
                            HStack {
                                Text("Starts").foregroundStyle(Theme.text2)
                                Spacer(minLength: 8)
                                DatePicker("Starts", selection: $start, displayedComponents: allDay ? [.date] : [.date, .hourAndMinute])
                                    .labelsHidden()
                            }
                        }
                        row(nil) {
                            HStack {
                                Text("Ends").foregroundStyle(Theme.text2)
                                Spacer(minLength: 8)
                                DatePicker("Ends", selection: $end, in: start..., displayedComponents: allDay ? [.date] : [.date, .hourAndMinute])
                                    .labelsHidden()
                            }
                        }
                    }

                    group {
                        row("repeat") {
                            Menu {
                                Picker("Repeat", selection: $every) {
                                    ForEach(EventEditor.repeats, id: \.1) { item in
                                        Text(item.1).tag(item.0)
                                    }
                                }
                            } label: {
                                value(EventEditor.repeats.first { $0.0 == every }?.1 ?? "Does not repeat")
                            }
                        }
                        row("bell") {
                            Menu {
                                Picker("Reminder", selection: $remind) {
                                    ForEach(EventEditor.reminders, id: \.self) { minutes in
                                        Text(minutes.map { Labels.reminder($0) } ?? "No reminder").tag(minutes)
                                    }
                                }
                            } label: {
                                value(remind.map { Labels.reminder($0) } ?? "No reminder")
                            }
                        }
                        row("palette") {
                            HStack(spacing: 10) {
                                ForEach(EventEditor.colors, id: \.self) { name in
                                    Button {
                                        color = name
                                    } label: {
                                        Circle()
                                            .fill(Theme.tint(name).bar)
                                            .frame(width: 20, height: 20)
                                            .overlay {
                                                if color == name {
                                                    Circle().strokeBorder(Theme.text, lineWidth: 2).padding(-4)
                                                }
                                            }
                                            .frame(width: 26, height: 26)
                                            .contentShape(Circle())
                                    }
                                    .buttonStyle(.plain)
                                    .accessibilityLabel(name.capitalized)
                                    .accessibilityAddTraits(color == name ? .isSelected : [])
                                }
                            }
                        }
                    }

                    group {
                        row("note", top: true) {
                            TextField("Notes", text: $notes, axis: .vertical)
                                .lineLimit(3...12)
                        }
                    }

                    if original != nil {
                        HStack {
                            Spacer()
                            Button {
                                asksDelete = true
                            } label: {
                                IconView("trash", size: 19)
                                    .foregroundStyle(Theme.text2)
                                    .frame(width: 44, height: 44)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Delete event")
                            .confirmationDialog(
                                original?.repeatRule == nil ? "Delete this event?" : "Delete every repeat of this event?",
                                isPresented: $asksDelete, titleVisibility: .visible
                            ) {
                                Button("Delete", role: .destructive) {
                                    guard let id = original?.id else { return }
                                    Task {
                                        await model.remove(event: id)
                                        dismiss()
                                        onDone()
                                    }
                                }
                            }
                        }
                        .padding(.top, 8)
                    }
                }
                .padding(.horizontal, 22)
                .padding(.top, 12)
                .padding(.bottom, 24)
            }
            .scrollDismissesKeyboard(.interactively)
            .background { AppBackground() }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        Task {
                            await model.save(event: made())
                            dismiss()
                            onDone()
                        }
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .onAppear(perform: fill)
        .onChange(of: start) { old, new in
            // The event keeps its length when its start moves.
            guard filled else { return }
            let length = end.timeIntervalSince(old)
            end = new.addingTimeInterval(max(length, 0))
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
    }

    // MARK: Parts

    /// Rows that belong together, with a hairline under them.
    private func group(@ViewBuilder _ rows: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            rows()
        }
        .padding(.vertical, 6)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Theme.separator).frame(height: 1)
        }
    }

    private func row(_ icon: String?, top: Bool = false, @ViewBuilder _ content: () -> some View) -> some View {
        HStack(alignment: top ? .firstTextBaseline : .center, spacing: 14) {
            Group {
                if let icon {
                    IconView(icon, size: 18)
                } else {
                    Color.clear
                }
            }
            .frame(width: 20)
            .foregroundStyle(Theme.text2)
            .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + 5 }
            content()
                .font(.system(size: 16))
                .foregroundStyle(Theme.text)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(minHeight: 46)
        .padding(.vertical, top ? 12 : 0)
    }

    private func value(_ text: String) -> some View {
        Text(text)
            .foregroundStyle(Theme.text)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
    }

    // MARK: From and to the event

    private func fill() {
        let when = model.when
        if let e = original {
            title = e.title
            allDay = e.allDay
            start = When.date(e.start)
            // An all-day event ends at midnight after its last day: show that day.
            end = When.date(e.allDay ? max(e.end - 1, e.start) : e.end)
            every = e.repeatRule?.every
            remind = e.remind
            color = e.color
            notes = e.notes
        } else {
            // The next full hour of the day (or 9:00 on another day), one hour long.
            let now = Date()
            let today = when.today()
            var hour = 9
            if day == today {
                hour = min(when.time(of: When.ms(now)).hour + 1, 23)
            }
            start = When.date(when.midnight(day) + Int64(hour) * 3_600_000)
            end = start.addingTimeInterval(3600)
            titleFocused = true
        }
        // After the start set above has been seen.
        DispatchQueue.main.async { filled = true }
    }

    private func made() -> Event {
        let when = model.when
        var startMS = When.ms(start)
        var endMS = When.ms(end)
        if allDay {
            startMS = when.midnight(when.day(of: startMS))
            endMS = when.midnight(when.day(of: endMS).adding(days: 1))
        }
        var rule = original?.repeatRule
        if let every {
            if rule?.every != every {
                rule = Repeat(every: every)
            }
        } else {
            rule = nil
        }
        var event = original ?? Event(id: SyncFormat.newRecordID(), spaceID: model.spaceID, start: startMS, end: endMS)
        event.title = title.trimmingCharacters(in: .whitespacesAndNewlines)
        event.start = startMS
        event.end = max(endMS, startMS + 1)
        event.allDay = allDay
        event.color = color
        event.notes = notes
        event.remind = remind
        event.repeatRule = rule
        return event
    }
}
