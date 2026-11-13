// Words for dates, times and reminders. The wording follows the desktop app
// (`src/when.rs`, `src/notify.rs`); times follow the iPhone's own format.

import Foundation
import RuneCore

enum Labels {
    static func date(_ day: Day, _ when: When) -> Date {
        When.date(when.at(day, hour: 12))
    }

    /// "Monday, November 9".
    static func longDate(_ day: Day, _ when: When) -> String {
        date(day, when).formatted(.dateTime.weekday(.wide).month(.wide).day())
    }

    /// "Today", "Tomorrow", "Thursday" (in the next week), else "Nov 20".
    static func dayName(_ day: Day, today: Day, _ when: When) -> String {
        switch day.ordinal - today.ordinal {
        case 0: "Today"
        case 1: "Tomorrow"
        case -1: "Yesterday"
        case 2...6: date(day, when).formatted(.dateTime.weekday(.wide))
        default:
            day.year == today.year
                ? date(day, when).formatted(.dateTime.month(.abbreviated).day())
                : date(day, when).formatted(.dateTime.month(.abbreviated).day().year())
        }
    }

    /// "2:00 PM".
    static func time(_ ms: Int64) -> String {
        When.date(ms).formatted(date: .omitted, time: .shortened)
    }

    /// "2:00", the short form beside an event.
    static func shortTime(_ ms: Int64) -> String {
        When.date(ms).formatted(Date.FormatStyle().hour(.defaultDigits(amPM: .omitted)).minute())
    }

    /// "2:00 – 3:00 PM".
    static func timeRange(_ start: Int64, _ end: Int64) -> String {
        (When.date(start)..<When.date(end)).formatted(.interval.hour().minute())
    }

    /// When an event is, on the day it is shown: "All day", "2:00 – 3:00 PM",
    /// or with the days for one that runs over midnight.
    static func span(of event: Event, _ when: When) -> String {
        let first = when.day(of: event.start)
        let last = when.day(of: event.end - 1)
        if event.allDay {
            if first == last {
                return "All day"
            }
            let range = date(first, when)..<date(last, when)
            return range.formatted(.interval.month(.abbreviated).day())
        }
        if first == last {
            return timeRange(event.start, event.end)
        }
        return (When.date(event.start)..<When.date(event.end)).formatted(.interval.month(.abbreviated).day().hour().minute())
    }

    /// "just now", "5 min ago", "3 hr ago", "Yesterday", "Nov 3".
    static func ago(_ ms: Int64, now: Date = Date(), _ when: When) -> String {
        let diff = When.ms(now) - ms
        let day = when.day(of: ms)
        let today = when.today(now: now)
        if diff < When.minute {
            return "just now"
        }
        if diff < When.hour {
            return "\(diff / When.minute) min ago"
        }
        if day == today {
            return "\(diff / When.hour) hr ago"
        }
        if day.adding(days: 1) == today {
            return "Yesterday"
        }
        return date(day, when).formatted(.dateTime.month(.abbreviated).day())
    }

    /// "10 min before".
    static func reminder(_ minutes: Int) -> String {
        switch minutes {
        case ...0: "At start"
        case 60: "1 hour before"
        case 24 * 60: "1 day before"
        case let m where m % (24 * 60) == 0: "\(m / (24 * 60)) days before"
        case let m where m % 60 == 0: "\(m / 60) hours before"
        default: "\(minutes) min before"
        }
    }

    static func greeting(now: Date = Date(), _ when: When) -> String {
        switch when.time(of: When.ms(now)).hour {
        case 5..<12: "Good morning"
        case 12..<18: "Good afternoon"
        default: "Good evening"
        }
    }
}
