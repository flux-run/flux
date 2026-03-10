/// Fluxbase Trigger Infrastructure
///
/// Triggers start flows. Every trigger eventually becomes a function execution.
///
/// Phase 1 triggers:
///   http     — any HTTP POST to a gateway route
///   webhook  — authenticated webhook from external services (Stripe, GitHub, etc.)
///   cron     — time-based schedule
///
/// Flow:
///   External event → Gateway → Trigger Router → Function execution → Tools

pub mod registry;
pub mod router;

pub use registry::{TriggerRegistry, TriggerConfig, TriggerKind};
pub use router::TriggerRouter;
