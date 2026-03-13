# Build {name} → WASM using wasi-sdk
# Install: https://github.com/WebAssembly/wasi-sdk/releases
WASI_SDK ?= /opt/wasi-sdk
CC        = $(WASI_SDK)/bin/clang

CFLAGS = --target=wasm32-wasi          \
         -nostdlib                      \
         -Wl,--no-entry                 \
         -Wl,--export={name}_handler   \
         -Wl,--allow-undefined

.PHONY: build clean

build: {name}.wasm

{name}.wasm: handler.c
	$(CC) $(CFLAGS) -O2 -o $@ $<

clean:
	rm -f {name}.wasm
