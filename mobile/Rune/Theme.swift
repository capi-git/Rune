// Colours and surfaces, from the mobile designs ("Notion 2.0 Mobile").
// Light mode sits on a soft tinted ground; dark mode is neutral grey
// throughout, with colour only on icons and events, quietly.

import RuneCore
import SwiftUI
import UIKit

extension Color {
    nonisolated init(hex: UInt32, opacity: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
        )
    }

    /// One colour in light mode and another in dark mode.
    ///
    /// SwiftUI may ask for the colour from a drawing thread, so the closure
    /// must not belong to the main actor.
    nonisolated static func dynamic(light: Color, dark: Color) -> Color {
        let light = UIColor(light)
        let dark = UIColor(dark)
        return Color(UIColor { @Sendable traits in
            traits.userInterfaceStyle == .dark ? dark : light
        })
    }
}

enum Theme {
    static let text = Color.dynamic(light: Color(hex: 0x15171A), dark: Color(hex: 0xF2F2F3))
    static let text2 = Color.dynamic(light: Color(hex: 0x4F5660), dark: Color(hex: 0xA1A1A6))
    static let ground = Color.dynamic(light: Color(hex: 0xF2F2F6), dark: Color(hex: 0x131315))

    /// The fill of a card.
    static let card = Color.dynamic(light: .white.opacity(0.55), dark: Color(hex: 0x3C3C40, opacity: 0.5))
    /// The surface a page is read on.
    static let paper = Color.dynamic(light: .white.opacity(0.8), dark: Color(hex: 0x1E1E20, opacity: 0.86))
    static let edge = Color.dynamic(light: .white.opacity(0.75), dark: .white.opacity(0.12))
    /// The highlight behind the selected tab.
    static let lens = Color.dynamic(light: .white.opacity(0.9), dark: .white.opacity(0.14))
    static let separator = Color.dynamic(light: Color(hex: 0x14181E, opacity: 0.08), dark: .white.opacity(0.09))
    static let field = Color.dynamic(light: Color(hex: 0x767680, opacity: 0.12), dark: Color(hex: 0x767680, opacity: 0.24))

    static let accent = Color.dynamic(light: Color(hex: 0x1D4ED8), dark: Color(hex: 0xE5E5EA))
    /// A filled control (the selected day, the selected space) and what is on it.
    static let ink = Color.dynamic(light: Color(hex: 0x15171A), dark: Color(hex: 0xF2F2F3))
    static let onInk = Color.dynamic(light: .white, dark: Color(hex: 0x131315))

    static let mark = Color.dynamic(light: Color(hex: 0xFEF08A), dark: .white.opacity(0.24))
    static let now = Color.dynamic(light: Color(hex: 0xDC2626), dark: Color(hex: 0xE07A7A))
    static let check = Color.dynamic(light: Color(hex: 0x2563EB), dark: Color(hex: 0xE5E5EA))
    static let checkMark = Color.dynamic(light: .white, dark: Color(hex: 0x1C1C1E))
    static let checkEmpty = Color.dynamic(light: Color(hex: 0x8A919A), dark: Color(hex: 0x7C7C80))
    static let calloutNeutral = Color.dynamic(light: Color(hex: 0xE2E8F0, opacity: 0.7), dark: .white.opacity(0.07))
    static let cardShadow = Color.dynamic(light: Color(hex: 0x28325A, opacity: 0.10), dark: .black.opacity(0.35))

    /// The colours of one of the nine icon colours.
    struct Tint {
        /// An icon, and the tile behind a large one.
        let icon: Color
        let tile: Color
        /// An event: its card, the text on it and the bar at its edge.
        let eventFill: Color
        let eventText: Color
        let bar: Color
    }

    static func tint(_ name: String) -> Tint {
        tints[name] ?? tints["gray"]!
    }

    private static func make(
        light: (icon: UInt32, tile: UInt32, text: UInt32, bar: UInt32),
        dark: (icon: UInt32, bar: UInt32)
    ) -> Tint {
        Tint(
            icon: .dynamic(light: Color(hex: light.icon), dark: Color(hex: dark.icon)),
            tile: .dynamic(light: Color(hex: light.tile), dark: .white.opacity(0.08)),
            eventFill: .dynamic(light: Color(hex: light.tile, opacity: 0.85), dark: .white.opacity(0.08)),
            eventText: .dynamic(light: Color(hex: light.text), dark: Color(hex: 0xF2F2F3)),
            bar: .dynamic(light: Color(hex: light.bar), dark: Color(hex: dark.bar))
        )
    }

    // Dark mode keeps every colour close to grey; purple is grey there on purpose.
    private static let tints: [String: Tint] = [
        "gray": make(light: (0x475569, 0xE2E8F0, 0x1E293B, 0x64748B), dark: (0xB8BEC8, 0x9AA1AC)),
        "brown": make(light: (0x8A5A3C, 0xF0E4D8, 0x5B3A26, 0xA9744F), dark: (0xC9AB93, 0xB08E74)),
        "orange": make(light: (0xC2410C, 0xFFEDD5, 0x7C2D12, 0xF97316), dark: (0xD9A27A, 0xC9976B)),
        "yellow": make(light: (0xA16207, 0xFEF3C7, 0x713F12, 0xEAB308), dark: (0xD6BE7A, 0xC4AB63)),
        "green": make(light: (0x15803D, 0xDCFCE7, 0x14532D, 0x22C55E), dark: (0x93C2A0, 0x86B293)),
        "blue": make(light: (0x1D4ED8, 0xDBEAFE, 0x1E3A8A, 0x3B82F6), dark: (0x9DB4D8, 0x8EA6CF)),
        "purple": make(light: (0x6D28D9, 0xEDE9FE, 0x4C1D95, 0x8B5CF6), dark: (0xC7C7CC, 0xAEAEB4)),
        "pink": make(light: (0xBE185D, 0xFCE7F3, 0x831843, 0xEC4899), dark: (0xD4A5BD, 0xC290A9)),
        "red": make(light: (0xB91C1C, 0xFEE2E2, 0x7F1D1D, 0xEF4444), dark: (0xDB9C9C, 0xCC8585)),
    ]
}

/// The ground every screen sits on.
struct AppBackground: View {
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        GeometryReader { proxy in
            let size = proxy.size
            ZStack {
                Theme.ground
                if scheme == .dark {
                    glow(Color(hex: 0x2B2B2E), at: UnitPoint(x: 0.15, y: 0), radii: CGSize(width: 1.2, height: 0.6), in: size)
                    glow(Color(hex: 0x232326), at: UnitPoint(x: 1, y: 1), radii: CGSize(width: 1.2, height: 0.6), in: size)
                } else {
                    glow(Color(hex: 0xE6E1FF), at: UnitPoint(x: 0, y: 0), radii: CGSize(width: 1.3, height: 0.55), in: size)
                    glow(Color(hex: 0xD9E8FF), at: UnitPoint(x: 1, y: 0.35), radii: CGSize(width: 1.1, height: 0.5), in: size)
                    glow(Color(hex: 0xFFE6D6), at: UnitPoint(x: 0.4, y: 1), radii: CGSize(width: 1.3, height: 0.6), in: size)
                }
            }
        }
        .ignoresSafeArea()
        .accessibilityHidden(true)
    }

    /// A soft oval of colour that fades out at 60% of its radius.
    private func glow(_ color: Color, at center: UnitPoint, radii: CGSize, in size: CGSize) -> some View {
        let radius = radii.height * size.height
        let stretch = radius > 0 ? (radii.width * size.width) / radius : 1
        return Rectangle()
            .fill(RadialGradient(
                stops: [.init(color: color, location: 0), .init(color: color.opacity(0), location: 0.6)],
                center: center,
                startRadius: 0,
                endRadius: radius
            ))
            .scaleEffect(x: stretch, y: 1, anchor: center)
    }
}

extension View {
    /// The look of a card: a translucent fill with a fine edge.
    func card(cornerRadius: CGFloat = 20) -> some View {
        background(Theme.card, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(Theme.edge, lineWidth: 1)
            }
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
    }

    /// A small heading above a card.
    func sectionHeading() -> some View {
        font(.system(size: 13, weight: .semibold))
            .foregroundStyle(Theme.text2)
            .padding(.leading, 4)
            .accessibilityAddTraits(.isHeader)
    }

    /// The title at the top of a screen.
    func screenTitle() -> some View {
        font(.system(size: 32, weight: .bold))
            .tracking(-0.8)
            .foregroundStyle(Theme.text)
            .accessibilityAddTraits(.isHeader)
    }
}

extension View {
    /// Keep the screen's name out of the top bar (the screen shows it in its
    /// own content). The name still serves the back button's menu and
    /// VoiceOver. An empty item in the middle of the bar keeps it out also
    /// when the name changes while the screen is open.
    func hidesBarTitle() -> some View {
        navigationBarTitleDisplayMode(.inline)
            .toolbar(removing: .title)
            .toolbar {
                ToolbarItem(placement: .principal) {
                    Color.clear.frame(width: 1, height: 1).accessibilityHidden(true)
                }
                .sharedBackgroundVisibility(.hidden)
            }
    }
}

/// A hairline between the rows of a card.
struct RowSeparator: View {
    var body: some View {
        Rectangle().fill(Theme.separator).frame(height: 1)
    }
}
