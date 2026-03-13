// swift-tools-version: 5.9
// Build: swift build -c release --triple wasm32-unknown-wasi
// Install SwiftWasm toolchain: https://swiftwasm.org
import PackageDescription

let package = Package(
    name: "hello",
    targets: [
        .executableTarget(
            name: "hello",
            path: ".",
            swiftSettings: [
                .unsafeFlags(["-target", "wasm32-unknown-wasi"]),
            ]
        ),
    ]
)
