// n2cal: for Rune. Reads the events of this Mac's calendars (the Calendar app, with
// the iCloud, Google or Outlook accounts it holds) for the app, when the
// person has turned that on in Settings. It only reads: it never makes,
// changes or deletes an event. It prints one line of JSON and ends.
//
//   n2cal status
//       {"status":"allowed" | "not-asked" | "denied" | "restricted"}
//   n2cal events <from> <to> [--ask]
//       The same, with "calendars" and "events" when the status is "allowed".
//       <from> and <to> are seconds since 1970. With --ask, macOS asks the
//       person first when it has not asked before.
//
// Two more commands change a calendar. The app runs them only when the
// person has turned on "Let Rune change them" (off until then):
//
//   n2cal move <id> <start> <end>
//       Give one event (the id as "events" gave it) a new start and end,
//       in seconds since 1970. Only that event, not the others of its
//       series. Refused for a meeting with other people in it, and for a
//       calendar that cannot be changed.
//   n2cal add <calendar id> <start> <end> <all day: 0 or 1> <name>
//       Make a new event in that calendar.
//       Both answer {"status":"ok"} or {"status":"error","problem":"..."}.
//
// Of an event it gives the name, the times, the calendar, the place, and the
// address to join its meeting by when its notes hold one. Not the notes
// themselves, not the people invited.
import EventKit
import Foundation

func status() -> String {
    let now = EKEventStore.authorizationStatus(for: .event)
    if #available(macOS 14.0, *) {
        switch now {
        case .fullAccess: return "allowed"
        case .notDetermined: return "not-asked"
        case .restricted: return "restricted"
        default: return "denied"
        }
    } else {
        switch now {
        case .authorized: return "allowed"
        case .notDetermined: return "not-asked"
        case .restricted: return "restricted"
        default: return "denied"
        }
    }
}

func ask() -> Bool {
    let store = EKEventStore()
    let done = DispatchSemaphore(value: 0)
    var allowed = false
    if #available(macOS 14.0, *) {
        store.requestFullAccessToEvents { granted, _ in
            allowed = granted
            done.signal()
        }
    } else {
        store.requestAccess(to: .event) { granted, _ in
            allowed = granted
            done.signal()
        }
    }
    done.wait()
    return allowed
}

func hex(_ color: CGColor?) -> String {
    guard let space = CGColorSpace(name: CGColorSpace.sRGB),
          let rgb = color?.converted(to: space, intent: .defaultIntent, options: nil),
          let parts = rgb.components, parts.count >= 3
    else { return "" }
    let byte = { (v: CGFloat) in Int(max(0, min(1, v)) * 255 + 0.5) }
    return String(format: "#%02x%02x%02x", byte(parts[0]), byte(parts[1]), byte(parts[2]))
}

/// The first web address in these words that a meeting is joined by (Teams,
/// Zoom, Meet, Webex...). The app checks it again before it shows a button.
func joinLink(_ words: [String?]) -> String {
    let text = words.compactMap { $0 }.joined(separator: "\n")
    guard let finder = try? NSRegularExpression(pattern: "https://[^\\s<>\"')\\]}]+") else { return "" }
    let hosts = ["teams.microsoft.com", "teams.live.com", "teams.microsoft.us", "zoom.us", "zoomgov.com", "meet.google.com", "webex.com",
                 "whereby.com", "gotomeeting.com", "gotomeet.me", "facetime.apple.com", "meet.jit.si", "chime.aws", "bluejeans.com", "app.slack.com"]
    for match in finder.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
        guard let range = Range(match.range, in: text) else { continue }
        var address = String(text[range])
        while let last = address.last, ".,;:!?".contains(last) { address.removeLast() }
        guard let host = URL(string: address)?.host?.lowercased() else { continue }
        if hosts.contains(where: { host == $0 || host.hasSuffix("." + $0) }) { return address }
    }
    return ""
}

func emit(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value), let text = String(data: data, encoding: .utf8) else {
        print("{\"status\":\"error\"}")
        return
    }
    print(text)
}

let args = CommandLine.arguments
guard args.count >= 2 else {
    emit(["status": "error", "problem": "usage: n2cal status | n2cal events <from> <to> [--ask]"])
    exit(2)
}

switch args[1] {
case "status":
    emit(["status": status()])
case "events":
    guard args.count >= 4, let from = Double(args[2]), let to = Double(args[3]), to > from else {
        emit(["status": "error", "problem": "usage: n2cal events <from> <to> [--ask]"])
        exit(2)
    }
    var state = status()
    if state == "not-asked" && args.contains("--ask") {
        state = ask() ? "allowed" : "denied"
    }
    guard state == "allowed" else {
        emit(["status": state])
        exit(0)
    }
    // A new store: one made before the person said yes sees no calendars.
    let store = EKEventStore()
    let calendars = store.calendars(for: .event)
    let found = store.events(matching: store.predicateForEvents(
        withStart: Date(timeIntervalSince1970: from), end: Date(timeIntervalSince1970: to), calendars: nil))
    emit([
        "status": "allowed",
        "calendars": calendars.map { c in
            ["id": c.calendarIdentifier, "title": c.title, "color": hex(c.cgColor), "source": c.source?.title ?? "", "writable": c.allowsContentModifications]
        },
        "events": found.map { e in
            // A repeat of an event has the id of its series: its start makes it one of a kind.
            let start = Int(e.startDate.timeIntervalSince1970)
            return [
                "id": "\(e.eventIdentifier ?? e.calendarItemIdentifier)@\(start)",
                "cal": e.calendar.calendarIdentifier,
                "title": e.title ?? "",
                "start": start * 1000,
                "end": Int(e.endDate.timeIntervalSince1970) * 1000,
                "allDay": e.isAllDay,
                "location": e.location ?? "",
                "join": joinLink([e.url?.absoluteString, e.location, e.notes]),
            ] as [String: Any]
        },
    ])
case "move":
    guard args.count >= 5, let start = Double(args[3]), let end = Double(args[4]), end > start else {
        emit(["status": "error", "problem": "usage: n2cal move <id> <start> <end>"])
        exit(2)
    }
    guard status() == "allowed" else {
        emit(["status": "error", "problem": "The calendars cannot be read."])
        exit(0)
    }
    // The id is "<the event's id>@<when this one starts>": a repeat has the
    // id of its series, so the start says which one it is.
    let parts = args[2].components(separatedBy: "@")
    guard parts.count >= 2, let was = Double(parts[parts.count - 1]) else {
        emit(["status": "error", "problem": "That event is not known."])
        exit(0)
    }
    let key = parts.dropLast().joined(separator: "@")
    let store = EKEventStore()
    let near = store.predicateForEvents(withStart: Date(timeIntervalSince1970: was - 86_400), end: Date(timeIntervalSince1970: was + 86_400), calendars: nil)
    guard let event = store.events(matching: near).first(where: { ($0.eventIdentifier ?? $0.calendarItemIdentifier) == key && abs($0.startDate.timeIntervalSince1970 - was) < 1 }) else {
        emit(["status": "error", "problem": "That event is not in the calendar any more."])
        exit(0)
    }
    guard event.calendar.allowsContentModifications else {
        emit(["status": "error", "problem": "That calendar cannot be changed."])
        exit(0)
    }
    // A meeting with other people: they would not be told of the change from here.
    guard !event.hasAttendees else {
        emit(["status": "error", "problem": "This meeting has other people in it. Change it in the Calendar app, so that they are told."])
        exit(0)
    }
    event.startDate = Date(timeIntervalSince1970: start)
    event.endDate = Date(timeIntervalSince1970: end)
    do {
        try store.save(event, span: .thisEvent, commit: true)
        emit(["status": "ok"])
    } catch {
        emit(["status": "error", "problem": error.localizedDescription])
    }
case "add":
    guard args.count >= 7, let start = Double(args[3]), let end = Double(args[4]), end > start else {
        emit(["status": "error", "problem": "usage: n2cal add <calendar id> <start> <end> <0|1> <name>"])
        exit(2)
    }
    guard status() == "allowed" else {
        emit(["status": "error", "problem": "The calendars cannot be read."])
        exit(0)
    }
    let store = EKEventStore()
    guard let calendar = store.calendar(withIdentifier: args[2]), calendar.allowsContentModifications else {
        emit(["status": "error", "problem": "That calendar cannot be changed."])
        exit(0)
    }
    let event = EKEvent(eventStore: store)
    event.calendar = calendar
    event.title = args[6...].joined(separator: " ")
    event.isAllDay = args[5] == "1"
    event.startDate = Date(timeIntervalSince1970: start)
    // An all-day event of the Mac ends in its last day, not at the midnight after it.
    event.endDate = Date(timeIntervalSince1970: event.isAllDay ? end - 1 : end)
    do {
        try store.save(event, span: .thisEvent, commit: true)
        emit(["status": "ok"])
    } catch {
        emit(["status": "error", "problem": error.localizedDescription])
    }
default:
    emit(["status": "error", "problem": "usage: n2cal status | events | move | add"])
    exit(2)
}
