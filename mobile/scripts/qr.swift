// Draw a QR code: swift qr.swift "<text>" out.png
// Used by wifi-mac.py to show the link that pairs an iPhone. The Mac app will
// draw the code itself; this is for a test with a real iPhone before that.

import AppKit
import CoreImage

let arguments = CommandLine.arguments
guard arguments.count == 3, let filter = CIFilter(name: "CIQRCodeGenerator") else {
    FileHandle.standardError.write(Data("usage: swift qr.swift <text> <out.png>\n".utf8))
    exit(2)
}
filter.setValue(Data(arguments[1].utf8), forKey: "inputMessage")
filter.setValue("M", forKey: "inputCorrectionLevel")
guard let code = filter.outputImage else { exit(1) }
// Large squares and a white edge, so that a camera reads it from a screen.
let scaled = code.transformed(by: CGAffineTransform(scaleX: 12, y: 12))
let edge: CGFloat = 48
let size = NSSize(width: scaled.extent.width + edge * 2, height: scaled.extent.height + edge * 2)
guard let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(size.width), pixelsHigh: Int(size.height), bitsPerSample: 8, samplesPerPixel: 4,
    hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
), let context = NSGraphicsContext(bitmapImageRep: bitmap) else { exit(1) }
NSGraphicsContext.current = context
NSColor.white.setFill()
NSRect(origin: .zero, size: size).fill()
context.cgContext.interpolationQuality = .none
NSCIImageRep(ciImage: scaled).draw(in: NSRect(x: edge, y: edge, width: scaled.extent.width, height: scaled.extent.height))
context.flushGraphics()
guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
do {
    try png.write(to: URL(fileURLWithPath: arguments[2]))
} catch {
    FileHandle.standardError.write(Data("\(error.localizedDescription)\n".utf8))
    exit(1)
}
