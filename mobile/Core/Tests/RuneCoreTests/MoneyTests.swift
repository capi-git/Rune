// The same cases as the tests in `src/table/money.rs` and the cell reading
// tests of `src/table.rs` of the desktop app. When a rule changes there,
// change the matching case here.

import Foundation
import Testing
@testable import RuneCore

private func d(_ y: Int, _ m: Int, _ day: Int) -> Day {
    Day(year: y, month: m, day: day)!
}

@Suite struct MoneyTests {
    @Test func numbersAreWrittenByTheirFormat() {
        #expect(Money.write("USD", 3500, places: nil) == "$3,500.00")
        #expect(Money.write("USD", -65.5, places: nil) == "-$65.50")
        #expect(Money.write("JPY", 1500.4, places: nil) == "¥1,500")
        #expect(Money.write("CHF", 20, places: nil) == "CHF 20.00")
        #expect(Money.write("EUR", 1234567.891, places: 1) == "€1,234,567.9")
        #expect(Money.write(Money.grouped, 1500, places: nil) == "1,500")
        #expect(Money.write(Money.grouped, 999, places: 2) == "999.00")
        #expect(Money.write(Money.percent, 15, places: nil) == "15%")
        #expect(Money.write("", 6.25, places: 1) == "6.2")
        #expect(Money.write("USD", -0.001, places: nil) == "$0.00")
        #expect(Money.write("???", 2, places: nil) == "2")
    }

    @Test func aTypedSignNamesTheFormat() {
        #expect(Money.typedFormat("$3,500") == "USD")
        #expect(Money.typedFormat(" -$65.00 ") == "USD")
        #expect(Money.typedFormat("$-65") == "USD")
        #expect(Money.typedFormat("CA$ 12") == "CAD")
        #expect(Money.typedFormat("€20") == "EUR")
        #expect(Money.typedFormat("20 €") == "EUR")
        #expect(Money.typedFormat("20 eur") == "EUR")
        #expect(Money.typedFormat("£4.50") == "GBP")
        #expect(Money.typedFormat("¥1500") == "JPY")
        #expect(Money.typedFormat("15%") == Money.percent)
        #expect(Money.typedFormat("15 %") == Money.percent)
        // Not money: a plain number, words, a sign alone, a sign in the middle.
        for text in ["3500", "", "$", "%", "$ a lot", "about $5", "5 dollars", "A$AP", "100% sure", "eur", "€€5"] {
            #expect(Money.typedFormat(text) == nil, "\(text)")
        }
    }

    @Test func signsComeOffBeforeANumberIsRead() {
        #expect(Money.stripSign("$3,500.00") == (false, "3,500.00"))
        #expect(Money.stripSign("-$65") == (true, "65"))
        #expect(Money.stripSign("$-65") == (true, "65"))
        #expect(Money.stripSign("CHF 20") == (false, "20"))
        #expect(Money.stripSign("  6.5 km") == (false, "6.5 km"))
        #expect(Money.stripSign("+4") == (false, "4"))
        #expect(Money.stripSign("words") == (false, "words"))
        #expect(Money.knownFormat("usd") == "USD")
        #expect(Money.knownFormat("Percent") == Money.percent)
        #expect(Money.knownFormat("km") == nil)
        #expect(Money.formatLabel("EUR") == "Euro")
        #expect(Money.formatLabel("") == "Number")
    }

    @Test func cellsAreReadByTheirKind() {
        #expect(CellText.number("6") == 6)
        #expect(CellText.number(" 6.5 km") == 6.5)
        #expect(CellText.number("6,5") == 6.5)
        #expect(CellText.number("1,500") == 1500)
        #expect(CellText.number("km") == nil)
        #expect(CellText.number("$3,500.00") == 3500)
        #expect(CellText.number("-$65.50") == -65.5)
        #expect(CellText.number("€ 20") == 20)
        #expect(CellText.number("-4") == -4)
        #expect(CellText.number("15%") == 15)
        #expect(CellText.number("$") == nil)
        #expect(CellText.number("Nokia 3310") == nil)
        func decimals(_ text: String) -> [Int] { Money.read(text).map { [$0.typed, $0.needed] } ?? [] }
        #expect([decimals("3.50"), decimals("6,5"), decimals("1,500"), decimals("$1,234.5"), decimals("7")] == [[2, 1], [1, 1], [0, 0], [1, 1], [0, 0]])
        #expect(CellText.parts(" Home, Food ,, ") == ["Home", "Food"])
        #expect(CellText.seconds("34:00") == 2040)
        #expect(CellText.seconds("1:02:30") == 3750)
        #expect(CellText.seconds("34 minutes") == 2040)
        #expect(CellText.seconds("1h 5m") == 3900)
        #expect(CellText.seconds("34") == 2040)
        #expect(CellText.seconds("soon") == nil)
        #expect(CellText.clock(2040) == "34:00")
        #expect(CellText.clock(15286) == "4:14:46")
        #expect(CellText.checked("x") && CellText.checked("Yes") && !CellText.checked("") && !CellText.checked("no"))
        let today = d(2026, 12, 1)
        #expect(CellText.day("Tue 1 Dec", today: today) == today)
        #expect(CellText.day("10 Nov", today: today) == d(2026, 11, 10))
        #expect(CellText.day("Dec 1, 2025", today: today) == d(2025, 12, 1))
        #expect(CellText.day("2027-01-03", today: today) == d(2027, 1, 3))
        #expect(CellText.day("3 January", today: today) == d(2027, 1, 3))
        // Without a year: the nearest such day, leaning to days gone by.
        #expect(CellText.day("20 Jun", today: today) == d(2026, 6, 20))
        #expect(CellText.day("1 Mar", today: d(2026, 9, 1)) == d(2026, 3, 1))
        #expect(CellText.day("yesterday", today: today) == d(2026, 11, 30))
        #expect(CellText.day("soon", today: today) == nil)
        #expect(CellText.formatDay(today, today: today) == "Tue 1 Dec")
        #expect(CellText.formatDay(d(2027, 1, 3), today: today) == "Sun 3 Jan 2027")
    }

    @Test func numbersAreReadTheWayTheyWereMeant() {
        // A space, a comma or a point between the thousands; the last of a
        // point and a comma is the decimal point.
        let cases: [(String, Double)] = [
            ("$1 500", 1500), ("1 234 567", 1_234_567), ("1 234,56 €", 1234.56), ("€1.234,56", 1234.56), ("1,234.56", 1234.56),
            ("1.234.567", 1_234_567), ("12,345,678", 12_345_678), ("1,500", 1500), ("6,5", 6.5), (".5", 0.5), ("5.", 5),
            ("2 bags", 2), ("3/10", 3), ("5 2026", 5), ("2 10", 2),
        ]
        for (text, value) in cases {
            #expect(CellText.number(text) == value, "\(text)")
        }
        for text in ["1.2.3", "192.168.1.1", "2026.11.10", "1,2.3,4", ",", ".", "", "km"] {
            #expect(CellText.number(text) == nil, "\(text)")
        }
        // One number and no more, or a number at the start of words.
        for text in ["$3,500", "20 €", "20 eur", "15 %", "-$65.00", "6.5 km", "1 500", "5."] {
            #expect(Money.wholeNumber(text, unit: "km") != nil, "\(text)")
        }
        for text in ["2 bags", "3/10", "6.5 km easy", "1 50", "10k", "5 of 7"] {
            #expect(Money.wholeNumber(text, unit: "km") == nil, "\(text)")
        }
    }

    @Test func wordsWithAMonthInThemAreNoDay() {
        let today = d(2026, 11, 10)
        for text in ["Tue 1 Dec", "1 December", "Dec 1, 2026", "2026-12-01", "1st of December", "the 3rd of march", "sept 4", "Thurs 12 nov", "today"] {
            #expect(CellText.wholeDay(text, today: today) != nil, "\(text)")
        }
        for text in ["Mark 3 done", "Decide on 2 options", "Marathon 10k", "May 3 be late", "3 dec (race)", "December", "12", ""] {
            #expect(CellText.wholeDay(text, today: today) == nil, "\(text)")
        }
        // In a date column a cell that says more than its day still has its day.
        #expect(CellText.day("3 dec (race)", today: today) == d(2026, 12, 3))
    }

    @Test func aCellOfSeveralTakesANameWithAComma() {
        #expect(CellText.holdsPart("Ana, smith, john", "Smith, John") && CellText.holdsPart("Ana, Smith, John", "ana"))
        #expect(!CellText.holdsPart("Ana, Smith", "Smith, John") && !CellText.holdsPart("", "Ana") && !CellText.holdsPart("Ana", ""))
    }
}
