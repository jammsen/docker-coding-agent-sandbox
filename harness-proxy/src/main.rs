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
    let vllm_url = env::var("VLLM_URL").unwrap_or_else(|_| "http://10.0.0.13:8000".to_string());
    let vllm_model = env::var("VLLM_MODEL").unwrap_or_else(|_| "qwen3.6-35b".to_string());
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

/// POST /v1/messages — translate Anthropic request, call vLLM, translate the response back.
async fn messages(State(state): State<AppState>, Json(req): Json<MessagesRequest>) -> Response {
    let oai = translate::to_openai(req, state.vllm_model.clone());

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
        Ok(cr) => Json(translate::to_anthropic(cr, state.vllm_model)).into_response(),
        Err(e) => anthropic_error(StatusCode::BAD_GATEWAY, format!("decoding vLLM response failed: {e}")),
    }
}

// ponytail: still a stub. Anthropic's own docs call input_tokens an estimate, so a rough count is
// spec-compliant; Step 4 swaps in vLLM /tokenize or a chars/4 heuristic (PLAN.md §4).
async fn count_tokens(Json(_req): Json<Value>) -> Json<Value> {
    Json(json!({ "input_tokens": 1 }))
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
