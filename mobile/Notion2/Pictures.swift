// Pictures in pages and page covers. A picture is a file with a name the Mac
// app made; the page only names it. On the iPhone the file is either beside
// the app's database or in a sync folder (`files/<name>`): the copy of the
// Mac's folder that came over the local network, or the folder in iCloud
// Drive, where iCloud may not have brought it yet. With a copy of the notes
// loaded by hand there are no picture files at all: the page then shows a
// quiet stand-in.

import ImageIO
import Notion2Core
import SwiftUI
import UIKit

enum PictureStore {
    /// The folder of pictures beside the app's own database.
    nonisolated static var ownFolder: URL {
        AppModel.storeURL.deletingLastPathComponent().appendingPathComponent("Files", isDirectory: true)
    }

    /// Load a picture, made no larger than `maxPixels` on its longer side
    /// (a page never needs the full size of a photo). Waits a while for a
    /// file that iCloud still has to bring. Nil when there is no such file.
    @concurrent
    nonisolated static func load(_ name: String, syncFiles: [URL], maxPixels: CGFloat) async -> UIImage? {
        guard Pictures.isValidName(name) else { return nil }
        let files = FileManager.default
        for folder in [ownFolder] + syncFiles {
            let url = folder.appendingPathComponent(name)
            let waiting = folder.appendingPathComponent(".\(name).icloud")
            if files.fileExists(atPath: waiting.path) || !isHere(url) {
                try? files.startDownloadingUbiquitousItem(at: url)
                for _ in 0..<60 where !Task.isCancelled && (files.fileExists(atPath: waiting.path) || !isHere(url)) {
                    try? await Task.sleep(for: .milliseconds(500))
                }
            }
            guard files.fileExists(atPath: url.path), isHere(url), let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { continue }
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: maxPixels,
            ]
            if let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) {
                return UIImage(cgImage: image)
            }
        }
        return nil
    }

    private nonisolated static func isHere(_ url: URL) -> Bool {
        guard let status = (try? url.resourceValues(forKeys: [.ubiquitousItemDownloadingStatusKey]))?.ubiquitousItemDownloadingStatus else {
            return true
        }
        return status == .current
    }
}

/// Loads a picture by name and hands it to `content`; `missing` shows while
/// it loads and when the file is not on the iPhone.
struct PictureLoader<Content: View, Missing: View>: View {
    let name: String
    var maxPixels: CGFloat = 1600
    @ViewBuilder let content: (UIImage) -> Content
    @ViewBuilder let missing: () -> Missing

    @Environment(SyncCoordinator.self) private var sync
    @State private var image: UIImage?

    var body: some View {
        Group {
            if let image {
                content(image)
            } else {
                missing()
            }
        }
        // Try again when sync finds its folder, or brings something new.
        .task(id: "\(name) \(sync.picturesStamp)") {
            if image == nil {
                image = await PictureStore.load(name, syncFiles: sync.filesFolders, maxPixels: maxPixels)
            }
        }
    }
}

/// The band across the top of a page.
struct CoverBand: View {
    let cover: Cover
    /// Which part of a picture shows: 0 its top … 100 its bottom.
    let positionY: Int

    var body: some View {
        switch cover {
        case let .preset(preset):
            PresetCover(preset: preset)
        case let .file(name):
            GeometryReader { proxy in
                PictureLoader(name: name, maxPixels: 1800) { image in
                    // Fill the band; show the part of the picture that was chosen.
                    let scale = max(proxy.size.width / max(image.size.width, 1), proxy.size.height / max(image.size.height, 1))
                    let extra = max(image.size.height * scale - proxy.size.height, 0)
                    Image(uiImage: image)
                        .resizable()
                        .frame(width: image.size.width * scale, height: image.size.height * scale)
                        .offset(y: -extra * CGFloat(positionY) / 100)
                        .frame(width: proxy.size.width, height: proxy.size.height, alignment: .top)
                        .clipped()
                } missing: {
                    Theme.field
                }
            }
        }
    }
}

/// One of the app's own covers: a colour, or a gradient that may have a glow.
private struct PresetCover: View {
    let preset: CoverPreset

    var body: some View {
        GeometryReader { proxy in
            let size = proxy.size
            ZStack {
                if preset.stops.count == 1 {
                    Color(hex: preset.stops[0].color)
                } else {
                    // The line of a CSS gradient: through the middle, at the
                    // angle, long enough to reach the corners.
                    let angle = preset.angle * .pi / 180
                    let (dx, dy) = (sin(angle), -cos(angle))
                    let half = (abs(size.width * dx) + abs(size.height * dy)) / 2
                    let x = size.width > 0 ? dx * half / size.width : 0
                    let y = size.height > 0 ? dy * half / size.height : 0
                    LinearGradient(
                        stops: preset.stops.map { .init(color: Color(hex: $0.color), location: $0.at) },
                        startPoint: UnitPoint(x: 0.5 - x, y: 0.5 - y),
                        endPoint: UnitPoint(x: 0.5 + x, y: 0.5 + y)
                    )
                }
                if let glow = preset.glow {
                    let radius = glow.height * size.height
                    let center = UnitPoint(x: glow.x, y: glow.y)
                    Rectangle()
                        .fill(RadialGradient(
                            stops: [
                                .init(color: Color(hex: glow.color), location: 0),
                                .init(color: Color(hex: glow.color, opacity: 0), location: glow.fade),
                            ],
                            center: center,
                            startRadius: 0,
                            endRadius: radius
                        ))
                        .scaleEffect(x: radius > 0 ? glow.width * size.width / radius : 1, y: 1, anchor: center)
                }
            }
            .clipped()
        }
        .accessibilityHidden(true)
    }
}
