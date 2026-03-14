# PHP WASM build — requires Emscripten 3.1 + php-wasm 8.3
#
# Install Emscripten SDK:
#   git clone https://github.com/emscripten-core/emsdk.git
#   cd emsdk && ./emsdk install 3.1.61 && ./emsdk activate 3.1.61
#   source emsdk/emsdk_env.sh
#
# Install php-wasm:
#   https://github.com/soyuka/php-wasm

EMCC    ?= emcc
PHP_SRC ?= handler.php
OUT     ?= handler.wasm

.PHONY: build check clean

build: $(OUT)

$(OUT): $(PHP_SRC)
	$(EMCC) -O2 \
		-s WASM=1 \
		-s EXPORTED_FUNCTIONS='["_handler"]' \
		-s EXTRA_EXPORTED_RUNTIME_METHODS='["ccall","cwrap"]' \
		-o handler.js \
		$(PHP_SRC)

check:
	php -l $(PHP_SRC)

clean:
	rm -f handler.js handler.wasm
