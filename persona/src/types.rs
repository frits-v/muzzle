use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Maps roles to qualifying expertise tags. Empty slice = any expertise qualifies.
pub fn expertise_for_role(role: &str) -> &'static [&'static str] {
    match role {
        "security-review" => &["security", "compliance", "red-team"],
        "architecture" => &["architecture", "infrastructure", "backend"],
        "red-team" => &["red-team", "security"],
        "implementation" => &["backend", "frontend", "infrastructure", "ML"],
        "testing" => &["testing", "backend", "frontend"],
        "infrastructure" => &["infrastructure", "devops", "architecture"],
        _ => &[],
    }
}

/// Normalize a role string against the vocabulary.
/// Exact match first, then prefix match (shortest wins), fallback "general".
pub fn normalize_role(input: &str) -> &'static str {
    if let Some(&role) = ROLE_VOCABULARY.iter().find(|&&r| r == input) {
        return role;
    }
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
    best.unwrap_or("general")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PersonaStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "archived")]
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: i64,
    pub name: String,
    pub traits: Vec<String>,
    pub expertise: Vec<String>,
    pub role_instructions: HashMap<String, String>,
    pub affinity_scores: HashMap<String, f32>,
    pub role_counts: HashMap<String, u32>,
    pub status: PersonaStatus,
    pub assigned_to_session: Option<String>,
    pub created_at: String,
    pub last_assigned: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub agent_slot: usize,
    pub persona_id: i64,
    pub name: String,
    pub traits: Vec<String>,
    pub expertise: Vec<String>,
    pub role_instructions: String,
}

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
