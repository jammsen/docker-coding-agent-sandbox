//! Anthropic Messages API types: what we read off the wire (request) and write back (response).
//!
//! Covers text, images, tool_use/tool_result and (optionally) thinking. Unknown/echoed block
//! types (e.g. assistant `thinking` blocks sent back on a later turn) deserialize to
//! `ContentBlock::Other` and are dropped during translation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub tools: Option<Vec<Tool>>,
    // Anthropic tool_choice ({type:auto|any|tool|none, name?}); mapped to OpenAI in translate.
    #[serde(default)]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub thinking: Option<Thinking>,
    // reasoning_effort / top_k / metadata are intentionally ignored (param strip, PLAN.md §5a).
}

impl MessagesRequest {
    /// Did the client ask for extended thinking? Governs whether we surface vLLM's reasoning as
    /// Anthropic `thinking` blocks (real Anthropic only returns them when enabled). PLAN.md §0.
    pub fn thinking_enabled(&self) -> bool {
        matches!(&self.thinking, Some(t) if t.kind == "enabled")
    }
}

#[derive(Deserialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub kind: String, // "enabled" | "disabled"
}

#[derive(Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    // JSON Schema; becomes OpenAI function.parameters verbatim.
    pub input_schema: Value,
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
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: Option<ToolResultContent>,
    },
    /// thinking / redacted_thinking (echoed assistant turns) and anything unknown -> dropped.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct ImageSource {
    // Always "base64" from Claude Code's Read; kept for shape, not branched on.
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub kind: String,
    #[serde(default)]
    pub media_type: String,
    #[serde(default)]
    pub data: String,
}

/// A `tool_result` carries either a bare string or a list of blocks (text + images).
#[derive(Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

// ── Response (serialized back to Claude) ────────────────────────────────────

#[derive(Serialize)]
pub struct MessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str, // "message"
    pub role: &'static str, // "assistant"
    pub model: String,
    pub content: Vec<OutputBlock>,
    pub stop_reason: String,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Serialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// vLLM gives us no thinking signature; Claude Code only displays the block (it never re-validates
/// against us — we are the server), so a synthetic marker keeps the block shape valid. PLAN.md §0.
pub const SYNTHETIC_SIGNATURE: &str = "harness-proxy-unsigned";
