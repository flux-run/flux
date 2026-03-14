// hello — Flux function (compiled to WASM via dotnet + wasi-experimental workload)
// Build: dotnet build -c Release
//
// Uses WASI stdin/stdout model: reads JSON from stdin, writes JSON to stdout.
using System;
using System.Text.Json;

class Program
{
    static void Main(string[] args)
    {
        var _ = Console.In.ReadToEnd(); // consume input (unused in hello)
        Console.Write(JsonSerializer.Serialize(new { ok = true }));
    }
}
