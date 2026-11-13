// The band across the top of a page, and the pictures kept with the notes.
// Mirrors `covers` in `src/model.rs` and `src/files.rs` of the desktop app.

import Foundation

/// A page's cover: one of the app's own colours and gradients, or a picture.
public enum Cover: Hashable, Sendable {
    case preset(CoverPreset)
    /// The file name of a picture (see `Pictures`).
    case file(String)

    /// The cover a page's stored value means ("preset:<name>" or
    /// "file:<name>"); nil for none, and for one this app does not know.
    public init?(stored: String?) {
        guard let stored else { return nil }
        if stored.hasPrefix("preset:"), let preset = CoverPreset.named(String(stored.dropFirst("preset:".count))) {
            self = .preset(preset)
        } else if stored.hasPrefix("file:"), Pictures.isValidName(String(stored.dropFirst("file:".count))) {
            self = .file(String(stored.dropFirst("file:".count)))
        } else {
            return nil
        }
    }
}

/// One of the app's own covers: a plain colour, or a gradient that may have
/// a soft glow on top. Colours are 0xRRGGBB.
public struct CoverPreset: Hashable, Sendable {
    public struct Stop: Hashable, Sendable {
        public var color: UInt32
        /// 0 at the start of the gradient … 1 at its end.
        public var at: Double
    }

    /// A soft oval of colour that fades out (CSS `radial-gradient`).
    public struct Glow: Hashable, Sendable {
        public var color: UInt32
        /// The centre, as parts of the width and the height.
        public var x: Double
        public var y: Double
        /// The radii, as parts of the width and the height.
        public var width: Double
        public var height: Double
        /// Where the colour has faded out, as a part of the radius.
        public var fade: Double
    }

    public var name: String
    public var label: String
    /// The angle of the gradient as CSS counts it: 90 runs left to right,
    /// 180 top to bottom.
    public var angle: Double
    /// One stop is a plain colour.
    public var stops: [Stop]
    public var glow: Glow?

    private static func gradient(_ name: String, _ label: String, _ angle: Double, _ stops: [(UInt32, Double)], glow: Glow? = nil) -> CoverPreset {
        CoverPreset(name: name, label: label, angle: angle, stops: stops.map { Stop(color: $0.0, at: $0.1) }, glow: glow)
    }

    private static func plain(_ name: String, _ label: String, _ color: UInt32) -> CoverPreset {
        CoverPreset(name: name, label: label, angle: 180, stops: [Stop(color: color, at: 0)], glow: nil)
    }

    /// The same list, in the same order, as `covers::PRESETS` in `src/model.rs`.
    public static let all: [CoverPreset] = [
        gradient("sand", "Sand", 135, [(0xEFE4CF, 0), (0xDCC7A3, 0.55), (0xBFA27C, 1)]),
        gradient("clay", "Clay", 135, [(0xF0C9AE, 0), (0xD98F6C, 0.55), (0xA85A43, 1)]),
        gradient("ember", "Ember", 120, [(0xF6D29A, 0), (0xE58A45, 0.5), (0xA8402F, 1)]),
        gradient("moss", "Moss", 135, [(0xD5E3BD, 0), (0x8FB58A, 0.55), (0x456F5F, 1)]),
        gradient("sea", "Sea", 135, [(0xBFE3E0, 0), (0x6AA9BD, 0.55), (0x2F5F85, 1)]),
        gradient("slate", "Slate", 135, [(0xCFD6DD, 0), (0x8794A3, 0.55), (0x46505E, 1)]),
        gradient("dawn", "Dawn", 120, [(0xF7E3C8, 0), (0xF0B9A8, 0.45), (0xB9A3C4, 1)]),
        gradient("night", "Night", 135, [(0x1F2A44, 0), (0x2F4F6F, 0.55), (0x5F8F8A, 1)]),
        gradient("meadow", "Meadow", 135, [(0xB9D99A, 0), (0x5F9F86, 1)], glow: Glow(color: 0xF3E7B1, x: 0.15, y: 0.20, width: 1.2, height: 1.4, fade: 0.55)),
        gradient("shore", "Shore", 135, [(0x9FD0D8, 0), (0x4F86A8, 1)], glow: Glow(color: 0xF6DCC0, x: 0.85, y: 0.15, width: 1.1, height: 1.3, fade: 0.55)),
        plain("stone", "Stone", 0xB9B4AB),
        plain("rust", "Rust", 0xC0623F),
        plain("honey", "Honey", 0xE0B24A),
        plain("fern", "Fern", 0x6F9F6A),
        plain("lake", "Lake", 0x4F86B5),
        plain("plum", "Plum", 0x8A5F86),
    ]

    public static func named(_ name: String) -> CoverPreset? {
        all.first { $0.name == name }
    }
}

/// The pictures kept with the notes: the images in pages, and page covers.
/// Each is a file with a name the app made, never a row of the database.
public enum Pictures {
    /// The endings of the pictures the app keeps.
    public static let endings: Set<String> = ["png", "jpg", "gif", "webp", "heic", "tiff", "bmp"]

    /// A name the app made: letters and digits, one dot, a known ending.
    /// Nothing else is ever read as a picture.
    public static func isValidName(_ name: String) -> Bool {
        let parts = name.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 2, let stem = parts.first, let ending = parts.last else { return false }
        return !stem.isEmpty && stem.count <= 40
            && stem.unicodeScalars.allSatisfy { $0.isASCII && ($0.properties.isAlphabetic || $0.properties.numericType != nil) }
            && endings.contains(String(ending))
    }
}
