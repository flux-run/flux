# Build hello → WASM using wasi-sdk
# Install: https://github.com/WebAssembly/wasi-sdk/releases
WASI_SDK ?= /opt/wasi-sdk
CXX       = $(WASI_SDK)/bin/clang++

CXXFLAGS = --target=wasm32-wasi        \
           -nostdlib                    \
           -fno-exceptions             \
           -Wl,--no-entry              \
           -Wl,--export=hello_handler \
           -Wl,--allow-undefined

.PHONY: build clean

build: hello.wasm

hello.wasm: handler.cpp
	$(CXX) $(CXXFLAGS) -O2 -o $@ $<

clean:
	rm -f hello.wasm
