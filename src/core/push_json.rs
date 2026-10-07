//! What `loom push` did, as JSON: agent mode's `push` object (spec 019).

use serde::Serialize;

/// What one `loom push` did.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct PushReport {
    pub remote: String,
    /// The `loom.remote-type` value in effect.
    pub forge: &'static str,
    pub pushed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub republished: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub not_pushed: Vec<String>,
    /// The server's `remote:` links shown on the success line.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prs: Vec<PrReport>,
    /// The GitHub stack linking the PRs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<u64>,
}

/// One PR line of a push: created, updated, or the `PR not created` warning.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PrReport {
    pub branch: String,
    pub base: String,
    pub state: PrState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub retargeted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Created,
    Updated,
    NotCreated,
}
