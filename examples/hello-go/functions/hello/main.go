//go:build wasip1

package main

import (
	"encoding/json"
	"fmt"
)

type Input struct {
	// TODO: add your input fields
}

type Output struct {
	OK bool `json:"ok"`
}

//export hello_handler
func handler(inputPtr, inputLen uint32) uint64 {
	inputBytes := readMemory(inputPtr, inputLen)

	var input Input
	if err := json.Unmarshal(inputBytes, &input); err != nil {
		panic(fmt.Sprintf("hello: unmarshal input: %v", err))
	}

	out := Output{OK: true}
	outBytes, _ := json.Marshal(out)
	return writeMemory(outBytes)
}

// Memory helpers (provided by the Flux WASM runtime).
func readMemory(ptr, length uint32) []byte {
	return (*[1 << 30]byte)(unsafe.Pointer(uintptr(ptr)))[:length:length]
}
func writeMemory(data []byte) uint64 {
	ptr := uintptr(unsafe.Pointer(&data[0]))
	return (uint64(ptr) << 32) | uint64(len(data))
}

func main() {}
