// The same cases as the tests in `src/repeat.rs` and `src/when.rs` of the
// desktop app, plus the days the clocks change. When a rule changes there,
// change the matching case here.

import Foundation
import Testing
@testable import Notion2Core

private func d(_ y: Int, _ m: Int, _ day: Int) -> Day {
    Day(year: y, month: m, day: day)!
}

private let newYork = When(timeZone: TimeZone(identifier: "America/New_York")!)
private let utc = When(timeZone: TimeZone(identifier: "UTC")!)

@Suite struct DayTests {
    @Test func daysCountFrom1970() {
        #expect(d(1970, 1, 1).ordinal == 0)
        #expect(Day(ordinal: 0) == d(1970, 1, 1))
        #expect(d(2026, 11, 9).ordinal == 20_766)
        for ordinal in stride(from: -800_000, through: 800_000, by: 997) {
            #expect(Day(ordinal: ordinal).ordinal == ordinal)
        }
        #expect(d(2024, 2, 28).adding(days: 1) == d(2024, 2, 29))
        #expect(d(2024, 2, 29).adding(days: 1) == d(2024, 3, 1))
        #expect(d(2026, 1, 1).adding(days: -1) == d(2025, 12, 31))
        #expect(Day(year: 2026, month: 2, day: 29) == nil)
        #expect(Day(year: 2026, month: 13, day: 1) == nil)
    }

    @Test func weekdays() {
        #expect(d(2026, 11, 9).weekdayFromMonday == 0, "a Monday")
        #expect(d(2026, 11, 8).weekdayFromMonday == 6, "a Sunday")
        #expect(d(2026, 11, 8).weekday == 1)
        #expect(d(2026, 11, 14).weekday == 7)
        #expect(d(1969, 12, 31).weekdayFromMonday == 2, "a Wednesday, before 1970")
    }

    @Test func storedForm() {
        #expect(d(2026, 11, 9).value == "2026-11-09")
        #expect(Day.parse("2026-11-09") == d(2026, 11, 9))
        #expect(Day.parse(" 2026-01-05 ") == d(2026, 1, 5))
        #expect(Day.parse("2026-02-30") == nil)
        #expect(Day.parse("not a date") == nil)
        #expect(d(2026, 11, 9).firstOfMonth(adding: 2) == d(2027, 1, 1))
        #expect(d(2026, 11, 9).firstOfMonth(adding: -11) == d(2025, 12, 1))
    }

    @Test func timesInAZone() {
        let t = newYork.at(d(2026, 11, 10), hour: 14, minute: 30)
        #expect(newYork.day(of: t) == d(2026, 11, 10))
        #expect(newYork.minutesIntoDay(t) == 14 * 60 + 30)
        #expect(utc.time(of: t).hour == 19, "New York is 5 hours behind in November")
        // 23:30 in New York is already the next day in UTC.
        let late = newYork.at(d(2026, 11, 10), hour: 23, minute: 30)
        #expect(utc.day(of: late) == d(2026, 11, 11))
    }

    @Test func clocksThatSkipOrRepeatAnHour() {
        // 8 March 2026: 2:00 to 3:00 does not exist in New York.
        let skipped = newYork.at(d(2026, 3, 8), hour: 2, minute: 30)
        #expect(newYork.day(of: skipped) == d(2026, 3, 8))
        #expect(newYork.time(of: skipped).hour == 3)
        // 1 November 2026: 1:30 happens twice; the first one is used.
        let twice = newYork.at(d(2026, 11, 1), hour: 1, minute: 30)
        #expect(twice == newYork.midnight(d(2026, 11, 1)) + 90 * When.minute)
        // That day has 25 hours.
        #expect(newYork.midnight(d(2026, 11, 2)) - newYork.midnight(d(2026, 11, 1)) == 25 * When.hour)
    }
}

@Suite struct RepeatTests {
    @Test func rulesFallOnTheRightDays() {
        let weekly = Repeat(every: .week)
        #expect(Repeat.days(first: d(2026, 11, 3), rule: weekly, from: d(2026, 11, 1), to: d(2026, 11, 30))
            == [d(2026, 11, 3), d(2026, 11, 10), d(2026, 11, 17), d(2026, 11, 24)])
        let fortnight = Repeat(every: .week, interval: 2)
        #expect(Repeat.days(first: d(2026, 1, 6), rule: fortnight, from: d(2026, 11, 1), to: d(2026, 11, 30))
            == [d(2026, 11, 10), d(2026, 11, 24)])
        let weekdays = Repeat(every: .weekday)
        #expect(Repeat.days(first: d(2026, 11, 1), rule: weekdays, from: d(2026, 11, 6), to: d(2026, 11, 9))
            == [d(2026, 11, 6), d(2026, 11, 9)])
        // The 31st is skipped in months without one.
        let monthly = Repeat(every: .month)
        #expect(Repeat.days(first: d(2026, 1, 31), rule: monthly, from: d(2026, 1, 1), to: d(2026, 5, 31))
            == [d(2026, 1, 31), d(2026, 3, 31), d(2026, 5, 31)])
        let yearly = Repeat(every: .year)
        #expect(Repeat.days(first: d(2024, 2, 29), rule: yearly, from: d(2024, 1, 1), to: d(2029, 1, 1))
            == [d(2024, 2, 29), d(2028, 2, 29)])
        // An end date and deleted repeats.
        let daily = Repeat(every: .day, until: "2026-11-05", skip: ["2026-11-03"])
        #expect(Repeat.days(first: d(2026, 11, 1), rule: daily, from: d(2026, 10, 1), to: d(2026, 12, 1))
            == [d(2026, 11, 1), d(2026, 11, 2), d(2026, 11, 4), d(2026, 11, 5)])
        // Nothing before the first day.
        #expect(Repeat.days(first: d(2026, 11, 20), rule: weekly, from: d(2026, 11, 1), to: d(2026, 11, 19)).isEmpty)
    }

    @Test func everyFewDaysAndMonths() {
        let everyThird = Repeat(every: .day, interval: 3)
        #expect(Repeat.days(first: d(2026, 11, 1), rule: everyThird, from: d(2026, 11, 5), to: d(2026, 11, 13))
            == [d(2026, 11, 7), d(2026, 11, 10), d(2026, 11, 13)])
        let quarterly = Repeat(every: .month, interval: 3)
        #expect(Repeat.days(first: d(2026, 1, 15), rule: quarterly, from: d(2026, 6, 1), to: d(2027, 2, 1))
            == [d(2026, 7, 15), d(2026, 10, 15), d(2027, 1, 15)])
        let everyOtherYear = Repeat(every: .year, interval: 2)
        #expect(Repeat.days(first: d(2020, 5, 5), rule: everyOtherYear, from: d(2025, 1, 1), to: d(2029, 1, 1))
            == [d(2026, 5, 5), d(2028, 5, 5)])
    }

    @Test func repeatsKeepTheirTimeAndFindTheirEvent() {
        let start = newYork.at(d(2026, 11, 3), hour: 13)
        let e = Event(id: "ev", spaceID: "s", title: "Standup", start: start, end: start + When.hour, repeatRule: Repeat(every: .week))
        let from = newYork.midnight(d(2026, 11, 9))
        let to = newYork.midnight(d(2026, 11, 16))
        let shown = Repeat.occurrences(of: e, from: from, to: to, when: newYork)
        #expect(shown.count == 1)
        #expect(shown[0].id == "ev@2026-11-10")
        #expect(newYork.time(of: shown[0].start).hour == 13)
        #expect(shown[0].end - shown[0].start == When.hour)
        #expect(shown[0].seriesID == "ev")
        #expect(Repeat.splitID(shown[0].id).day == d(2026, 11, 10))
        #expect(Repeat.splitID("ev").day == nil)
    }

    @Test func aRepeatKeepsItsClockTimeWhenTheClocksChange() {
        // First held in summer time; shown after the clocks went back.
        let start = newYork.at(d(2026, 10, 27), hour: 9, minute: 30)
        let e = Event(id: "ev", start: start, end: start + 30 * When.minute, repeatRule: Repeat(every: .week))
        let from = newYork.midnight(d(2026, 11, 3))
        let shown = Repeat.occurrences(of: e, from: from, to: from + 24 * When.hour, when: newYork)
        #expect(shown.count == 1)
        #expect(newYork.minutesIntoDay(shown[0].start) == 9 * 60 + 30)
        #expect(shown[0].start - start == 7 * 24 * When.hour + When.hour, "one hour more than seven days")
    }

    @Test func allDayRepeatsCoverWholeDays() {
        let first = d(2026, 11, 2)
        let e = Event(
            id: "trip", title: "Trip",
            start: newYork.midnight(first), end: newYork.midnight(d(2026, 11, 4)),
            allDay: true, repeatRule: Repeat(every: .month)
        )
        let shown = Repeat.occurrences(of: e, from: newYork.midnight(d(2026, 12, 3)), to: newYork.midnight(d(2026, 12, 4)), when: newYork)
        #expect(shown.count == 1, "a two-day repeat that began the day before still shows")
        #expect(newYork.day(of: shown[0].start) == d(2026, 12, 2))
        #expect(shown[0].end == newYork.midnight(d(2026, 12, 4)))
    }

    @Test func oneOffEventsComeBackAsTheyAre() {
        let start = utc.at(d(2026, 11, 9), hour: 10)
        let e = Event(id: "once", start: start, end: start + When.hour)
        #expect(Repeat.occurrences(of: e, from: start, to: start + 1, when: utc) == [e])
        #expect(Repeat.occurrences(of: e, from: start + When.hour, to: start + 2 * When.hour, when: utc).isEmpty)
        let later = Event(id: "b", start: start + When.hour, end: start + 2 * When.hour, repeatRule: Repeat(every: .day))
        let all = Repeat.expand([later, e], from: utc.midnight(d(2026, 11, 9)), to: utc.midnight(d(2026, 11, 11)), when: utc)
        #expect(all.map(\.id) == ["once", "b@2026-11-09", "b@2026-11-10"])
    }

    @Test func storedRules() {
        #expect(Repeat.fromJSON("") == nil)
        #expect(Repeat.fromJSON("  ") == nil)
        #expect(Repeat.fromJSON("not json") == nil)
        #expect(Repeat.fromJSON(#"{"every":"sometimes"}"#) == nil)
        #expect(Repeat.fromJSON(#"{"every":"week","interval":1}"#) == Repeat(every: .week))
        #expect(Repeat.fromJSON(#"{"every":"weekday"}"#) == Repeat(every: .weekday), "the interval may be left out")
        let full = Repeat.fromJSON(#"{"every":"month","interval":2,"until":"2026-12-20","skip":["2026-11-01"],"extra":true}"#)
        #expect(full == Repeat(every: .month, interval: 2, until: "2026-12-20", skip: ["2026-11-01"]))
        #expect(full?.untilDay == d(2026, 12, 20))
        // The desktop writes exactly this form.
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        let data = try! encoder.encode(Repeat(every: .week, interval: 2))
        #expect(String(decoding: data, as: UTF8.self) == #"{"every":"week","interval":2}"#)
    }

    @Test func words() {
        var r = Repeat(every: .month)
        r.until = "2026-12-20"
        #expect(r.describe(first: d(2026, 11, 1)) == "Every month on the 1st, until Dec 20, 2026")
        #expect(Repeat(every: .week).describe(first: d(2026, 11, 10)) == "Every week on Tuesday")
        #expect(Repeat(every: .week, interval: 2).describe(first: d(2026, 11, 9)) == "Every 2 weeks on Monday")
        #expect(Repeat(every: .weekday).describe(first: d(2026, 11, 9)) == "Every weekday")
        #expect(Repeat(every: .day).describe(first: d(2026, 11, 9)) == "Every day")
        #expect(Repeat(every: .year).describe(first: d(2026, 11, 22)) == "Every year on November 22")
        #expect(Repeat(every: .month).describe(first: d(2026, 11, 23)) == "Every month on the 23rd")
        #expect(Repeat(every: .month).describe(first: d(2026, 11, 12)) == "Every month on the 12th")
    }
}
