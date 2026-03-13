//! Dispatch implementations for the runtime crate.
//!
//! In multi-process mode, these HTTP clients wrap the control-plane network
//! calls.  The `server` crate provides in-process alternatives that bypass
//! the network entirely.

pub mod http_api;
pub mod http_queue;

pub use http_api::HttpApiDispatch;
pub use http_queue::HttpQueueDispatch;
