// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Spektrafilm",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "SpektrafilmApp", targets: ["SpektrafilmApp"])],
    dependencies: [.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.10.0")],
    targets: [
        .executableTarget(name: "SpektrafilmApp", dependencies: ["Sparkle"], path: "Sources/Spektrafilm",
                          linkerSettings: [.unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"])]),
        .testTarget(name: "SpektrafilmTests", dependencies: ["SpektrafilmApp"])
    ]
)
