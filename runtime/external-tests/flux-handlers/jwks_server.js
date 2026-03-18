const http = require("http");

const port = Number(process.env.PORT || 9020);

const jwks = process.env.JWKS_JSON
  ? JSON.parse(process.env.JWKS_JSON)
  : {
      keys: [
        {
          kty: "RSA",
          kid: "test-key",
          use: "sig",
          n: "abc",
          e: "AQAB",
        },
      ],
    };

const server = http.createServer((req, res) => {
  if (req.url === "/.well-known/jwks.json") {
    const body = JSON.stringify(jwks);
    res.writeHead(200, {
      "content-type": "application/json",
      "cache-control": "public, max-age=600",
      "content-length": Buffer.byteLength(body),
    });
    res.end(body);
    return;
  }

  res.writeHead(404, { "content-type": "application/json" });
  res.end(JSON.stringify({ ok: false, error: "not found" }));
});

server.listen(port, "127.0.0.1", () => {
  console.log(`jwks server listening on http://127.0.0.1:${port}`);
});