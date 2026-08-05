// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Kurmanci",
    platforms: [
        .macOS(.v10_15)
    ],
    products: [
        .library(name: "Kurmanci", targets: ["Kurmanci"]),
        .executable(name: "KurmanciExample", targets: ["CommandLineExample"])
    ],
    targets: [
        .target(
            name: "CKurmanci",
            publicHeadersPath: "include"
        ),
        .target(
            name: "Kurmanci",
            dependencies: ["CKurmanci"],
            linkerSettings: [
                .linkedLibrary("kurmanci_ffi")
            ]
        ),
        .executableTarget(
            name: "CommandLineExample",
            dependencies: ["Kurmanci"],
            path: "Examples/CommandLineExample"
        ),
        .testTarget(
            name: "KurmanciTests",
            dependencies: ["Kurmanci"],
            resources: [
                .copy("Fixtures")
            ]
        )
    ]
)
