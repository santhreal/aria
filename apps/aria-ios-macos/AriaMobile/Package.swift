// swift-tools-version: 5.9
import PackageDescription

// Local package wrapping the Rust `aria-mobile` engine for the iOS/macOS app.
//
// Add it to the app with: File > Add Package Dependencies… > Add Local… and
// pick this `AriaMobile` folder, then add the `AriaMobile` library to the
// app target. The binary target carries the prebuilt static lib; the Swift
// target links the system frameworks wgpu/Metal interop needs.
let package = Package(
    name: "AriaMobile",
    platforms: [
        .iOS(.v15),
        .macOS(.v13),
    ],
    products: [
        .library(name: "AriaMobile", targets: ["AriaMobile"]),
    ],
    targets: [
        .binaryTarget(
            name: "AriaMobileFFI",
            path: "AriaMobileFFI.xcframework"
        ),
        .target(
            name: "AriaMobile",
            dependencies: ["AriaMobileFFI"],
            resources: [
                .copy("Resources/sample.mp4"),
            ],
            linkerSettings: [
                .linkedFramework("Metal"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("IOSurface"),
                .linkedFramework("Foundation"),
                // Decoder (AVAssetReader + VideoToolbox).
                .linkedFramework("AVFoundation"),
                .linkedFramework("VideoToolbox"),
                .linkedFramework("AudioToolbox"),
                .linkedLibrary("c++"),
            ]
        ),
        .testTarget(
            name: "AriaMobileTests",
            dependencies: ["AriaMobile"]
        ),
    ]
)
