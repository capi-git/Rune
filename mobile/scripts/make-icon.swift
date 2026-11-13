// Renders mobile/Support/AppIcon.svg into the 1024 px icon the iPhone app
// needs: opaque (iOS refuses an icon with see-through pixels).
// Usage: swift mobile/scripts/make-icon.swift
import AppKit

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let svg = root.appendingPathComponent("Support/AppIcon.svg")
let out = root.appendingPathComponent("Rune/Assets.xcassets/AppIcon.appiconset/AppIcon.png")
guard let image = NSImage(contentsOf: svg) else {
    FileHandle.standardError.write("could not load \(svg.path)\n".data(using: .utf8)!)
    exit(1)
}
let pixels = 1024
// No alpha channel at all: each pixel is colour only.
guard let context = CGContext(
    data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
else { exit(1) }
context.setFillColor(NSColor.black.cgColor)
context.fill(CGRect(x: 0, y: 0, width: pixels, height: pixels))
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
NSGraphicsContext.current?.imageInterpolation = .high
image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels), from: .zero, operation: .sourceOver, fraction: 1.0)
NSGraphicsContext.restoreGraphicsState()
guard let cg = context.makeImage(),
      let target = CGImageDestinationCreateWithURL(out as CFURL, "public.png" as CFString, 1, nil)
else { exit(1) }
CGImageDestinationAddImage(target, cg, nil)
guard CGImageDestinationFinalize(target) else { exit(1) }
print("wrote \(out.path)")
