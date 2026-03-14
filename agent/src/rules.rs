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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Rule;

    fn require(before: &str, require: &str) -> Rule {
        Rule::Require { before: before.to_string(), require: require.to_string() }
    }

    fn max_calls(tool: &str, n: u32) -> Rule {
        Rule::MaxCalls { tool: tool.to_string(), max_calls: n }
    }

    // ── No rules ──────────────────────────────────────────────────────────

    #[test]
    fn no_rules_allows_any_tool() {
        let state = RuleState::new(&[]);
        assert!(state.check("any_tool").is_ok());
        assert!(state.check("another_tool").is_ok());
    }

    // ── Require rules ─────────────────────────────────────────────────────

    #[test]
    fn require_fails_when_dep_not_called() {
        let rules = [require("notify_slack", "create_issue")];
        let state = RuleState::new(&rules);
        let err = state.check("notify_slack").unwrap_err();
        assert!(err.contains("notify_slack"), "error should mention the blocked tool");
        assert!(err.contains("create_issue"), "error should mention the required dep");
    }

    #[test]
    fn require_passes_when_dep_was_called() {
        let rules = [require("notify_slack", "create_issue")];
        let mut state = RuleState::new(&rules);
        state.record("create_issue");
        assert!(state.check("notify_slack").is_ok());
    }

    #[test]
    fn require_rule_does_not_block_the_dep_itself() {
        // Rule: before notify_slack, require create_issue.
        // Calling create_issue itself must never be blocked by this rule.
        let rules = [require("notify_slack", "create_issue")];
        let state = RuleState::new(&rules);
        assert!(state.check("create_issue").is_ok());
    }

    #[test]
    fn require_rule_does_not_affect_unrelated_tools() {
        let rules = [require("notify_slack", "create_issue")];
        let state = RuleState::new(&rules);
        assert!(state.check("send_email").is_ok());
    }

    #[test]
    fn require_still_passes_after_dep_called_multiple_times() {
        let rules = [require("notify_slack", "create_issue")];
        let mut state = RuleState::new(&rules);
        state.record("create_issue");
        state.record("create_issue");
        assert!(state.check("notify_slack").is_ok());
    }

    #[test]
    fn require_error_message_format() {
        let rules = [require("B", "A")];
        let state = RuleState::new(&rules);
        let err = state.check("B").unwrap_err();
        assert!(err.contains("rule_require"), "should start with rule_require prefix");
    }

    // ── MaxCalls rules ────────────────────────────────────────────────────

    #[test]
    fn max_calls_first_call_passes() {
        let rules = [max_calls("send_sms", 1)];
        let state = RuleState::new(&rules);
        assert!(state.check("send_sms").is_ok());
    }

    #[test]
    fn max_calls_blocks_after_limit_reached() {
        let rules = [max_calls("send_sms", 1)];
        let mut state = RuleState::new(&rules);
        assert!(state.check("send_sms").is_ok()); // first check passes
        state.record("send_sms");                  // record the call
        let err = state.check("send_sms").unwrap_err(); // second check blocked
        assert!(err.contains("send_sms"));
        assert!(err.contains("1"), "error should mention the limit");
    }

    #[test]
    fn max_calls_allows_up_to_limit() {
        let rules = [max_calls("tool", 3)];
        let mut state = RuleState::new(&rules);
        for _ in 0..3 {
            assert!(state.check("tool").is_ok());
            state.record("tool");
        }
        assert!(state.check("tool").is_err(), "4th call must be blocked");
    }

    #[test]
    fn max_calls_does_not_affect_different_tools() {
        let rules = [max_calls("restricted_tool", 1)];
        let mut state = RuleState::new(&rules);
        state.record("restricted_tool");
        // restricted_tool is now blocked, but other tools are fine
        assert!(state.check("other_tool").is_ok());
    }

    #[test]
    fn max_calls_error_message_format() {
        let rules = [max_calls("tool", 2)];
        let mut state = RuleState::new(&rules);
        state.record("tool");
        state.record("tool");
        let err = state.check("tool").unwrap_err();
        assert!(err.contains("rule_max_calls"), "should contain rule_max_calls prefix");
    }

    // ── Multiple rules ────────────────────────────────────────────────────

    #[test]
    fn multiple_rules_all_enforced() {
        let rules = [
            require("B", "A"),
            max_calls("C", 1),
        ];
        let state = RuleState::new(&rules);
        // B blocked (A not called)
        assert!(state.check("B").is_err());
        // C allowed (first call)
        assert!(state.check("C").is_ok());
    }

    #[test]
    fn require_and_max_calls_combined_on_same_tool() {
        // B requires A, and B can only be called once.
        let rules = [require("B", "A"), max_calls("B", 1)];
        let mut state = RuleState::new(&rules);
        // Without A, B is blocked by require rule
        assert!(state.check("B").is_err());
        state.record("A");
        // Now A is recorded, B passes
        assert!(state.check("B").is_ok());
        state.record("B");
        // Second call to B is blocked by max_calls (limit=1)
        assert!(state.check("B").is_err());
    }
}
