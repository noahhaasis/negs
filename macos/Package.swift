// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Spektrafilm",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "SpektrafilmApp", targets: ["SpektrafilmApp"])],
    targets: [
        .executableTarget(name: "SpektrafilmApp"),
        .testTarget(name: "SpektrafilmTests", dependencies: ["SpektrafilmApp"])
    ]
)
