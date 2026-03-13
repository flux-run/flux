# Build {name} → WASM using wasi-sdk
# Install: https://github.com/WebAssembly/wasi-sdk/releases
WASI_SDK ?= /opt/wasi-sdk
CXX       = $(WASI_SDK)/bin/clang++

CXXFLAGS = --target=wasm32-wasi        \
           -nostdlib                    \
           -fno-exceptions             \
           -Wl,--no-entry              \
           -Wl,--export={name}_handler \
           -Wl,--allow-undefined

.PHONY: build clean

build: {name}.wasm

{name}.wasm: handler.cpp
	$(CXX) $(CXXFLAGS) -O2 -o $@ $<

clean:
	rm -f {name}.wasm
