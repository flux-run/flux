//go:build wasip1

// hello — Flux function (Go → WASM, wasip1)
// Build: GOOS=wasip1 GOARCH=wasm go build -o hello.wasm .
//
// The Flux runtime calls your function via the wasip1 ABI:
//   stdin  → JSON-encoded input payload
//   stdout → JSON-encoded return value
//   stderr → logged as warning-level spans in flux trace
//
// Host imports (available to WASM modules compiled with wasip1):
// ──────────────────────────────────────────────────────────────
// The runtime exposes the following functions under the "fluxbase" module.
// In Go, wasip1 host imports use the //go:wasmimport directive (Go 1.21+):
//
//   //go:wasmimport fluxbase db_query
//   func db_query(sqlPtr, sqlLen, paramsPtr, paramsLen, outPtr, outMax uint32) int32
//
//   //go:wasmimport fluxbase queue_push
//   func queue_push(reqPtr, reqLen, outPtr, outMax uint32) int32
//
//   //go:wasmimport fluxbase http_fetch
//   func http_fetch(reqPtr, reqLen, outPtr, outMax uint32) int32
//
//   //go:wasmimport fluxbase secrets_get
//   func secrets_get(keyPtr, keyLen, outPtr, outMax uint32) int32
//
//   //go:wasmimport fluxbase log
//   func log(level int32, msgPtr, msgLen uint32)
//
// See examples/hello-rust/functions/hello/src/lib.rs for the memory convention.
// A higher-level Go helper package is available at:
//   https://github.com/fluxbase/fluxbase-go

package main

import (
	"encoding/json"
	"os"
)

// Input and output types are validated against flux.json "schema" at the gateway.
type Input struct {
	// TODO: add your input fields
	// Example: UserID string `json:"user_id"`
}

type Output struct {
	OK bool `json:"ok"`
}

func main() {
	var input Input
	if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil {
		// Write a structured error to stdout; the runtime records it as a 500.
		_ = json.NewEncoder(os.Stdout).Encode(map[string]string{
			"error":   "INPUT_PARSE_ERROR",
			"message": err.Error(),
		})
		os.Exit(0) // exit 0 — the runtime reads stdout, not exit code
	}

	// ── Database example (requires Go 1.21+ for //go:wasmimport) ─────────────
	// Uncomment and adapt once you import the fluxbase-go helper:
	//
	//   rows, err := fluxbase.DBQuery("SELECT id FROM users WHERE active = $1", true)
	//   if err != nil { ... }
	//
	// Or call the host function directly (advanced):
	//   sql := "SELECT id FROM users LIMIT 5"
	//   params := "[]"
	//   out := make([]byte, 65536)
	//   n := db_query(ptr(sql), uint32(len(sql)), ptr(params), uint32(len(params)),
	//                 ptr2(out), uint32(len(out)))
	//   if n < 0 { /* error in out[:abs(n)] */ }
	//   rows := out[:n]  // JSON array of row objects

	// ── Queue example ─────────────────────────────────────────────────────────
	// Uncomment to enqueue an async job:
	//
	//   req := `{"function":"send_welcome_email","payload":{"userId":"123"}}`
	//   out := make([]byte, 4096)
	//   n := queue_push(ptr(req), uint32(len(req)), ptr2(out), uint32(len(out)))

	out := Output{OK: true}
	if err := json.NewEncoder(os.Stdout).Encode(out); err != nil {
		os.Exit(1)
	}
}
