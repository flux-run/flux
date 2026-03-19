import { Hono } from "npm:hono";

const app = new Hono();

app.get("/binary-get", async (c) => {
  // Fetch a known binary file (e.g. a small image or just a range of bytes)
  // We'll use a service that returns random bytes
  const res = await fetch("https://httpbin.org/bytes/100");
  const buffer = await res.arrayBuffer();
  
  if (buffer.byteLength !== 100) {
    return c.json({ ok: false, error: `Expected 100 bytes, got ${buffer.byteLength}` }, 500);
  }
  
  const view = new Uint8Array(buffer);
  return c.json({ 
    ok: true, 
    byteLength: buffer.byteLength,
    firstByte: view[0],
    lastByte: view[99]
  });
});

app.post("/binary-post", async (c) => {
  const inputBuffer = new Uint8Array([1, 2, 3, 4, 5, 255]);
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    body: inputBuffer
  });
  
  const json = await res.json();
  // httpbin returns the posted data in 'data' field (as a string or hex usually)
  // but if it's binary it might be in 'data' as a data URI or similar.
  // We just want to check if it worked.
  return c.json({ 
    ok: true, 
    receivedLength: json.data?.length 
  });
});

Deno.serve(app.fetch);
