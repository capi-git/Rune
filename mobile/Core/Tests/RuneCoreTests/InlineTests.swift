// The same cases as the tests in `src/inline.rs` of the desktop app. When a
// rule changes there, change the matching case here.

import Testing
@testable import RuneCore

private func run(_ text: String, _ marks: String = "", link: String? = nil) -> Inline.Run {
    Inline.Run(
        text: text,
        marks: Inline.Marks(bold: marks.contains("b"), italic: marks.contains("i"), strike: marks.contains("s"), code: marks.contains("c")),
        link: link
    )
}

@Suite struct InlineTests {
    @Test func plainTextEscapesAndRoundTrips() {
        for s in ["2*3*4", "snake_case_name", "a ~ b ~~ c", "[not](a link)", "back\\slash", "`tick`", "**x**", "_y_"] {
            let md = Inline.escape(s)
            #expect(Inline.plain(md) == s, "md was \(md)")
            #expect(Inline.parse(md).count == 1)
        }
    }

    /// Stored forms, as `to_md` on the desktop writes them.
    @Test func readsTheStoredForms() {
        #expect(Inline.parse("**bold** then _italic_") == [run("bold", "b"), run(" then "), run("italic", "i")])
        #expect(Inline.parse("_a**b**_**c**") == [run("a", "i"), run("b", "bi"), run("c", "b")])
        #expect(Inline.parse("**a**_b_") == [run("a", "b"), run("b", "i")])
        #expect(Inline.parse("**_~~x~~_**") == [run("x", "bis")])
        #expect(Inline.parse("**_x_**") == [run("x", "bi")])
        #expect(Inline.parse("in_word_italic") == [run("in"), run("word", "i"), run("italic")])
        #expect(Inline.parse("`code \\` with \\\\ tick` and ~~struck~~") == [run("code ` with \\ tick", "c"), run(" and "), run("struck", "s")])
        #expect(Inline.parse("**`bold code`**") == [run("bold code", "bc")])
        #expect(Inline.parse("**a** b") == [run("a", "b"), run(" b")])
        #expect(Inline.parse("`a`") == [run("a", "c")])
    }

    @Test func readsLinks() {
        #expect(Inline.parse("[here](https://x.dev)") == [run("here", link: "https://x.dev")])
        #expect(Inline.parse("see [**site**](https://example.com/a) now") == [
            run("see "), run("site", "b", link: "https://example.com/a"), run(" now"),
        ])
        #expect(Inline.parse("[**site**](https://example.com/a%20b%28c%29)") == [
            run("site", "b", link: "https://example.com/a%20b%28c%29"),
        ])
        // Not links: no address, an escaped bracket, a line break in the address.
        #expect(Inline.plain("[x]()") == "[x]()")
        #expect(Inline.plain("\\[not\\](a link)") == "[not](a link)")
        #expect(Inline.plain("[a](b\nc)") == "[a](b\nc)")
        #expect(Inline.pageID(ofLink: "notion2://page/abc123") == "abc123")
        #expect(Inline.pageID(ofLink: "rune://page/abc123") == "abc123", "the form of Rune 0.1.9")
        #expect(Inline.isSafeURL("rune://page/abc123"))
        #expect(Inline.pageID(ofLink: "https://example.com") == nil)
    }

    @Test func readsCommonMarkdownFromElsewhere() {
        func foreign(_ s: String) -> [Inline.Run] { Inline.parse(s, foreign: true) }
        #expect(foreign("**Note:** use _this_ and `that`").map(\.text).joined() == "Note: use this and that")
        #expect(foreign("__b__")[0].marks.bold)
        #expect(foreign("*i* and ***both***")[0].marks.italic)
        #expect(foreign("***both***").allSatisfy { $0.marks.bold && $0.marks.italic })
        #expect(foreign("a * b * c").count == 1, "spaced asterisks stay literal")
        #expect(foreign("my_var_name").count == 1, "underscores inside words stay literal")
        #expect(foreign("unclosed **bold").map(\.text).joined() == "unclosed **bold")
        let runs = foreign("see [the docs](https://d.dev) now")
        #expect(runs[1].link == "https://d.dev")
        #expect(runs[1].text == "the docs")
    }

    @Test func unpairedMarksStayAsText() {
        #expect(Inline.plain("a ** b") == "a ** b")
        #expect(Inline.plain("~~~x~~~") == "~~~x~~~")
        #expect(Inline.plain("5 * 3 _ 2") == "5 * 3 _ 2")
        #expect(Inline.parse("**open") == [run("**open")])
        #expect(Inline.parse("`no end") == [run("`no end")])
    }

    @Test func keepsEveryCharacter() {
        // Emoji, joined emoji and text in other scripts pass through whole.
        #expect(Inline.plain("**👩‍👩‍👧 café** 日本語") == "👩‍👩‍👧 café 日本語")
        #expect(Inline.parse("a\nb") == [run("a\nb")])
    }

    @Test func safeLinks() {
        #expect(Inline.isSafeURL("https://a"))
        #expect(Inline.isSafeURL(" MAILTO:a@b.c"))
        #expect(Inline.isSafeURL("notion2://page/1"))
        #expect(!Inline.isSafeURL("javascript:alert(1)"))
        #expect(!Inline.isSafeURL("file:///etc/passwd"))
    }

    @Test func blocksKnowTheirPlainText() {
        #expect(Block(kind: .text, text: "**hello** world").plainText == "hello world")
        #expect(Block(kind: .code, text: "a*b // **raw**").plainText == "a*b // **raw**")
    }
}
