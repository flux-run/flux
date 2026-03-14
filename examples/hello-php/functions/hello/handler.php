<?php
// hello — Flux function (PHP 8.2 → WASM/WASI)
//
// Build: run build.sh to embed this script into php.wasm as a flux.wasi-args
// custom WASM section, producing hello.wasm.  The Flux runtime reads that
// section and passes ["php", "-r", "<script>"] as WASI argv before calling
// _start, so php boots and executes this code inline.
//
// I/O contract (same as all Flux WASM functions):
//   stdin  → JSON-encoded input payload
//   stdout → JSON-encoded return value
//   stderr → error/log messages (captured by the executor)

$input = json_decode(file_get_contents('php://stdin'), true) ?? [];

$response = [
    'message' => 'Hello from PHP!',
    'runtime' => 'php-8.2-wasm',
    'input'   => $input,
];

echo json_encode($response) . "\n";
