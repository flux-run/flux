/// Re-export from the canonical error module.
///
/// `types::response` is kept as a compatibility shim so existing `use crate::types::response::{...}`
/// imports continue to work during the migration.  New code should import from
/// `crate::error` directly.
pub use crate::error::{ApiError, ApiResponse, ApiResult};

