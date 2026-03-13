// {name} — Flux function (compiled to WASM via Kotlin/Wasm)
// Build: ./gradlew wasmWasiJar
import kotlinx.serialization.json.*

@Suppress("unused")
@WasmExport("{name}_handler")
fun {Name}Handler(inputPtr: Int, inputLen: Int): Long {
    // TODO: decode input JSON at inputPtr
    val response = """{ "ok": true }"""
    val bytes    = response.encodeToByteArray()
    // Allocation: host is responsible for freeing.
    return (inputPtr.toLong() shl 32) or bytes.size.toLong()
}
