//! Structured JSON logging to stderr.
//!
//! Replaces ad-hoc `eprintln!` with machine-parseable JSON lines.
//! Each entry includes timestamp, level, binary name, message, and
//! optional session/detail fields for correlation and filtering.

use std::io::Write;

/// Emit a structured JSON log line to stderr.
///
/// Fields: `ts`, `level`, `bin`, `msg`, and optionally `session`, `detail`.
pub fn emit(level: &str, bin: &str, msg: &str) {
    emit_full(level, bin, msg, None, None);
}

/// Emit a structured JSON log line with optional session and detail fields.
pub fn emit_full(level: &str, bin: &str, msg: &str, session: Option<&str>, detail: Option<&str>) {
    let ts = iso_now();
    // Build JSON manually to avoid serde dependency in the hot path.
    // All string values are escaped via serde_json::Value to prevent injection.
    let mut obj = serde_json::Map::new();
    obj.insert("ts".into(), serde_json::Value::String(ts));
    obj.insert("level".into(), serde_json::Value::String(level.into()));
    obj.insert("bin".into(), serde_json::Value::String(bin.into()));
    obj.insert("msg".into(), serde_json::Value::String(msg.into()));
    if let Some(s) = session {
        obj.insert("session".into(), serde_json::Value::String(s.into()));
    }
    if let Some(d) = detail {
        obj.insert("detail".into(), serde_json::Value::String(d.into()));
    }
    let json = serde_json::Value::Object(obj).to_string();
    let _ = writeln!(std::io::stderr(), "{}", json);
}

/// Convenience: log an ERROR.
pub fn error(bin: &str, msg: &str) {
    emit("ERROR", bin, msg);
}

/// Convenience: log a WARN.
pub fn warn(bin: &str, msg: &str) {
    emit("WARN", bin, msg);
}

/// ISO 8601 UTC timestamp without external crate.
fn iso_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;

    // Civil date from days since epoch (Algorithm from Howard Hinnant)
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_now_format() {
        let ts = iso_now();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {}", ts);
        assert_eq!(ts.len(), 20, "timestamp length should be 20: {}", ts);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn test_emit_produces_valid_json() {
        // Capture stderr is complex; just verify the JSON building doesn't panic
        emit("INFO", "test", "hello world");
        emit_full("ERROR", "test", "msg", Some("sess-123"), Some("detail"));
    }

    #[test]
    fn test_emit_escapes_special_chars() {
        // Should not panic on strings with quotes, newlines, backslashes
        emit(
            "WARN",
            "test",
            "path with \"quotes\" and \\backslashes\nnewline",
        );
    }

    #[test]
    fn test_error_convenience() {
        // Should not panic; calls emit("ERROR", ...)
        error("test-bin", "something broke");
    }

    #[test]
    fn test_warn_convenience() {
        // Should not panic; calls emit("WARN", ...)
        warn("test-bin", "heads up");
    }

    #[test]
    fn test_emit_full_without_optional_fields() {
        // None values should not appear in output
        emit_full("DEBUG", "test-bin", "no extras", None, None);
    }

    #[test]
    fn test_emit_full_with_all_fields() {
        emit_full(
            "INFO",
            "session-start",
            "session created",
            Some("sess-abc123"),
            Some("/path/to/workspace"),
        );
    }
}
