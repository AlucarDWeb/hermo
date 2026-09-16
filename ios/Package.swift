// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "hermo-ios-deps",
    platforms: [
        .iOS(.v18),
        .macOS(.v15)
    ],
    dependencies: [
        .package(url: "https://github.com/pointfreeco/swift-composable-architecture", exact: "1.26.2")
    ],
    targets: []
)
