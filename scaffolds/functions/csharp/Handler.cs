// {name} — Flux function (compiled to WASM via dotnet wasi)
// Requires: dotnet add package Wasi.Sdk
using System.Text.Json;
using System.Runtime.InteropServices;

public static class {Name}Handler
{
    [UnmanagedCallersOnly(EntryPoint = "{name}_handler")]
    public static long Handle(IntPtr inputPtr, int inputLen)
    {
        // TODO: decode input, run logic
        var output = JsonSerializer.SerializeToUtf8Bytes(new { ok = true });
        var outPtr = Marshal.AllocHGlobal(output.Length);
        Marshal.Copy(output, 0, outPtr, output.Length);
        return ((long)outPtr << 32) | output.Length;
    }
}
