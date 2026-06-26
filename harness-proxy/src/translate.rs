//! Anthropic <-> OpenAI translation (PLAN.md §5). Request: messages/images/tools + image hoist.
//! Response: non-streaming OpenAI -> Anthropic (streaming lives in stream.rs).

use serde_json::{Value, json};

use crate::anthropic::{
    self, Content, ContentBlock, ImageSource, MessagesRequest, MessagesResponse, OutputBlock,
    SystemPrompt, ToolResultContent,
};
use crate::openai::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, FunctionCallOut, FunctionDef, ImageUrl,
    MessageContent, ToolCallOut, ToolDef,
};

const HOIST_PLACEHOLDER: &str = "[image returned by tool — see following message]";
const HOIST_LEAD: &str = "Image(s) returned by the tool call above:";

/// Anthropic request -> OpenAI chat request. `model` is forced to VLLM_MODEL (alias map).
pub fn to_openai(req: MessagesRequest, model: String) -> ChatRequest {
    let mut messages = Vec::with_capacity(req.messages.len() + 1);

    // Anthropic `system` (string or blocks) -> a leading OpenAI system message.
    if let Some(system) = req.system {
        let text = match system {
            SystemPrompt::Text(t) => t,
            SystemPrompt::Blocks(b) => blocks_text(&b),
        };
        if !text.is_empty() {
            messages.push(ChatMessage::text("system", text));
        }
    }

    for m in req.messages {
        match m.content {
            Content::Text(t) => messages.push(ChatMessage::text(&m.role, t)),
            Content::Blocks(blocks) => push_blocks(&mut messages, &m.role, blocks),
        }
    }

    let tools = req.tools.map(|ts| {
        ts.into_iter()
            .map(|t| ToolDef {
                kind: "function",
                function: FunctionDef { name: t.name, description: t.description, parameters: t.input_schema },
            })
            .collect()
    });

    ChatRequest {
        model,
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stop: req.stop_sequences,
        // The streaming handler flips these on; the non-streaming path leaves them off.
        stream: None,
        stream_options: None,
        tools,
        tool_choice: map_tool_choice(req.tool_choice),
    }
}

/// Expand one Anthropic block-list message into one or more OpenAI messages.
/// Assistant: text + tool_use -> a single assistant message with content + tool_calls.
/// User: tool_result -> a `tool` message each (+ image hoist); text/image -> a user message.
fn push_blocks(out: &mut Vec<ChatMessage>, role: &str, blocks: Vec<ContentBlock>) {
    if role == "assistant" {
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        for b in blocks {
            match b {
                ContentBlock::Text { text: t } => push_line(&mut text, &t),
                ContentBlock::ToolUse { id, name, input } => tool_calls.push(ToolCallOut {
                    id,
                    kind: "function",
                    function: FunctionCallOut { name, arguments: input.to_string() },
                }),
                _ => {} // images/thinking from an assistant turn are dropped
            }
        }
        out.push(ChatMessage {
            role: "assistant".into(),
            content: (!text.is_empty()).then(|| MessageContent::Text(text)),
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            tool_call_id: None,
        });
        return;
    }

    // user turn: tool_result -> tool messages (in order); text/image -> parts; hoist tool images.
    let mut parts: Vec<ContentPart> = Vec::new();
    let mut hoisted: Vec<ContentPart> = Vec::new();
    for b in blocks {
        match b {
            ContentBlock::Text { text } => parts.push(ContentPart::Text { text }),
            ContentBlock::Image { source } => parts.push(image_part(source)),
            ContentBlock::ToolResult { tool_use_id, content } => {
                let (text, mut images) = split_tool_result(content);
                let content = if text.is_empty() && !images.is_empty() {
                    HOIST_PLACEHOLDER.to_string()
                } else {
                    text
                };
                out.push(ChatMessage {
                    role: "tool".into(),
                    content: Some(MessageContent::Text(content)),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id),
                });
                hoisted.append(&mut images);
            }
            _ => {}
        }
    }
    if !parts.is_empty() {
        out.push(ChatMessage { role: role.into(), content: Some(MessageContent::Parts(parts)), tool_calls: None, tool_call_id: None });
    }
    // 🔑 Image hoist: OpenAI can't carry images in a `tool` message, so re-attach them in a
    // fresh user message right after (claude-shim.js `hoistToolResultImages`, PLAN.md §5a).
    if !hoisted.is_empty() {
        let mut content = vec![ContentPart::Text { text: HOIST_LEAD.to_string() }];
        content.extend(hoisted);
        out.push(ChatMessage { role: "user".into(), content: Some(MessageContent::Parts(content)), tool_calls: None, tool_call_id: None });
    }
}

/// Split a tool_result's content into (text, image parts). Text sub-blocks are concatenated.
fn split_tool_result(content: Option<ToolResultContent>) -> (String, Vec<ContentPart>) {
    let mut text = String::new();
    let mut images = Vec::new();
    match content {
        None => {}
        Some(ToolResultContent::Text(t)) => text = t,
        Some(ToolResultContent::Blocks(blocks)) => {
            for b in blocks {
                match b {
                    ContentBlock::Text { text: t } => push_line(&mut text, &t),
                    ContentBlock::Image { source } => images.push(image_part(source)),
                    _ => {}
                }
            }
        }
    }
    (text, images)
}

fn image_part(source: ImageSource) -> ContentPart {
    ContentPart::ImageUrl {
        image_url: ImageUrl { url: format!("data:{};base64,{}", source.media_type, source.data) },
    }
}

/// Anthropic tool_choice -> OpenAI tool_choice.
fn map_tool_choice(tc: Option<Value>) -> Option<Value> {
    let tc = tc?;
    Some(match tc.get("type").and_then(Value::as_str) {
        Some("auto") => json!("auto"),
        Some("any") => json!("required"),
        Some("none") => json!("none"),
        Some("tool") => json!({
            "type": "function",
            "function": { "name": tc.get("name").and_then(Value::as_str).unwrap_or_default() }
        }),
        _ => json!("auto"),
    })
}

/// Concatenate text blocks; non-text blocks are ignored (used for system + flattening).
fn blocks_text(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        if let ContentBlock::Text { text } = b {
            push_line(&mut out, text);
        }
    }
    out
}

fn push_line(buf: &mut String, s: &str) {
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(s);
}

/// All request text flattened, for the count_tokens estimate (PLAN.md §4).
pub fn request_text(req: &MessagesRequest) -> String {
    let mut out = String::new();
    if let Some(system) = &req.system {
        match system {
            SystemPrompt::Text(t) => push_line(&mut out, t),
            SystemPrompt::Blocks(b) => push_line(&mut out, &blocks_text(b)),
        }
    }
    for m in &req.messages {
        match &m.content {
            Content::Text(t) => push_line(&mut out, t),
            Content::Blocks(b) => push_line(&mut out, &blocks_text(b)),
        }
    }
    out
}

/// OpenAI chat response -> Anthropic Messages response (non-streaming). `thinking` surfaces vLLM's
/// reasoning as a thinking block only when the client enabled it (PLAN.md §0).
pub fn to_anthropic(resp: ChatResponse, model: String, thinking: bool) -> MessagesResponse {
    let (msg, finish) = match resp.choices.into_iter().next() {
        Some(c) => (c.message, c.finish_reason),
        None => Default::default(),
    };
    let usage = resp.usage.unwrap_or_default();

    let mut content = Vec::new();
    if thinking {
        if let Some(r) = msg.reasoning.filter(|r| !r.is_empty()) {
            content.push(OutputBlock::Thinking { thinking: r, signature: anthropic::SYNTHETIC_SIGNATURE.into() });
        }
    }
    if let Some(t) = msg.content.filter(|t| !t.is_empty()) {
        content.push(OutputBlock::Text { text: t });
    }
    for tc in msg.tool_calls {
        content.push(OutputBlock::ToolUse {
            id: tc.id.unwrap_or_else(|| "toolu_proxy".into()),
            name: tc.function.name.unwrap_or_default(),
            // arguments is a JSON string; parse to an object, else pass through as a string.
            input: tc.function.arguments.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or(json!({})),
        });
    }

    MessagesResponse {
        id: resp.id.map(|i| format!("msg_{i}")).unwrap_or_else(|| "msg_proxy".to_string()),
        kind: "message",
        role: "assistant",
        model,
        content,
        stop_reason: stop_reason(finish.as_deref()),
        stop_sequence: None,
        usage: anthropic::Usage { input_tokens: usage.prompt_tokens, output_tokens: usage.completion_tokens },
    }
}

/// OpenAI `finish_reason` -> Anthropic `stop_reason` (PLAN.md §5b).
pub(crate) fn stop_reason(finish: Option<&str>) -> String {
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
    use crate::openai::ChatResponse;

    fn req(v: Value) -> MessagesRequest {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn request_forces_model_and_maps_system_plus_messages() {
        let oai = to_openai(
            req(json!({
                "model": "claude-sonnet-4-5", "system": "be brief",
                "messages": [{ "role": "user", "content": "hi" }],
                "max_tokens": 100, "temperature": 0.2
            })),
            "qwen3.6-35b".to_string(),
        );
        assert_eq!(oai.model, "qwen3.6-35b"); // alias map: incoming model ignored
        assert_eq!(oai.messages.len(), 2);
        assert_eq!(oai.messages[0].role, "system");
        assert_eq!(oai.messages[1].role, "user");
        assert_eq!(oai.max_tokens, Some(100));
    }

    #[test]
    fn tool_result_image_is_hoisted_into_a_trailing_user_message() {
        // A user turn whose tool_result returns text + an image (the LiteLLM-dropped case).
        let oai = to_openai(
            req(json!({
                "messages": [{
                    "role": "user",
                    "content": [{
                        "type": "tool_result", "tool_use_id": "toolu_1",
                        "content": [
                            { "type": "text", "text": "here it is" },
                            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }
                        ]
                    }]
                }]
            })),
            "m".to_string(),
        );

        // tool message keeps text only; image is re-attached in a following user message.
        assert_eq!(oai.messages.len(), 2);
        assert_eq!(oai.messages[0].role, "tool");
        assert_eq!(oai.messages[0].tool_call_id.as_deref(), Some("toolu_1"));
        match oai.messages[0].content.as_ref().unwrap() {
            MessageContent::Text(t) => assert_eq!(t, "here it is"), // image stripped out
            _ => panic!("tool content should be plain text"),
        }
        assert_eq!(oai.messages[1].role, "user");
        let body = serde_json::to_value(&oai.messages[1]).unwrap();
        assert_eq!(body["content"][0]["text"], HOIST_LEAD);
        assert_eq!(body["content"][1]["type"], "image_url");
        assert!(body["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,AAAA"));
    }

    #[test]
    fn assistant_tool_use_maps_to_openai_tool_calls() {
        let oai = to_openai(
            req(json!({
                "messages": [{
                    "role": "assistant",
                    "content": [{ "type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": { "city": "Paris" } }]
                }]
            })),
            "m".to_string(),
        );
        let tc = oai.messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].id, "toolu_1");
        assert_eq!(tc[0].function.name, "get_weather");
        assert_eq!(tc[0].function.arguments, r#"{"city":"Paris"}"#);
    }

    #[test]
    fn response_maps_tool_use_and_drops_reasoning_when_thinking_off() {
        let resp: ChatResponse = serde_json::from_value(json!({
            "id": "cmpl-1",
            "choices": [{
                "message": {
                    "content": null, "reasoning": "hmm",
                    "tool_calls": [{ "id": "call_1", "function": { "name": "f", "arguments": "{\"x\":1}" } }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
        }))
        .unwrap();

        let a = to_anthropic(resp, "m".to_string(), false);
        assert_eq!(a.stop_reason, "tool_use");
        assert_eq!(a.content.len(), 1); // reasoning dropped (thinking off)
        let v = serde_json::to_value(&a.content[0]).unwrap();
        assert_eq!(v["type"], "tool_use");
        assert_eq!(v["name"], "f");
        assert_eq!(v["input"]["x"], 1);
    }

    #[test]
    fn response_surfaces_thinking_block_when_enabled() {
        let resp: ChatResponse = serde_json::from_value(json!({
            "choices": [{ "message": { "content": "answer", "reasoning": "because" }, "finish_reason": "stop" }]
        }))
        .unwrap();
        let a = to_anthropic(resp, "m".to_string(), true);
        let kinds: Vec<_> = a.content.iter().map(|b| serde_json::to_value(b).unwrap()["type"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["thinking", "text"]);
    }
}
