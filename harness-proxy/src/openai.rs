//! OpenAI chat-completions wire types — the format vLLM speaks.
//! What we send (`ChatRequest`) and what we read back (`ChatResponse`). Non-streaming, Step 2.

use serde::{Deserialize, Serialize};

// ── Request (serialized to vLLM) ────────────────────────────────────────────

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    // Skip when absent so we don't send `null` and let vLLM apply its own defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatMessage {
    pub role: String,
    // Step 2 is text-only, so a flat string suffices. Multimodal parts arrive with image-hoist (Step 4).
    pub content: String,
}

// ── Response (deserialized from vLLM) ───────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub message: ChoiceMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ChoiceMessage {
    #[serde(default)]
    pub content: Option<String>,
    // tool_calls handled in Step 5.
}

#[derive(Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}
