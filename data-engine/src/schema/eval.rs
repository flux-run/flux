//! `EvalCtx` — runtime context injected into rule and transform evaluation.
//!
//! Mirrors the TypeScript `ctx` object available in schema rules/hooks:
//! ```ts
//! ctx.user.id        // authenticated user ID
//! ctx.user.role      // e.g. "admin", "user"
//! ctx.user.email
//! ctx.request_id     // trace ID
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Runtime evaluation context — one per request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCtx {
    pub user: UserCtx,
    pub request_id: Option<String>,
}

/// Authenticated user fields available in rules and hooks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserCtx {
    pub id:    Option<String>,
    pub role:  Option<String>,
    pub email: Option<String>,
    /// Any additional claims from the JWT / session token.
    #[serde(default)]
    pub claims: Value,
}

impl EvalCtx {
    pub fn anonymous() -> Self {
        Self {
            user: UserCtx {
                id:    None,
                role:  Some("anon".into()),
                email: None,
                claims: Value::Null,
            },
            request_id: None,
        }
    }

    /// Resolve a dot-separated ctx path like `["user", "id"]` to a JSON value.
    /// Returns `Value::Null` for unknown paths.
    pub fn resolve(&self, path: &[String]) -> Value {
        match path {
            [p] if p == "request_id" =>
                self.request_id.as_deref().map(Value::from).unwrap_or(Value::Null),
            [p, rest @ ..] if p == "user" => self.user.resolve(rest),
            _ => Value::Null,
        }
    }
}

impl UserCtx {
    fn resolve(&self, path: &[String]) -> Value {
        match path.first().map(String::as_str) {
            Some("id")    => self.id.as_deref().map(Value::from).unwrap_or(Value::Null),
            Some("role")  => self.role.as_deref().map(Value::from).unwrap_or(Value::Null),
            Some("email") => self.email.as_deref().map(Value::from).unwrap_or(Value::Null),
            Some(key)     => self.claims.get(key).cloned().unwrap_or(Value::Null),
            None          => Value::Null,
        }
    }
}
