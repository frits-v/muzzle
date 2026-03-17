//! Context injection formatter for SessionStart hook.
//!
//! Converts recent [`Observation`]s into a markdown block that Claude Code can
//! consume at session start to restore cross-session context.

use crate::store::Observation;

/// Maximum number of observations to include in the formatted output.
const MAX_OBS: usize = 10;

/// Maximum number of characters to show for an observation's content.
const MAX_CONTENT: usize = 150;

/// Format a slice of observations as a markdown context block.
///
/// Returns an empty string when `observations` is empty.
/// Caps output at 10 observations even if more are passed.
/// Truncates individual content fields to 150 chars.
pub fn format_context(observations: &[Observation], project: &str) -> String {
    if observations.is_empty() {
        return String::new();
    }

    let mut out = format!("# Session Memory ({})\n", project);

    for obs in observations.iter().take(MAX_OBS) {
        let preview = truncate(&obs.content, MAX_CONTENT);
        out.push_str(&format!(
            "\n- **{}** [{}]: {}\n  {}\n",
            obs.obs_type, obs.source, obs.title, preview,
        ));
    }

    out
}

/// Truncate `s` to at most `max` characters.  Appends `"..."` when truncated.
///
/// Truncation is on char boundaries (Unicode-safe).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}...")
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_obs(obs_type: &str, source: &str, title: &str, content: &str) -> Observation {
        Observation {
            id: 1,
            session_id: "sess-1".to_string(),
            obs_type: obs_type.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            project: "test-proj".to_string(),
            scope: "project".to_string(),
            topic_key: None,
            source: source.to_string(),
            revision_count: 1,
            duplicate_count: 0,
            created_at: "2026-03-16T00:00:00Z".to_string(),
            updated_at: "2026-03-16T00:00:00Z".to_string(),
        }
    }

    // 1. empty slice returns empty string --------------------------------------

    #[test]
    fn test_format_context_empty() {
        let result = format_context(&[], "my-project");
        assert_eq!(result, "");
    }

    // 2. two observations produce correct markdown ----------------------------

    #[test]
    fn test_format_context_formats_observations() {
        let obs = vec![
            make_obs(
                "learning",
                "changelog",
                "Retry logic",
                "Use exponential backoff",
            ),
            make_obs(
                "decision",
                "manual",
                "DB choice",
                "Picked PostgreSQL for FTS",
            ),
        ];
        let result = format_context(&obs, "my-project");

        assert!(
            result.starts_with("# Session Memory (my-project)\n"),
            "missing header"
        );
        assert!(
            result.contains("**learning** [changelog]: Retry logic"),
            "first obs missing"
        );
        assert!(
            result.contains("Use exponential backoff"),
            "first obs content missing"
        );
        assert!(
            result.contains("**decision** [manual]: DB choice"),
            "second obs missing"
        );
        assert!(
            result.contains("Picked PostgreSQL for FTS"),
            "second obs content missing"
        );
    }

    // 3. long content is truncated to 150 chars + "..." ----------------------

    #[test]
    fn test_format_context_truncates_long_content() {
        let long_content = "x".repeat(500);
        let obs = vec![make_obs("learning", "changelog", "Big note", &long_content)];
        let result = format_context(&obs, "proj");

        // Expect exactly 150 "x" characters followed by "..."
        let expected_preview = format!("{}{}", "x".repeat(150), "...");
        assert!(
            result.contains(&expected_preview),
            "expected truncated preview not found in output"
        );
        // The full 500-char string must NOT appear.
        assert!(
            !result.contains(&"x".repeat(151)),
            "content should be cut off at 150 chars"
        );
    }

    // 4. more than 10 observations → only 10 emitted -------------------------

    #[test]
    fn test_format_context_caps_at_10() {
        let obs: Vec<Observation> = (0..15)
            .map(|i| make_obs("learning", "changelog", &format!("Note {i}"), "body"))
            .collect();

        let result = format_context(&obs, "proj");

        // Notes 0-9 must appear; notes 10-14 must not.
        for i in 0..10 {
            assert!(result.contains(&format!("Note {i}")), "Note {i} missing");
        }
        for i in 10..15 {
            assert!(
                !result.contains(&format!("Note {i}")),
                "Note {i} should be absent"
            );
        }
    }
}
