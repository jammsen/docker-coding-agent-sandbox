# harness-proxy

A tiny Rust proxy that lets **Claude Code** talk to a local **vLLM** (or any
OpenAI-compatible) inference server. It translates the Anthropic Messages API
(`/v1/messages`) into OpenAI Chat Completions (`/v1/chat/completions`) and back.

It is the in-house replacement for the **LiteLLM sidecar + `claude-shim.js`** pair
described in the [root README](../README.md) (see *Included Software*). Tracks
GitHub issue **#10**. The full design, rationale and roadmap live in
[`PLAN.md`](./PLAN.md) — this file is the short "what / why / how" for someone
landing in this directory.

---

## Why this exists

The sandbox runs agentic coding tools against *your own* vLLM model — no cloud
API keys (see the root README). Claude Code only speaks the **Anthropic** wire
format; vLLM only speaks the **OpenAI** one. Something has to translate between
them. Today that job is split across two moving parts:

- **LiteLLM** (a Python service) does the Anthropic↔OpenAI translation, and
- **`claude-shim.js`** (a Node sidecar) patches around a LiteLLM bug that drops
  images out of `tool_result` blocks.

That's two languages, two processes, a large dependency surface, and a supply-chain
footprint (LiteLLM had compromised PyPI releases — see the version note in
`compose.yml`). `harness-proxy` collapses both into **one ~1 MB static binary**
with no runtime dependencies, doing the translation — including the image fix —
directly.

## How it works

```
Claude Code ──Anthropic /v1/messages──▶ harness-proxy ──OpenAI /v1/chat/completions──▶ vLLM
            ◀──Anthropic response──────              ◀──OpenAI response───────────────
```

- **Request:** force the upstream model (alias map — any incoming model →
  `VLLM_MODEL`), turn the Anthropic `system` + message content blocks into OpenAI
  messages, strip Anthropic-only params.
- **Response:** map `choices[].message` → Anthropic `content`, `finish_reason` →
  `stop_reason`, and `usage` token counts back to Anthropic's names.

Translation lives in `src/translate.rs`; the wire types are in `src/anthropic.rs`
and `src/openai.rs`; `src/main.rs` is the axum server. See [`PLAN.md` §5](./PLAN.md)
for the field-by-field mapping.

## Status

Built incrementally (roadmap in [`PLAN.md` §6](./PLAN.md)):

| | Capability | State |
|---|---|---|
| 1 | Scaffold, Dockerfile, `/health` | ✅ done |
| 2 | Non-streaming `/v1/messages` translation (text) | ✅ done |
| 3 | Streaming SSE | ✅ done |
| 4 | Image hoist, param strip, `count_tokens` | ✅ done |
| 5 | Tool-call translation | ✅ done |
| 6 | Production logging & error handling | ✅ done |
| 7 | Cut over: remove LiteLLM + `claude-shim.js` | ⏳ planned (gated — do last) |

Until the cutover the proxy runs **alongside** LiteLLM on a separate port, so nothing
in the existing sandbox breaks while it's being proven.

## Configuration

All config is via environment variables — no config file (see `compose.yml`,
service `harness-proxy`):

| Env | Required | Meaning |
|---|---|---|
| `VLLM_URL` | **yes** | Base URL of the OpenAI-compatible server, e.g. `http://10.0.0.13:8000` |
| `VLLM_MODEL` | **yes** | Upstream model id every request is forced to, e.g. `qwen3.6-35b` |
| `HARNESS_PROXY_BIND` | no (default `0.0.0.0:4000`) | Listen address |

`VLLM_URL` / `VLLM_MODEL` are deployment-specific and **not** baked into the
binary — the process refuses to start if either is missing.

## Build & run

The image is a multi-stage build: an Ubuntu builder produces a fully static
musl binary, copied into a `FROM scratch` final stage (no libc, runs as a
non-root numeric UID). See `Dockerfile` and [`PLAN.md` §3](./PLAN.md).

```bash
# from the repo root
docker compose build harness-proxy
docker compose up -d harness-proxy
curl -s localhost:4000/health   # -> ok   (only if you publish the port; see compose.yml)
```

## Develop & test

A local Rust toolchain (edition 2024 / Rust ≥ 1.85) is enough for the unit tests:

```bash
cd harness-proxy
cargo test            # translation unit tests
cargo clippy --all-targets -- -D warnings
```

vLLM lives on the LAN and usually isn't reachable from a dev laptop. To exercise
the live HTTP path without it, point `VLLM_URL` at a small local OpenAI mock that
returns a canned `chat.completion`, then run the container against it. The real
end-to-end test is in-container against vLLM — see [`PLAN.md` §7](./PLAN.md).
