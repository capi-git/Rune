// The app's own icon set: line icons on a 24 by 24 grid with a 1.75 stroke.
// The drawings come from the desktop app (`IconData.swift` is generated from
// `src/ui/icons.rs`); this file reads the small part of SVG they use and
// turns stored icons ("name:color", or an old emoji) into a name and a colour.

import Foundation

/// A page or callout icon: which drawing, in which colour.
public struct IconRef: Hashable, Sendable {
    public var name: String
    public var color: String

    public init(name: String, color: String) {
        self.name = name
        self.color = color
    }
}

/// One step of an outline.
public enum PathElement: Hashable, Sendable {
    case move(x: Double, y: Double)
    case line(x: Double, y: Double)
    case curve(x: Double, y: Double, c1x: Double, c1y: Double, c2x: Double, c2y: Double)
    case close
}

/// One shape of an icon.
public struct IconShape: Hashable, Sendable {
    public var elements: [PathElement]
    /// Filled with the icon's colour (otherwise only stroked).
    public var fill: Bool
    public var stroke: Bool
    /// Stroke width, when it is not the icon's usual 1.75.
    public var strokeWidth: Double?
    public var strokeOpacity: Double?
}

public enum Icons {
    public static let defaultName = "file"
    public static let defaultColor = "gray"
    public static let strokeWidth = 1.75

    /// Icon colours, in the order the desktop app offers them.
    public static var colors: [String] { IconData.colors }

    /// Every icon name.
    public static var names: [String] { IconData.markup.keys.sorted() }

    public static func exists(_ name: String) -> Bool {
        IconData.markup[name] != nil
    }

    /// Split a stored icon into name and colour. Accepts "name",
    /// "name:color" and old emoji; anything else is the plain page icon.
    public static func parse(_ stored: String?) -> IconRef {
        let cleaned = String(String.UnicodeScalarView(
            (stored ?? "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .unicodeScalars
                .filter { $0 != "\u{FE0E}" && $0 != "\u{FE0F}" && $0 != "\u{200D}" }
        ))
        let name: String
        let color: String
        if let colon = cleaned.firstIndex(of: ":") {
            name = String(cleaned[..<colon])
            color = String(cleaned[cleaned.index(after: colon)...])
        } else {
            name = cleaned
            color = ""
        }
        if exists(name) {
            return IconRef(name: name, color: colors.contains(color) ? color : defaultColor)
        }
        if let old = IconData.legacyEmoji.first(where: { $0.0 == cleaned }) {
            return IconRef(name: old.1, color: old.2)
        }
        return IconRef(name: defaultName, color: defaultColor)
    }

    /// The shapes of an icon. An unknown name draws the plain page icon.
    public static func shapes(for name: String) -> [IconShape] {
        if let known = parsed[name] {
            return known
        }
        return parsed[defaultName] ?? []
    }

    private static let parsed: [String: [IconShape]] = IconData.markup.mapValues(SVG.shapes)
}

// MARK: - The part of SVG the icons use

enum SVG {
    /// The shapes in a run of `<path>`, `<rect>` and `<circle>` elements.
    static func shapes(in markup: String) -> [IconShape] {
        var out: [IconShape] = []
        var rest = Substring(markup)
        while let open = rest.firstIndex(of: "<") {
            guard let close = rest[open...].firstIndex(of: ">") else { break }
            let tag = rest[rest.index(after: open)..<close]
            rest = rest[rest.index(after: close)...]
            let name = tag.prefix { $0.isLetter }
            let attributes = self.attributes(in: tag.dropFirst(name.count))
            func number(_ key: String) -> Double {
                attributes[key].flatMap { Double($0) } ?? 0
            }
            let elements: [PathElement]
            switch name {
            case "path":
                var data = PathData(attributes["d"] ?? "")
                elements = data.elements()
            case "rect":
                elements = roundedRect(x: number("x"), y: number("y"), width: number("width"), height: number("height"), radius: number("rx"))
            case "circle":
                elements = circle(cx: number("cx"), cy: number("cy"), r: number("r"))
            default:
                continue
            }
            if elements.isEmpty {
                continue
            }
            let fill = attributes["fill"] == "currentColor"
            out.append(IconShape(
                elements: elements,
                fill: fill,
                stroke: attributes["stroke"] != "none",
                strokeWidth: attributes["stroke-width"].flatMap { Double($0) },
                strokeOpacity: attributes["stroke-opacity"].flatMap { Double($0) }
            ))
        }
        return out
    }

    static func attributes(in text: Substring) -> [String: String] {
        var out: [String: String] = [:]
        var rest = text
        while let equals = rest.firstIndex(of: "=") {
            let key = rest[..<equals].trimmingCharacters(in: CharacterSet(charactersIn: " \n\t/"))
            let after = rest[rest.index(after: equals)...]
            guard let quote = after.firstIndex(of: "\"") else { break }
            let valueStart = after.index(after: quote)
            guard let end = after[valueStart...].firstIndex(of: "\"") else { break }
            out[key] = String(after[valueStart..<end])
            rest = after[after.index(after: end)...]
        }
        return out
    }

    /// How far the control points of a quarter circle are from its ends.
    private static let kappa = 0.552_284_749_830_793_4

    static func circle(cx: Double, cy: Double, r: Double) -> [PathElement] {
        guard r > 0 else { return [] }
        let k = kappa * r
        return [
            .move(x: cx + r, y: cy),
            .curve(x: cx, y: cy + r, c1x: cx + r, c1y: cy + k, c2x: cx + k, c2y: cy + r),
            .curve(x: cx - r, y: cy, c1x: cx - k, c1y: cy + r, c2x: cx - r, c2y: cy + k),
            .curve(x: cx, y: cy - r, c1x: cx - r, c1y: cy - k, c2x: cx - k, c2y: cy - r),
            .curve(x: cx + r, y: cy, c1x: cx + k, c1y: cy - r, c2x: cx + r, c2y: cy - k),
            .close,
        ]
    }

    static func roundedRect(x: Double, y: Double, width: Double, height: Double, radius: Double) -> [PathElement] {
        guard width > 0, height > 0 else { return [] }
        let r = min(max(radius, 0), width / 2, height / 2)
        if r == 0 {
            return [.move(x: x, y: y), .line(x: x + width, y: y), .line(x: x + width, y: y + height), .line(x: x, y: y + height), .close]
        }
        let k = kappa * r
        let (x1, y1) = (x + width, y + height)
        return [
            .move(x: x + r, y: y),
            .line(x: x1 - r, y: y),
            .curve(x: x1, y: y + r, c1x: x1 - r + k, c1y: y, c2x: x1, c2y: y + r - k),
            .line(x: x1, y: y1 - r),
            .curve(x: x1 - r, y: y1, c1x: x1, c1y: y1 - r + k, c2x: x1 - r + k, c2y: y1),
            .line(x: x + r, y: y1),
            .curve(x: x, y: y1 - r, c1x: x + r - k, c1y: y1, c2x: x, c2y: y1 - r + k),
            .line(x: x, y: y + r),
            .curve(x: x + r, y: y, c1x: x, c1y: y + r - k, c2x: x + r - k, c2y: y),
            .close,
        ]
    }
}

/// Reads the `d` of an SVG path.
struct PathData {
    private let bytes: [UInt8]
    private var i = 0

    init(_ d: String) {
        bytes = Array(d.utf8)
    }

    private mutating func skipSeparators() {
        while i < bytes.count, bytes[i] == 0x20 || bytes[i] == 0x2C || bytes[i] == 0x0A || bytes[i] == 0x09 || bytes[i] == 0x0D {
            i += 1
        }
    }

    private static func isCommand(_ b: UInt8) -> Bool {
        let lower = b | 0x20
        return lower >= 0x61 && lower <= 0x7A && lower != 0x65 // a letter, not "e"
    }

    private mutating func number() -> Double? {
        skipSeparators()
        let start = i
        if i < bytes.count, bytes[i] == 0x2B || bytes[i] == 0x2D {
            i += 1
        }
        var digits = 0
        while i < bytes.count, bytes[i] >= 0x30, bytes[i] <= 0x39 {
            i += 1
            digits += 1
        }
        if i < bytes.count, bytes[i] == 0x2E {
            i += 1
            while i < bytes.count, bytes[i] >= 0x30, bytes[i] <= 0x39 {
                i += 1
                digits += 1
            }
        }
        guard digits > 0 else {
            i = start
            return nil
        }
        if i < bytes.count, bytes[i] | 0x20 == 0x65 {
            var j = i + 1
            if j < bytes.count, bytes[j] == 0x2B || bytes[j] == 0x2D {
                j += 1
            }
            if j < bytes.count, bytes[j] >= 0x30, bytes[j] <= 0x39 {
                while j < bytes.count, bytes[j] >= 0x30, bytes[j] <= 0x39 {
                    j += 1
                }
                i = j
            }
        }
        return Double(String(decoding: bytes[start..<i], as: UTF8.self))
    }

    /// An arc's flag: one "0" or "1", which may have no space after it.
    private mutating func flag() -> Bool? {
        skipSeparators()
        guard i < bytes.count, bytes[i] == 0x30 || bytes[i] == 0x31 else { return nil }
        defer { i += 1 }
        return bytes[i] == 0x31
    }

    mutating func elements() -> [PathElement] {
        var out: [PathElement] = []
        var x = 0.0, y = 0.0 // current point
        var startX = 0.0, startY = 0.0 // start of the subpath
        var lastC2: (Double, Double)? // second control point of the last curve
        var lastQ: (Double, Double)? // control point of the last quadratic
        var command: UInt8 = 0

        func curve(_ c1x: Double, _ c1y: Double, _ c2x: Double, _ c2y: Double, _ ex: Double, _ ey: Double) {
            out.append(.curve(x: ex, y: ey, c1x: c1x, c1y: c1y, c2x: c2x, c2y: c2y))
        }

        while true {
            skipSeparators()
            guard i < bytes.count else { break }
            if PathData.isCommand(bytes[i]) {
                command = bytes[i]
                i += 1
            } else if command == 0 {
                break
            }
            let relative = command >= 0x61
            let (ox, oy) = relative ? (x, y) : (0, 0)
            switch command | 0x20 {
            case UInt8(ascii: "m"):
                guard let px = number(), let py = number() else { return out }
                x = ox + px
                y = oy + py
                startX = x
                startY = y
                out.append(.move(x: x, y: y))
                // More pairs after a move are lines.
                command = relative ? UInt8(ascii: "l") : UInt8(ascii: "L")
                lastC2 = nil
                lastQ = nil
            case UInt8(ascii: "l"):
                guard let px = number(), let py = number() else { return out }
                x = ox + px
                y = oy + py
                out.append(.line(x: x, y: y))
                lastC2 = nil
                lastQ = nil
            case UInt8(ascii: "h"):
                guard let px = number() else { return out }
                x = ox + px
                out.append(.line(x: x, y: y))
                lastC2 = nil
                lastQ = nil
            case UInt8(ascii: "v"):
                guard let py = number() else { return out }
                y = oy + py
                out.append(.line(x: x, y: y))
                lastC2 = nil
                lastQ = nil
            case UInt8(ascii: "c"):
                guard let a = number(), let b = number(), let c = number(), let d = number(), let e = number(), let f = number() else { return out }
                curve(ox + a, oy + b, ox + c, oy + d, ox + e, oy + f)
                lastC2 = (ox + c, oy + d)
                lastQ = nil
                x = ox + e
                y = oy + f
            case UInt8(ascii: "s"):
                guard let c = number(), let d = number(), let e = number(), let f = number() else { return out }
                let (c1x, c1y) = lastC2.map { (2 * x - $0.0, 2 * y - $0.1) } ?? (x, y)
                curve(c1x, c1y, ox + c, oy + d, ox + e, oy + f)
                lastC2 = (ox + c, oy + d)
                lastQ = nil
                x = ox + e
                y = oy + f
            case UInt8(ascii: "q"), UInt8(ascii: "t"):
                let qx: Double, qy: Double
                if command | 0x20 == UInt8(ascii: "q") {
                    guard let a = number(), let b = number() else { return out }
                    qx = ox + a
                    qy = oy + b
                } else {
                    (qx, qy) = lastQ.map { (2 * x - $0.0, 2 * y - $0.1) } ?? (x, y)
                }
                guard let e = number(), let f = number() else { return out }
                let (ex, ey) = (ox + e, oy + f)
                curve(x + 2 / 3 * (qx - x), y + 2 / 3 * (qy - y), ex + 2 / 3 * (qx - ex), ey + 2 / 3 * (qy - ey), ex, ey)
                lastQ = (qx, qy)
                lastC2 = nil
                x = ex
                y = ey
            case UInt8(ascii: "a"):
                guard let rx = number(), let ry = number(), let rotation = number(),
                      let large = flag(), let sweep = flag(),
                      let e = number(), let f = number()
                else { return out }
                let (ex, ey) = (ox + e, oy + f)
                out.append(contentsOf: PathData.arc(x0: x, y0: y, x1: ex, y1: ey, rx: rx, ry: ry, rotation: rotation, large: large, sweep: sweep))
                lastC2 = nil
                lastQ = nil
                x = ex
                y = ey
            case UInt8(ascii: "z"):
                out.append(.close)
                x = startX
                y = startY
                lastC2 = nil
                lastQ = nil
                command = 0
            default:
                return out
            }
        }
        return out
    }

    /// An SVG arc as curves (each at most a quarter turn).
    static func arc(x0: Double, y0: Double, x1: Double, y1: Double, rx: Double, ry: Double, rotation: Double, large: Bool, sweep: Bool) -> [PathElement] {
        if x0 == x1 && y0 == y1 {
            return []
        }
        var rx = abs(rx), ry = abs(ry)
        if rx == 0 || ry == 0 {
            return [.line(x: x1, y: y1)]
        }
        let phi = rotation * .pi / 180
        let (cosPhi, sinPhi) = (cos(phi), sin(phi))
        let dx = (x0 - x1) / 2, dy = (y0 - y1) / 2
        let xp = cosPhi * dx + sinPhi * dy
        let yp = -sinPhi * dx + cosPhi * dy
        let lambda = (xp * xp) / (rx * rx) + (yp * yp) / (ry * ry)
        if lambda > 1 {
            rx *= lambda.squareRoot()
            ry *= lambda.squareRoot()
        }
        let numerator = rx * rx * ry * ry - rx * rx * yp * yp - ry * ry * xp * xp
        let denominator = rx * rx * yp * yp + ry * ry * xp * xp
        var factor = denominator == 0 ? 0 : max(0, numerator / denominator).squareRoot()
        if large == sweep {
            factor = -factor
        }
        let cxp = factor * rx * yp / ry
        let cyp = -factor * ry * xp / rx
        let cx = cosPhi * cxp - sinPhi * cyp + (x0 + x1) / 2
        let cy = sinPhi * cxp + cosPhi * cyp + (y0 + y1) / 2

        func angle(_ ux: Double, _ uy: Double, _ vx: Double, _ vy: Double) -> Double {
            atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        }
        let (ux, uy) = ((xp - cxp) / rx, (yp - cyp) / ry)
        let theta = angle(1, 0, ux, uy)
        var delta = angle(ux, uy, (-xp - cxp) / rx, (-yp - cyp) / ry)
        if !sweep && delta > 0 {
            delta -= 2 * .pi
        } else if sweep && delta < 0 {
            delta += 2 * .pi
        }
        let count = max(1, Int((abs(delta) / (.pi / 2) - 1e-9).rounded(.up)))
        let step = delta / Double(count)
        let t = 4 / 3 * tan(step / 4)
        func point(_ ex: Double, _ ey: Double) -> (Double, Double) {
            (cx + cosPhi * rx * ex - sinPhi * ry * ey, cy + sinPhi * rx * ex + cosPhi * ry * ey)
        }
        var out: [PathElement] = []
        var a = theta
        for n in 0..<count {
            let b = a + step
            let c1 = point(cos(a) - t * sin(a), sin(a) + t * cos(a))
            let c2 = point(cos(b) + t * sin(b), sin(b) - t * cos(b))
            // End exactly on the arc's end, so the outline has no gaps.
            let end = n == count - 1 ? (x1, y1) : point(cos(b), sin(b))
            out.append(.curve(x: end.0, y: end.1, c1x: c1.0, c1y: c1.1, c2x: c2.0, c2y: c2.1))
            a = b
        }
        return out
    }
}
