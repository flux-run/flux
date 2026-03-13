// hello — Flux function (AssemblyScript → WASM)
// Build: asc index.ts --target release -o index.wasm

export function hello_handler(input_ptr: i32, input_len: i32): i64 {
  // TODO: decode input JSON at input_ptr with length input_len
  const resp = `{"ok":true}`;
  const buf  = String.UTF8.encode(resp);
  const ptr  = changetype<i32>(buf);
  return (<i64>ptr << 32) | buf.byteLength;
}
