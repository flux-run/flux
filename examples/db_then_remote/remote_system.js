const http = require("http");

const port = Number(process.env.PORT || 9010);

const server = http.createServer((req, res) => {
  if (req.method !== "POST" || req.url !== "/ingest") {
    res.writeHead(404, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: false, error: "not found" }));
    return;
  }

  let raw = "";
  req.on("data", (chunk) => {
    raw += chunk;
  });

  req.on("end", () => {
    let payload;

    try {
      payload = raw ? JSON.parse(raw) : {};
    } catch {
      res.writeHead(400, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: false, error: "invalid json" }));
      return;
    }

    res.writeHead(200, { "content-type": "application/json" });
    res.end(
      JSON.stringify({
        ok: true,
        accepted: true,
        receivedDispatchId: payload.dispatchId ?? null,
      }),
    );
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`remote system listening on http://127.0.0.1:${port}`);
});