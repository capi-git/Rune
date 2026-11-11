// A block's text with its inline formatting: bold, italic, strikethrough,
// inline code and links.

import Notion2Core
import SwiftUI

enum InlineText {
    /// Stored inline Markdown as text ready to show.
    static func attributed(_ stored: String, size: CGFloat, weight: Font.Weight = .regular) -> AttributedString {
        var out = AttributedString()
        for run in Inline.parse(stored) {
            var piece = AttributedString(run.text)
            var font: Font = run.marks.code
                ? .system(size: size * 0.9, weight: weight, design: .monospaced)
                : .system(size: size, weight: run.marks.bold ? .bold : weight)
            if run.marks.italic {
                font = font.italic()
            }
            piece.font = font
            if run.marks.strike {
                piece.strikethroughStyle = .single
            }
            if run.marks.code {
                piece.backgroundColor = Theme.field
            }
            if let link = run.link, Inline.isSafeURL(link), let url = URL(string: link.trimmingCharacters(in: .whitespaces)) {
                piece.link = url
                piece.foregroundColor = Theme.accent
                piece.underlineStyle = .single
            }
            out.append(piece)
        }
        return out
    }

    /// Plain text with every place `query` is found marked.
    static func highlighted(_ text: String, query: String) -> AttributedString {
        var out = AttributedString()
        var from = text.startIndex
        for range in SearchIndex.ranges(of: query.trimmingCharacters(in: .whitespacesAndNewlines), in: text) {
            out.append(AttributedString(String(text[from..<range.lowerBound])))
            var hit = AttributedString(String(text[range]))
            hit.backgroundColor = Theme.mark
            hit.foregroundColor = Theme.text
            out.append(hit)
            from = range.upperBound
        }
        out.append(AttributedString(String(text[from...])))
        return out
    }
}

extension View {
    /// Links inside text: a link to a page opens the page here; a web or mail
    /// link opens in its app; anything else does nothing.
    func opensLinks(with router: Router, in workspace: Workspace) -> some View {
        environment(\.openURL, OpenURLAction { url in
            if let id = Inline.pageID(ofLink: url.absoluteString) {
                if workspace.pages[id] != nil {
                    router.openPage(id)
                }
                return .handled
            }
            return Inline.isSafeURL(url.absoluteString) ? .systemAction : .discarded
        })
    }
}
