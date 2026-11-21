// swift-tools-version: 5.9
import PackageDescription

/// The shared app core's static library (core/, Odin), built by
/// `python3 core/build.py lib` (universal with --universal).
let coreLibrary = Context.packageDirectory + "/../target/negs-core"

let package = Package(
    name: "Negs",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "NegsApp", targets: ["NegsApp"])],
    dependencies: [.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.10.0")],
    targets: [
        .systemLibrary(name: "CNegsCore", path: "Sources/CNegsCore"),
        .executableTarget(name: "NegsApp", dependencies: ["Sparkle", "CNegsCore"],
                          linkerSettings: [.unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks",
                                                         "-L", coreLibrary])]),
        .testTarget(name: "NegsTests", dependencies: ["NegsApp", "CNegsCore"],
                    linkerSettings: [.unsafeFlags(["-L", coreLibrary])])
    ]
)
