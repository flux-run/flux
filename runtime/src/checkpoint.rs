//! Checkpoint types and the `ExecutionMode` enum.
//!
//! ## The recording model
//!
//! Every time user code crosses an IO boundary (outbound `fetch()` or a DB write),
//! Flux writes a **checkpoint** — a serialised (request, response) pair labelled
//! with a monotonically increasing `call_index`.
//!
//! `call_index` is the replay key. It is reset to 0 at the start of each execution
//! and incremented with `AtomicU32::fetch_add(1, SeqCst)` **before** the real call
//! goes out. During replay the isolate sees its first `fetch()` call → look up
//! `call_index = 0` → return the recorded response. Never match by URL.
//!
//! ## Mode semantics
//!
//! | Mode   | fetch() behaviour       | DB write behaviour      |
//! |--------|-------------------------|-------------------------|
//! | Live   | make real call, record  | execute real, record    |
//! | Replay | return recorded by idx  | return recorded (dry)   |
//! | Resume | replay until exhausted, then Live | same        |
//!
//! In Replay mode, if `call_index` has no recorded checkpoint → error (never fall
//! through to the real network).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use uuid::Uuid;

// ── Execution mode ────────────────────────────────────────────────────────────

/// Controls how IO boundary crossings are handled for one execution.
#[derive(Debug, Clone)]
pub enum ExecutionMode {
    /// Normal production execution.
    /// Record every checkpoint to Postgres.
    Live,

    /// Full replay.
    /// For each boundary crossing, return the recorded response from Postgres
    /// by `call_index`. **Never** make real network calls.
    /// DB writes are dry-run unless `commit` is set to `true`.
    Replay {
        execution_id: Uuid,
        /// If true, actually commit DB writes during replay (--commit flag).
        commit: bool,
    },

    /// Fast-forward through recorded checkpoints, then switch to Live.
    /// Used to resume a failed execution from its exact failure point.
    Resume {
        execution_id:    Uuid,
        from_checkpoint: u32,
    },
}

impl ExecutionMode {
    /// Returns the source execution_id for Replay/Resume modes.
    pub fn source_execution_id(&self) -> Option<Uuid> {
        match self {
            ExecutionMode::Live                 => None,
            ExecutionMode::Replay { execution_id, .. } => Some(*execution_id),
            ExecutionMode::Resume { execution_id, .. } => Some(*execution_id),
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self, ExecutionMode::Live)
    }

    pub fn is_replay(&self) -> bool {
        matches!(self, ExecutionMode::Replay { .. })
    }
}

// ── Checkpoint record ─────────────────────────────────────────────────────────

/// A single IO boundary crossing — the unit of the recording model.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id:           Uuid,
    pub execution_id: Uuid,
    /// 0-based, incrementing per execution. The replay key.
    pub call_index:   u32,
    pub boundary:     BoundaryType,
    /// Serialised request (JSON bytes).
    pub request:      Vec<u8>,
    /// Serialised response (JSON bytes).
    pub response:     Vec<u8>,
    pub started_at_ms: i64,
    pub duration_ms:  u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryType {
    Http,
    Db,
}

impl BoundaryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BoundaryType::Http => "http",
            BoundaryType::Db   => "db",
        }
    }
}

// ── Per-execution call index ──────────────────────────────────────────────────

/// Monotonically increasing counter for `call_index` within one execution.
///
/// Stored in V8 `OpState` and incremented with `SeqCst` ordering before each
/// IO boundary crossing. This guarantees call_index is unique and ordered even
/// when concurrent ops race.
#[derive(Clone)]
pub struct CallIndex(pub Arc<AtomicU32>);

impl CallIndex {
    pub fn new() -> Self {
        CallIndex(Arc::new(AtomicU32::new(0)))
    }

    /// Atomically fetch the current index and increment it.
    /// Returns the value **before** incrementing (i.e. the index for this call).
    pub fn next(&self) -> u32 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }

    /// Reset to zero for a new execution.
    pub fn reset(&self) {
        self.0.store(0, Ordering::SeqCst);
    }
}

impl Default for CallIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ── Execution context injected into OpState ───────────────────────────────────

/// Per-request context injected into `OpState` before each execution.
/// Carries execution identity, mode, and the call-index counter.
#[derive(Clone)]
pub struct ExecutionContext {
    pub execution_id: Uuid,
    pub mode:         ExecutionMode,
    pub call_index:   CallIndex,
}

impl ExecutionContext {
    pub fn new_live(execution_id: Uuid) -> Self {
        ExecutionContext {
            execution_id,
            mode:       ExecutionMode::Live,
            call_index: CallIndex::new(),
        }
    }

    pub fn new_replay(new_execution_id: Uuid, source_execution_id: Uuid, commit: bool) -> Self {
        ExecutionContext {
            execution_id: new_execution_id,
            mode:         ExecutionMode::Replay { execution_id: source_execution_id, commit },
            call_index:   CallIndex::new(),
        }
    }

    pub fn new_resume(new_execution_id: Uuid, source_execution_id: Uuid, from_checkpoint: u32) -> Self {
        ExecutionContext {
            execution_id: new_execution_id,
            mode:         ExecutionMode::Resume { execution_id: source_execution_id, from_checkpoint },
            call_index:   CallIndex::new(),
        }
    }
}

// ── Checkpoint store trait ────────────────────────────────────────────────────

/// Abstracts checkpoint storage. In production: Postgres via `sqlx`.
/// In tests: in-memory `HashMap`.
#[async_trait::async_trait]
pub trait CheckpointStore: Send + Sync + 'static {
    /// Write a checkpoint. Must be called **before** returning the response to user code.
    async fn write(&self, cp: &Checkpoint) -> Result<(), String>;

    /// Look up a checkpoint by execution_id + call_index.
    async fn get(&self, execution_id: Uuid, call_index: u32) -> Result<Option<Checkpoint>, String>;
}

// ── In-memory checkpoint store (for tests) ───────────────────────────────────

#[cfg(test)]
pub mod test_store {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    pub struct MemCheckpointStore {
        inner: Mutex<HashMap<(Uuid, u32), Checkpoint>>,
    }

    impl MemCheckpointStore {
        pub fn new() -> Arc<Self> {
            Arc::new(MemCheckpointStore {
                inner: Mutex::new(HashMap::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl CheckpointStore for MemCheckpointStore {
        async fn write(&self, cp: &Checkpoint) -> Result<(), String> {
            self.inner.lock().await.insert((cp.execution_id, cp.call_index), cp.clone());
            Ok(())
        }

        async fn get(&self, execution_id: Uuid, call_index: u32) -> Result<Option<Checkpoint>, String> {
            Ok(self.inner.lock().await.get(&(execution_id, call_index)).cloned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_index_increments_sequentially() {
        let ci = CallIndex::new();
        assert_eq!(ci.next(), 0);
        assert_eq!(ci.next(), 1);
        assert_eq!(ci.next(), 2);
    }

    #[test]
    fn call_index_reset() {
        let ci = CallIndex::new();
        ci.next();
        ci.next();
        ci.reset();
        assert_eq!(ci.next(), 0);
    }

    #[test]
    fn execution_mode_is_live() {
        assert!(ExecutionMode::Live.is_live());
        assert!(!ExecutionMode::Replay { execution_id: Uuid::new_v4(), commit: false }.is_live());
    }

    #[test]
    fn execution_mode_is_replay() {
        let id = Uuid::new_v4();
        assert!(ExecutionMode::Replay { execution_id: id, commit: false }.is_replay());
        assert!(!ExecutionMode::Live.is_replay());
    }

    #[test]
    fn execution_context_new_live() {
        let id = Uuid::new_v4();
        let ctx = ExecutionContext::new_live(id);
        assert!(ctx.mode.is_live());
        assert_eq!(ctx.execution_id, id);
    }

    #[test]
    fn boundary_type_str() {
        assert_eq!(BoundaryType::Http.as_str(), "http");
        assert_eq!(BoundaryType::Db.as_str(), "db");
    }

    #[tokio::test]
    async fn mem_store_write_and_get() {
        use test_store::MemCheckpointStore;

        let store = MemCheckpointStore::new();
        let exec_id = Uuid::new_v4();
        let cp = Checkpoint {
            id:           Uuid::new_v4(),
            execution_id: exec_id,
            call_index:   0,
            boundary:     BoundaryType::Http,
            request:      b"{\"url\":\"https://example.com\"}".to_vec(),
            response:     b"{\"status\":200}".to_vec(),
            started_at_ms: 1000,
            duration_ms:  50,
        };

        store.write(&cp).await.unwrap();
        let got = store.get(exec_id, 0).await.unwrap().unwrap();
        assert_eq!(got.call_index, 0);
        assert_eq!(got.boundary, BoundaryType::Http);
    }

    #[tokio::test]
    async fn mem_store_get_missing_returns_none() {
        use test_store::MemCheckpointStore;

        let store = MemCheckpointStore::new();
        let result = store.get(Uuid::new_v4(), 42).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn replay_mode_no_checkpoint_detected() {
        // In Replay mode, if no checkpoint found at call_index, the handler must error.
        // This test validates the ExecutionMode machinery is in place for that check.
        use test_store::MemCheckpointStore;

        let store = MemCheckpointStore::new();
        let exec_id = Uuid::new_v4();
        let ctx = ExecutionContext::new_replay(Uuid::new_v4(), exec_id, false);

        // No checkpoint written → get returns None
        let result = store.get(exec_id, ctx.call_index.next()).await.unwrap();
        assert!(result.is_none(), "replay with no checkpoint must return None");
    }
}
