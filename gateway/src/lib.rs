//! # `gateway` — public-facing API gateway
//!
//! The gateway is the **only** component that faces the internet.  It
//! validates, authenticates, rate-limits, and traces every incoming request —
//! it never executes user code itself.  Execution is delegated to the Runtime
//! service via the [`job_contract::dispatch::RuntimeDispatch`] trait.
//!
//! ## 8-stage request pipeline
//!
//! ```text
//!  Inbound HTTP
//!       │
//!       ▼  [1] content-length guard   — reject oversized bodies before reading
//!       ▼  [2] route resolution       — look up (METHOD, /path) in snapshot
//!       ▼  [3] CORS preflight         — OPTIONS fast-path, no auth needed
//!       ▼  [4] authentication         — none | api_key | jwt per route config
//!       ▼  [5] rate limiting          — per-route token bucket (route×IP key)
//!       ▼  [6] read + validate body   — stream body, optional JSON Schema check
//!       ▼  [7] write trace root       — fire-and-forget: path, headers,
//!       │                               query_params, body → gateway_trace_requests
//!       ▼  [8] forward to runtime     — POST /execute via RuntimeDispatch trait
//!       │
//!  Outbound HTTP (x-request-id echoed in response)
//! ```
//!
//! ## Mental model
//!
//! > *Gateway is the only public-facing component.  It validates, authenticates,
//! > and traces — it never executes user code.*
//!
//! All business logic lives in the Runtime or API (control-plane) services.
//! The gateway is intentionally thin: route table changes arrive via Postgres
//! LISTEN/NOTIFY so no restart is required after a deployment.
//!
//! ## Module tree
//!
//! | Module             | Responsibility                                        |
//! |--------------------|-------------------------------------------------------|
//! | [`auth`]           | Request authentication (none / api_key / jwt)         |
//! | [`config`]         | Environment-variable configuration loader             |
//! | [`forward`]        | Runtime dispatch (HTTP impl + `RuntimeDispatch` trait)|
//! | [`handlers`]       | Axum handler functions (dispatch + health + readiness)|
//! | [`rate_limit`]     | Per-route token-bucket rate limiter                   |
//! | [`router`]         | Axum router factory — wires routes to handlers        |
//! | [`snapshot`]       | In-memory route table with LISTEN/NOTIFY refresh      |
//! | [`state`]          | Shared `GatewayState` injected into every handler     |
//! | [`trace`]          | Trace-root capture and request-ID resolution          |

pub mod auth;
pub mod config;
pub mod forward;
pub mod handlers;
pub mod metrics;
pub mod rate_limit;
pub mod router;
pub mod snapshot;
pub mod state;
pub mod trace;

// Convenience re-exports at crate root.
pub use router::create_router;
pub use state::GatewayState;
