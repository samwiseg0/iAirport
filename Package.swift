// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "iairport",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "IAirportCore", targets: ["IAirportCore"]),
        .executable(name: "iairport", targets: ["iairport"])
    ],
    targets: [
        .target(
            name: "IAirportCore",
            path: "Sources/IAirportCore",
            linkerSettings: [
                .linkedFramework("CoreWLAN"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreLocation"),
                .linkedFramework("UserNotifications")
            ]
        ),
        .executableTarget(
            name: "iairport",
            dependencies: ["IAirportCore"],
            path: "Sources/iairport",
            linkerSettings: [
                .linkedFramework("CoreWLAN"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreLocation"),
                .linkedFramework("UserNotifications")
            ]
        ),
        .testTarget(
            name: "IAirportCoreTests",
            dependencies: ["IAirportCore"],
            path: "Tests/IAirportCoreTests"
        )
    ],
    swiftLanguageVersions: [.v5]
)
