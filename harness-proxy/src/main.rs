// harness-proxy — Anthropic Messages API -> OpenAI/vLLM translating proxy.
// Replaces the LiteLLM sidecar + claude-shim.js (issue #10). See harness-proxy/PLAN.md.
//
// Step 2 (this file): non-streaming /v1/messages translation against real vLLM. Streaming SSE
// (Step 3), image-hoist + count_tokens (Step 4) and tool-calls (Step 5) still to come.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::env;

mod anthropic;
mod openai;
mod stream;
mod translate;

use anthropic::MessagesRequest;
use openai::ChatResponse;

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    vllm_url: String,   // base URL, no trailing slash
    vllm_model: String, // forced upstream model
}

#[tokio::main]
async fn main() {
    // Standalone container binds 0.0.0.0:4000 (network-reachable, like the old litellm service);
    // the in-image final deploy can override to 127.0.0.1:4000.
    let bind = env::var("HARNESS_PROXY_BIND").unwrap_or_else(|_| "0.0.0.0:4000".to_string());
    // Deployment-specific — required, never baked into the binary (set via compose). Fail fast.
    let vllm_url = require_env("VLLM_URL");
    let vllm_model = require_env("VLLM_MODEL");
    let vllm_url = vllm_url.trim_end_matches('/').to_string();

    let state = AppState {
        client: reqwest::Client::new(),
        vllm_url: vllm_url.clone(),
        vllm_model: vllm_model.clone(),
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("harness-proxy: cannot bind {bind}: {e}"));
    eprintln!("> harness-proxy listening on {bind} -> vLLM {vllm_url} (model {vllm_model})");
    axum::serve(listener, app).await.expect("server error");
}

fn require_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("harness-proxy: {key} must be set"))
}

/// POST /v1/messages — translate Anthropic request, call vLLM, translate the response back.
async fn messages(State(state): State<AppState>, Json(req): Json<MessagesRequest>) -> Response {
    let streaming = req.stream.unwrap_or(false);
    let thinking = req.thinking_enabled();
    let oai = translate::to_openai(req, state.vllm_model.clone());

    if streaming {
        return stream::stream(state.client, state.vllm_url, state.vllm_model, oai, thinking).await;
    }

    let upstream = state
        .client
        .post(format!("{}/v1/chat/completions", state.vllm_url))
        .bearer_auth("dummy") // vLLM ignores the key; Step 2 sends a placeholder (PLAN.md §1).
        .json(&oai)
        .send()
        .await;

    let resp = match upstream {
        Ok(r) => r,
        Err(e) => {
            return anthropic_error(StatusCode::BAD_GATEWAY, format!("upstream request failed: {e}"));
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        return anthropic_error(code, format!("vLLM error {status}: {body}"));
    }

    match resp.json::<ChatResponse>().await {
        Ok(cr) => Json(translate::to_anthropic(cr, state.vllm_model, thinking)).into_response(),
        Err(e) => anthropic_error(StatusCode::BAD_GATEWAY, format!("decoding vLLM response failed: {e}")),
    }
}

/// POST /v1/messages/count_tokens — Anthropic returns `{"input_tokens": N}`, officially an estimate.
/// We flatten the request text and ask vLLM's /tokenize; on any failure, fall back to chars/4.
async fn count_tokens(State(state): State<AppState>, Json(req): Json<MessagesRequest>) -> Json<Value> {
    let text = translate::request_text(&req);
    let approx = (text.len() / 4) as u64;

    let count = match state
        .client
        .post(format!("{}/tokenize", state.vllm_url))
        .json(&json!({ "model": state.vllm_model, "prompt": text }))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("count").and_then(Value::as_u64))
            .unwrap_or(approx),
        _ => approx,
    };
    Json(json!({ "input_tokens": count }))
}

/// Anthropic-shaped error envelope so Claude Code parses our failures the same as upstream's.
fn anthropic_error(status: StatusCode, message: String) -> Response {
    (
        status,
        Json(json!({
            "type": "error",
            "error": { "type": "api_error", "message": message }
        })),
    )
        .into_response()
}
