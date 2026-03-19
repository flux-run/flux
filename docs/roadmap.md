# Strategic Roadmap 🧭

This document outlines the productization strategy and engineering roadmap for Flux. Our primary goal is to provide a "Zero Friction" experience for developers by supporting the most widely used libraries and frameworks in the Node.js and Deno ecosystems.

## Mission Strategy 🎯
> "Support the 20 packages that power 80% of Node/Deno backends."

Flux wins when a developer can run their existing API without rewriting code. We prioritize support based on adoption and technical feasibility within our deterministic execution model.

---

## Phase 1: MVP (0–1 Month) 🚀
**Goal**: Basic adoption for modern web APIs.
- [x] **Frameworks**: Native support for **Hono**.
- [x] **Clients**: Intercepted `fetch()`, `axios`, and `node-fetch`.
- [x] **Databases**: **pg** (node-postgres) and **node-redis** compatibility shims.
- [x] **Validation**: Full support for **Zod** and **Valibot**.
- [x] **Determinism**: Patched `Date`, `Math.random`, and `performance.now`.

## Phase 2: Ecosystem (1–2 Months) ⚡
**Goal**: Unlocking production-grade applications.
- [ ] **Drivers**: Full support for **postgres.js** and **ioredis**.
- [ ] **ORMs**: Optimized support for **Drizzle** and **Kysely**.
- [ ] **Frameworks**: Shims for **Express** and **Fastify** middleware.
- [ ] **Storage**: Formalized adapters for **AWS S3** and **Cloudflare R2**.

## Phase 3: Advanced Replay (2–3 Months) 🧠
**Goal**: Solving complex state challenges.
- [ ] **Queues**: Support for **BullMQ** and **Bee-Queue** (requires robust Redis replay).
- [ ] **Mutations**: Capture and compare full DB mutations in trace view.
- [ ] **Scheduling**: Deterministic `setTimeout` and `setInterval` persistence across resumes.

## Phase 4: Hard Problems (3–6 Months) 🔥
**Goal**: Enterprise-grade robustness.
- [ ] **ORM**: Limited support for **Prisma** (wrapping the query engine).
- [ ] **Network**: Improved **TLS interception** and SNI handling.
- [ ] **Node APIs**: Improved coverage for `net`, `tls`, and `buffer`.

## Phase 5: Power Users & Plugins 🧪
**Goal**: Extension and community contribution.
- [ ] **Plugins**: Architecture for adding custom IO adapters.
- [ ] **Databases**: Non-Postgres support (MySQL, MongoDB shim).
- [ ] **Testing**: Automated "deterministic drift" detection suite.

---

## Prioritization & Adoption Path 📊

We prioritize development based on what unlocks the next level of adoption.

### 🛑 Adoption Blockers (Must Have for any use)
*These tools enable the basic "Run & Debug" workflow.*
- **Databases**: `pg` (node-postgres)
- **Clients**: `fetch`, `axios`
- **Frameworks**: `Hono`, `Express`, `Fastify`

### ⚡ Production Unlocks (Must Have for real apps)
*These tools enable moving basic prototypes into production.*
- **Cache**: `redis` (node-redis), `ioredis`
- **Queues**: `BullMQ`, `Bee-Queue`
- **Storage**: `S3` / `R2` adapters

### 🧠 Advanced Ecosystem
*These tools broaden the reach into complex enteprise stacks.*
- **ORMs**: `Prisma` deep support
- **Interception**: Advanced `TLS` and `Network` introspection
- **Extensibility**: Plugin system for custom IO

---

## Implementation Principles
1. **Adapters, not Hacks**: We build clean shims (like our `pg` adapter) rather than global monkey-patching where possible.
2. **Explicit Fallbacks**: If a driver isn't supported, we provide a clear migration path to a fetch-based or Flux-native alternative.
3. **Determinism First**: We never compromise the integrity of the trace for convenience. If it can't be made deterministic, it stays in Tier 5/6.
