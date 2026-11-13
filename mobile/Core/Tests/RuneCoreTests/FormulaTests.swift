// The same cases as the tests in `src/formula.rs` of the desktop app. When
// a rule changes there, change the matching case here.

import Foundation
import Testing
@testable import RuneCore

private let yes = FormulaValue.bool(true)
private let no = FormulaValue.bool(false)
private let none = FormulaValue.empty
private func n(_ number: Double) -> FormulaValue { .number(number) }
private func t(_ text: String) -> FormulaValue { .text(text) }

/// One row to work the formulas out for. Names match without regard to case.
private func row(_ name: String) -> FormulaValue? {
    let cells: [(String, FormulaValue)] = [
        ("price", n(10)), ("qty", n(3)), ("unit price", n(2.5)), ("amount", n(150)), ("a", n(4)), ("b", n(9)),
        ("größe", n(7)), ("_x1", n(1)), ("paid", yes), ("name", t("Ann")), ("blank", none),
    ]
    return cells.first { $0.0 == name.lowercased() }?.1
}

private func calc(_ source: String) -> Result<FormulaValue, FormulaError> {
    Formula.parse(source).flatMap { $0.eval(row) }
}

/// Each formula must be worth the value next to it.
private func all(_ cases: [(String, FormulaValue)], sourceLocation: SourceLocation = #_sourceLocation) {
    for (source, wanted) in cases {
        #expect(calc(source) == .success(wanted), "\(source)", sourceLocation: sourceLocation)
    }
}

/// Each formula must fail with the sentence next to it.
private func fail(_ cases: [(String, String)], sourceLocation: SourceLocation = #_sourceLocation) {
    for (source, sentence) in cases {
        #expect(calc(source) == .failure(FormulaError(sentence)), "\(source)", sourceLocation: sourceLocation)
    }
}

@Suite struct FormulaTests {
    @Test func readsNumbersTextAndTheExamples() {
        all([
            ("12", n(12)), ("1.5", n(1.5)), (".5", n(0.5)), ("5.", n(5)), ("  7\n", n(7)), ("true", yes), ("FALSE", no),
            ("\"big\"", t("big")), (#""say \"hi\"""#, t("say \"hi\"")), (#""a\b""#, t("a\\b")), ("\"[1 + 2]\"", t("[1 + 2]")),
            ("Price * Qty", n(30)), ("[Unit price] * Qty * 1.2", n(9)), ("if(Paid, 0, Amount)", n(0)),
            ("round(Amount / 3, 2)", n(50)), ("round(Price / 3, 2)", n(3.33)), ("max(A, B) - min(A, B)", n(5)),
            ("if(Amount >= 100 and not Paid, \"big\", \"\")", t("")), ("if(Amount >= 100 and Paid, \"big\", \"\")", t("big")),
        ])
    }

    @Test func operatorsBindInOrder() {
        all([
            ("1 + 2 * 3", n(7)), ("(1 + 2) * 3", n(9)), ("10 - 4 - 3", n(3)), ("8 / 2 / 2", n(2)), ("7 % 4", n(3)),
            ("2 * 7 % 4", n(2)), ("1 + 7 % 4", n(4)), ("2 * 3 ^ 2", n(18)), ("-2 ^ 2", n(-4)), ("(-2) ^ 2", n(4)),
            ("2 ^ 3 ^ 2", n(512)), ("2 ^ -1", n(0.5)), ("2 * -3", n(-6)), ("1 - -1", n(2)), ("--3", n(3)), ("1-1", n(0)),
            ("1 + 2 = 3", yes), ("2 * 3 > 5 + 0", yes), ("not 1 = 2", yes), ("not true or true", yes), ("not (true or true)", no),
            ("true or false and false", yes), ("(true or false) and false", no), ("1 < 2 and 2 < 3", yes),
            ("NOT not TRUE", yes), ("True AND False", no), ("false Or true", yes),
        ])
    }

    @Test func comparesNumbersAndText() {
        all([
            ("1 = 1", yes), ("1 == 2", no), ("1 != 2", yes), ("1 <> 1", no), ("1 < 2", yes), ("2 < 2", no), ("2 <= 2", yes),
            ("1 > 2", no), ("3 > 2", yes), ("2 >= 3", no), ("3>=3", yes), ("1<>2", yes), ("\"10\" > \"9\"", yes),
            ("\"12\" = 12.0", yes), ("\"Big\" = \"big\"", yes), ("\"a\" < \"B\"", yes), ("\"b\" >= \"A\"", yes),
            ("\"b\" > \"abc\"", yes), ("Name != \"Bob\"", yes), ("Name = \"ANN\"", yes), ("Paid = true", yes), ("Paid = 1", yes),
            ("Paid = \"TRUE\"", yes), ("Blank = \"\"", yes), ("Blank = Blank", yes), ("Blank <= Blank", yes),
            ("Blank != Blank", no), ("Blank < Blank", no), ("Blank = 0", no), ("Blank != 0", yes), ("Blank < 5", no),
            ("Blank > 5", no), ("Blank <= 5", no), ("5 >= Blank", no), ("Blank < \"a\"", no), ("\"a\" > Blank", no),
        ])
        fail([("1 < 2 < 3", "The sign < does not belong here."), ("1 = 1 = 1", "The sign = does not belong here.")])
    }

    @Test func joinsTextAndKeepsAMissingCellEmpty() {
        all([
            ("\"a\" + \"b\"", t("ab")), ("Name + \" \" + 1", t("Ann 1")), ("1.50 + Name", t("1.5Ann")), ("Name + Blank", t("Ann")),
            ("Blank + Name", t("Ann")), ("Name + Paid", t("Anntrue")), ("\"1\" + 2", n(3)), ("\" 2 \" * \"-.5\"", n(-1)),
            ("Paid + 1", n(2)), ("Blank + 1", none), ("1 - Blank", none), ("Blank * 2", none), ("2 / Blank", none),
            ("Blank % 2", none), ("Blank ^ 2", none), ("-Blank", none), ("Blank + Blank", none), ("Name * 2", none),
            ("Name - 1", none), ("-Name", none), ("1 / 0", none), ("5 % 0", none), ("0 / 0", none), ("1 / 0 + 1", none),
            ("(0 - 8) ^ 0.5", none), ("10 ^ 400", none), ("concat(0 * -1)", t("0")), ("0 - 7 % 4", n(-3)),
        ])
        // A number that is not finite counts as empty, in a cell and as written.
        let formula = try! Formula.parse("A").get()
        #expect(formula.eval { _ in n(.nan) } == .success(none))
        #expect(formula.eval { _ in n(.infinity) } == .success(none))
        #expect(calc(String(repeating: "9", count: 400)) == .success(none))
    }

    @Test func logicUsesTruthAndStopsEarly() {
        all([
            ("1 and \"x\"", yes), ("0 or \"\"", no), ("Blank or 2", yes), ("Blank and 2", no), ("not Blank", yes), ("not Name", no),
            ("not 0", yes), ("false and Missing", no), ("true or Missing", yes), ("false and Missing or true", yes),
        ])
        let missing = "There is no column \"Missing\"."
        fail([("true and Missing", missing), ("false or Missing", missing), ("not Missing", missing)])
    }

    @Test func worksOutEachFunction() {
        all([
            ("if(1 > 2, \"a\", \"b\")", t("b")), ("if(Paid, 1)", n(1)), ("if(not Paid, 1)", none), ("if(Paid, 1, Missing)", n(1)),
            ("if(Blank, Missing, 2)", n(2)), ("IF(true, if(false, 1, 2), 3)", n(2)),
            ("round(2.5)", n(3)), ("round(-2.5)", n(-3)), ("round(2.4)", n(2)), ("round(2.346, 2)", n(2.35)),
            ("round(1234.5678, 1)", n(1234.6)), ("round(1250, -2)", n(1300)), ("round(1.29, 1.9)", n(1.3)),
            ("round(1.5, 99)", n(1.5)), ("round(Blank)", none), ("round(1.5, Blank)", none), ("round(Name, 1)", none),
            ("floor(1.9)", n(1)), ("floor(-1.1)", n(-2)), ("ceil(1.1)", n(2)), ("ceil(-1.9)", n(-1)), ("abs(-3)", n(3)),
            ("abs(3)", n(3)), ("abs(Blank)", none), ("floor(Name)", none), ("ceil(Blank)", none),
            ("min(3, 1, 2)", n(1)), ("max(3, 1, 2)", n(3)), ("sum(1, 2, 3)", n(6)), ("average(1, 2, 6)", n(3)),
            ("min(Blank, 5, Name)", n(5)), ("max(Blank, \"7\", 5)", n(7)), ("sum(Blank, 2, Name)", n(2)),
            ("average(Blank, 2, 4)", n(3)), ("sum(Paid, Paid)", n(2)), ("min(7)", n(7)), ("min(Blank)", none),
            ("max(Name, Blank)", none), ("sum(Blank, Blank)", none), ("average(Blank)", none),
            ("empty(Blank)", yes), ("empty(\"\")", yes), ("empty(\" \")", yes), ("empty(1 / 0)", yes), ("empty(0)", no),
            ("empty(Name)", no), ("empty(false)", no), ("length(Name)", n(3)), ("length(12.50)", n(4)),
            ("length(Blank)", n(0)), ("length(\"Größe\")", n(5)), ("concat(Blank)", t("")),
            ("concat(Name, \" \", 3.0, Blank, 1.5, true)", t("Ann 31.5true")),
            ("ROUND(2.4)", n(2)), ("Max (1, 2)", n(2)), ("sum(1, sum(2, 3)) * 2", n(12)),
        ])
    }

    @Test func readsColumnNames() throws {
        all([
            ("[Unit price] * Qty", n(7.5)), ("[ unit PRICE ]", n(2.5)), ("Größe + 1", n(8)), ("[Größe]", n(7)),
            ("price", n(10)), ("_x1 + 1", n(2)), ("[A]+[B]", n(13)),
        ])
        // The name goes to the caller as written, without the spaces around it.
        #expect(try Formula.parse("[ Unit price ]").get().eval { t($0) } == .success(t("Unit price")))
        #expect(try Formula.parse("[a-b (c), \"d\"]").get().names == ["a-b (c), \"d\""])
        // A word is a function only when a ( follows it, and [and] is a column.
        let formula = try Formula.parse("sum + if + [and]").get()
        #expect(formula.names == ["sum", "if", "and"])
        #expect(formula.eval { _ in n(2) } == .success(n(6)))
        let long = try Formula.parse("[Unit price] * Qty + qty - if(Paid, 0, max(Amount, [ unit price ])) > 0 and not _x1").get()
        #expect(long.names == ["Unit price", "Qty", "Paid", "Amount", "_x1"])
        #expect(try Formula.parse("1 + 2").get().names.isEmpty)
        #expect(Formula.parse("1+2*A") == Formula.parse(" 1 + 2 * A "))
    }

    @Test func saysWhatIsWrong() {
        let end = "A value is missing at the end.", one = "needs one value."
        fail([
            ("", "The formula is empty."), ("  \n ", "The formula is empty."), ("(1 + 2", "A ) is missing."),
            ("min(1, 2", "A ) is missing."), ("[x", "A ] is missing."), ("[", "A ] is missing."),
            ("\"abc", "A closing \" is missing."), ("\"abc\\\"", "A closing \" is missing."),
            ("1 +", end), ("if(", end), ("((((", end), ("not", end), ("-", end), ("2 ^", end), ("min(1,", end),
            ("1 + * 2", "The sign * does not belong here."), ("1 )", "The sign ) does not belong here."),
            (",", "The sign , does not belong here."), ("()", "The sign ) does not belong here."),
            ("(1, 2)", "The sign , does not belong here."), ("1 # 2", "The sign # does not belong here."),
            ("1 ! 2", "The sign ! does not belong here."), (".", "The sign . does not belong here."),
            ("1.2.3", "The sign . does not belong here."), ("]", "The sign ] does not belong here."),
            ("[x](1)", "The sign ( does not belong here."), ("true(1)", "The sign ( does not belong here."),
            ("= 1", "The sign = does not belong here."), ("1 2", "The value 2 does not belong here."),
            ("min(1 2)", "The value 2 does not belong here."), ("1 true", "The value true does not belong here."),
            ("1 \"a\"", "The text \"a\" does not belong here."), ("Unit price", "The word price does not belong here."),
            ("1 [x]", "The name [x] does not belong here."), ("1 and or 2", "The word or does not belong here."),
            ("1 + not 2", "The word not does not belong here."),
            ("foo(1)", "There is no function \"foo\"."), ("Größe(1)", "There is no function \"Größe\"."),
            ("min()", "min needs at least one value."), ("max()", "max needs at least one value."),
            ("sum()", "sum needs at least one value."), ("average()", "average needs at least one value."),
            ("CONCAT()", "concat needs at least one value."), ("floor()", "floor \(one)"),
            ("ceil()", "ceil \(one)"), ("abs(1, 2)", "abs \(one)"), ("empty()", "empty \(one)"),
            ("length(1, 2)", "length \(one)"), ("round()", "round needs one or two values."),
            ("round(1, 2, 3)", "round needs one or two values."), ("if(1)", "if needs two or three values."),
            ("if(1, 2, 3, 4)", "if needs two or three values."),
            ("Foo + 1", "There is no column \"Foo\"."), ("[No such]", "There is no column \"No such\"."),
            ("[]", "There is no column \"\"."),
        ])
    }

    @Test func keepsToTheLimits() throws {
        func brackets(_ levels: Int) -> String { String(repeating: "(", count: levels) + "1" + String(repeating: ")", count: levels) }
        let deep = Result<FormulaValue, FormulaError>.failure(FormulaError("The formula is nested too deep."))
        #expect(calc(brackets(64)) == .success(n(1)))
        #expect(calc(brackets(65)) == deep)
        #expect(calc(String(repeating: "(", count: 1999)) == deep)
        #expect(calc(String(repeating: "-", count: 64) + "1") == .success(n(1)))
        #expect(calc(String(repeating: "-", count: 65) + "1") == deep)
        #expect(calc(String(repeating: "not ", count: 65) + "1") == deep)
        #expect(calc("2" + String(repeating: "^1", count: 64)) == .success(n(2)))
        #expect(calc("2" + String(repeating: "^1", count: 65)) == deep)
        #expect(calc(String(repeating: "abs(", count: 65) + "1" + String(repeating: ")", count: 65)) == deep)
        #expect(calc(String(repeating: "if(1,", count: 399)) == deep)
    }

    /// (A test of its own: a test with many expectations takes much of the
    /// small stack of a test's thread.)
    @Test func theDeepestTreeThereCanBe() throws {
        // Every kind of operator and a call on each of the 64 levels.
        let deepest = (0..<64).reduce("1") { inside, _ in "0 or 1 and 2=3+4*abs(\(inside))^2" }
        let formula = try Formula.parse(deepest).get()
        let worked = formula.eval(row)
        #expect(worked == .success(no))
        #expect(formula.names.isEmpty)
    }

    @Test func keepsToTheLimitsOfLength() {
        // A long formula that is not nested is no problem.
        #expect(calc("1" + String(repeating: "+1", count: 999)) == .success(n(1000)))
        #expect(calc("1" + String(repeating: " or 0", count: 399)) == .success(yes))
        // The limit of length counts characters, not bytes.
        let long = Result<FormulaValue, FormulaError>.failure(FormulaError("The formula is too long."))
        #expect(calc(String(repeating: " ", count: 1999) + "1") == .success(n(1)))
        #expect(calc(String(repeating: " ", count: 2000) + "1") == long)
        #expect(calc(String(repeating: "(", count: 10_000)) == long)
        #expect(calc("length(\"" + String(repeating: "ß", count: 1900) + "\")") == .success(n(1900)))
    }

    @Test func hostileInputFailsAndNeverCrashes() {
        let hostile = [
            "((((", "\"abc", "1 +", "[x", "if(", ",", "))))", "]", "\\", "\"", "[", "!@#$%^&*", "= = =", "1 ..2", "..", "if(,)",
            "min(,", "min(1,)", "^", "*/%", "\0", "🙂", "1 +\u{0}", "a.b", "<>", "'x'", "and", "(,)", "[]]", "\"\"\"", "1e5",
            "1 = = 1", "“big”", "1 + + 2", "a b c", "if if(", "[a] [b]", "--", "not not", "()()",
        ]
        for source in hostile {
            guard case .failure(let error) = calc(source) else {
                Issue.record("\(source) should fail")
                continue
            }
            #expect(error.sentence.hasSuffix(".") && !error.sentence.contains("\n"), "\(source): \(error.sentence)")
        }
        // Formulas made of pieces put together at random (the same ones each time).
        let pieces = [
            "(", ")", "[", "]", "\"", ",", "+", "-", "*", "/", "%", "^", "=", "<", ">", "!", "1", ".5", "0", "a", " ", "\\", "#", "ß",
            "if(", "min(", "round(", "concat(", "empty(", "not ", " and ", " or ", "true", "\"x\"", "[a]", "Blank",
        ]
        var seed: UInt64 = 42
        func next() -> Int {
            seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
            return Int(seed >> 33)
        }
        var good = 0, bad = 0
        for _ in 0..<20_000 {
            let source = (0..<1 + next() % 12).map { _ in pieces[next() % pieces.count] }.joined()
            switch calc(source) {
            case .success: good += 1
            case .failure(let error):
                #expect(error.sentence.hasSuffix("."), "\(source): \(error.sentence)")
                bad += 1
            }
        }
        #expect(good > 100 && bad > 100, "\(good) good, \(bad) bad")
    }

    @Test func valueAsANumberAndAsATruth() {
        #expect(n(1.5).number == 1.5 && yes.number == 1 && no.number == 0 && none.number == nil)
        let texts: [(String, Double?)] = [
            ("12", 12), (" -1.5 ", -1.5), (".5", 0.5), ("5.", 5), ("-.5", -0.5), ("", nil),
            ("-", nil), (".", nil), ("-.", nil), ("1.2.3", nil), ("--1", nil), ("1e5", nil), ("+1", nil), ("1 000", nil),
            ("1,5", nil), ("inf", nil), ("NaN", nil), ("12 km", nil), ("١٢", nil),
        ]
        for (text, wanted) in texts {
            #expect(t(text).number == wanted, "\(text)")
        }
        #expect(t(String(repeating: "9", count: 400)).number == nil)
        #expect(yes.truthy && n(-2).truthy && t("x").truthy && t("0").truthy)
        #expect(!no.truthy && !n(0).truthy && !t("").truthy && !none.truthy)
    }

    @Test func numbersAreWrittenAsTheMacWritesThem() {
        #expect([3, 1.5, -2, 0.1 + 0.2, 1e21, 1e-7, 123456789012, 2.5e-3].map(Formula.write)
            == ["3", "1.5", "-2", "0.30000000000000004", "1000000000000000000000", "0.0000001", "123456789012", "0.0025"])
    }
}
