// Renders assets/logo.svg into the PNG sizes a macOS icon needs.
// Usage: swift scripts/make-icon.swift <logo.svg> <out.iconset> <ui-logo.png>
// scripts/make-icon.sh wraps this and runs iconutil to produce AppIcon.icns.
import AppKit

let args = CommandLine.arguments
guard args.count == 4 else {
    FileHandle.standardError.write("usage: make-icon.swift <svg> <iconset dir> <png>\n".data(using: .utf8)!)
    exit(2)
}
let svgURL = URL(fileURLWithPath: args[1])
let iconset = URL(fileURLWithPath: args[2])
guard let image = NSImage(contentsOf: svgURL) else {
    FileHandle.standardError.write("could not load \(svgURL.path)\n".data(using: .utf8)!)
    exit(1)
}
try? FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

func render(_ pixels: Int, to url: URL) {
    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
    else { exit(1) }
    rep.size = NSSize(width: pixels, height: pixels)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels),
               from: .zero, operation: .copy, fraction: 1.0)
    NSGraphicsContext.restoreGraphicsState()
    guard let png = rep.representation(using: .png, properties: [:]) else { exit(1) }
    try! png.write(to: url)
}

for base in [16, 32, 128, 256, 512] {
    render(base, to: iconset.appendingPathComponent("icon_\(base)x\(base).png"))
    render(base * 2, to: iconset.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
}
render(256, to: URL(fileURLWithPath: args[3]))
print("rendered \(iconset.path)")
