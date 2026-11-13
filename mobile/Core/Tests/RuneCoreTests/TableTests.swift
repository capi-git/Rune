// The same cases as the tests in `src/table.rs`, `src/table/choice.rs` and
// `src/table/view.rs` of the desktop app that are about reading and showing
// a table (the iPhone does not change tables, so the tables are made here
// as the Mac would have kept them). When a rule changes there, change the
// matching case here.

import Foundation
import Testing
@testable import RuneCore

private func d(_ y: Int, _ m: Int, _ day: Int) -> Day {
    Day(year: y, month: m, day: day)!
}

private func col(_ kind: ColumnKind, total: ColumnTotal? = nil, format: String = "", show: ColumnShow = .number, goal: Double? = nil, formula: String = "", signed: Bool = false) -> TableColumn {
    TableColumn(kind: kind, total: total, format: format, show: show, goal: goal, formula: formula, signed: signed)
}

/// The run log of the mocks: a date, a distance, a time and a pace.
private func log(_ totals: [Int: ColumnTotal] = [:], more: [[String]] = []) -> TableData {
    let kinds: [ColumnKind] = [.date, .choice, .number, .time, .pace, .checkbox]
    return TableData(
        rows: [
            ["Date", "Run", "km", "Time", "Pace", "Stretch"],
            ["Tue 10 Nov", "Easy", "3.0", "19:30", "", "x"],
            ["Thu 12 Nov", "Easy", "3.0", "19:12", "", ""],
            ["Sun 15 Nov", "Long", "3.5", "23:05", "", "x"],
        ] + more,
        columnInfo: kinds.enumerated().map { col($1, total: totals[$0]) }
    )
}

/// The spending of the mocks, as the Mac keeps it once its columns have kinds.
private func spending(_ more: [[String]] = []) -> TableData {
    TableData(
        rows: [
            ["Item", "Amount", "Category", "Date", "Paid"],
            ["Rent", "$2,000.00", "Home", "2026-11-01", "x"],
            ["Internet", "$120.00", "Home", "2026-11-03", "x"],
            ["Groceries", "$150.00", "Food", "2026-11-05", "x"],
            ["Dinner with Rachel", "$65.00", "Food", "2026-11-07", "x"],
            ["Coffee beans", "$56.40", "food", "2026-11-08", "x"],
            ["Cinema", "$32.00", "Fun", "2026-11-08", "x"],
            ["Concert tickets", "$63.00", "Fun", "2026-11-13", ""],
            ["Train to Portland", "$48.00", "Travel", "2026-11-14", ""],
        ] + more,
        columnInfo: [col(.text), col(.number, format: "USD"), col(.choice), col(.date), col(.checkbox)]
    )
}

private let today = d(2026, 11, 20)

private func names(_ t: TableData, _ rows: [Int]) -> [String] {
    rows.map { t.title($0) }
}

private func rule(_ column: Int, _ op: TableRuleOp, _ text: String = "", color: String = "") -> TableRule {
    TableRule(column: column, op: op, text: text, color: color)
}

@Suite struct TableReadingTests {
    @Test func aTableThatCannotBeReadAndOneMadeSquare() throws {
        #expect(TableData.parse("") == nil && TableData.parse("not json") == nil)
        let ragged = try #require(TableData.parse(#"{"rows":[["a"],["b","c","d"],[]]}"#))
        #expect(ragged.rows == [["a", "", ""], ["b", "c", "d"], ["", "", ""]])
        #expect(ragged.columnInfo.isEmpty && ragged.info.isEmpty && ragged.view.isPlain)
        // What a newer Mac may write is passed over, and an unknown kind is words.
        let odd = try #require(TableData.parse(#"{"rows":[["a","b"]],"columns":[{"kind":"hologram","sparkle":1},{"kind":"number","total":"sum","places":"two"}],"later":true}"#))
        #expect(odd.kind(0) == .text && odd.kind(1) == .number && odd.column(1).total == .sum && odd.column(1).places == nil)
    }

    @Test func aViewFromAFileWrittenWrongIsPutRight() throws {
        let t = try #require(TableData.parse(#"{"rows":[["A","B","C"],["1","2","3"]],"view":{"frozen":18446744073709551615,"hidden":[0,7,2,2],"group":40}}"#))
        #expect(t.view.frozen == 2 && t.view.hidden == [2] && t.view.group == nil)
        let odd = try #require(TableData.parse(#"{"rows":[["a","b"]],"view":{"sort":[{"column":7},{"column":1},{"column":1,"down":true}],"hidden":[0,1,1,9],"frozen":5,"group":4}}"#))
        #expect(odd.view.sort.count == 1 && odd.view.hidden == [1] && odd.view.frozen == 1 && odd.view.group == nil)
        #expect(odd.shownColumns == [0])
        // Nesting that makes no sense in a file is put right.
        let nested = try #require(TableData.parse(#"{"rows":[["a"],["b"],["c"]],"info":[{"indent":2},{"indent":3},{"indent":3}]}"#))
        let indents: [Int] = (0..<3).map { nested.indent($0) }
        #expect(indents == [0, 0, 1])
    }

    @Test func paceAndTotals() {
        var t = log()
        #expect(t.shown(1, 4) == "6:30")
        #expect(t.shown(3, 4) == "6:36")
        #expect(t.total(2, today: today) == nil)
        t = log([2: .sum, 3: .sum, 4: .average, 5: .checked, 0: .count])
        #expect(t.total(2, today: today) == TableTotal(word: "Sum", value: "9.5"))
        #expect(t.total(3, today: today) == TableTotal(word: "Sum", value: "1:01:47"))
        #expect(t.total(4, today: today)?.text == "Avg 6:30")
        #expect(t.total(5, today: today)?.text == "2 of 3")
        #expect(t.total(0, today: today)?.text == "3 rows")
        // The other totals.
        let others: [(ColumnTotal, String)] = [(.median, "Med 3.0"), (.range, "Range 0.5"), (.highest, "Max 3.5"), (.lowest, "Min 3.0"), (.unique, "2 different"), (.empty, "0 empty")]
        for (total, text) in others {
            #expect(log([2: total]).total(2, today: today)?.text == text, "\(total)")
        }
        let rest = ["", "Rest", "", "", "", ""]
        #expect(log([2: .empty], more: [rest]).total(2, today: today)?.text == "1 empty")
        var five = rest
        five[2] = "5"
        #expect(log([2: .median], more: [five]).total(2, today: today)?.text == "Med 3.2")
    }

    @Test func aTableWithNothingInItsRowsHasNoTotals() {
        var t = TableData(rows: [["Date", "P&L", "Rules kept"], ["", "", ""]], columnInfo: [col(.date, total: .count), col(.number, total: .sum), col(.checkbox, total: .checked)])
        #expect(t.total(0, today: today) == nil && t.total(1, today: today) == nil && t.total(2, today: today) == nil)
        t.rows[1][0] = "Tue 10 Nov"
        #expect(t.total(0, today: today)?.text == "1 row" && t.total(1, today: today) == nil && t.total(2, today: today)?.text == "0 of 1")
    }

    @Test func aFormatAndAFormulaAndAName() {
        let t = TableData(
            rows: [["Item", "Price", "Qty", "Cost"], ["Tea", "$4.50", "2", ""], ["A | B", "$1.00", "3", ""]],
            columnInfo: [col(.text), col(.number, format: "USD"), col(.number), col(.formula, total: .sum, format: "USD", formula: "Price * Qty")],
            name: "Shop | list"
        )
        #expect(t.shown(1, 3) == "$9.00")
        #expect(t.total(3, today: today)?.text == "Sum $12.00")
        // The name is in the words that search reads.
        #expect(t.plainText.hasPrefix("Shop | list\n"))
    }

    @Test func formulasAreWorkedOutForEachRow() {
        var t = TableData(
            rows: [["Item", "Price", "Qty", "Cost", "Big"], ["Tea", "4.5", "2", "", ""], ["Cup", "", "3", "", ""], ["Pot", "30", "1", "", ""]],
            columnInfo: [col(.text), col(.number), col(.number), col(.formula, formula: "Price * Qty"), col(.formula, formula: "if(Cost >= 20, \"big\", \"\")")]
        )
        #expect([t.shown(1, 3), t.shown(2, 3), t.shown(3, 3)] == ["9", "", "30"])
        #expect([t.shown(1, 4), t.shown(3, 4)] == ["", "big"])
        #expect(t.value(1, 3) == 9 && t.value(2, 3) == nil)
        t.columnInfo[3].total = .sum
        #expect(t.total(3, today: today)?.text == "Sum 39")
        t.rows[1][1] = "4.25"
        #expect(t.shown(1, 3) == "8.5")
        // What is wrong with a formula is said in a sentence; its cells are empty.
        #expect(t.formulaError(3) == nil)
        t.columnInfo[3].formula = "Price * Weight"
        #expect(t.formulaError(3) == "There is no column \"Weight\".")
        #expect(t.shown(1, 3) == "")
        t.columnInfo[3].formula = "Price *"
        #expect(t.formulaError(3) != nil)
        // A formula that reads itself, and two that read each other, are worth nothing (and end).
        t.columnInfo[3].formula = "Cost + 1"
        #expect(t.shown(1, 3) == "" && t.formulaError(3) == "There is no column \"Cost\".")
        t.columnInfo[3].formula = "Big + 1"
        t.columnInfo[4].formula = "Cost + 1"
        #expect(t.shown(1, 3) == "" && t.shown(1, 4) == "")
    }

    @Test func nestedRowsShowTheSumOfTheirRows() throws {
        let t = try #require(TableData.parse(#"""
            {"rows":[["Item","Amount"],["Food",""],["Bread","4"],["Milk","2"],["Rent","900"]],
             "columns":[{},{"kind":"number","total":"sum"}],
             "info":[{},{"id":"a1"},{"id":"a2","indent":1},{"id":"a3","indent":1},{"id":"a4"}]}
            """#))
        #expect((0..<5).map { t.indent($0) } == [0, 0, 1, 1, 0])
        #expect(t.hasChildren(1) && !t.hasChildren(2) && !t.hasChildren(4) && t.subtreeEnd(1) == 4)
        #expect(t.rolledUp(1, 1) == "6" && t.rolledUp(2, 1) == nil && t.rolledUp(1, 0) == nil)
        #expect(t.total(1, today: today)?.text == "Sum 906")
        #expect(t.rowWithID("a4") == 4 && t.rowID(0) == nil && t.rowWithID("") == nil)
        #expect(t.display(1, 1) == .amount(CellAmount(text: "6", rolledUp: true, sign: 0, meter: nil, show: .number)))
    }

    @Test func aStatusColumnStartsWithThreeWordsAndKeepsItsOwn() throws {
        let t = try #require(TableData.parse(#"{"rows":[["Task","State"],["Write",""],["Ship","Waiting"]],"columns":[{},{"kind":"status"}]}"#))
        #expect(t.column(1).options.map(\.name) == ["Not started", "In progress", "Done", "Waiting"])
        #expect(t.column(1).color(of: "done") == .green)
    }

    @Test func optionsComeFromTheCellsAndKeepTheirColours() throws {
        let t = TableData(rows: [["Ticker", "Side", "Tags"], ["AAPL", "Long", "Swing, Earnings"], ["TSLA", "Short", "swing"], ["NVDA", "long", ""]], columnInfo: [col(.text), col(.choice), col(.multi)])
        #expect(t.column(1).options.map(\.name) == ["Long", "Short"] && t.column(2).options.map(\.name) == ["Swing", "Earnings"])
        #expect(t.column(1).color(of: "LONG") == .green && t.column(1).color(of: "Short") == .orange)
        // Colours kept with the column are the ones shown.
        let kept = try #require(TableData.parse(#"{"rows":[["T","S"],["a","Short"]],"columns":[{},{"kind":"choice","options":[{"name":"Short","color":"red"},{"name":"Long"}]}]}"#))
        #expect(kept.column(1).color(of: "short") == .red && kept.column(1).color(of: "Long") == .default)
        #expect(kept.display(1, 1) == .tags([CellTag(name: "Short", color: .red)], dots: false))
        #expect(t.display(1, 2) == .tags([CellTag(name: "Swing", color: TagColor.next(2)), CellTag(name: "Earnings", color: TagColor.next(3))], dots: false))
    }

    @Test func cellsShowByTheirKind() throws {
        let t = TableData(
            rows: [
                ["Name", "P&L", "Done", "Site", "Mail", "Phone", "Pics", "Linked", "Progress", "Note"],
                ["A", "-5", "x", "example.com", "a@b.co", "+1 (555) 010", "abc123.png, ../evil.png, x.exe", "Ann, Bo", "40", "hi"],
                ["B", "$12", "", "https://a.org/x", "", "", "", "", "120", ""],
            ],
            columnInfo: [col(.text), col(.number, format: "USD", signed: true), col(.checkbox), col(.link), col(.email), col(.phone), col(.file), col(.relation), col(.number, format: "percent", show: .ring), col(.text)]
        )
        let fulls = t.meterFulls()
        #expect(fulls[8] == 100 && fulls[1] == 0)
        #expect(t.display(1, 1) == .amount(CellAmount(text: "-5", rolledUp: false, sign: -1, meter: nil, show: .number)))
        #expect(t.display(2, 1) == .amount(CellAmount(text: "$12", rolledUp: false, sign: 1, meter: nil, show: .number)))
        #expect(t.display(1, 2) == .checkbox(true) && t.display(2, 2) == .checkbox(false))
        #expect(t.display(1, 3) == .link(text: "example.com", url: "https://example.com"))
        #expect(t.display(2, 3) == .link(text: "https://a.org/x", url: "https://a.org/x"))
        #expect(t.display(1, 4) == .link(text: "a@b.co", url: "mailto:a@b.co"))
        #expect(t.display(2, 4) == .link(text: "", url: nil))
        #expect(t.display(1, 5) == .link(text: "+1 (555) 010", url: "tel:+1555010"))
        #expect(t.display(1, 6) == .pictures(["abc123.png"]))
        #expect(t.display(1, 7) == .rows(["Ann", "Bo"]))
        #expect(t.display(1, 8, fulls: fulls) == .amount(CellAmount(text: "40", rolledUp: false, sign: 0, meter: 0.4, show: .ring)))
        #expect(t.display(2, 8) == .amount(CellAmount(text: "120", rolledUp: false, sign: 0, meter: 1, show: .ring)))
        #expect(t.display(1, 9) == .text("hi"))
        #expect(t.picture(1) == "abc123.png" && t.nameOf(1) == "A")
        // A bar with no goal is full at the highest amount.
        let bars = TableData(rows: [["N", "km"], ["a", "5"], ["b", "20"]], columnInfo: [col(.text), col(.number, show: .bar)])
        #expect(bars.display(1, 1) == .amount(CellAmount(text: "5", rolledUp: false, sign: 0, meter: 0.25, show: .bar)))
    }
}

@Suite struct TableViewTests {
    @Test func aPlainViewShowsTheRowsAsTheyAre() {
        let t = spending()
        #expect(t.order(today: today) == Array(1..<9))
        #expect(t.counted(today: today).count == 8)
        #expect(t.shownColumns == [0, 1, 2, 3, 4])
    }

    @Test func rowsAreOrderedByTheSorts() {
        var t = spending([["", "", "", "", ""]])
        t.view.sort = [TableSort(column: 1, down: true)]
        // High to low, and the row with nothing in it last.
        #expect(names(t, t.order(today: today)) == ["Rent", "Groceries", "Internet", "Dinner with Rachel", "Concert tickets", "Coffee beans", "Train to Portland", "Cinema", ""])
        t.view.sort = [TableSort(column: 1)]
        #expect(Array(names(t, t.order(today: today)).prefix(2)) == ["Cinema", "Train to Portland"])
        #expect(t.order(today: today).last == 9)
        // By the words of a column, then by the day inside equal words.
        t.view.sort = [TableSort(column: 2), TableSort(column: 3, down: true)]
        #expect(Array(names(t, t.order(today: today)).prefix(5)) == ["Internet", "Rent", "Coffee beans", "Dinner with Rachel", "Groceries"])
        // Ticked boxes last when low to high.
        t.view.sort = [TableSort(column: 4)]
        #expect(Array(names(t, t.order(today: today)).prefix(2)) == ["Concert tickets", "Train to Portland"])
        #expect(t.title(1) == "Rent")
    }

    @Test func filtersLeaveTheRowsThatPassAndTheTotalsCountOnlyThose() {
        var t = spending()
        t.columnInfo[0].total = .count
        t.columnInfo[1].total = .sum
        t.columnInfo[4].total = .checked
        t.view.filter = [rule(2, .is, "food")]
        #expect(names(t, t.order(today: today)) == ["Groceries", "Dinner with Rachel", "Coffee beans"])
        #expect(t.total(0, today: today)?.text == "3 of 8 rows")
        #expect(t.total(1, today: today)?.text == "Sum $271.40")
        #expect(t.total(4, today: today)?.text == "3 of 3")
        #expect(t.summary(today: today) == ["3 of 8 rows", "Sum $271.40"])
        // A second filter: both must pass. A row with nothing in it shows, to be filled in.
        t.view.filter.append(rule(1, .atLeast, "65"))
        t.rows.append(["", "", "", "", ""])
        #expect(names(t, t.order(today: today)) == ["Groceries", "Dinner with Rachel", ""])
        #expect(t.total(1, today: today)?.text == "Sum $215.00")
        // A filter that still waits for its words hides nothing.
        t.view.filter = [rule(1, .above, " ")]
        #expect(t.order(today: today).count == 9)
    }

    @Test func eachKindOfColumnIsAskedInItsOwnWay() {
        let t = spending()
        func passing(_ r: TableRule) -> [String] { names(t, (1..<9).filter { t.passes($0, r, today: today) }) }
        #expect(passing(rule(1, .above, "$120")) == ["Rent", "Groceries"])
        #expect(passing(rule(1, .atMost, "48")) == ["Cinema", "Train to Portland"])
        #expect(passing(rule(1, .is, "65")) == ["Dinner with Rachel"])
        #expect(passing(rule(1, .isNot, "65")).count == 7)
        #expect(passing(rule(1, .below, "words")) == [])
        #expect(passing(rule(3, .below, "3 Nov 2026")) == ["Rent"])
        #expect(passing(rule(3, .above, "2026-11-08")) == ["Concert tickets", "Train to Portland"])
        #expect(passing(rule(3, .is, "Nov 8, 2026")) == ["Coffee beans", "Cinema"])
        #expect(passing(rule(4, .notTicked)) == ["Concert tickets", "Train to Portland"])
        #expect(passing(rule(4, .ticked)).count == 6)
        #expect(passing(rule(0, .has, "IN")) == ["Internet", "Dinner with Rachel", "Cinema", "Train to Portland"])
        #expect(passing(rule(0, .hasNot, "e")) == ["Train to Portland"])
        #expect(passing(rule(2, .isNot, "Food")).count == 5)
        #expect(passing(rule(2, .empty)).count == 0)
        #expect(passing(rule(2, .filled)).count == 8)
        // Several choices: one of them is enough.
        let tags = TableData(rows: [["Name", "Tags"], ["a", "Home, Food"], ["b", "Fun"]], columnInfo: [col(.text), col(.multi)])
        #expect(tags.passes(1, rule(1, .is, "food")) && !tags.passes(2, rule(1, .is, "food")) && tags.passes(1, rule(1, .has, "hom")))
    }

    @Test func groupsKeepTheirPlaceAndARowTakesTheFirstColourItPasses() {
        var t = spending()
        let groups = t.groups(t.order(today: today), 2)
        #expect(groups.map(\.value) == ["Home", "Food", "Fun", "Travel"] && groups.map(\.rows.count) == [2, 3, 2, 1])
        // Sorted and filtered: the groups stay where they were, one of them empty now; rows with no value are last.
        t.rows[8][2] = ""
        t.view.sort = [TableSort(column: 1)]
        t.view.filter = [rule(1, .below, "100")]
        let after = t.groups(t.order(today: today), 2)
        #expect(after.map(\.value) == ["Home", "Food", "Fun", "Travel", ""])
        #expect(after.map { names(t, $0.rows) } == [[], ["Coffee beans", "Dinner with Rachel"], ["Cinema", "Concert tickets"], [], ["Train to Portland"]])
        #expect(t.boardColumn == 2)
        #expect(t.groupLabel(2, "") == "No Category" && t.groupLabel(4, "x") == "Ticked")
        t.view.group = 4
        #expect(t.boardColumn == 4)
        // Colours.
        t.view.paint = [rule(4, .notTicked, color: "red"), rule(1, .below, "60", color: "green"), rule(2, .is, "", color: "blue")]
        #expect(t.color(7, today: today) == "red" && t.color(8, today: today) == "red" && t.color(5, today: today) == "green" && t.color(1, today: today) == nil)
        #expect(t.rowColor(5, today: today) == .green)
    }

    @Test func rowsSortAndGroupByTheOrderOfTheOptions() throws {
        let t = try #require(TableData.parse(#"""
            {"rows":[["Task","Status"],["a","Done"],["b","Todo"],["c","Doing"]],
             "columns":[{},{"kind":"choice","options":[{"name":"Todo","color":"blue"},{"name":"Doing","color":"green"},{"name":"Done","color":"orange"},{"name":"Blocked","color":"purple"}]}],
             "view":{"sort":[{"column":1}]}}
            """#))
        #expect(t.order(today: today) == [2, 3, 1])
        // A board has a column for each option, with rows or without.
        let groups = t.groups(t.order(today: today), 1)
        #expect(groups.map(\.value) == ["Todo", "Doing", "Done", "Blocked"] && groups.map(\.rows.count) == [1, 1, 1, 0])
    }

    @Test func nestedRowsStayUnderTheirRow() throws {
        var t = try #require(TableData.parse(#"""
            {"rows":[["Item","Amount"],["Food","9"],["Bread","4"],["Milk","2"],["Rent","900"],["Heat","50"],["Fun","30"]],
             "columns":[{},{"kind":"number"}],
             "info":[{},{},{"indent":1},{"indent":1},{},{"indent":1},{}],
             "view":{"sort":[{"column":1}]}}
            """#))
        // Each level by itself: Food (9) before Fun (30) before Rent (900), Milk before Bread.
        #expect(names(t, t.order(today: today)) == ["Food", "Milk", "Bread", "Fun", "Rent", "Heat"])
        #expect(t.order(today: today).map { t.shownIndent($0) } == [0, 1, 1, 0, 0, 1])
        t.info[1].closed = true
        #expect(names(t, t.order(today: today)) == ["Food", "Fun", "Rent", "Heat"])
        // With a filter the rows are a plain list, closed or not.
        t.view.filter = [rule(1, .below, "40")]
        #expect(names(t, t.order(today: today)) == ["Milk", "Bread", "Food", "Fun"])
        #expect(t.order(today: today).allSatisfy { t.shownIndent($0) == 0 })
    }

    @Test func theViewOfTheTradesMock() throws {
        // What `<!-- table: Trades | icon target:green | board by Result | sort Date down | sort Ticker | frozen 1 | hide Notes |
        // filter Reviewed is not ticked | color P&L below 0 red | color Result is Win green | locked -->` keeps.
        let t = try #require(TableData.parse(#"""
            {"rows":[["Date","Ticker","Side","P&L","Progress","Result","Reviewed","Notes"],
                     ["2026-11-10","AAPL","Long","120","40","Win","x","Clean"],
                     ["2026-11-11","TSLA","Short","-80","75","Loss","",""],
                     ["2026-11-12","NVDA","Long","-10","10","Loss","",""]],
             "columns":[{"kind":"date"},{},{"kind":"choice","options":[{"name":"Long","color":"green"},{"name":"Short","color":"red"}]},
                        {"kind":"number","total":"sum","format":"USD","signed":true},{"kind":"number","format":"percent","show":"ring","goal":100.0},
                        {"kind":"choice","options":[{"name":"Win","color":"green"},{"name":"Loss","color":"red"}]},{"kind":"checkbox"},{}],
             "name":"Trades","icon":"target:green",
             "view":{"layout":"board","sort":[{"column":0,"down":true},{"column":1}],"filter":[{"column":6,"op":"notticked"}],"group":5,
                     "hidden":[7],"frozen":1,"paint":[{"column":3,"op":"below","text":"0","color":"red"},{"column":5,"op":"is","text":"Win","color":"green"}],"locked":true}}
            """#))
        #expect(t.name == "Trades" && t.icon == "target:green" && t.view.layout == .board && t.view.locked)
        #expect(t.shownColumns == [0, 1, 2, 3, 4, 5, 6] && t.view.frozen == 1 && t.boardColumn == 5)
        #expect(t.order(today: today) == [3, 2])
        #expect(t.rowColor(2, today: today) == .red && t.rowColor(1, today: today) == .green)
        #expect(t.total(3, today: today)?.text == "Sum -$90.00")
        #expect(t.groups(t.order(today: today), 5).map(\.value) == ["Win", "Loss"])
        #expect(t.display(1, 3) == .amount(CellAmount(text: "120", rolledUp: false, sign: 1, meter: nil, show: .number)))
    }

    @Test func aCalendarAndCardsFindTheirRows() {
        let t = spending()
        let days = t.rowsByDay(t.order(today: today), today: today)
        #expect(days[d(2026, 11, 8)] == [5, 6] && days[d(2026, 11, 1)] == [1] && days.count == 7)
        #expect(t.facts(7) == ["$63.00", "Fun"] && t.facts(1, but: 1, most: 3) == ["Home", "2026-11-01", "Paid"])
    }
}
