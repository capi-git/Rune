// swift-tools-version: 6.0
import PackageDescription

// The rules of Notion 2.0 that the iPhone app shares with the desktop app:
// the data model, the read-only database reader, inline text, repeat rules,
// search. No UI code, so `swift test` runs it on the Mac in a few seconds.
let package = Package(
    name: "Notion2Core",
    platforms: [.iOS(.v18), .macOS(.v14)],
    products: [
        .library(name: "Notion2Core", targets: ["Notion2Core"]),
    ],
    targets: [
        .target(name: "Notion2Core"),
        .testTarget(name: "Notion2CoreTests", dependencies: ["Notion2Core"]),
    ]
)
