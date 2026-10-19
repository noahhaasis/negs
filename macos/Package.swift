// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Negs",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "NegsApp", targets: ["NegsApp"])],
    targets: [
        .executableTarget(name: "NegsApp"),
        .testTarget(name: "NegsTests", dependencies: ["NegsApp"])
    ]
)
