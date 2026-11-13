// One block of a page, drawn by its kind.

import Notion2Core
import SwiftUI

struct BlockView: View {
    /// The page the block is on (a chart finds its table there).
    let pageID: ID
    let block: Block
    /// The number of a numbered-list item.
    let number: Int?
    /// Whether a toggle is open.
    let open: Bool
    let toggle: () -> Void

    @Environment(AppModel.self) private var model

    /// The face of the page's title and headings (`Page.font`).
    private var pageFace: Font.Design {
        PageFace.design(model.workspace.pages[pageID]?.font ?? "")
    }

    private static let bodySize: CGFloat = 16
    /// How far each level of nesting moves a block to the right.
    private static let indentStep: CGFloat = 26
    /// The mark of a bulleted item, by level. The last one is a small square
    /// in its text form (without the selector it shows as an emoji).
    private static let bullets = ["\u{2022}", "\u{25E6}", "\u{25AA}\u{FE0E}"]

    /// The room above a block, which depends on what is above it.
    static func spacing(before kind: BlockKind, after previous: BlockKind?) -> CGFloat {
        guard let previous else { return 8 }
        let lists: Set<BlockKind> = [.bulleted, .numbered, .todo, .toggle]
        switch kind {
        case .heading1: return 26
        case .heading2: return 22
        case .heading3: return 18
        case .divider: return 10
        case .callout, .code, .quote, .drawing, .image, .table, .chart: return 14
        case .pageLink: return previous == .pageLink ? 6 : 12
        default:
            if lists.contains(kind) && lists.contains(previous) {
                return 6
            }
            return [.heading1, .heading2, .heading3].contains(previous) ? 8 : 10
        }
    }

    var body: some View {
        content
            .padding(.leading, CGFloat(block.indent) * Self.indentStep)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var content: some View {
        switch block.kind {
        case .text:
            text()
        case .heading1:
            text(size: 25, weight: .bold, spacing: 2).fontDesign(pageFace).accessibilityAddTraits(.isHeader)
        case .heading2:
            text(size: 20, weight: .semibold, spacing: 2).fontDesign(pageFace).accessibilityAddTraits(.isHeader)
        case .heading3:
            text(size: 17, weight: .semibold, spacing: 2).fontDesign(pageFace).accessibilityAddTraits(.isHeader)
        case .bulleted:
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text(Self.bullets[block.indent % 3])
                    .font(.system(size: Self.bodySize, weight: .semibold))
                    .foregroundStyle(Theme.text)
                    .frame(width: 22, alignment: .leading)
                    .accessibilityHidden(true)
                text()
            }
        case .numbered:
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text("\(number ?? 1).")
                    .font(.system(size: Self.bodySize).monospacedDigit())
                    .foregroundStyle(Theme.text)
                    .frame(minWidth: 18, alignment: .leading)
                text()
            }
        case .todo:
            HStack(alignment: .top, spacing: 8) {
                Checkbox(checked: block.checked)
                    .padding(.top, 1.5)
                    .contentShape(Rectangle().inset(by: -10))
                    .onTapGesture {
                        guard model.canEdit else { return }
                        let (checked, id, page) = (!block.checked, block.id, pageID)
                        Task { await model.setChecked(checked, block: id, page: page) }
                    }
                text(struck: block.checked, color: block.checked ? Theme.text2 : Theme.text)
            }
            .accessibilityElement(children: .combine)
            .accessibilityValue(block.checked ? "Done" : "Not done")
        case .toggle:
            HStack(alignment: .top, spacing: 4) {
                Button(action: toggle) {
                    IconView("triangle-right", size: 16)
                        .foregroundStyle(Theme.text2)
                        .rotationEffect(.degrees(open ? 90 : 0))
                        .frame(width: 22, height: 24)
                        .contentShape(Rectangle().inset(by: -10))
                }
                .buttonStyle(.plain)
                .accessibilityLabel(open ? "Collapse" : "Expand")
                text()
            }
        case .quote:
            text()
                .padding(.leading, 15)
                .overlay(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 1.5)
                        .fill(Theme.text)
                        .frame(width: 3)
                }
        case .callout:
            let stored = block.icon ?? "lightbulb:yellow"
            HStack(alignment: .top, spacing: 10) {
                PageIconView(icon: stored, size: 20)
                    .padding(.top, 1)
                text(size: 15, spacing: 4)
            }
            .padding(.vertical, 12)
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.tint(Icons.parse(stored).color).eventFill, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        case .code where block.language?.lowercased() == "mermaid":
            DiagramView(source: block.text)
        case .code:
            VStack(alignment: .leading, spacing: 6) {
                if let language = block.language, !language.isEmpty {
                    Text(language)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Theme.text2)
                }
                ScrollView(.horizontal) {
                    Text(block.text.isEmpty ? " " : block.text)
                        .font(.system(size: 13.5, design: .monospaced))
                        .lineSpacing(3)
                        .foregroundStyle(Theme.text)
                        .fixedSize(horizontal: true, vertical: true)
                }
                .scrollIndicators(.hidden)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.field, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        case .divider:
            Rectangle()
                .fill(Theme.separator)
                .frame(height: 1)
                .padding(.vertical, 2)
                .accessibilityHidden(true)
        case .pageLink:
            pageLink
        case .drawing:
            standIn("drawing", "Drawing", "Drawings show on the Mac. The iPhone does not draw them yet.")
        case .image:
            if block.data.isEmpty {
                standIn(pictureIcon, "Image", "No picture is chosen yet.")
            } else {
                PictureLoader(name: block.data) { image in
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .accessibilityLabel("Image")
                } missing: {
                    standIn(pictureIcon, "Image", "This picture is not on the iPhone.")
                }
            }
        case .table:
            if let table = TableData.parse(block.data) {
                TableBlockView(pageID: pageID, blockID: block.id, table: table)
            } else {
                standIn("note", "Table", "This table cannot be read.")
            }
        case .chart:
            ChartBlockView(pageID: pageID, block: block)
        case .locked:
            standIn("lock", "This page is locked", "It opens only in Rune on the Mac, with its password.")
        }
    }

    /// The icon for a picture. The Mac's icon set gained "image" after the
    /// iPhone's copy of it was made; until that copy is made again, a
    /// camera stands for it.
    private var pictureIcon: String {
        Icons.exists("image") ? "image" : "camera"
    }

    /// A quiet row in place of something the iPhone cannot show.
    private func standIn(_ icon: String, _ title: String, _ detail: String) -> some View {
        HStack(spacing: 12) {
            IconView(icon, size: 24)
                .foregroundStyle(Theme.text2)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(Theme.text)
                Text(detail)
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.text2)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.field, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    /// The block's text with its inline formatting.
    private func text(
        size: CGFloat = bodySize,
        weight: Font.Weight = .regular,
        spacing: CGFloat = 5,
        struck: Bool = false,
        color: Color = Theme.text
    ) -> some View {
        let shown = block.text.isEmpty ? AttributedString(" ") : InlineText.attributed(block.text, size: size, weight: weight)
        return Text(shown)
            .font(.system(size: size, weight: weight))
            .strikethrough(struck)
            .foregroundStyle(color)
            .lineSpacing(spacing)
            .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder
    private var pageLink: some View {
        if let target = block.target, let page = model.workspace.pages[target], !page.trashed {
            NavigationLink(value: Route.page(page.id)) {
                HStack(spacing: 10) {
                    PageIconView(icon: page.icon, size: 20)
                    Text(page.displayTitle)
                        .font(.system(size: 16, weight: .medium))
                        .foregroundStyle(Theme.text)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    IconView("chevron-right", size: 15, weight: 1.9)
                        .foregroundStyle(Theme.text2)
                }
                .padding(.horizontal, 12)
                .frame(height: 46)
                .background(Theme.field, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        } else {
            HStack(spacing: 10) {
                IconView("file", size: 20)
                Text(block.target.flatMap { model.workspace.pages[$0] } == nil ? "Page not found" : "Page in the trash")
                    .font(.system(size: 16))
            }
            .foregroundStyle(Theme.text2)
            .padding(.horizontal, 12)
            .frame(height: 46)
        }
    }
}

/// The box of a to-do, as in the designs.
private struct Checkbox: View {
    let checked: Bool

    var body: some View {
        ZStack {
            if checked {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(Theme.check)
                IconView("check", size: 14, weight: 2.6)
                    .foregroundStyle(Theme.checkMark)
            } else {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .strokeBorder(Theme.checkEmpty, lineWidth: 1.75)
            }
        }
        .frame(width: 20, height: 20)
        .accessibilityHidden(true)
    }
}

/// The face a page's title and headings are set in: "" (the app's own),
/// "serif" or "mono", as on the Mac (`PAGE_FONTS` in src/model.rs).
enum PageFace {
    static func design(_ stored: String) -> Font.Design {
        switch stored {
        case "serif": .serif
        case "mono": .monospaced
        default: .default
        }
    }
}
