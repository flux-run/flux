// {name} — Flux function (compiled to WASM via swiftwasm)
// Build: swiftc -target wasm32-unknown-wasi Handler.swift -o {name}.wasm

@_cdecl("{name}_handler")
func {Name}Handler(inputPtr: UnsafePointer<UInt8>, inputLen: UInt32) -> UInt64 {
// TODO: decode JSON at inputPtr
let response = #"{\"ok\":true}"#
let bytes    = Array(response.utf8)
let outPtr   = UnsafeMutablePointer<UInt8>.allocate(capacity: bytes.count)
outPtr.initialize(from: bytes, count: bytes.count)
return (UInt64(UInt(bitPattern: outPtr)) << 32) | UInt64(bytes.count)
}
