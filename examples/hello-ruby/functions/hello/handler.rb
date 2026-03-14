# hello — Flux function (compiled to WASM via rbwasm / ruby.wasm)
# Build: rbwasm build --ruby-version 3.4 -o hello.wasm -- handler.rb
#
# Uses WASI stdin/stdout model: reads JSON from stdin, writes JSON to stdout.
require 'json'

raw = $stdin.read
begin
  _input = JSON.parse(raw)
rescue JSON::ParserError
  _input = {}
end

result = { ok: true }
$stdout.print(JSON.generate(result))
