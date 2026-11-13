// Repeating calendar events. Mirrors `src/repeat.rs` of the desktop app: the
// same stored form and the same days, so both apps show the same calendar.
//
// A repeating event is stored once: its first time, plus a rule. Each repeat
// is worked out when it is shown and has an id of its own,
// "<event id>@<YYYY-MM-DD>".

import Foundation

public struct Repeat: Codable, Hashable, Sendable {
    public enum Every: String, Codable, Sendable {
        case day
        /// Monday to Friday.
        case weekday
        case week
        case month
        case year
    }

    public var every: Every
    /// Every `interval` days, weeks, months or years (not used for weekdays).
    public var interval: Int
    /// The last day a repeat may fall on ("YYYY-MM-DD"); nil: no end.
    public var until: String?
    /// Days ("YYYY-MM-DD") whose repeat was deleted, or changed on its own.
    public var skip: [String]

    public init(every: Every, interval: Int = 1, until: String? = nil, skip: [String] = []) {
        self.every = every
        self.interval = max(interval, 1)
        self.until = until
        self.skip = skip
    }

    enum CodingKeys: String, CodingKey {
        case every, interval, until, skip
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        every = try c.decode(Every.self, forKey: .every)
        interval = try c.decodeIfPresent(Int.self, forKey: .interval) ?? 1
        // The desktop stores an unsigned number; a negative one is not a rule.
        guard interval >= 0 else {
            throw DecodingError.dataCorruptedError(forKey: .interval, in: c, debugDescription: "negative interval")
        }
        until = try c.decodeIfPresent(String.self, forKey: .until)
        skip = try c.decodeIfPresent([String].self, forKey: .skip) ?? []
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(every, forKey: .every)
        try c.encode(interval, forKey: .interval)
        try c.encodeIfPresent(until, forKey: .until)
        if !skip.isEmpty {
            try c.encode(skip, forKey: .skip)
        }
    }

    /// The rule a stored value means ("" and anything unreadable: none).
    public static func fromJSON(_ text: String) -> Repeat? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let data = trimmed.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(Repeat.self, from: data)
    }

    public var untilDay: Day? {
        until.flatMap(Day.parse)
    }

    func skips(_ day: Day) -> Bool {
        skip.contains(day.value)
    }

    // MARK: Days

    /// The days from `from` to `to` (both included) on which a rule for an
    /// event first held on `first` falls.
    public static func days(first: Day, rule: Repeat, from: Day, to: Day) -> [Day] {
        let from = max(from, first)
        let to = rule.untilDay.map { min(to, $0) } ?? to
        var out: [Day] = []
        if from > to {
            return out
        }
        let n = max(rule.interval, 1)
        switch rule.every {
        case .day, .week:
            let step = rule.every == .day ? n : 7 * n
            let gap = from.ordinal - first.ordinal
            var k = (gap + step - 1) / step
            while true {
                let d = first.adding(days: k * step)
                if d > to { break }
                out.append(d)
                k += 1
            }
        case .weekday:
            var d = from
            while d <= to {
                if d.weekdayFromMonday < 5 {
                    out.append(d)
                }
                d = d.adding(days: 1)
            }
        case .month, .year:
            let step = rule.every == .month ? n : 12 * n
            func monthNumber(_ d: Day) -> Int { d.year * 12 + (d.month - 1) }
            var k = max(monthNumber(from) - monthNumber(first), 0) / step
            while true {
                let m = monthNumber(first) + k * step
                let y = Int((Double(m) / 12).rounded(.down))
                let month = m - y * 12 + 1
                guard let start = Day(year: y, month: month, day: 1), start <= to else { break }
                // A month without that day (the 31st, 29 February) is skipped.
                if let d = Day(year: y, month: month, day: first.day), d >= from, d <= to {
                    out.append(d)
                }
                k += 1
            }
        }
        out.removeAll { rule.skips($0) }
        return out
    }

    // MARK: Occurrences

    public static func instanceID(series: ID, day: Day) -> ID {
        "\(series)@\(day.value)"
    }

    /// The stored event an id belongs to, and the day of the repeat it names.
    public static func splitID(_ id: ID) -> (series: ID, day: Day?) {
        guard let at = id.firstIndex(of: "@") else { return (id, nil) }
        return (String(id[..<at]), Day.parse(String(id[id.index(after: at)...])))
    }

    /// How many days an all-day event covers.
    static func dayCount(_ e: Event, _ when: When) -> Int {
        max(when.day(of: e.end - 1).ordinal - when.day(of: e.start).ordinal + 1, 1)
    }

    /// The repeat of `e` on day `d` (whether or not the rule falls on it).
    public static func at(_ e: Event, _ d: Day, when: When = When()) -> Event {
        var o = e
        o.id = instanceID(series: e.id, day: d)
        if e.allDay {
            o.start = when.midnight(d)
            o.end = when.midnight(d.adding(days: dayCount(e, when)))
        } else {
            let t = when.time(of: e.start)
            o.start = when.at(d, hour: t.hour, minute: t.minute)
            o.end = o.start + (e.end - e.start)
        }
        return o
    }

    /// Each time `e` happens that overlaps `from..<to` (ms). A one-off event
    /// comes back as it is; repeats get ids of their own.
    public static func occurrences(of e: Event, from: Int64, to: Int64, when: When = When()) -> [Event] {
        guard let rule = e.repeatRule else {
            return e.overlaps(from, to) ? [e] : []
        }
        if to <= from {
            return []
        }
        // A repeat that starts a little before `from` may still run into it.
        let span = max(when.day(of: e.end - 1).ordinal - when.day(of: e.start).ordinal, 0) + 1
        let lo = when.day(of: from).adding(days: -span)
        let hi = when.day(of: to - 1)
        return days(first: when.day(of: e.start), rule: rule, from: lo, to: hi)
            .map { at(e, $0, when: when) }
            .filter { $0.overlaps(from, to) }
    }

    /// Every event and repeat that overlaps `from..<to`, in order of start.
    public static func expand(_ events: [Event], from: Int64, to: Int64, when: When = When()) -> [Event] {
        events
            .flatMap { occurrences(of: $0, from: from, to: to, when: when) }
            .sorted { ($0.start, $0.end, $0.id) < ($1.start, $1.end, $1.id) }
    }

    // MARK: Words

    static func ordinal(_ n: Int) -> String {
        let suffix: String
        switch (n % 10, n % 100) {
        case (_, 11...13): suffix = "th"
        case (1, _): suffix = "st"
        case (2, _): suffix = "nd"
        case (3, _): suffix = "rd"
        default: suffix = "th"
        }
        return "\(n)\(suffix)"
    }

    static let weekdayNames = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
    static let monthNames = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ]

    /// Words for a rule: "Every week on Tuesday, until Dec 20, 2026".
    public func describe(first: Day) -> String {
        let weekday = Repeat.weekdayNames[first.weekdayFromMonday]
        let monthDay = "\(Repeat.monthNames[first.month - 1]) \(first.day)"
        let base: String
        switch (every, max(interval, 1)) {
        case (.weekday, _): base = "Every weekday"
        case (.day, 1): base = "Every day"
        case (.day, let n): base = "Every \(n) days"
        case (.week, 1): base = "Every week on \(weekday)"
        case (.week, let n): base = "Every \(n) weeks on \(weekday)"
        case (.month, 1): base = "Every month on the \(Repeat.ordinal(first.day))"
        case (.month, let n): base = "Every \(n) months on the \(Repeat.ordinal(first.day))"
        case (.year, 1): base = "Every year on \(monthDay)"
        case (.year, let n): base = "Every \(n) years on \(monthDay)"
        }
        guard let u = untilDay else { return base }
        let month = String(Repeat.monthNames[u.month - 1].prefix(3))
        return "\(base), until \(month) \(u.day), \(u.year)"
    }
}
