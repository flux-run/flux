/// W3C Trace Context Level 1 — https://www.w3.org/TR/trace-context/
///
/// Header format: `{version}-{trace_id_32hex}-{parent_id_16hex}-{flags_2hex}`
///   trace_id   (128-bit): maps 1-to-1 to a UUID when formatted with hyphens.
///   parent_id   (64-bit): stored as a lowercase 16-char hex string; NOT a UUID.
///
/// We use the traceparent only as a fallback when the caller does NOT send
/// `x-request-id` / `x-parent-span-id` natively — preserving full backwards
/// compatibility with existing Fluxbase clients.

#[derive(Debug, Clone)]
pub struct Traceparent {
    /// 128-bit trace identifier formatted as a lowercase UUID string.
    pub trace_id: String,
    /// 64-bit parent-span identifier as a lowercase 16-char hex string.
    pub parent_id: String,
}

/// Parse a W3C `traceparent` header value.
///
/// Returns `None` when:
/// - the value is malformed or the wrong length
/// - version `ff` is encountered (reserved by spec)
/// - `trace_id` or `parent_id` are all-zero (invalid per spec)
pub fn parse(header: &str) -> Option<Traceparent> {
    let parts: Vec<&str> = header.trim().splitn(4, '-').collect();
    if parts.len() < 4 {
        return None;
    }

    let version      = parts[0];
    let trace_id_hex = parts[1];
    let parent_id_hex = parts[2];
    // parts[3] = flags (0x01 = sampled); must be present but we don't gate on it.

    // Spec: version "ff" is reserved for future use — reject.
    if version == "ff" {
        return None;
    }

    // Exact lengths required.
    if trace_id_hex.len() != 32 || parent_id_hex.len() != 16 {
        return None;
    }

    // Must be valid lowercase hex.
    if !is_hex(trace_id_hex) || !is_hex(parent_id_hex) {
        return None;
    }

    // All-zero values are invalid per the W3C spec.
    if trace_id_hex == "00000000000000000000000000000000"
        || parent_id_hex == "0000000000000000"
    {
        return None;
    }

    // Format trace_id as a standard UUID string (insert hyphens at canonical offsets).
    let trace_uuid = format!(
        "{}-{}-{}-{}-{}",
        &trace_id_hex[0..8],
        &trace_id_hex[8..12],
        &trace_id_hex[12..16],
        &trace_id_hex[16..20],
        &trace_id_hex[20..32],
    );

    Some(Traceparent {
        trace_id: trace_uuid,
        parent_id: parent_id_hex.to_lowercase(),
    })
}

#[inline]
fn is_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_traceparent() {
        let tp = parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").unwrap();
        assert_eq!(tp.trace_id,  "4bf92f35-77b3-4da6-a3ce-929d0e0e4736");
        assert_eq!(tp.parent_id, "00f067aa0ba902b7");
    }

    #[test]
    fn rejects_all_zero_trace_id() {
        assert!(parse("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_none());
    }

    #[test]
    fn rejects_all_zero_parent_id() {
        assert!(parse("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01").is_none());
    }

    #[test]
    fn rejects_ff_version() {
        assert!(parse("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").is_none());
    }

    #[test]
    fn rejects_wrong_lengths() {
        assert!(parse("00-tooshort-00f067aa0ba902b7-01").is_none());
        assert!(parse("00-4bf92f3577b34da6a3ce929d0e0e4736-short-01").is_none());
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(parse("").is_none());
        assert!(parse("00-4bf92f3577b34da6a3ce929d0e0e4736").is_none());
    }

    #[test]
    fn tolerates_future_version() {
        // A version other than "00" but not "ff" should still parse (spec says so).
        let tp = parse("01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
        assert!(tp.is_some());
    }
}
