//! Guard-rail enforcement for agent rules.
//!
//! Two rule types:
//!   `Require`  — before calling tool X, tool Y must have been called.
//!   `MaxCalls` — tool X may be invoked at most N times per run.

use std::collections::HashMap;

use crate::schema::Rule;

pub struct RuleState<'a> {
    rules:    &'a [Rule],
    /// How many times each tool has been called so far.
    call_log: HashMap<String, u32>,
}

impl<'a> RuleState<'a> {
    pub fn new(rules: &'a [Rule]) -> Self {
        Self { rules, call_log: HashMap::new() }
    }

    /// Call before dispatching `tool_name`.
    /// Returns `Err(message)` if any rule is violated.
    pub fn check(&self, tool_name: &str) -> Result<(), String> {
        for rule in self.rules {
            match rule {
                Rule::Require { before, require } if before == tool_name => {
                    if self.call_log.get(require.as_str()).copied().unwrap_or(0) == 0 {
                        return Err(format!(
                            "rule_require: must call `{}` before calling `{}`",
                            require, before
                        ));
                    }
                }
                Rule::MaxCalls { tool, max_calls } if tool == tool_name => {
                    let count = self.call_log.get(tool_name).copied().unwrap_or(0);
                    if count >= *max_calls {
                        return Err(format!(
                            "rule_max_calls: `{}` may only be called {} time(s) per run",
                            tool_name, max_calls
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Record that `tool_name` was called successfully.
    pub fn record(&mut self, tool_name: &str) {
        *self.call_log.entry(tool_name.to_string()).or_insert(0) += 1;
    }
}
