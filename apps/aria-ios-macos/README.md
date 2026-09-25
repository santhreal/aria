# aria-ios-macos

The iOS/macOS harness app (SwiftUI) plus the `AriaMobile` Swift package. It
links the `aria-mobile` `staticlib` through the plain C ABI to render engine
frames on the device GPU.

Build the prebuilt FFI bundle with `scripts/build-ios-xcframework.sh`, then open
`aria-ios-macos.xcodeproj` in Xcode.
