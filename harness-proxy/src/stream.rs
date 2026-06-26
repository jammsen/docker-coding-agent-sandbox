//! Streaming translation: OpenAI `chat.completion.chunk` SSE -> Anthropic Messages SSE (PLAN.md §5c).
//!
//! Maps text, tool_use (incremental `input_json_delta`) and — only when the client enabled
//! thinking — vLLM's `delta.reasoning` to Anthropic `thinking` blocks. A single monotonic block
//! index spans thinking/text/tool_use blocks.

use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::ProxyError;
use crate::anthropic::SYNTHETIC_SIGNATURE;
use crate::openai::{ChatChunk, ChatRequest};

/// POST the streaming request to vLLM and return an axum SSE response carrying the translated
/// Anthropic Messages event stream. Pre-stream failures (connect/timeout/non-2xx) become a clean
/// `ProxyError` (correct status); once the 200 SSE has started we can no longer change the status.
pub async fn stream(
    client: reqwest::Client,
    url: String,
    model: String,
    mut oai: ChatRequest,
    thinking: bool,
) -> Result<Response, ProxyError> {
    oai.stream = Some(true);
    oai.stream_options = Some(crate::openai::StreamOptions { include_usage: true });

    // No overall timeout: a stream legitimately runs long (connect_timeout still guards the dial).
    let resp = client
        .post(format!("{url}/v1/chat/completions"))
        .bearer_auth("dummy")
        .json(&oai)
        .send()
        .await
        .map_err(ProxyError::from_reqwest)?;

    let status = resp.status();
    tracing::info!(model = %model, upstream_status = status.as_u16(), stream = true, "upstream");
    if !status.is_success() {
        return Err(ProxyError::from_upstream_status(status));
    }

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);
    tokio::spawn(pump(resp, model, thinking, tx));

    Ok(Sse::new(ReceiverStream::new(rx)).into_response())
}

/// Read vLLM's SSE chunks, translate them, and push the resulting events onto `tx`.
async fn pump(mut resp: reqwest::Response, model: String, thinking: bool, tx: mpsc::Sender<Result<Event, Infallible>>) {
    let mut buf = String::new();
    let mut t = Translator::new(model, thinking);

    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            _ => break, // end of stream or read error — fall through to the closing events
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // SSE lines can span chunk boundaries; process only complete (newline-terminated) lines.
        while let Some(nl) = buf.find('\n') {
            let line: String = buf.drain(..=nl).collect();
            let Some(data) = line.trim_end().strip_prefix("data: ") else { continue };
            if data == "[DONE]" {
                continue;
            }
            let Ok(c) = serde_json::from_str::<ChatChunk>(data) else { continue };
            for (name, payload) in t.push(c) {
                if tx.send(Ok(event(name, payload))).await.is_err() {
                    return; // client hung up
                }
            }
        }
    }

    for (name, payload) in t.finish() {
        if tx.send(Ok(event(name, payload))).await.is_err() {
            return;
        }
    }
}

/// Which content block (if any) is currently open, and its Anthropic block index.
enum Open {
    None,
    Thinking(i32),
    Text(i32),
    Tool(i32),
}

/// The streaming state machine. `push` translates one OpenAI chunk into zero or more Anthropic
/// events; `finish` emits the closing events. Pure (no I/O) so it can be unit-tested.
struct Translator {
    model: String,
    thinking: bool,
    started: bool,
    next_index: i32,
    open: Open,
    current_tool: Option<u32>, // openai tool_calls index currently streaming
    stop_reason: String,
    output_tokens: u32,
    msg_id: String,
}

impl Translator {
    fn new(model: String, thinking: bool) -> Self {
        Self {
            model,
            thinking,
            started: false,
            next_index: 0,
            open: Open::None,
            current_tool: None,
            stop_reason: "end_turn".to_string(),
            output_tokens: 0,
            msg_id: "msg_proxy".to_string(),
        }
    }

    fn push(&mut self, c: ChatChunk) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        if self.msg_id == "msg_proxy" {
            if let Some(id) = c.id.as_deref() {
                self.msg_id = format!("msg_{id}");
            }
        }
        if let Some(u) = c.usage {
            self.output_tokens = u.completion_tokens;
        }
        if !self.started {
            self.started = true;
            out.push(self.message_start());
        }

        for choice in c.choices {
            if let Some(fr) = choice.finish_reason {
                self.stop_reason = crate::translate::stop_reason(Some(&fr));
            }
            let d = choice.delta;

            if self.thinking {
                if let Some(r) = d.reasoning.filter(|r| !r.is_empty()) {
                    if !matches!(self.open, Open::Thinking(_)) {
                        let i = self.open_block(&mut out, "content_block_start", json!({"type":"thinking","thinking":""}));
                        self.open = Open::Thinking(i);
                    }
                    if let Open::Thinking(i) = self.open {
                        out.push(delta(i, json!({"type":"thinking_delta","thinking":r})));
                    }
                }
            }

            if let Some(text) = d.content.filter(|t| !t.is_empty()) {
                if !matches!(self.open, Open::Text(_)) {
                    let i = self.open_block(&mut out, "content_block_start", json!({"type":"text","text":""}));
                    self.open = Open::Text(i);
                }
                if let Open::Text(i) = self.open {
                    out.push(delta(i, json!({"type":"text_delta","text":text})));
                }
            }

            for tc in d.tool_calls {
                if self.current_tool != Some(tc.index) {
                    // New tool call: close whatever was open and start a fresh tool_use block.
                    let i = self.open_block(
                        &mut out,
                        "content_block_start",
                        json!({
                            "type": "tool_use",
                            "id": tc.id.clone().unwrap_or_else(|| "toolu_proxy".into()),
                            "name": tc.function.name.clone().unwrap_or_default(),
                            "input": {}
                        }),
                    );
                    self.open = Open::Tool(i);
                    self.current_tool = Some(tc.index);
                }
                if let (Open::Tool(i), Some(args)) = (&self.open, tc.function.arguments) {
                    if !args.is_empty() {
                        out.push(delta(*i, json!({"type":"input_json_delta","partial_json":args})));
                    }
                }
            }
        }
        out
    }

    fn finish(mut self) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(self.message_start());
        }
        self.close_open(&mut out);
        out.push((
            "message_delta",
            json!({
                "type":"message_delta",
                "delta":{"stop_reason":self.stop_reason,"stop_sequence":null},
                "usage":{"output_tokens":self.output_tokens}
            }),
        ));
        out.push(("message_stop", json!({"type":"message_stop"})));
        out
    }

    /// Emit a `content_block_start` at the next index, close any previously open block first.
    fn open_block(&mut self, out: &mut Vec<(&'static str, Value)>, _kind: &str, block: Value) -> i32 {
        self.close_open(out);
        let i = self.next_index;
        self.next_index += 1;
        out.push((
            "content_block_start",
            json!({"type":"content_block_start","index":i,"content_block":block}),
        ));
        i
    }

    fn close_open(&mut self, out: &mut Vec<(&'static str, Value)>) {
        match self.open {
            Open::None => return,
            Open::Thinking(i) => {
                // Thinking blocks close with a signature (synthetic — vLLM gives none).
                out.push(delta(i, json!({"type":"signature_delta","signature":SYNTHETIC_SIGNATURE})));
                out.push(("content_block_stop", json!({"type":"content_block_stop","index":i})));
            }
            Open::Text(i) | Open::Tool(i) => {
                out.push(("content_block_stop", json!({"type":"content_block_stop","index":i})));
            }
        }
        self.open = Open::None;
        self.current_tool = None;
    }

    fn message_start(&self) -> (&'static str, Value) {
        (
            "message_start",
            json!({
                "type":"message_start",
                "message":{
                    "id":self.msg_id,"type":"message","role":"assistant","model":self.model,
                    "content":[],"stop_reason":null,"stop_sequence":null,
                    "usage":{"input_tokens":0,"output_tokens":0}
                }
            }),
        )
    }
}

fn delta(index: i32, delta: Value) -> (&'static str, Value) {
    ("content_block_delta", json!({"type":"content_block_delta","index":index,"delta":delta}))
}

/// Build an Anthropic SSE event; axum's Sse adds the `event:`/`data:` framing.
fn event(name: &str, data: Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(v: Value) -> ChatChunk {
        serde_json::from_value(v).unwrap()
    }

    fn run(thinking: bool, chunks: Vec<Value>) -> (Vec<&'static str>, Vec<Value>) {
        let mut t = Translator::new("m".to_string(), thinking);
        let mut names = Vec::new();
        let mut payloads = Vec::new();
        for c in chunks {
            for (n, p) in t.push(chunk(c)) {
                names.push(n);
                payloads.push(p);
            }
        }
        for (n, p) in t.finish() {
            names.push(n);
            payloads.push(p);
        }
        (names, payloads)
    }

    #[test]
    fn text_stream_drops_reasoning_when_thinking_off() {
        let (names, payloads) = run(
            false,
            vec![
                json!({"id":"abc","choices":[{"delta":{"role":"assistant","content":""}}]}),
                json!({"choices":[{"delta":{"reasoning":"think"}}]}), // dropped
                json!({"choices":[{"delta":{"content":"Hi"}}]}),
                json!({"choices":[{"delta":{"content":" there"},"finish_reason":"stop"}]}),
                json!({"choices":[],"usage":{"completion_tokens":7}}),
            ],
        );
        assert_eq!(
            names,
            ["message_start", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "message_delta", "message_stop"]
        );
        let text: String = payloads
            .iter()
            .filter_map(|p| (p["type"] == "content_block_delta").then(|| p["delta"]["text"].as_str().unwrap_or("")))
            .collect();
        assert_eq!(text, "Hi there");
    }

    #[test]
    fn tool_call_stream_maps_to_tool_use_and_input_json_delta() {
        let (names, payloads) = run(
            false,
            vec![
                json!({"id":"abc","choices":[{"delta":{"role":"assistant"}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{\"ci"}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ty\":\"Paris\"}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
                json!({"choices":[],"usage":{"completion_tokens":9}}),
            ],
        );
        assert_eq!(
            names,
            ["message_start", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "message_delta", "message_stop"]
        );
        // tool_use block carries id + name; arguments arrive as input_json_delta fragments.
        let start = payloads.iter().find(|p| p["type"] == "content_block_start").unwrap();
        assert_eq!(start["content_block"]["type"], "tool_use");
        assert_eq!(start["content_block"]["name"], "get_weather");
        let args: String = payloads
            .iter()
            .filter_map(|p| (p["delta"]["type"] == "input_json_delta").then(|| p["delta"]["partial_json"].as_str().unwrap()))
            .collect();
        assert_eq!(args, r#"{"city":"Paris"}"#);
        let md = payloads.iter().find(|p| p["type"] == "message_delta").unwrap();
        assert_eq!(md["delta"]["stop_reason"], "tool_use");
    }

    #[test]
    fn thinking_block_emitted_with_signature_when_enabled() {
        let (names, _) = run(
            true,
            vec![
                json!({"id":"x","choices":[{"delta":{"reasoning":"because"}}]}),
                json!({"choices":[{"delta":{"content":"answer"},"finish_reason":"stop"}]}),
            ],
        );
        // thinking block opens, closes (with signature_delta) before the text block opens.
        assert_eq!(
            names,
            [
                "message_start",
                "content_block_start", // thinking
                "content_block_delta", // thinking_delta
                "content_block_delta", // signature_delta
                "content_block_stop",  // thinking closed
                "content_block_start", // text
                "content_block_delta", // text_delta
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
    }
}
