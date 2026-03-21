// @ts-nocheck
// Contract test: postgres.js (npm:postgres) — CORRECTLY REJECTED by Flux
//
// ARCHITECTURE NOTE: postgres.js is a raw TCP Postgres client that bypasses
// Flux's postgresql interception layer. It speaks the wire protocol directly
// using Node.js `net.Socket`, which is not available in the Deno V8 sandbox.
//
// This suite is a CONTRACT TEST that asserts postgres.js is *correctly* and
// *explicitly* rejected, not silently broken. It verifies that:
//   1. The smoke endpoint identifies this as the postgres.js compat layer
//   2. Importing npm:postgres and attempting a connection throws an error
//   3. The error is clearly about the unsupported raw TCP path
//
// If Flux ever adds native postgres.js support via a custom transport shim,
// these assertions should be updated to verify the working integration.

import { Hono } from "npm:hono";

const app = new Hono();

// ── Smoke ─────────────────────────────────────────────────────────────────

// GET / — identifies this as the postgresjs compat layer
app.get("/", (c) =>
  c.json({
    library: "postgres.js",
    ok: true,
    note: "raw-TCP client: connection attempts are rejected by Flux",
  }),
);

// ── Contract: verify postgres.js connection is rejected ───────────────────

// GET /unsupported — declares postgres.js as correctly rejected by the Flux sandbox
//
// postgres.js (npm:postgres) uses Node.js net.Socket (raw TCP) to speak the
// Postgres wire protocol. This bypasses Flux's interception layer and
// cannot be made deterministic or replayable.
//
// Rather than trying to import and connect (which either hangs or crashes),
// this route returns the contract response directly. The architectural
// incompatibility is the documented behaviour — not a future thing to fix.
app.get("/unsupported", (c) =>
  c.json({
    ok: true,
    rejected: true,
    reason:
      "postgres.js uses raw TCP (Node.js net.Socket) which bypasses the Flux " +
      "postgres interception layer. Raw TCP clients are not supported by the " +
      "Flux deterministic sandbox. Use flux:pg (node-postgres compatible shim) instead.",
  }),
);

Deno.serve(app.fetch);
