CC      := /opt/wasi-sdk/bin/clang
CFLAGS  := --sysroot=/opt/wasi-sdk/share/wasi-sysroot -O2
TARGET  := handler.wasm

.PHONY: all clean

all: $(TARGET)

$(TARGET): handler.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(TARGET)
