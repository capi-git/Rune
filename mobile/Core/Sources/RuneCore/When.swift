// Days and times for the calendar, in a time zone (the device's by default).
// Mirrors `src/when.rs` of the desktop app.

import Foundation

/// A calendar day (Gregorian) with no time and no time zone.
public struct Day: Hashable, Comparable, Sendable, CustomStringConvertible {
    public let year: Int
    public let month: Int
    public let day: Int

    /// Nil when there is no such day (30 February, month 13).
    public init?(year: Int, month: Int, day: Int) {
        guard (1...12).contains(month), day >= 1, day <= Day.length(ofMonth: month, year: year) else { return nil }
        self.year = year
        self.month = month
        self.day = day
    }

    /// The day `ordinal` days after 1 January 1970.
    public init(ordinal: Int) {
        // Civil-from-days (Howard Hinnant's algorithm).
        let z = ordinal + 719_468
        let era = (z >= 0 ? z : z - 146_096) / 146_097
        let doe = z - era * 146_097
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100)
        let mp = (5 * doy + 2) / 153
        let d = doy - (153 * mp + 2) / 5 + 1
        let m = mp < 10 ? mp + 3 : mp - 9
        self.year = yoe + era * 400 + (m <= 2 ? 1 : 0)
        self.month = m
        self.day = d
    }

    /// Days since 1 January 1970.
    public var ordinal: Int {
        let y = month <= 2 ? year - 1 : year
        let era = (y >= 0 ? y : y - 399) / 400
        let yoe = y - era * 400
        let doy = (153 * (month > 2 ? month - 3 : month + 9) + 2) / 5 + day - 1
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
        return era * 146_097 + doe - 719_468
    }

    public static func isLeap(_ year: Int) -> Bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    public static func length(ofMonth month: Int, year: Int) -> Int {
        switch month {
        case 2: isLeap(year) ? 29 : 28
        case 4, 6, 9, 11: 30
        default: 31
        }
    }

    public func adding(days: Int) -> Day {
        Day(ordinal: ordinal + days)
    }

    /// The first day of the month `months` after (or before) this day's month.
    public func firstOfMonth(adding months: Int = 0) -> Day {
        let m = year * 12 + (month - 1) + months
        let y = Int((Double(m) / 12).rounded(.down))
        return Day(year: y, month: m - y * 12 + 1, day: 1) ?? self
    }

    /// 0 for Monday … 6 for Sunday.
    public var weekdayFromMonday: Int {
        // 1 January 1970 was a Thursday.
        (((ordinal % 7) + 7) + 3) % 7
    }

    /// 1 for Sunday … 7 for Saturday (as `Calendar` counts weekdays).
    public var weekday: Int {
        (weekdayFromMonday + 1) % 7 + 1
    }

    /// "2026-11-10", the form days are stored in.
    public var value: String {
        let y = year < 0 ? "-" + String(format: "%04d", -year) : String(format: "%04d", year)
        return y + "-" + String(format: "%02d", month) + "-" + String(format: "%02d", day)
    }

    public var description: String { value }

    /// Reads "YYYY-MM-DD".
    public static func parse(_ text: String) -> Day? {
        let parts = text.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: "-", omittingEmptySubsequences: false)
        guard parts.count == 3,
              parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy(\.isASCIIDigit) }),
              let y = Int(parts[0]), let m = Int(parts[1]), let d = Int(parts[2])
        else { return nil }
        return Day(year: y, month: m, day: d)
    }

    public static func < (a: Day, b: Day) -> Bool {
        (a.year, a.month, a.day) < (b.year, b.month, b.day)
    }
}

private extension Character {
    var isASCIIDigit: Bool { isASCII && isNumber }
}

/// Turns stored times (milliseconds since 1970) into days and back, in one
/// time zone.
public struct When: Sendable {
    public static let minute: Int64 = 60_000
    public static let hour: Int64 = 60 * minute

    public let calendar: Calendar

    public init(timeZone: TimeZone = .current) {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = timeZone
        self.calendar = calendar
    }

    public static func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: Double(ms) / 1000)
    }

    public static func ms(_ date: Date) -> Int64 {
        Int64((date.timeIntervalSince1970 * 1000).rounded(.down))
    }

    /// The day a time falls on.
    public func day(of ms: Int64) -> Day {
        let c = calendar.dateComponents([.year, .month, .day], from: When.date(ms))
        return Day(year: c.year ?? 1970, month: c.month ?? 1, day: c.day ?? 1) ?? Day(ordinal: 0)
    }

    public func today(now: Date = Date()) -> Day {
        day(of: When.ms(now))
    }

    /// Hour and minute of a time.
    public func time(of ms: Int64) -> (hour: Int, minute: Int) {
        let c = calendar.dateComponents([.hour, .minute], from: When.date(ms))
        return (c.hour ?? 0, c.minute ?? 0)
    }

    /// Minutes since the day began.
    public func minutesIntoDay(_ ms: Int64) -> Int {
        let t = time(of: ms)
        return t.hour * 60 + t.minute
    }

    /// The time on `day` at `hour`:`minute`. When the clocks skip that time
    /// it is the next time that exists; when they repeat it, the first one.
    public func at(_ day: Day, hour: Int = 0, minute: Int = 0) -> Int64 {
        var c = DateComponents()
        c.year = day.year
        c.month = day.month
        c.day = day.day
        c.hour = hour
        c.minute = minute
        c.second = 0
        guard let date = calendar.date(from: c) else { return 0 }
        return When.ms(date)
    }

    public func midnight(_ day: Day) -> Int64 {
        at(day)
    }
}
