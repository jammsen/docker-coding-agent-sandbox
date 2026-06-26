//! OpenAI chat-completions wire types — the format vLLM speaks.
//! What we send (`ChatRequest`) and what we read back (`ChatResponse` / `ChatChunk`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    // include_usage makes vLLM emit a final chunk with token counts (Step 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: &'static str, // "function"
    pub function: FunctionDef,
}

#[derive(Serialize)]
pub struct FunctionDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Serialize)]
pub struct ChatMessage {
    pub role: String,
    // Absent for an assistant message that is purely tool_calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: &str, text: String) -> Self {
        Self { role: role.into(), content: Some(MessageContent::Text(text)), tool_calls: None, tool_call_id: None }
    }
}

/// Message content is either a plain string or a list of multimodal parts (text + images).
#[derive(Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
pub struct ImageUrl {
    pub url: String, // "data:<media_type>;base64,<data>"
}

#[derive(Serialize)]
pub struct ToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str, // "function"
    pub function: FunctionCallOut,
}

#[derive(Serialize)]
pub struct FunctionCallOut {
    pub name: String,
    pub arguments: String, // JSON string
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
    // This vLLM puts chain-of-thought here (reasoning model) — note `reasoning`, not `reasoning_content`.
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallIn>,
}

#[derive(Deserialize)]
pub struct ToolCallIn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: FunctionCallIn,
}

#[derive(Deserialize, Default)]
pub struct FunctionCallIn {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

// ── Streaming chunk (`chat.completion.chunk`, deserialized from vLLM SSE) ────

#[derive(Deserialize)]
pub struct ChatChunk {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    // Present only on the final chunk (we requested include_usage).
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct ChunkChoice {
    #[serde(default)]
    pub delta: ChunkDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ChunkDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
pub struct ToolCallDelta {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: ToolCallDeltaFn,
}

#[derive(Deserialize, Default)]
pub struct ToolCallDeltaFn {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}
