// Draws the app's own icons (the same drawings as the desktop app).

import RuneCore
import SwiftUI

/// An icon in the current foreground colour.
struct IconView: View {
    let name: String
    var size: CGFloat = 20
    /// Stroke width on the 24-point grid the icons are drawn on.
    var weight: CGFloat = 1.75

    init(_ name: String, size: CGFloat = 20, weight: CGFloat = 1.75) {
        self.name = name
        self.size = size
        self.weight = weight
    }

    var body: some View {
        let shapes = IconCache.shapes(for: name)
        let weight = weight
        // SwiftUI may draw from another thread: the closure is not on the main actor.
        return Canvas { @Sendable context, canvas in
            let scale = canvas.width / 24
            let transform = CGAffineTransform(scaleX: scale, y: scale)
            for shape in shapes {
                let path = shape.path.applying(transform)
                if shape.fill {
                    context.fill(path, with: .foreground)
                }
                if shape.stroke {
                    var layer = context
                    layer.opacity = shape.strokeOpacity
                    layer.stroke(
                        path,
                        with: .foreground,
                        style: StrokeStyle(lineWidth: (shape.strokeWidth ?? weight) * scale, lineCap: .round, lineJoin: .round)
                    )
                }
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// The icon of a page or callout in its colour. `tile` draws it on a soft
/// tile of that colour. A page with no icon shows a quiet page glyph.
struct PageIconView: View {
    let icon: String?
    var size: CGFloat = 20
    var tile = false

    var body: some View {
        let ref = Icons.parse(icon)
        let tint = Theme.tint(ref.color)
        let hasIcon = !(icon ?? "").isEmpty
        if tile {
            IconView(ref.name, size: size * 0.53)
                .foregroundStyle(tint.icon)
                .frame(width: size, height: size)
                .background(tint.tile, in: RoundedRectangle(cornerRadius: size * 0.31, style: .continuous))
        } else {
            IconView(ref.name, size: size)
                .foregroundStyle(hasIcon ? tint.icon : Theme.text2)
        }
    }
}

/// Icon outlines, made once.
enum IconCache {
    struct Shape: Sendable {
        let path: Path
        let fill: Bool
        let stroke: Bool
        let strokeWidth: CGFloat?
        let strokeOpacity: Double
    }

    private static var cache: [String: [Shape]] = [:]

    static func shapes(for name: String) -> [Shape] {
        if let known = cache[name] {
            return known
        }
        let made = Icons.shapes(for: name).map { shape in
            var path = Path()
            for element in shape.elements {
                switch element {
                case let .move(x, y):
                    path.move(to: CGPoint(x: x, y: y))
                case let .line(x, y):
                    path.addLine(to: CGPoint(x: x, y: y))
                case let .curve(x, y, c1x, c1y, c2x, c2y):
                    path.addCurve(to: CGPoint(x: x, y: y), control1: CGPoint(x: c1x, y: c1y), control2: CGPoint(x: c2x, y: c2y))
                case .close:
                    path.closeSubpath()
                }
            }
            return Shape(
                path: path,
                fill: shape.fill,
                stroke: shape.stroke,
                strokeWidth: shape.strokeWidth.map { CGFloat($0) },
                strokeOpacity: shape.strokeOpacity ?? 1
            )
        }
        cache[name] = made
        return made
    }
}
