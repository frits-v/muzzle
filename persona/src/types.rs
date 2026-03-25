//! Core types and role vocabulary for muzzle-persona.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Role vocabulary
// ---------------------------------------------------------------------------

/// All recognized roles a persona can be assigned to.
pub const ROLE_VOCABULARY: &[&str] = &[
    "code-reviewer",
    "security-review",
    "researcher",
    "architecture",
    "implementation",
    "testing",
    "documentation",
    "red-team",
    "infrastructure",
    "debugging",
    "general",
];

/// Return the qualifying expertise keywords for a given role.
/// An empty slice means any expertise qualifies.
pub fn expertise_for_role(role: &str) -> &'static [&'static str] {
    match role {
        "code-reviewer" => &["backend", "frontend", "performance", "architecture"],
        "security-review" => &["security", "infrastructure"],
        "researcher" => &["ML", "architecture", "backend"],
        "architecture" => &["architecture", "backend", "infrastructure"],
        "implementation" => &["backend", "frontend", "performance"],
        "testing" => &["testing", "frontend", "backend"],
        "documentation" => &["documentation", "frontend"],
        "red-team" => &["security"],
        "infrastructure" => &["infrastructure", "backend"],
        "debugging" => &["backend", "frontend", "performance"],
        _ => &[],
    }
}

/// Normalize a raw role string to a canonical role from [`ROLE_VOCABULARY`].
///
/// Resolution order:
/// 1. Exact match (case-sensitive).
/// 2. Prefix match — the shortest matching role wins.
/// 3. Fallback: `"general"`.
///
/// An empty input returns `"general"` immediately.
pub fn normalize_role(input: &str) -> &'static str {
    if input.is_empty() {
        return "general";
    }

    // Exact match.
    for &role in ROLE_VOCABULARY {
        if role == input {
            return role;
        }
    }

    // Prefix match — collect all candidates, pick shortest.
    let mut best: Option<&'static str> = None;
    for &role in ROLE_VOCABULARY {
        if role.starts_with(input) {
            match best {
                None => best = Some(role),
                Some(prev) if role.len() < prev.len() => best = Some(role),
                _ => {}
            }
        }
    }
    if let Some(matched) = best {
        return matched;
    }

    "general"
}

// ---------------------------------------------------------------------------
// Persona types
// ---------------------------------------------------------------------------

/// Lifecycle status of a persona.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersonaStatus {
    Active,
    Archived,
}

/// A persistent agent persona stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: i64,
    pub name: String,
    pub traits: Vec<String>,
    pub expertise: Vec<String>,
    /// Per-role instruction strings.
    pub role_instructions: HashMap<String, String>,
    /// Smoothed affinity score per role (higher = preferred).
    pub affinity_scores: HashMap<String, f32>,
    /// Number of times assigned to each role.
    pub role_counts: HashMap<String, u32>,
    pub status: PersonaStatus,
    pub assigned_to_session: Option<String>,
    pub created_at: String,
    pub last_assigned: Option<String>,
}

/// The result of assigning a persona to an agent slot for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub agent_slot: String,
    pub persona_id: i64,
    pub name: String,
    pub traits: Vec<String>,
    pub expertise: Vec<String>,
    /// Role-specific instructions to inject into the agent preamble.
    pub role_instructions: String,
    /// Pre-formatted recent work entries, e.g. `"code-reviewer on acme-api (2026-03-19)"`.
    pub recent_work: Vec<String>,
}

/// A single feedback entry recorded after an assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    pub id: i64,
    pub persona_id: i64,
    pub timestamp: String,
    pub project: String,
    pub role: String,
    pub observation: String,
    pub source: String,
    pub compacted: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_exact_match() {
        assert_eq!(normalize_role("architecture"), "architecture");
        assert_eq!(normalize_role("red-team"), "red-team");
        assert_eq!(normalize_role("general"), "general");
    }

    #[test]
    fn normalize_prefix_match() {
        // "impl" is a prefix of "implementation" only.
        assert_eq!(normalize_role("impl"), "implementation");
        // "arch" is a prefix of "architecture" only.
        assert_eq!(normalize_role("arch"), "architecture");
        // "debug" is a prefix of "debugging" only.
        assert_eq!(normalize_role("debug"), "debugging");
    }

    #[test]
    fn normalize_unknown_falls_back_to_general() {
        assert_eq!(normalize_role("unknown-xyz"), "general");
        assert_eq!(normalize_role(""), "general");
    }

    #[test]
    fn expertise_for_known_roles() {
        let security = expertise_for_role("security-review");
        assert!(security.contains(&"security"));
        assert!(security.contains(&"infrastructure"));

        let testing = expertise_for_role("testing");
        assert!(testing.contains(&"testing"));
    }

    #[test]
    fn expertise_for_general_roles_is_empty() {
        assert!(expertise_for_role("general").is_empty());
        assert!(expertise_for_role("nonexistent").is_empty());
    }
}
