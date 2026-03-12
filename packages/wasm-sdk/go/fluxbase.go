// Package fluxbase is the Fluxbase WASM SDK for Go (TinyGo).
//
// Build your function with:
//
//	tinygo build -o handler.wasm -target wasip1 -scheduler none -no-debug .
//
// # ABI contract
//
// The Fluxbase runtime calls:
//
//	handle(payload_ptr i32, payload_len i32) i32
//
// The returned i32 is a pointer into linear memory where:
//
//	[4-byte u32 LE length][UTF-8 JSON]
//
// JSON must contain either {"output": ...} or {"error": "..."}.
//
// # Usage
//
//	func main() {
//	    fluxbase.Register(func(ctx *fluxbase.Ctx, payload []byte) (any, error) {
//	        var inp MyInput
//	        json.Unmarshal(payload, &inp)
//	        return MyOutput{Message: "Hello " + inp.Name}, nil
//	    })
//	}
package fluxbase

import (
	"encoding/json"
	"unsafe"
)

// ── Host imports ──────────────────────────────────────────────────────────────

//go:wasmimport fluxbase log
func hostLog(level int32, ptr unsafe.Pointer, length int32)

//go:wasmimport fluxbase secrets_get
func hostSecretsGet(keyPtr unsafe.Pointer, keyLen int32, outPtr unsafe.Pointer, outMax int32) int32

//go:wasmimport fluxbase http_fetch
func hostHttpFetch(reqPtr unsafe.Pointer, reqLen int32, outPtr unsafe.Pointer, outMax int32) int32

// ── Ctx ───────────────────────────────────────────────────────────────────────

// Ctx provides access to Fluxbase host capabilities within a handler.
type Ctx struct{}

// Log emits a log line at the given level (1=info, 2=warn, 3=error).
func (c *Ctx) Log(msg string) {
	b := []byte(msg)
	if len(b) == 0 {
		return
	}
	hostLog(1, unsafe.Pointer(&b[0]), int32(len(b)))
}

// LogLevel emits a log line at a specific level.
func (c *Ctx) LogLevel(level int, msg string) {
	b := []byte(msg)
	if len(b) == 0 {
		return
	}
	hostLog(int32(level), unsafe.Pointer(&b[0]), int32(len(b)))
}

// Secret retrieves a secret by key. Returns ("", false) if not found.
func (c *Ctx) Secret(key string) (string, bool) {
	kb := []byte(key)
	buf := make([]byte, 4096)
	n := hostSecretsGet(
		unsafe.Pointer(&kb[0]), int32(len(kb)),
		unsafe.Pointer(&buf[0]), int32(len(buf)),
	)
	if n < 0 {
		return "", false
	}
	return string(buf[:n]), true
}

// HttpRequest is the input to Ctx.Fetch.
type HttpRequest struct {
	Method  string            `json:"method"`
	URL     string            `json:"url"`
	Headers map[string]string `json:"headers,omitempty"`
	// Body should be base64-encoded when non-empty.
	Body string `json:"body,omitempty"`
}

// HttpResponse is the result of Ctx.Fetch.
type HttpResponse struct {
	Status  int               `json:"status"`
	Headers map[string]string `json:"headers,omitempty"`
	// Body is base64-encoded.
	Body string `json:"body"`
}

// Fetch performs an outbound HTTP request via the Fluxbase host.
func (c *Ctx) Fetch(req HttpRequest) (*HttpResponse, error) {
	reqBytes, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}
	out := make([]byte, 65536)
	n := hostHttpFetch(
		unsafe.Pointer(&reqBytes[0]), int32(len(reqBytes)),
		unsafe.Pointer(&out[0]), int32(len(out)),
	)
	if n < 0 {
		return nil, &FetchError{code: int(n)}
	}
	var resp HttpResponse
	if err := json.Unmarshal(out[:n], &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// FetchError is returned when a host http_fetch call fails.
type FetchError struct{ code int }

func (e *FetchError) Error() string {
	return "http_fetch failed with code " + itoa(e.code)
}

func itoa(i int) string {
	if i == 0 {
		return "0"
	}
	neg := i < 0
	if neg {
		i = -i
	}
	buf := [20]byte{}
	pos := len(buf)
	for i > 0 {
		pos--
		buf[pos] = byte('0' + i%10)
		i /= 10
	}
	if neg {
		pos--
		buf[pos] = '-'
	}
	return string(buf[pos:])
}

// ── Handler registration ──────────────────────────────────────────────────────

// HandlerFunc is the signature of a Fluxbase WASM handler.
type HandlerFunc func(ctx *Ctx, payload []byte) (any, error)

var registeredHandler HandlerFunc

// Register sets the function that will handle incoming invocations.
// Call it from main().
func Register(fn HandlerFunc) {
	registeredHandler = fn
}

// ── WASM ABI exports ──────────────────────────────────────────────────────────

// resultBuf holds the most-recent handler result so the GC doesn't collect it
// before the host reads it.
var resultBuf []byte

//export __flux_alloc
func fluxAlloc(size int32) int32 {
	buf := make([]byte, size)
	return int32(uintptr(unsafe.Pointer(&buf[0])))
}

//export handle
func handle(payloadPtr int32, payloadLen int32) int32 {
	payload := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(payloadPtr))), payloadLen)

	ctx := &Ctx{}
	out, err := registeredHandler(ctx, payload)

	var resultJSON []byte
	if err != nil {
		type errResp struct {
			Error string `json:"error"`
		}
		resultJSON, _ = json.Marshal(errResp{Error: err.Error()})
	} else {
		type okResp struct {
			Output any `json:"output"`
		}
		resultJSON, _ = json.Marshal(okResp{Output: out})
	}

	// Prepend 4-byte LE length
	length := uint32(len(resultJSON))
	resultBuf = make([]byte, 4+len(resultJSON))
	resultBuf[0] = byte(length)
	resultBuf[1] = byte(length >> 8)
	resultBuf[2] = byte(length >> 16)
	resultBuf[3] = byte(length >> 24)
	copy(resultBuf[4:], resultJSON)
	return int32(uintptr(unsafe.Pointer(&resultBuf[0])))
}
