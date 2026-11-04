// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Negs",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "NegsApp", targets: ["NegsApp"])],
    dependencies: [.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.10.0")],
    targets: [
        .executableTarget(name: "NegsApp", dependencies: ["Sparkle"],
                          linkerSettings: [.unsafeFlags([
                              "-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks",
                              // SwiftPM records the deployment target as the SDK version, and
                              // AppKit gives an app linked against macOS 14 its old window
                              // design. Still runs on macOS 14; asks for the current look on 27.
                              "-Xlinker", "-platform_version", "-Xlinker", "macos", "-Xlinker", "14.0", "-Xlinker", "27.0"
                          ])]),
        .testTarget(name: "NegsTests", dependencies: ["NegsApp"])
    ]
)
