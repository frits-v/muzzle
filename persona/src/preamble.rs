//! Preamble formatter — converts an [`Assignment`] into a compact persona
//! injection string that fits within [`MAX_PREAMBLE_CHARS`].

pub const MAX_PREAMBLE_CHARS: usize = 500;
const MAX_INSTRUCTIONS_CHARS: usize = 200;

/// Format an [`Assignment`] into a preamble string.
///
/// Layout:
/// ```text
/// You are {name}. You are {traits joined with " and "} by nature.
/// Your expertise: {expertise joined with ", "}.
/// {role_instructions truncated to 200 chars, if non-empty}
/// Recent: {recent_work joined with ", ", if non-empty}
/// ---
/// ```
///
/// Total output is guaranteed to be at most [`MAX_PREAMBLE_CHARS`] characters.
pub fn format_preamble(assignment: &crate::types::Assignment) -> String {
    const CLOSING: &str = "---\n";

    let traits = assignment.traits.join(" and ");
    let expertise = assignment.expertise.join(", ");

    let instructions_raw = if assignment.role_instructions.is_empty() {
        String::new()
    } else {
        let trimmed = truncate(&assignment.role_instructions, MAX_INSTRUCTIONS_CHARS);
        format!("{trimmed}\n")
    };

    let base = format!(
        "You are {}. You are {} by nature.\nYour expertise: {}.\n{}",
        assignment.name, traits, expertise, instructions_raw,
    );

    // Compute remaining budget for the "Recent:" line.
    // base + recent_line + "\n" + CLOSING must fit in MAX_PREAMBLE_CHARS.
    let base_and_closing_overhead = base.len() + CLOSING.len();
    let recent_raw = if !assignment.recent_work.is_empty() {
        let joined = assignment.recent_work.join(", ");
        let line = format!("Recent: {joined}");
        // "Recent: ...\n" overhead = line.len() + 1
        if base_and_closing_overhead + line.len() < MAX_PREAMBLE_CHARS {
            format!("{line}\n")
        } else {
            // Budget remaining for the line content (including "Recent: " prefix and trailing \n)
            let available = MAX_PREAMBLE_CHARS
                .saturating_sub(base_and_closing_overhead)
                .saturating_sub(1); // trailing \n
            if available > "Recent: ".len() {
                let truncated = truncate(&line, available);
                format!("{truncated}\n")
            } else {
                String::new()
            }
        }
    } else {
        String::new()
    };

    let body = format!("{base}{recent_raw}{CLOSING}");

    if body.len() <= MAX_PREAMBLE_CHARS {
        return body;
    }

    // Body is over budget — truncate at the word boundary, then re-append closing.
    // The separator "\n" between prefix and CLOSING costs 1 byte, so budget is
    // MAX_PREAMBLE_CHARS - len(CLOSING) - len("\n") = 500 - 4 - 1 = 495.
    let budget = MAX_PREAMBLE_CHARS - CLOSING.len() - 1;
    let prefix = truncate(&body[..body.len() - CLOSING.len()], budget);
    format!("{prefix}\n{CLOSING}")
}

/// UTF-8-safe truncation at a word boundary.
///
/// Returns a sub-slice of `s` that is at most `max` bytes long, ending on a
/// UTF-8 character boundary. Prefers breaking at the last space before the
/// boundary so we don't cut mid-word.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Walk back from `max` to find the last valid UTF-8 char boundary.
    // A byte is a continuation byte (10xxxxxx) when (byte & 0xC0) == 0x80.
    let mut boundary = max;
    while boundary > 0 && (s.as_bytes()[boundary] & 0xC0) == 0x80 {
        boundary -= 1;
    }
    match s[..boundary].rfind(' ') {
        Some(pos) => &s[..pos],
        None => &s[..boundary],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Assignment;

    #[test]
    fn format_preamble_under_budget() {
        let assignment = Assignment {
            agent_slot: 0,
            persona_id: 1,
            name: "Elena Vasquez".into(),
            traits: vec!["pragmatic".into(), "skeptical".into()],
            expertise: vec!["security".into(), "backend".into()],
            role_instructions: "Check for injection, auth bypass, secrets in code".into(),
            recent_work: vec![],
        };
        let preamble = format_preamble(&assignment);
        assert!(preamble.len() <= MAX_PREAMBLE_CHARS);
        assert!(preamble.contains("Elena Vasquez"));
        assert!(preamble.contains("pragmatic"));
        assert!(preamble.contains("security"));
        assert!(preamble.ends_with("---\n"));
    }

    #[test]
    fn format_preamble_truncates_long_instructions() {
        let assignment = Assignment {
            agent_slot: 0,
            persona_id: 1,
            name: "Test Persona".into(),
            traits: vec!["trait1".into()],
            expertise: vec!["exp1".into()],
            role_instructions: "x".repeat(400),
            recent_work: vec![],
        };
        let preamble = format_preamble(&assignment);
        assert!(preamble.len() <= MAX_PREAMBLE_CHARS);
    }

    #[test]
    fn format_preamble_over_budget_path_never_exceeds_500() {
        // Force the over-budget path: very long name + traits + expertise.
        let assignment = Assignment {
            agent_slot: 0,
            persona_id: 2,
            name: "A".repeat(200),
            traits: vec!["B".repeat(100)],
            expertise: vec!["C".repeat(100)],
            role_instructions: "D".repeat(200),
            recent_work: vec![],
        };
        let preamble = format_preamble(&assignment);
        assert!(
            preamble.len() <= MAX_PREAMBLE_CHARS,
            "preamble was {} bytes, expected <= {MAX_PREAMBLE_CHARS}",
            preamble.len()
        );
        assert!(preamble.ends_with("---\n"));
    }

    #[test]
    fn format_preamble_includes_recent_work() {
        let assignment = Assignment {
            agent_slot: 0,
            persona_id: 3,
            name: "Test Persona".into(),
            traits: vec!["trait1".into()],
            expertise: vec!["exp1".into()],
            role_instructions: String::new(),
            recent_work: vec![
                "code-reviewer on acme-api (2026-03-19)".into(),
                "researcher on web-app (2026-03-10)".into(),
            ],
        };
        let preamble = format_preamble(&assignment);
        assert!(preamble.len() <= MAX_PREAMBLE_CHARS);
        assert!(preamble.contains("Recent:"));
        assert!(preamble.contains("code-reviewer on acme-api"));
        assert!(preamble.ends_with("---\n"));
    }
}
