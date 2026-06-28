// harness-proxy — Anthropic Messages API -> OpenAI/vLLM translating proxy.
// Replaces the LiteLLM sidecar + claude-shim.js (issue #10). See harness-proxy/PLAN.md.

use axum::{
    Json, Router,
    extract::{Request, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::{env, io};
use tracing::Instrument;

mod anthropic;
mod openai;
mod stream;
mod translate;

use anthropic::MessagesRequest;
use openai::ChatResponse;

// vLLM can be slow on long generations but should never hang a Claude session: bound the connect
// and (non-streaming) request. Streaming omits the overall timeout — a stream legitimately runs long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// 600s matches the proven ceiling of the claude-shim.js it replaces; reasoning (now on by default)
// makes non-stream replies longer, so a tighter cap would 504 legitimate generations (PLAN.md §0a #3).
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 600;

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    vllm_url: String,         // base URL, no trailing slash
    vllm_model: String,       // forced upstream model
    request_timeout: Duration, // non-streaming overall timeout (HARNESS_PROXY_TIMEOUT_SECS)
}

#[tokio::main]
async fn main() {
    // Structured logs to stderr; metadata only (§5d). RUST_LOG overrides; default info.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(io::stderr)
        .with_ansi(false)
        .init();

    // Standalone container binds 0.0.0.0:4000 (network-reachable, like the old litellm service);
    // the in-image final deploy can override to 127.0.0.1:4000.
    let bind = env::var("HARNESS_PROXY_BIND").unwrap_or_else(|_| "0.0.0.0:4000".to_string());
    // Deployment-specific — required, never baked into the binary (set via compose). Fail fast.
    let vllm_url = normalize_vllm_url(&require_env("VLLM_URL"));
    let vllm_model = require_env("VLLM_MODEL");
    let request_timeout = Duration::from_secs(
        env::var("HARNESS_PROXY_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECS),
    );

    let state = AppState {
        client: reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("build reqwest client"),
        vllm_url: vllm_url.clone(),
        vllm_model: vllm_model.clone(),
        request_timeout,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .layer(middleware::from_fn(trace_requests))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("harness-proxy: cannot bind {bind}: {e}"));
    tracing::info!(%bind, vllm_url, vllm_model, "harness-proxy listening");
    axum::serve(listener, app).await.expect("server error");
}

fn require_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("harness-proxy: {key} must be set"))
}

/// Normalize `VLLM_URL` to the server root (no trailing slash, no `/v1`). The repo's other consumer
/// (litellm) sets `VLLM_URL` *with* a `/v1` suffix; we append `/v1/chat/completions` and `/tokenize`
/// ourselves, so accept either form and avoid a `/v1/v1/...` at cutover (PLAN.md §0a #2).
fn normalize_vllm_url(raw: &str) -> String {
    raw.trim_end_matches('/').trim_end_matches("/v1").trim_end_matches('/').to_string()
}

/// Per-request access log + correlation id. Wraps each request in a span carrying a generated
/// request id so every event from the handler is correlated; echoes it back as `x-request-id`.
/// Logs metadata only (method, path, status, latency) — never bodies/headers (§5d).
async fn trace_requests(req: Request, next: Next) -> Response {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = format!("req-{:08x}", COUNTER.fetch_add(1, Ordering::Relaxed));
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let span = tracing::info_span!("req", id = %id, %method, path = %path);

    let start = Instant::now();
    let mut resp = next.run(req).instrument(span.clone()).await;
    span.in_scope(|| {
        tracing::info!(status = resp.status().as_u16(), latency_ms = start.elapsed().as_millis() as u64, "handled");
    });
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-request-id", v);
    }
    resp
}

/// POST /v1/messages — translate Anthropic request, call vLLM, translate the response back.
async fn messages(
    State(state): State<AppState>,
    req: Result<Json<MessagesRequest>, JsonRejection>,
) -> Result<Response, ProxyError> {
    let Json(req) = req.map_err(|_| ProxyError::BadRequest("invalid request body"))?;
    let streaming = req.stream.unwrap_or(false);
    let thinking = req.thinking_enabled();
    let oai = translate::to_openai(req, state.vllm_model.clone());

    if streaming {
        return stream::stream(state.client, state.vllm_url, state.vllm_model, oai, thinking).await;
    }

    let resp = state
        .client
        .post(format!("{}/v1/chat/completions", state.vllm_url))
        .timeout(state.request_timeout)
        .bearer_auth("dummy") // vLLM ignores the key; we send a placeholder (PLAN.md §1).
        .json(&oai)
        .send()
        .await
        .map_err(ProxyError::from_reqwest)?;

    let status = resp.status();
    tracing::info!(model = %state.vllm_model, upstream_status = status.as_u16(), "upstream");
    if !status.is_success() {
        return Err(ProxyError::from_upstream_status(status));
    }

    let cr = resp
        .json::<ChatResponse>()
        .await
        .map_err(|_| ProxyError::Upstream("failed to decode upstream response"))?;
    Ok(Json(translate::to_anthropic(cr, state.vllm_model, thinking)).into_response())
}

/// POST /v1/messages/count_tokens — Anthropic returns `{"input_tokens": N}`, officially an estimate.
/// We flatten the request text and ask vLLM's /tokenize; on any failure, fall back to chars/4.
async fn count_tokens(
    State(state): State<AppState>,
    req: Result<Json<MessagesRequest>, JsonRejection>,
) -> Result<Json<Value>, ProxyError> {
    let Json(req) = req.map_err(|_| ProxyError::BadRequest("invalid request body"))?;
    let text = translate::request_text(&req);
    let approx = (text.len() / 4) as u64;

    let count = match state
        .client
        .post(format!("{}/tokenize", state.vllm_url))
        .timeout(state.request_timeout)
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
        _ => approx, // tokenize unavailable -> estimate is spec-compliant
    };
    Ok(Json(json!({ "input_tokens": count })))
}

/// The single error type for the request path. One place maps cause -> HTTP status -> Anthropic
/// envelope, so Claude Code parses our failures exactly like upstream's. Messages are short and
/// non-sensitive — upstream bodies are never echoed or logged (§5d).
pub enum ProxyError {
    BadRequest(&'static str),  // 400 invalid_request_error
    Upstream(&'static str),    // 502 api_error (connect/DNS/TLS/5xx/decode)
    Timeout,                   // 504 api_error
    RateLimited,               // 429 rate_limit_error
}

impl ProxyError {
    /// reqwest transport failure -> timeout (504) vs. unavailable (502).
    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            ProxyError::Timeout
        } else {
            ProxyError::Upstream("upstream request failed")
        }
    }

    /// Non-2xx from vLLM: surface 429 as rate-limit, everything else as 502 (status only, no body).
    pub fn from_upstream_status(status: StatusCode) -> Self {
        if status == StatusCode::TOO_MANY_REQUESTS {
            ProxyError::RateLimited
        } else {
            ProxyError::Upstream("upstream returned an error")
        }
    }

    fn parts(&self) -> (StatusCode, &'static str, &'static str) {
        match self {
            ProxyError::BadRequest(m) => (StatusCode::BAD_REQUEST, "invalid_request_error", m),
            ProxyError::Upstream(m) => (StatusCode::BAD_GATEWAY, "api_error", m),
            ProxyError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "api_error", "upstream request timed out"),
            ProxyError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", "upstream rate limited"),
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, kind, message) = self.parts();
        if status.is_client_error() {
            tracing::warn!(status = status.as_u16(), kind, message, "client error");
        } else {
            tracing::error!(status = status.as_u16(), kind, message, "proxy error");
        }
        (status, Json(json!({ "type": "error", "error": { "type": kind, "message": message } }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_vllm_url;

    #[test]
    fn normalize_vllm_url_accepts_root_and_v1_forms() {
        let want = "http://10.0.0.13:8000";
        for input in ["http://10.0.0.13:8000", "http://10.0.0.13:8000/", "http://10.0.0.13:8000/v1", "http://10.0.0.13:8000/v1/"] {
            assert_eq!(normalize_vllm_url(input), want, "input: {input}");
        }
    }
}
