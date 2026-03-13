# {name} — Flux function (compiled to WASM via ruby.wasm)
# Build: ruby.wasm build handler.rb -o {name}.wasm
require 'json'

# @param input_json [String]  JSON-encoded input payload
# @return [String]            JSON-encoded output
def {name}_handler(input_json)
  _input = JSON.parse(input_json)

  # TODO: implement {name}

  JSON.generate(ok: true)
end
