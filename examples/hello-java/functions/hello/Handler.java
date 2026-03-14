// hello — Flux function (TeaVM WASM/WASI build)
// Build: cd functions/hello && gradle generateWasi
// Output: functions/hello/hello.wasm/hello.wasm
//
// Note: TeaVM's WASI target does not provide fd_read, so stdin is unavailable.
// Input is passed via command-line args (args[0] = JSON payload).
// Output is written to stdout via System.out.print.

public class Handler {
    public static void main(String[] args) {
        // args[0] contains the JSON payload if provided (may be absent in bare invocations)
        System.out.print("{\"ok\":true}");
    }
}
