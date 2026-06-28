# harness-proxy — Implementation Plan & Handoff

Self-contained brief so a fresh Claude/LLM session can pick this up cold.
Tracks GitHub issue **#10**. Headroom (compression) is **out of scope** → issue **#11**.

Branch: `feat/harness-proxy` (was off `feat/webtty`; that branch is now merged, so this branch
has been **rebased onto `main`** — see §0a for the main-side deltas to account for at cutover).

---

## 0. Current status (where a fresh session picks up)

- ✅ **Step 1 done** (commits on `feat/harness-proxy`): Cargo crate + axum binary serving stub
  `POST /v1/messages`, `POST /v1/messages/count_tokens`, `GET /health`. Multi-stage Dockerfile
  (Ubuntu builder → static musl → `FROM scratch`) builds for the **native arch** (amd64/arm64 via
  `TARGETARCH`); image is **~860 kB**, runs non-root. Hardened compose service `harness-proxy` added.
  All three endpoints verified with `curl`.
- ✅ **Step 2 done**: non-streaming `/v1/messages` Anthropic→OpenAI translation, real vLLM call.
  `reqwest` (rustls-tls, json; default-features off) + `serde` (derive) added. New typed modules
  `src/{anthropic,openai,translate}.rs`. Request: alias map (any model → `VLLM_MODEL`), system +
  message/content-block mapping (text concatenated; image/tool_* blocks deserialize but drop —
  Steps 4/5), param strip, `Authorization: Bearer dummy`. Response: content/finish_reason/usage →
  Anthropic shape; upstream failures → 502 Anthropic-shaped error. 3 translation unit tests.
  Verified: musl→scratch `docker compose build`, container vs. a local OpenAI mock (vLLM
  `10.0.0.13:8000` unreachable from dev host). **Not yet:** streaming, image-hoist, count_tokens
  (still a stub), tool-calls.
- **Toolchain decisions made:** edition **2024** (`rust-version 1.85`), **axum 0.8**, tokio 1,
  serde_json 1, **reqwest 0.12** (rustls-tls + json, default-features off), **serde 1** (derive).
- **Files that exist:** `harness-proxy/{Cargo.toml,Dockerfile,.dockerignore,src/main.rs,
  src/anthropic.rs,src/openai.rs,src/translate.rs}`.
- ✅ **Step 3 done**: streaming SSE translation (text only). New `src/stream.rs`; `reqwest` gains the
  `stream` feature, added `tokio-stream` (ReceiverStream → axum `Sse` body). `/v1/messages` branches
  on `"stream":true`; a spawned task reads vLLM chunks (line-buffered across boundaries) and emits the
  Anthropic event order `message_start → content_block_start → content_block_delta* →
  content_block_stop → message_delta → message_stop`. Pure `Translator` state machine + 1 unit test
  (4 total). **Verified live against real vLLM** (`10.0.0.13:8000`, now reachable from dev host) and
  the musl→scratch `docker compose build` still links statically.
- **⚠️ Finding — vLLM model is a reasoning model.** `qwen3.6-35b` streams chain-of-thought in
  `delta.reasoning` (note: `reasoning`, **not** `reasoning_content`) before `delta.content`; non-stream
  puts it in `message.reasoning`. **Step 3 drops reasoning** (text only, per §5c) — a normal reply
  streams correctly, but the model is silent during its (often long) thinking phase, and a too-small
  `max_tokens` can exhaust the budget on reasoning alone (empty `content`, `stop_reason:max_tokens`).
  **Now handled conditionally (Step 4):** when the request enables `thinking`, reasoning is surfaced as
  Anthropic `thinking` blocks (non-stream + streaming `thinking_delta` + a synthetic `signature_delta`,
  since vLLM gives no signature and we are the server, so nothing re-validates it); when thinking is
  off (this sandbox: `MAX_THINKING_TOKENS=0`), reasoning is dropped, matching real Anthropic behavior.
- ✅ **Steps 4 & 5 done** (blocker resolved — see below). Expanded the wire types (Anthropic
  image/tool_use/tool_result/thinking blocks, tools, tool_choice; OpenAI multimodal parts, tool_calls,
  `tool` role, tool defs, streaming tool deltas). `translate.rs`: multimodal messages, **image hoist**
  (tool_result images → trailing user message, ported from `claude-shim.js`), tool_use→tool_calls,
  tool_result→`tool` role, tools/tool_choice map, param strip. Response: text + tool_use blocks, plus
  a `thinking` block **only when the client enabled thinking**. `stream.rs`: tool_use streaming
  (`input_json_delta`) + conditional reasoning→thinking, one monotonic block index. `count_tokens`
  now calls vLLM `/tokenize` (chars/4 fallback). **8 unit tests.** Verified live against real vLLM and
  via the **real `claude` CLI**: streaming chat, non-stream + streaming tool calls, a Read-tool task
  (returned the file's secret), `count_tokens`, and the **image-in-`tool_result` hoist** — the
  vision model answered "Red" both by curl and through `claude -p` reading a PNG. Scratch image still
  builds.
- **✅ Blocker resolved (was §2): vLLM emits standard OpenAI `tool_calls`** (`finish_reason:tool_calls`,
  `message.tool_calls[].function.{name,arguments}`) — the `hermes` path the PLAN predicted, confirmed
  empirically. No text-template parsing needed.
- ✅ **Step 6 done**: production logging & error handling (§5d). Added `tracing` +
  `tracing-subscriber` (stderr, `RUST_LOG` default info). A `from_fn` middleware wraps each request
  in a span with a generated **`req-XXXXXXXX` id**, echoes it as **`x-request-id`**, and logs one
  `handled` line (method, path, status, latency_ms) — **metadata only, never bodies/prompts/auth**.
  One **`ProxyError`** enum (`IntoResponse`) is the single map cause→status→Anthropic envelope:
  400 invalid_request_error (malformed body), 502 api_error (connect/DNS/TLS/5xx/decode), 504
  api_error (timeout), 429 rate_limit_error — messages short and non-sensitive (upstream bodies never
  echoed/logged). reqwest client has a **connect timeout (10s)** + tunable overall timeout
  (`HARNESS_PROXY_TIMEOUT_SECS`, default 600 — matches the shim, §0a #3; streaming omits the overall
  one). **Verified live:**
  malformed→400, dead upstream→502, hung upstream→504 (~2s with the cap), happy path 200 — all with a
  correlated request id and no content in the logs.
- **Wiring:** proxy is NOT yet in Claude's path. Sandbox still uses litellm + `claude-shim.js`.
  Compose runs the proxy standalone (`agentic-harness-proxy`, bind `0.0.0.0:4000`, no published
  ports) so it doesn't interfere. Cutover is Step 7.
- **➡️ Next: Step 7 (final)** — cutover: remove litellm + claude-shim, point `ANTHROPIC_BASE_URL` at
  the proxy, wire it into `entrypoint.sh`/`compose.yml`. See §0a for the main-side deltas first.

---

## 0a. Rebased onto `main` — deltas to account for at cutover (Step 7)

This branch was cut off `feat/webtty`; that branch is now merged and `main` moved on (searxng/valkey
MCP stack, webtty fixes, shim hardening). Rebased clean (only `compose.yml` conflicted — both sides
just add services). **Concept still holds — vLLM re-verified live (2026-06-28):**

| Re-verified against `10.0.0.13:8000` | Result |
|---|---|
| reasoning field, non-stream | `message.reasoning` (✅ matches `openai.rs` — **not** `reasoning_content`) |
| reasoning field, stream | `delta.reasoning` (✅ matches `stream.rs`) |
| tool calls | native OpenAI `tool_calls`, `finish_reason:tool_calls` (✅ 1:1, no text parsing) |

**What `main` changed that the cutover must honor:**
1. **Thinking is now ON end-to-end.** `config/claude/settings.json` flipped `MAX_THINKING_TOKENS`
   0→**8000** and `alwaysThinkingEnabled` false→**true**. So Claude now *requests* thinking and expects
   `thinking` blocks back. Our proxy already surfaces `delta.reasoning`→`thinking` when the client
   enables it (Step 4) — so this works — but it makes that path **load-bearing**, not optional. §5a's
   "param strip drops `thinking`" means: drop it from the *upstream* OpenAI body (vLLM rejects the
   Anthropic object; thinking is on server-side via `--default-chat-template-kwargs enable_thinking`),
   **not** ignore it — we still read it to gate reasoning surfacing.
2. **`VLLM_URL` convention diverges.** `main`'s litellm + entrypoint preflight use `VLLM_URL` **with**
   a `/v1` suffix (`http://host:8000/v1`); our proxy treats `VLLM_URL` as the server **root** and
   appends `/v1/chat/completions` + `/tokenize` itself. To let one env value serve both during
   coexistence and avoid `/v1/v1/...` at cutover, the proxy now **normalizes** a trailing `/v1` away
   (see `normalize_vllm_url` in `main.rs`).
3. **Inherit the hardened shim's timeout.** `main`'s `claude-shim.js` gained a **600 s** upstream
   timeout + a `headersSent` guard (don't re-write status mid-stream). Our streaming path already
   omits the overall timeout and ends cleanly mid-stream (covers the guard); the non-stream default
   was raised 180→**600 s** to match the shim's proven ceiling now that reasoning makes replies longer.
4. **New searxng MCP** (`workspace/.mcp.json`, `config/omp/mcp.json`) and `WebSearch`/`WebFetch` now
   `deny`. This rides the MCP path, not `/v1/messages`, so the proxy is untouched — but it means *more*
   tool traffic, raising the stakes on tool-call streaming (already in scope, already verified).

---

## 1. What we're building and why

Replace the **LiteLLM Python sidecar** + the **`claude-shim.js` Node proxy** with one
statically-linked Rust binary, `harness-proxy`.

It is an **API translator**: Claude Code speaks the Anthropic Messages API; our backend is a
**vLLM** server speaking the OpenAI Chat Completions API. The proxy translates between them,
including streaming, and hoists images out of `tool_result` blocks (which LiteLLM drops).

Target: a ~15 MB `FROM scratch` image, no Python/Node in the request path.

### Concrete environment (current, do not guess)

| Thing | Value |
|---|---|
| vLLM base URL | `http://10.0.0.13:8000` (OpenAI API at `/v1`) — make it env `VLLM_URL` |
| vLLM model name | `qwen3.6-35b` — env `VLLM_MODEL` |
| Vision capable | yes (`supports_vision: true`) |
| Anthropic aliases Claude sends | `claude-sonnet-4-5`, `claude-haiku-4-5` (set via `ANTHROPIC_DEFAULT_*_MODEL` in `config/claude/settings.json`) — both map to the one vLLM model |
| Claude points at | `ANTHROPIC_BASE_URL` (today `http://127.0.0.1:4001`) |
| API key | `dummy` (header `x-api-key` / `Authorization: Bearer dummy`) |

The proxy should **listen on `127.0.0.1:4000`** (replacing both the old shim:4001 and the
litellm:4000). Final step re-points `ANTHROPIC_BASE_URL` → `http://127.0.0.1:4000`.

---

## 2. ✅ RESOLVED blocker (was: gates tool-call work)

**Confirmed empirically (2026-06-26):** the live vLLM at `10.0.0.13:8000` returns **standard OpenAI
`tool_calls`** with `finish_reason:tool_calls` for a normal tools request — the `hermes`/auto-tool-choice
path below. The proxy's 1:1 tool translation works (verified non-stream + streaming + via `claude` CLI).
No text-template parsing needed; the jammsen question is moot. Original note kept for context:

**How is our vLLM launched re: tool calls?** Asked on issue #10, waiting on @jammsen.
- vLLM needs `--enable-auto-tool-choice --tool-call-parser <name>` to emit tool calls. Per the vLLM
  docs, **Qwen2.5's chat template already ships Hermes-style tool support, so the parser is `hermes`**
  (Qwen3-Coder would use `qwen3_xml`). With a parser enabled, vLLM returns **standard OpenAI
  `tool_calls`** (`choices[].message.tool_calls[].function.{name,arguments}`) → simple 1:1 translation.
  **Build for this case by default** (model is `qwen3.6-35b`, almost certainly `hermes`).
- If auto-tool-choice / a parser is **not** set → vLLM emits tool calls as plain text in a template;
  the proxy would then have to parse that text itself (much more work, brittle). Only handle this if
  jammsen confirms it's the case — easiest fix is to enable the flags on the vLLM side instead.

Everything except Step 5 below is independent of this answer. Do not wait — build Steps 1–4.

---

## 3. Stack & build

- Rust **edition 2024** (`rust-version 1.85`). **axum 0.8** + **tokio 1**, **serde_json 1**. Step 2
  adds **reqwest with `rustls-tls`** (default features off, `rustls-tls` on) + **serde** derive.
  Upstream vLLM call is plain `http`, so no outbound TLS is needed in practice, but rustls (not
  OpenSSL) keeps the binary fully static.
- Build the binary **statically** for the **native** arch — do NOT hardcode `x86_64`. The Dockerfile
  maps BuildKit's `TARGETARCH` → musl triple (`amd64`→`x86_64-unknown-linux-musl`,
  `arm64`→`aarch64-unknown-linux-musl`) and builds native. (Hardcoding x86_64 broke the arm64 dev
  build with a `cc -m64` cross-link error.) Static musl is what makes `FROM scratch` work.
- Single crate, single binary. **Do not** build a multi-crate workspace — YAGNI.

### Image strategy — **no Alpine**. Ubuntu builder → `FROM scratch` target (DONE, see `harness-proxy/Dockerfile`)

Two stages, both consistent with the repo (which already pins `ubuntu:26.04` by digest):

1. **Builder stage = Ubuntu, NOT `rust:*-alpine`.** Compile on `ubuntu:26.04` (same pinned digest as
   the repo): `apt-get install build-essential ca-certificates curl musl-tools`, rustup minimal,
   `rustup target add <native musl triple>`, `cargo build --release --target <triple>`, then copy the
   binary to a fixed `/harness-proxy` path. The Alpine builder is explicitly rejected.
2. **Final stage = `FROM scratch`** with only the static binary, `USER 65532:65532`, `EXPOSE 4000`.
   Static musl + plain http means scratch needs **nothing else** — no libc, no ca-certificates.

**Fallback (only if a fully-static musl build proves impractical**, e.g. a transitive crate that won't
build on musl): instead of `FROM scratch`, ship a **chiselled Ubuntu rootfs** built with **Chisel**
(`chisel cut --release ubuntu-26.04 --root /rootfs libc6_libs ca-certificates_data …`) as the final
stage and copy a glibc-dynamic binary into it. This keeps us on minimal Ubuntu slices instead of a full
base image — still tiny, still no Alpine. See the Chisel "use in a Dockerfile" how-to in References.
Default to scratch+musl; treat chisel as the documented escape hatch.

Suggested layout (keep it flat):
```
harness-proxy/
  PLAN.md          # this file
  Cargo.toml       # EXISTS (edition 2024, axum 0.8)
  Dockerfile       # EXISTS — multi-stage: ubuntu builder (native musl) -> FROM scratch
  .dockerignore    # EXISTS
  src/
    main.rs        # EXISTS — axum router, HARNESS_PROXY_BIND env, stub handlers
    anthropic.rs   # TODO (Step 2) — Anthropic request/response/SSE types (serde)
    openai.rs      # TODO (Step 2) — OpenAI request/response/chunk types (serde)
    translate.rs   # TODO (Step 2) — request: Anthropic->OpenAI (+image hoist, param strip, alias map)
                   #                  response: OpenAI->Anthropic (non-stream + streaming SSE)
```
Split further only if a file gets unwieldy. Note: the binary binds `HARNESS_PROXY_BIND`
(default `0.0.0.0:4000` for the standalone container); the in-image final deploy can set
`127.0.0.1:4000`.

---

## 4. Endpoints to implement

Claude Code calls these against `ANTHROPIC_BASE_URL`:
1. `POST /v1/messages` — non-streaming **and** streaming (`"stream": true`). The bulk of the work.
2. `POST /v1/messages/count_tokens` — accepts the same body shape as `/v1/messages` and returns
   exactly `{"input_tokens": N}` (verified, Anthropic docs). Anthropic itself states the count is an
   **estimate**, so an approximation is spec-compliant. Strategy: call vLLM's `/tokenize` endpoint
   (vLLM exposes one) for accuracy, else approximate (e.g. chars/4). Don't over-engineer; a rough
   count unblocks the client.

Headers: pass through, force the upstream model to `VLLM_MODEL`, send `Authorization: Bearer dummy`.

---

## 5. Translation details (the actual work)

### 5a. Request: Anthropic `/v1/messages` → OpenAI `/v1/chat/completions`
- **Alias map:** any incoming `model` → `VLLM_MODEL`.
- **System prompt:** Anthropic `system` (string or blocks) → OpenAI `{"role":"system"}` message.
- **Messages:** map roles; Anthropic content blocks → OpenAI parts:
  - `text` → `{"type":"text"}`
  - `image` (`source.type=base64`) → `{"type":"image_url","image_url":{"url":"data:<media_type>;base64,<data>"}}`
  - `tool_use` (assistant) → OpenAI `tool_calls` entry (`id`, `function.name`, `function.arguments` = JSON string)
  - `tool_result` (user) → OpenAI `{"role":"tool","tool_call_id":...,"content": <text>}`
- **🔑 Image hoist** (replaces `claude-shim.js`): OpenAI/vLLM **cannot** carry images in a
  `role:"tool"` message. So when a `tool_result` contains image sub-blocks:
  - keep the tool message **text-only** (use placeholder `"[image returned by tool — see following message]"` if no text),
  - append a **new `role:"user"`** message right after with `[{type:text,"Image(s) returned by the tool call above:"}, <the image_url parts>]`.
  - Only do this when an image is actually present (text-only tool_results stay byte-identical).
  - Reference logic: `scripts/claude-shim.js` (`hoistToolResultImages`) and
    `ideas/litellm-issue-tool_result-image-drop.md` (full root-cause writeup + suggested upstream fix).
- **Param strip** (replaces LiteLLM `drop_params: true`): drop fields vLLM rejects from the *upstream*
  body — `thinking`, `reasoning_effort`, and any Anthropic-only knobs. **Note:** `thinking` is still
  *read* (to gate reasoning→`thinking`-block surfacing, §0a #1) — it's stripped from what we send to
  vLLM, not ignored. Map `max_tokens`, `temperature`, `top_p`,
  `stop_sequences`→`stop`, `stream`. Map `tools` (Anthropic `input_schema` → OpenAI
  `function.parameters`) and `tool_choice`.

### 5b. Response (non-streaming): OpenAI → Anthropic
- `choices[0].message.content` → Anthropic `content: [{type:text}]`.
- `choices[0].message.tool_calls` → Anthropic `tool_use` blocks (`input` = parsed JSON of `arguments`).
- `finish_reason` → `stop_reason`: `stop`→`end_turn`, `length`→`max_tokens`, `tool_calls`→`tool_use`.
- `usage.prompt_tokens`/`completion_tokens` → `usage.input_tokens`/`output_tokens`.

### 5c. Response (streaming): OpenAI `chat.completion.chunk` SSE → Anthropic Messages SSE
This is ~80% of the effort and the main correctness risk. vLLM sends `data: {chunk}` lines ending
with `data: [DONE]`. The proxy must emit the Anthropic event sequence (each as
`event: <type>\ndata: <json>\n\n`):
1. `message_start` (with `message` skeleton: role assistant, empty content, model, usage stub)
2. For the text content block: `content_block_start` (index 0, `{type:text,text:""}`), then one
   `content_block_delta` per chunk (`{type:text_delta, text:<delta>}`), then `content_block_stop`.
3. For **tool calls**: each `tool_calls` delta → a content block of `{type:tool_use,...}`;
   stream the function arguments as `content_block_delta` with `{type:input_json_delta, partial_json:<delta>}`.
   (This is the part gated by the vLLM tool-call format — Step 5 / blocker above.)
4. `message_delta` with `stop_reason` + final `usage`, then `message_stop`.
5. Send periodic `ping` events if needed; Claude tolerates them.

Keep an index counter for content blocks. Tool-call argument deltas arrive incrementally — buffer
per `tool_calls[].index`.

### 5d. Error handling & logging contract (production)

Step 2 ships a deliberately minimal version (one `anthropic_error` helper, upstream failures → 502,
`eprintln!` for startup). Before cutover (§6 step 6) harden it to this contract. Goal: an operator can
debug a failure from the logs **without** any prompt content, secret, or token ever being written.

**Logging — structured, on stderr, never bodies.**
- Use `tracing` + `tracing-subscriber` (the axum-native stack). Level from `RUST_LOG` (default `info`).
  Emit to **stderr** so it interleaves with the container's other startup logs; one event per request.
- Per request log **metadata only**: a generated request id, method, path, chosen `model`, response
  status, upstream status, and latency (ms). Add the request id to a response header (`x-request-id`)
  for correlation.
- **Never log**: the request or response body, `system`/message text, image data, the `Authorization`
  header, or any token counts that could leak content. (Prompts routinely contain secrets/PII —
  treat them as such.) This is also a §8 don't.
- No `unwrap()`/`panic!` on the request path — a panic both crashes the worker and can dump state.
  `panic = "abort"` is set, so a stray panic kills the process; keep them out of handlers.

**Error codes — correct status, clean propagation.**
- Replace the ad-hoc tuple returns with **one `ProxyError` enum implementing `IntoResponse`**, so
  handlers are `Result<Json<…>, ProxyError>` and there is exactly one place that maps error → status
  → Anthropic envelope. Each arm logs at the right level (client errors `warn`, upstream/proxy `error`).
- Map to the status the client should actually see, and to Anthropic's `error.type`
  (`invalid_request_error` / `authentication_error` / `not_found_error` / `rate_limit_error` /
  `overloaded_error` / `api_error`):

  | Cause | HTTP | Anthropic `error.type` |
  |---|---|---|
  | Malformed/invalid client JSON (axum rejection) | `400` | `invalid_request_error` |
  | Upstream connection refused / DNS / TLS failure | `502` | `api_error` |
  | Upstream timed out (see below) | `504` | `api_error` |
  | Upstream returned 429 | `429` | `rate_limit_error` |
  | Upstream returned 5xx | `502` | `api_error` |
  | Upstream body failed to decode | `502` | `api_error` |

  Always return the Anthropic envelope `{"type":"error","error":{"type":…,"message":…}}` so Claude
  Code parses our failures exactly like upstream's; never leak a raw axum plaintext 422 or a Rust
  error string verbatim — keep `message` short and non-sensitive.
- Set explicit timeouts on the `reqwest::Client` (connect + overall request) so a hung vLLM surfaces
  as a clean `504` instead of hanging the Claude session. (Streaming, Step 3, needs a longer/disabled
  overall timeout — handle that when streaming lands.)

---

## 6. Step-by-step (each step independently testable)

1. ✅ **DONE — Scaffold + Dockerfile.** Cargo bin, axum, `HARNESS_PROXY_BIND` env, stub
   `POST /v1/messages` + `count_tokens` + `/health`. Multi-stage Ubuntu→scratch (native arch) builds,
   runs non-root, ~860 kB. Verified via `curl`. Compose service `harness-proxy` added (hardened).
2. ✅ **DONE — Non-streaming translation** against real vLLM (text only). Added `reqwest`(rustls)+`serde`;
   created `anthropic.rs`/`openai.rs`/`translate.rs`; replaced the `/v1/messages` stub with a real
   Anthropic→OpenAI request, POST to `${VLLM_URL}/v1/chat/completions`, response translated back.
   `VLLM_URL`/`VLLM_MODEL` are required env (no hardcoded defaults). Verified vs. a local OpenAI mock
   (vLLM `10.0.0.13:8000` unreachable from dev host); re-verify in compose against real vLLM.
3. ✅ **DONE — Streaming SSE** (text only). `src/stream.rs` + `tokio-stream`; vLLM chunk SSE →
   Anthropic Messages events. Verified live against real vLLM and via a `Translator` unit test
   (event order + reasoning-drop + usage). vLLM reasoning (`delta.reasoning`) is dropped — see the
   reasoning-model finding in §0.
4. ✅ **DONE — Image hoist + param strip + alias map + `count_tokens`.** Read-an-image flow works
   end-to-end (the thing LiteLLM broke): `claude -p` Read of a PNG → vision model answered "Red".
5. ✅ **DONE (blocker resolved)** — Tool-call translation, non-stream + streaming. A `claude -p`
   Read-tool task ran the full agentic loop through the proxy and returned the file's secret.
6. ✅ **DONE — Production logging & error handling** (§5d). `tracing`→stderr, request-id span +
   `x-request-id`, metadata-only access log; one `ProxyError` enum → correct status + Anthropic
   envelope; connect + tunable request timeouts. Verified live: malformed→400, dead upstream→502,
   hung upstream→504, with a request id + status + latency and **no message content** in logs.
7. **(GATED — do last, after proxy proven)** Remove the old stack (account for §0a deltas):
   - `compose.yml`: delete the `litellm` service + `LITELLM_UPSTREAM` env; add proxy build/run.
   - delete `config/litellm-config.yaml`, `scripts/claude-shim.js`.
   - `entrypoint.sh`: drop the `claude-shim` supervisor block; start `harness-proxy` instead.
     Keep the `VLLM_URL` preflight — the proxy now accepts it **with or without** `/v1` (§0a #2).
   - `config/claude/settings.json`: `ANTHROPIC_BASE_URL` → `http://127.0.0.1:4000`. Leave
     `MAX_THINKING_TOKENS=8000` / `alwaysThinkingEnabled=true` as-is — the proxy handles thinking (§0a #1).
   - Leave `scripts/upload-server.js` and `scripts/analyze-image.js` **untouched** (out of scope).

Until Step 6, run the proxy **alongside** LiteLLM (different port) so nothing breaks during dev.

---

## 7. How to test (in-container)

- Build & run the sandbox (see top-level `README.md` / `compose.yml`).
- Control vs. broken-case curls for the image path are in
  `ideas/litellm-issue-tool_result-image-drop.md` (steps 2 vs 3) — reuse them against the proxy.
- Real integration test: open WeTTY (`https://<host>:1111`), launch Claude Code, run a normal
  prompt (streaming), then `Read` an image in `workspace/uploads/` (image hoist), then a
  tool-using task (after Step 5).
- Leave one runnable check behind for the translation logic (the non-trivial part): a small Rust
  unit test asserting (a) a `tool_result`-with-image request hoists the image into a trailing user
  message, and (b) an OpenAI chunk stream maps to the expected Anthropic event order.

---

## 8. Don'ts (scope guard)

- ❌ No Headroom / compression here → #11.
- ❌ Don't port `upload-server.js` or `analyze-image.js` (Node stdlib, not the bloat).
- ❌ No multi-crate workspace, no plugin abstraction, no config file — env vars only.
- ❌ Don't delete LiteLLM until the proxy is proven (Step 6 is last).
- ❌ Don't oversell "<15 MB RAM" — realistic is 15–40 MB with tokio + stream buffers.
- ❌ **Never log request/response bodies, prompt or `system` text, image data, the `Authorization`
  header, or token counts** — metadata only (§5d). Prompts carry secrets/PII.

---

## 9. References (authoritative — build against these, not assumptions)

**Anthropic Messages API (what Claude Code sends/expects)**
- Messages API reference: https://platform.claude.com/docs/en/api/messages
- **Streaming SSE** (canonical event order + delta shapes — the core of §5c):
  https://platform.claude.com/docs/en/docs/build-with-claude/streaming
  - Verified sequence: `message_start` → (per content block) `content_block_start` →
    `content_block_delta` → `content_block_stop` → `message_delta` → `message_stop`, with `ping`
    interspersed. Text deltas use `{"type":"text_delta","text":"…"}`; **tool_use** argument deltas
    use `{"type":"input_json_delta","partial_json":"…"}` (accumulate to get the full JSON args).
- **count_tokens** endpoint: https://platform.claude.com/docs/en/docs/build-with-claude/token-counting
  and ref https://platform.claude.com/docs/en/api/messages-count-tokens
  - Verified: same input body as `/v1/messages`, response is `{"input_tokens": N}`, officially an estimate.
- Tool use overview (Anthropic side of `tool_use`/`tool_result`):
  https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview

**vLLM (the OpenAI-compatible backend we translate to)**
- OpenAI-compatible server: https://docs.vllm.ai/en/latest/serving/openai_compatible_server.html
- **Tool calling** (flags + parsers): https://docs.vllm.ai/en/latest/features/tool_calling.html
  - Verified: `--enable-auto-tool-choice` + `--tool-call-parser` required; **Qwen2.5 → `hermes`**;
    response carries standard OpenAI `tool_calls` (`message.tool_calls[].function.{name,arguments}`).

**OpenAI Chat Completions (the wire format vLLM speaks)**
- Create chat completion + streaming chunk object (`chat.completion.chunk`, `choices[].delta`,
  `finish_reason`, `usage`): https://platform.openai.com/docs/api-reference/chat

**Build / packaging**
- Chisel (minimal Ubuntu rootfs; the fallback to scratch): https://ubuntu.com/chisel/docs/latest/
  - `chisel cut` CLI + slices reference, and "Use Chisel in a Dockerfile" how-to (multi-stage builds).
  - chisel-releases (slice definitions, e.g. `libc6_libs`, `ca-certificates_data`):
    https://github.com/canonical/chisel-releases
- Rust musl static target: https://doc.rust-lang.org/rustc/platform-support.html (x86_64-unknown-linux-musl)
- reqwest rustls feature: https://docs.rs/reqwest (enable `rustls-tls`, disable default `native-tls`)
- axum: https://docs.rs/axum • tokio: https://docs.rs/tokio

**In-repo references (existing behavior to preserve)**
- `scripts/claude-shim.js` — `hoistToolResultImages()` is the exact image-hoist logic to port (§5a).
- `ideas/litellm-issue-tool_result-image-drop.md` — full root-cause writeup of the `tool_result`
  image-drop bug + reproduction curls (reuse for testing) + the byte-identical-when-no-image rule.
- `config/litellm-config.yaml` — the model aliases + `drop_params: true` behavior we're replacing.
- `config/claude/settings.json` — `ANTHROPIC_BASE_URL` and `ANTHROPIC_DEFAULT_*_MODEL` wiring.
- `Dockerfile` (top-level) — pins `ubuntu:26.04`; keep the builder on the same Ubuntu lineage.
- GitHub: issue **#10** (this work), issue **#11** (Headroom, out of scope).
