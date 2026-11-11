// Inline formatting inside a block: bold, italic, strikethrough, inline code
// and links. Mirrors the reader in `src/inline.rs` of the desktop app.
//
// A block's text is stored as a strict subset of Markdown: `**bold**`,
// `_italic_`, `~~strike~~`, `` `code` `` and `[label](url)`. Every other
// character with a meaning is escaped with a backslash. Code blocks are the
// exception: their text is stored raw.
//
// Only reading is here (milestone 1 is read-only). Writing (`to_md` on the
// desktop) comes with editing.

import Foundation

public enum Inline {
    public struct Marks: Hashable, Sendable {
        public var bold = false
        public var italic = false
        public var strike = false
        public var code = false

        public init(bold: Bool = false, italic: Bool = false, strike: Bool = false, code: Bool = false) {
            self.bold = bold
            self.italic = italic
            self.strike = strike
            self.code = code
        }
    }

    /// A stretch of text with the same formatting.
    public struct Run: Hashable, Sendable {
        public var text: String
        public var marks: Marks
        public var link: String?

        public init(text: String, marks: Marks = Marks(), link: String? = nil) {
            self.text = text
            self.marks = marks
            self.link = link
        }
    }

    // MARK: Public

    /// Parse a block's stored text into formatted runs.
    public static func parse(_ md: String) -> [Run] {
        parse(md, foreign: false)
    }

    /// Parse Markdown. `foreign` reads Markdown from elsewhere (underscores
    /// inside a word are then literal, as in `snake_case`).
    public static func parse(_ md: String, foreign: Bool) -> [Run] {
        let chars = Array(md.unicodeScalars)
        var out: [Run] = []
        build(tokenize(chars[...], foreign: foreign), base: Marks(), link: nil, into: &out)
        return out
    }

    /// The visible text of stored Markdown.
    public static func plain(_ md: String) -> String {
        parse(md).map(\.text).joined()
    }

    /// Plain text to stored Markdown (escaping anything with a meaning).
    public static func escape(_ plain: String) -> String {
        var out = String.UnicodeScalarView()
        for c in plain.unicodeScalars {
            switch c {
            case "\\", "*", "_", "~", "`", "[", "]":
                out.append("\\")
            default:
                break
            }
            out.append(c)
        }
        return String(out)
    }

    /// Links the app may open: the web, mail, and this app's own page links.
    public static func isSafeURL(_ url: String) -> Bool {
        let lower = url.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return ["http://", "https://", "mailto:", "rune://", "notion2://"].contains { lower.hasPrefix($0) }
    }

    /// The page a `rune://page/<id>` link points to (or `notion2://page/<id>`,
    /// as notes from before Rune 0.1.9 write it).
    public static func pageID(ofLink url: String) -> ID? {
        let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
        for prefix in ["rune://page/", "notion2://page/"] where trimmed.lowercased().hasPrefix(prefix) {
            let id = String(trimmed.dropFirst(prefix.count))
            return id.isEmpty ? nil : id
        }
        return nil
    }

    // MARK: Tokens

    private enum Kind: Int {
        case bold = 0
        case italic = 1
        case strike = 2
    }

    private indirect enum Tok {
        case text(String)
        /// A run of `*`, `_` or `~` that may open or close formatting.
        case delims(ch: Unicode.Scalar, len: Int, open: Bool, close: Bool)
        case code(String)
        case link(label: [Tok], url: String)
    }

    private static func isASCIIPunctuation(_ c: Unicode.Scalar) -> Bool {
        switch c.value {
        case 33...47, 58...64, 91...96, 123...126: true
        default: false
        }
    }

    private static func isWhitespace(_ c: Unicode.Scalar) -> Bool {
        c.properties.isWhitespace
    }

    private static func isAlphanumeric(_ c: Unicode.Scalar) -> Bool {
        c.properties.isAlphabetic || c.properties.numericType != nil
    }

    private static func string(_ scalars: ArraySlice<Unicode.Scalar>) -> String {
        var view = String.UnicodeScalarView()
        view.append(contentsOf: scalars)
        return String(view)
    }

    private static func tokenize(_ chars: ArraySlice<Unicode.Scalar>, foreign: Bool) -> [Tok] {
        var toks: [Tok] = []
        var buf = String.UnicodeScalarView()
        func flush() {
            if !buf.isEmpty {
                toks.append(.text(String(buf)))
                buf = String.UnicodeScalarView()
            }
        }
        let n = chars.endIndex
        var i = chars.startIndex
        while i < n {
            let c = chars[i]
            switch c {
            case "\\" where i + 1 < n && isASCIIPunctuation(chars[i + 1]):
                buf.append(chars[i + 1])
                i += 2
            case "`":
                if let end = findCodeEnd(chars, from: i + 1) {
                    flush()
                    toks.append(.code(unescapeCode(chars[(i + 1)..<end])))
                    i = end + 1
                } else {
                    buf.append(c)
                    i += 1
                }
            case "[":
                if let (labelEnd, urlEnd) = findLink(chars, start: i) {
                    flush()
                    let label = tokenize(chars[(i + 1)..<labelEnd], foreign: foreign)
                    let raw = string(chars[(labelEnd + 2)..<urlEnd])
                    let url = raw.unicodeScalars.split(whereSeparator: isWhitespace).first.map { String($0) } ?? ""
                    toks.append(.link(label: label, url: url))
                    i = urlEnd + 1
                } else {
                    buf.append(c)
                    i += 1
                }
            case "*", "_", "~":
                var len = 1
                while i + len < n && chars[i + len] == c {
                    len += 1
                }
                let prev: Unicode.Scalar? = i > chars.startIndex ? chars[i - 1] : nil
                let next: Unicode.Scalar? = i + len < n ? chars[i + len] : nil
                var open = next.map { !isWhitespace($0) } ?? false
                var close = prev.map { !isWhitespace($0) } ?? false
                let intraword = (prev.map(isAlphanumeric) ?? false) && (next.map(isAlphanumeric) ?? false)
                if foreign && c == "_" && intraword {
                    open = false
                    close = false
                }
                let usable = c == "~" ? (len == 2 || len == 4) : len <= 3
                if usable && (open || close) {
                    flush()
                    toks.append(.delims(ch: c, len: len, open: open, close: close))
                } else {
                    for _ in 0..<len { buf.append(c) }
                }
                i += len
            default:
                buf.append(c)
                i += 1
            }
        }
        flush()
        return toks
    }

    private static func findCodeEnd(_ chars: ArraySlice<Unicode.Scalar>, from: Int) -> Int? {
        var j = from
        while j < chars.endIndex {
            if chars[j] == "\\", j + 1 < chars.endIndex, chars[j + 1] == "`" || chars[j + 1] == "\\" {
                j += 2
            } else if chars[j] == "`" {
                return j
            } else {
                j += 1
            }
        }
        return nil
    }

    private static func unescapeCode(_ chars: ArraySlice<Unicode.Scalar>) -> String {
        var out = String.UnicodeScalarView()
        var j = chars.startIndex
        while j < chars.endIndex {
            if chars[j] == "\\", j + 1 < chars.endIndex, chars[j + 1] == "`" || chars[j + 1] == "\\" {
                out.append(chars[j + 1])
                j += 2
            } else {
                out.append(chars[j])
                j += 1
            }
        }
        return String(out)
    }

    /// For a `[` at `start`, the index of its `]` and of the `)` closing `(url)`.
    private static func findLink(_ chars: ArraySlice<Unicode.Scalar>, start: Int) -> (Int, Int)? {
        let n = chars.endIndex
        var depth = 0
        var j = start
        scan: while j < n {
            switch chars[j] {
            case "\\":
                j += 2
                continue scan
            case "[":
                depth += 1
            case "]":
                depth -= 1
                if depth == 0 {
                    break scan
                }
            default:
                break
            }
            j += 1
        }
        guard j < n, j + 1 < n, chars[j + 1] == "(" else { return nil }
        var k = j + 2
        while k < n && chars[k] != ")" {
            if chars[k] == "\n" {
                return nil
            }
            k += 1
        }
        guard k < n else { return nil }
        let url = string(chars[(j + 2)..<k])
        guard !url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        return (j, k)
    }

    // MARK: Pairing

    private enum Role {
        case open
        case close
    }

    /// One use of part of a delimiter run.
    private struct Use {
        var kind: Kind
        var width: Int
        var role: Role
        var matched: Bool
    }

    private static func width(_ ch: Unicode.Scalar, _ kind: Kind) -> Int {
        if ch == "~" { return 2 }
        return kind == .bold ? 2 : 1
    }

    /// Pair openers with closers, CommonMark style but simpler: a closing run
    /// closes the innermost matching opener; openers left unpaired become text.
    private static func pair(_ toks: [Tok]) -> [(uses: [Use], left: Int)] {
        var uses: [(uses: [Use], left: Int)] = toks.map { _ in ([], 0) }
        var stack: [(tok: Int, use: Int, ch: Unicode.Scalar)] = []
        for (t, tok) in toks.enumerated() {
            guard case let .delims(ch, len, open, close) = tok else { continue }
            var left = len
            if close {
                while left > 0 {
                    guard let pos = stack.lastIndex(where: { $0.ch == ch }) else { break }
                    let (ot, ou, _) = stack[pos]
                    let kind = uses[ot].uses[ou].kind
                    let w = width(ch, kind)
                    if left < w {
                        break
                    }
                    stack.removeSubrange(pos...)
                    uses[ot].uses[ou].matched = true
                    uses[t].uses.append(Use(kind: kind, width: w, role: .close, matched: true))
                    left -= w
                }
            }
            if open && left > 0 {
                let kinds: [Kind]
                switch (ch, left) {
                case ("~", 2): kinds = [.strike]
                case ("~", _): kinds = []
                case (_, 1): kinds = [.italic]
                case (_, 2): kinds = [.bold]
                case (_, 3): kinds = [.bold, .italic]
                default: kinds = []
                }
                for kind in kinds {
                    let w = width(ch, kind)
                    uses[t].uses.append(Use(kind: kind, width: w, role: .open, matched: false))
                    stack.append((t, uses[t].uses.count - 1, ch))
                    left -= w
                }
            }
            uses[t].left = left
        }
        return uses
    }

    private static func pushRun(_ out: inout [Run], _ text: String, _ marks: Marks, _ link: String?) {
        if text.isEmpty {
            return
        }
        if let last = out.indices.last, out[last].marks == marks, out[last].link == link {
            out[last].text += text
            return
        }
        out.append(Run(text: text, marks: marks, link: link))
    }

    private static func build(_ toks: [Tok], base: Marks, link: String?, into out: inout [Run]) {
        let uses = pair(toks)
        var depth = [0, 0, 0]
        func marksNow() -> Marks {
            Marks(
                bold: base.bold || depth[0] > 0,
                italic: base.italic || depth[1] > 0,
                strike: base.strike || depth[2] > 0,
                code: base.code
            )
        }
        func repeated(_ ch: Unicode.Scalar, _ count: Int) -> String {
            String(repeating: String(ch), count: count)
        }
        for (t, tok) in toks.enumerated() {
            switch tok {
            case let .text(s):
                pushRun(&out, s, marksNow(), link)
            case let .code(s):
                var m = marksNow()
                m.code = true
                pushRun(&out, s, m, link)
            case let .link(label, url):
                build(label, base: marksNow(), link: url, into: &out)
            case let .delims(ch, _, _, _):
                for u in uses[t].uses {
                    if u.matched {
                        let slot = u.kind.rawValue
                        switch u.role {
                        case .open: depth[slot] += 1
                        case .close: depth[slot] = max(depth[slot] - 1, 0)
                        }
                    } else {
                        pushRun(&out, repeated(ch, u.width), marksNow(), link)
                    }
                }
                if uses[t].left > 0 {
                    pushRun(&out, repeated(ch, uses[t].left), marksNow(), link)
                }
            }
        }
    }
}
