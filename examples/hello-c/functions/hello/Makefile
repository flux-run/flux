# Build hello → WASM using wasi-sdk
# Install: https://github.com/WebAssembly/wasi-sdk/releases
WASI_SDK ?= /opt/wasi-sdk
CC        = $(WASI_SDK)/bin/clang

CFLAGS = --target=wasm32-wasi          \
         -nostdlib                      \
         -Wl,--no-entry                 \
         -Wl,--export=hello_handler   \
         -Wl,--allow-undefined

.PHONY: build clean

build: hello.wasm

hello.wasm: handler.c
	$(CC) $(CFLAGS) -O2 -o $@ $<

clean:
	rm -f hello.wasm
