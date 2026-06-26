//! Anthropic <-> OpenAI translation, non-streaming and text-only (PLAN.md §5a / §5b, Step 2).
//! Streaming (Step 3), image-hoist (Step 4) and tool-calls (Step 5) are out of scope here.

use crate::anthropic::{
    self, Content, ContentBlock, MessagesRequest, MessagesResponse, SystemPrompt, TextBlock,
};
use crate::openai::{ChatMessage, ChatRequest, ChatResponse};

/// Anthropic request -> OpenAI chat request.
/// `model` is forced to the upstream vLLM model (alias map: any incoming model -> VLLM_MODEL).
pub fn to_openai(req: MessagesRequest, model: String) -> ChatRequest {
    let mut messages = Vec::with_capacity(req.messages.len() + 1);

    // Anthropic `system` (string or blocks) -> a leading OpenAI system message.
    if let Some(system) = req.system {
        let text = match system {
            SystemPrompt::Text(t) => t,
            SystemPrompt::Blocks(b) => blocks_text(&b),
        };
        if !text.is_empty() {
            messages.push(ChatMessage { role: "system".into(), content: text });
        }
    }

    for m in req.messages {
        let content = match m.content {
            Content::Text(t) => t,
            Content::Blocks(b) => blocks_text(&b),
        };
        messages.push(ChatMessage { role: m.role, content });
    }

    ChatRequest {
        model,
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stop: req.stop_sequences,
    }
}

/// Concatenate text blocks; non-text blocks (image / tool_*) are dropped in Step 2.
fn blocks_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::Other => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// OpenAI chat response -> Anthropic Messages response (non-streaming).
pub fn to_anthropic(resp: ChatResponse, model: String) -> MessagesResponse {
    let (text, finish) = match resp.choices.into_iter().next() {
        Some(c) => (c.message.content.unwrap_or_default(), c.finish_reason),
        None => (String::new(), None),
    };
    let usage = resp.usage.unwrap_or_default();

    MessagesResponse {
        id: resp.id.map(|i| format!("msg_{i}")).unwrap_or_else(|| "msg_proxy".to_string()),
        kind: "message",
        role: "assistant",
        model,
        content: vec![TextBlock { kind: "text", text }],
        stop_reason: stop_reason(finish.as_deref()),
        stop_sequence: None,
        usage: anthropic::Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        },
    }
}

/// OpenAI `finish_reason` -> Anthropic `stop_reason` (PLAN.md §5b).
fn stop_reason(finish: Option<&str>) -> String {
    match finish {
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        _ => "end_turn", // "stop" and anything unknown
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_forces_model_and_maps_system_plus_messages() {
        let req: MessagesRequest = serde_json::from_value(json!({
            "model": "claude-sonnet-4-5",
            "system": "be brief",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 100,
            "temperature": 0.2
        }))
        .unwrap();

        let oai = to_openai(req, "qwen3.6-35b".to_string());

        assert_eq!(oai.model, "qwen3.6-35b"); // alias map: incoming model ignored
        assert_eq!(oai.messages.len(), 2);
        assert_eq!(oai.messages[0].role, "system");
        assert_eq!(oai.messages[0].content, "be brief");
        assert_eq!(oai.messages[1].role, "user");
        assert_eq!(oai.messages[1].content, "hi");
        assert_eq!(oai.max_tokens, Some(100));
    }

    #[test]
    fn request_concatenates_text_blocks_and_drops_non_text() {
        let req: MessagesRequest = serde_json::from_value(json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "look at this" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } },
                    { "type": "text", "text": "thanks" }
                ]
            }]
        }))
        .unwrap();

        let oai = to_openai(req, "m".to_string());

        // text blocks joined; the image block is dropped (Step 4).
        assert_eq!(oai.messages.len(), 1);
        assert_eq!(oai.messages[0].content, "look at this\nthanks");
    }

    #[test]
    fn response_maps_content_finish_reason_and_usage() {
        let resp: ChatResponse = serde_json::from_value(json!({
            "id": "cmpl-1",
            "choices": [{ "message": { "content": "hello" }, "finish_reason": "length" }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
        }))
        .unwrap();

        let a = to_anthropic(resp, "qwen3.6-35b".to_string());

        assert_eq!(a.id, "msg_cmpl-1");
        assert_eq!(a.content[0].text, "hello");
        assert_eq!(a.stop_reason, "max_tokens"); // length -> max_tokens
        assert_eq!(a.usage.input_tokens, 5);
        assert_eq!(a.usage.output_tokens, 2);
    }
}
