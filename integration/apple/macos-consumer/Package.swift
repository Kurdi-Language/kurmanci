// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MacOSConsumer",
    platforms: [
        .macOS(.v11)
    ],
    dependencies: [
        .package(path: "../../../dist/swift-package-local")
    ],
    targets: [
        .executableTarget(
            name: "MacOSConsumer",
            dependencies: [
                .product(name: "Kurmanci", package: "swift-package-local")
            ],
            path: "."
        )
    ]
)
