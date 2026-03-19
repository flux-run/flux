// @ts-nocheck
import express from "npm:express";

const app = express();
app.use(express.json());

app.get("/", (req, res) => {
  res.send("hello from express on flux");
});

app.get("/app-health", (req, res) => {
  res.json({ ok: true });
});

app.post("/data", (req, res) => {
  res.json({ received: req.body });
});

// Shimming Express to work with Deno.serve (basic handler only)
Deno.serve((req) => {
  // This is a very basic shim for testing purposes. 
  // In a real scenario, users might use a more robust adapter.
  return app(req);
});
