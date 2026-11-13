// swift-tools-version: 6.0
import PackageDescription

// The rules of Rune that the iPhone app shares with the desktop app:
// the data model, the read-only database reader, inline text, repeat rules,
// search. No UI code, so `swift test` runs it on the Mac in a few seconds.
let package = Package(
    name: "RuneCore",
    platforms: [.iOS(.v18), .macOS(.v14)],
    products: [
        .library(name: "RuneCore", targets: ["RuneCore"]),
    ],
    targets: [
        .target(name: "RuneCore"),
        .testTarget(name: "RuneCoreTests", dependencies: ["RuneCore"]),
    ]
)
