// hello_wasm_go — Fluxbase "hello world" function written in Go (TinyGo).
//
// Build:
//   tinygo build -o handler.wasm -target wasip1 -scheduler none -no-debug .
//
// Deploy:
//   flux deploy   (runs the build command from flux.json, then uploads handler.wasm)
//
// Invoke:
//   flux invoke hello_wasm_go '{"name":"Alice"}'
//   # → {"message":"Hello Alice!"}
package main

import (
	"encoding/json"
	"unsafe"
)

// ── Host imports ──────────────────────────────────────────────────────────────

//go:wasmimport fluxbase log
func hostLog(level int32, ptr unsafe.Pointer, length int32)

//go:wasmimport fluxbase http_fetch
func hostHttpFetch(reqPtr unsafe.Pointer, reqLen int32, outPtr unsafe.Pointer, outMax int32) int32

// ── ABI helpers ───────────────────────────────────────────────────────────────

var resultBuf []byte

//export __flux_alloc
func fluxAlloc(size int32) int32 {
	buf := make([]byte, size)
	return int32(uintptr(unsafe.Pointer(&buf[0])))
}

func logMsg(msg string) {
	b := []byte(msg)
	if len(b) > 0 {
		hostLog(1, unsafe.Pointer(&b[0]), int32(len(b)))
	}
}

func writeResult(jsonBytes []byte) int32 {
	length := uint32(len(jsonBytes))
	resultBuf = make([]byte, 4+len(jsonBytes))
	resultBuf[0] = byte(length)
	resultBuf[1] = byte(length >> 8)
	resultBuf[2] = byte(length >> 16)
	resultBuf[3] = byte(length >> 24)
	copy(resultBuf[4:], jsonBytes)
	return int32(uintptr(unsafe.Pointer(&resultBuf[0])))
}

// ── Handler ───────────────────────────────────────────────────────────────────

type Input struct {
	Name string `json:"name"`
}

type Output struct {
	Message string `json:"message"`
}

//export handle
func handle(payloadPtr int32, payloadLen int32) int32 {
	payload := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(payloadPtr))), payloadLen)

	logMsg("hello_wasm_go: executing")

	var inp Input
	if err := json.Unmarshal(payload, &inp); err != nil {
		type errResp struct {
			Error string `json:"error"`
		}
		b, _ := json.Marshal(errResp{Error: err.Error()})
		return writeResult(b)
	}

	type okResp struct {
		Output Output `json:"output"`
	}
	b, _ := json.Marshal(okResp{Output: Output{
		Message: "Hello " + inp.Name + "!",
	}})
	return writeResult(b)
}

func main() {}
