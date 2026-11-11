// The same cases as the tests in `src/chart.rs` of the desktop app. When a
// rule changes there, change the matching case here.

import Foundation
import Testing
@testable import Notion2Core

private func d(_ y: Int, _ m: Int, _ day: Int) -> Day {
    Day(year: y, month: m, day: day)!
}

/// Four weeks of runs, as in the mocks.
private func log(dropLast: Bool = false) -> TableData {
    var rows = [
        ["Date", "km", "Time", "Pace"],
        ["Tue 10 Nov", "3.0", "19:30", ""],
        ["Thu 12 Nov", "3.0", "19:12", ""],
        ["Sun 15 Nov", "3.5", "23:05", ""],
        ["Tue 17 Nov", "3.5", "22:10", ""],
        ["Thu 19 Nov", "4.0", "24:48", ""],
        ["Sun 22 Nov", "4.9", "31:30", ""],
        ["Tue 24 Nov", "4.0", "24:40", ""],
        ["Thu 26 Nov", "4.1", "24:36", ""],
        ["Sun 29 Nov", "5.0", "31:15", ""],
        ["Tue 1 Dec", "6.0", "34:00", ""],
    ]
    if dropLast { rows.removeLast() }
    return TableData(rows: rows, columnInfo: [TableColumn(kind: .date), TableColumn(kind: .number, total: .sum), TableColumn(kind: .time), TableColumn(kind: .pace)])
}

/// A small trade log: wins, a loss, and tags.
private func trades(more: [[String]] = [], signed: Bool = false, winColor: TagColor? = nil) -> TableData {
    var outcome = TableColumn(kind: .choice)
    if let winColor { outcome.options = [Choice(name: "Win", color: winColor)] }
    return TableData(
        rows: [["Date", "Outcome", "PnL"], ["2026-11-03", "Loss", "-100"], ["2026-11-02", "Win", "300"], ["2026-11-09", "Win", "50"]] + more,
        columnInfo: [TableColumn(kind: .date), outcome, TableColumn(kind: .number, signed: signed)]
    )
}

@Suite struct ChartTests {
    @Test func kmPerWeek() {
        let today = d(2026, 12, 1)
        let chart = Chart()
        let t = log()
        #expect(chart.columns(t) == (0, 1))
        #expect(chart.heading(t) == "km per week")
        let got = chart.points(t, today: today).map { "\($0.label) \(String(format: "%.1f", $0.value)) \($0.current)" }
        #expect(got == ["9 Nov 9.5 false", "16 Nov 12.4 false", "23 Nov 13.1 false", "30 Nov 6.0 true"])
        #expect(chart.format(t, 12.4) == "12.4")
    }

    @Test func aWeekWithNoRunsIsDrawnEmptyAndTodayIsReached() {
        let points = Chart().points(log(dropLast: true), today: d(2026, 12, 15))
        #expect(points.map(\.label) == ["9 Nov", "16 Nov", "23 Nov", "30 Nov", "7 Dec", "14 Dec"])
        #expect(points[5].value == 0 && points[5].current)
    }

    @Test func paceForEachRunIsDrawnUpsideDown() {
        let t = log()
        let chart = Chart(shape: .line, by: .row, y: "Pace", add: .average)
        #expect(chart.upsideDown(t))
        let points = chart.points(t, today: d(2026, 12, 1))
        #expect(points.count == 10)
        #expect(points[0].label == "Tue 10 Nov")
        #expect(chart.format(t, points[9].value) == "5:40")
        #expect(points[9].current && !points[8].current)
        #expect(chart.heading(t) == "Pace")
    }

    @Test func aRingAndTheDays() {
        let today = d(2026, 12, 1)
        let t = log()
        let ring = Chart(shape: .ring, add: .highest, goal: 10)
        #expect(ring.ring(t) == 6)
        #expect(ring.heading(t) == "Highest km")
        let days = Chart(shape: .days).days(t, today: today)
        #expect(days.count == 28)
        #expect(days[0].date == d(2026, 11, 9))
        #expect(days.filter(\.hasRow).map(\.date.day) == [10, 12, 15, 17, 19, 22, 24, 26, 29, 1])
        #expect(days[22].today && days[23].ahead)
        let count = Chart(add: .count)
        #expect(count.points(t, today: today)[0].value == 3)
        #expect(count.heading(t) == "Rows per week")
    }

    @Test func readFromTheBlockWithTheMacsOtherWords() {
        let chart = Chart.parse(#"{"kind": "line", "table": "Run log", "y": "Pace", "by": "row", "agg": "avg", "title": "Pace per run"}"#)
        #expect(chart.shape == .line && chart.from == "Run log" && chart.by == .row && chart.add == .average && chart.title == "Pace per run")
        #expect(Chart.parse("") == Chart())
        #expect(Chart.parse("[1, 2]") == Chart())
        #expect(Chart.parse(#"{"shape":"hologram","add":"max","cumulative":true,"goal":20}"#) == Chart(add: .highest, goal: 20, running: true))
    }

    @Test func roundTops() {
        #expect(Chart.niceTop(13.1) == 20)
        #expect(Chart.niceTop(19.5) == 20)
        #expect(Chart.niceTop(21) == 25)
        #expect(Chart.niceTop(2040) == 2500)
        #expect(Chart.niceTop(0) == 1)
    }

    @Test func aRunningTotalGoesByDayAndKeepsWhatWasBefore() {
        let today = d(2026, 11, 10)
        let chart = Chart(shape: .line, x: "Date", by: .row, y: "PnL", running: true)
        #expect(chart.points(trades(), today: today).map(\.value) == [300, 200, 250])
        #expect(chart.heading(trades()) == "PnL, running total")
        // More rows than show: the first of those shown still counts all before it.
        let many = trades(more: (0..<40).map { ["2026-10-" + String(format: "%02d", $0 % 28 + 1), "Win", "1"] })
        let points = chart.points(many, today: today)
        #expect(points.count == 30)
        #expect(points.last?.value == 290)
    }

    @Test func lossesDrawBelowZeroAndASignedColumnSaysSo() {
        let chart = Chart(shape: .bars, x: "Date", by: .row, y: "PnL")
        #expect(chart.points(trades(), today: d(2026, 11, 10)).map(\.value) == [300, -100, 50], "a loss stays below zero")
        #expect(!chart.signed(trades()))
        #expect(chart.signed(trades(signed: true)))
        var count = chart
        count.add = .count
        #expect(!count.signed(trades(signed: true)), "a count has no sign")
    }

    @Test func aPieCountsEachTagInItsColour() {
        let t = trades(winColor: .green)
        let chart = Chart(shape: .pie, x: "Outcome", add: .count)
        let slices = chart.slices(t)
        #expect(slices.count == 2)
        #expect(slices.first { $0.label == "Win" } == ChartSlice(label: "Win", value: 2, color: .green))
        #expect(chart.heading(t) == "Rows by outcome")
        // With no column named, the first column of tags.
        #expect(Chart(shape: .pie, add: .count).columns(t).x == 1)
    }
}
