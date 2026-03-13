// swift-tools-version: 5.9
// Build: swift build -c release --triple wasm32-unknown-wasi
// Install SwiftWasm toolchain: https://swiftwasm.org
import PackageDescription

let package = Package(
    name: "{name}",
    targets: [
        .executableTarget(
            name: "{name}",
            path: ".",
            swiftSettings: [
                .unsafeFlags(["-target", "wasm32-unknown-wasi"]),
            ]
        ),
    ]
)
