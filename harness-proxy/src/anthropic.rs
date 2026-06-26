//! Anthropic Messages API types: what we read off the wire (request) and write back (response).
//!
//! Step 2 is text-only. `image` / `tool_use` / `tool_result` content blocks still deserialize
//! (so requests don't 422), but they collapse into `ContentBlock::Other` and are dropped during
//! translation. Image-hoist is Step 4, tool-calls are Step 5 (PLAN.md §5a, §8).

use serde::{Deserialize, Serialize};

// ── Request (deserialized from Claude) ──────────────────────────────────────

#[derive(Deserialize)]
pub struct MessagesRequest {
    // Deliberately unused: the alias map forces VLLM_MODEL regardless of what Claude sends.
    #[allow(dead_code)]
    pub model: Option<String>,
    #[serde(default)]
    pub system: Option<SystemPrompt>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    // tools / tool_choice / metadata / top_k are ignored for Step 2 (param strip, PLAN.md §5a).
}

/// Anthropic `system` is either a bare string or a list of content blocks.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Content,
}

/// Message `content` is either a bare string or a list of content blocks.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    /// image / tool_use / tool_result land in Step 4/5; everything non-text drops here for now.
    #[serde(other)]
    Other,
}

// ── Response (serialized back to Claude) ────────────────────────────────────

#[derive(Serialize)]
pub struct MessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str, // "message"
    pub role: &'static str, // "assistant"
    pub model: String,
    pub content: Vec<TextBlock>,
    pub stop_reason: String,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

#[derive(Serialize)]
pub struct TextBlock {
    #[serde(rename = "type")]
    pub kind: &'static str, // "text"
    pub text: String,
}

#[derive(Serialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
