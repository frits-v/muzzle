//! Persona preamble formatter for agent context injection.

use crate::types::Assignment;

/// Maximum number of UTF-8 characters in a formatted preamble (inclusive).
pub const MAX_PREAMBLE_CHARS: usize = 500;

/// Format a persona [`Assignment`] into an agent preamble string.
///
/// Structure:
/// ```text
/// You are {name}. You are {traits joined " and "} by nature.
/// Your expertise: {expertise joined ", "}.
/// {role_instructions truncated to 200 chars, if non-empty}
/// Recent: {recent_work joined ", ", if non-empty}
/// ---
/// ```
///
/// The result is always terminated with `---\n`.  If the formatted string
/// exceeds [`MAX_PREAMBLE_CHARS`], the body is UTF-8-safely truncated before
/// re-appending the closing `---\n`.
pub fn format_preamble(assignment: &Assignment) -> String {
    let traits_str = assignment.traits.join(" and ");
    let expertise_str = assignment.expertise.join(", ");

    let mut body = format!(
        "You are {}. You are {} by nature.\nYour expertise: {}.\n",
        assignment.name, traits_str, expertise_str
    );

    if !assignment.role_instructions.is_empty() {
        let instr = truncate_str(&assignment.role_instructions, 200);
        body.push_str(instr);
        body.push('\n');
    }

    if !assignment.recent_work.is_empty() {
        body.push_str("Recent: ");
        body.push_str(&assignment.recent_work.join(", "));
        body.push('\n');
    }

    body.push_str("---\n");

    // Enforce the total budget.
    if body.chars().count() <= MAX_PREAMBLE_CHARS {
        return body;
    }

    // Truncate: budget = MAX_PREAMBLE_CHARS - len("---\n") - 1 (newline separator)
    // "---\n" is 4 chars; the separator newline before it is 1 char → 5 total overhead.
    const CLOSING: &str = "---\n";
    let budget = MAX_PREAMBLE_CHARS - CLOSING.len() - 1;

    // Strip the closing "---\n" that was already appended, then truncate.
    let raw_body = body
        .strip_suffix(CLOSING)
        .unwrap_or(&body)
        .trim_end_matches('\n');

    let truncated = truncate_str(raw_body, budget);
    format!("{truncated}\n{CLOSING}")
}

/// Return a sub-slice of `s` containing at most `max_chars` Unicode scalar
/// values.  The cut is always at a valid UTF-8 scalar boundary because we
/// walk `char_indices` and stop at exactly the right character.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    // Fast path: already within budget.
    if s.chars().count() <= max_chars {
        return s;
    }
    // Walk char_indices so we get the byte offset of the (max_chars)-th char.
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Assignment;

    fn make_assignment(
        name: &str,
        traits: &[&str],
        expertise: &[&str],
        role_instructions: &str,
        recent_work: &[&str],
    ) -> Assignment {
        Assignment {
            agent_slot: "agent-0".to_string(),
            persona_id: 1,
            name: name.to_string(),
            traits: traits.iter().map(|s| s.to_string()).collect(),
            expertise: expertise.iter().map(|s| s.to_string()).collect(),
            role_instructions: role_instructions.to_string(),
            recent_work: recent_work.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn format_preamble_under_budget() {
        let a = make_assignment(
            "Alice Smith",
            &["pragmatic", "methodical"],
            &["backend", "performance"],
            "Focus on correctness.",
            &["code-reviewer on acme-api (2026-03-19)"],
        );

        let preamble = format_preamble(&a);

        assert!(
            preamble.contains("Alice Smith"),
            "must contain the persona name"
        );
        assert!(
            preamble.contains("pragmatic") && preamble.contains("methodical"),
            "must contain traits"
        );
        assert!(
            preamble.contains("backend") && preamble.contains("performance"),
            "must contain expertise"
        );
        assert!(
            preamble.ends_with("---\n"),
            "must end with closing delimiter"
        );

        let char_count = preamble.chars().count();
        assert!(
            char_count <= MAX_PREAMBLE_CHARS,
            "preamble is {char_count} chars, exceeds budget of {MAX_PREAMBLE_CHARS}"
        );
    }

    #[test]
    fn format_preamble_truncates_long_instructions() {
        let long_instructions = "X".repeat(400);
        let a = make_assignment(
            "Bob Jones",
            &["curious"],
            &["security"],
            &long_instructions,
            &[],
        );

        let preamble = format_preamble(&a);

        // Instructions are capped at 200 chars before being embedded.
        // Verify the preamble does not contain more than 200 X chars.
        let x_count = preamble.chars().filter(|&c| c == 'X').count();
        assert!(
            x_count <= 200,
            "role_instructions must be truncated to 200 chars; got {x_count} X chars"
        );
        assert!(preamble.ends_with("---\n"));
    }

    #[test]
    fn format_preamble_over_budget_path_never_exceeds_500() {
        // Construct an assignment with a very long name, many traits, and many
        // expertise entries so the naive render would exceed 500 chars.
        let long_name = "A".repeat(100);
        let traits: Vec<&str> = vec!["trait-one"; 10];
        let expertise: Vec<&str> = vec!["expertise-area"; 10];
        let long_instructions = "I".repeat(300);
        let recent: Vec<&str> = vec!["code-reviewer on very-long-project-name (2026-01-01)"; 5];

        let a = make_assignment(&long_name, &traits, &expertise, &long_instructions, &recent);

        let preamble = format_preamble(&a);
        let char_count = preamble.chars().count();

        assert!(
            char_count <= MAX_PREAMBLE_CHARS,
            "preamble is {char_count} chars, must never exceed {MAX_PREAMBLE_CHARS}"
        );
        assert!(
            preamble.ends_with("---\n"),
            "truncated preamble must still end with ---\\n"
        );
    }
}
