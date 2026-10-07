// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "MacDimScreen",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "MacDimScreen", targets: ["MacDimScreen"]),
    ],
    targets: [
        // Protocol models and the daemon socket client. No UI; fully testable.
        .target(name: "DimKit"),
        // SwiftUI views, the app model and the dimming overlay.
        .target(name: "DimUI", dependencies: ["DimKit"]),
        // The menu bar app.
        .executableTarget(name: "MacDimScreen", dependencies: ["DimUI"]),
        // Self-checking test runner. XCTest isn't available with Command Line
        // Tools alone, so `swift run KitChecks` works on any setup.
        .executableTarget(name: "KitChecks", dependencies: ["DimKit"], path: "Sources/KitChecks"),
    ]
)
