<!--
  Draft GitHub issue for BerriAI/litellm — fill into the "Bug Report" form at
  https://github.com/BerriAI/litellm/issues/new?template=bug_report.yml
  Each H2 below maps to a field in that template. Suggested title:

  [Bug]: Anthropic /v1/messages → OpenAI bridge drops `image` blocks nested in `tool_result`
-->

## Title

[Bug]: Anthropic `/v1/messages` → OpenAI chat/completions bridge silently drops `image` blocks nested inside `tool_result`

## Are you searching for a similar issue?

- [x] I have searched the existing issues and discussions.

Closest existing reports, none of which cover this exact case:
- #6953 / PR #6965 — fixed the **mirror** direction (OpenAI → Anthropic `convert_to_anthropic_tool_result` was text-only and dropped images). The Anthropic → OpenAI direction was never given the same treatment.
- #23841 / PR #23844 (open) — fixes `input_text` **text** blocks (incl. nested in `tool_result`) in the same adapter, but explicitly does **not** touch `image` blocks.
- #16195 — image dropped from `tool_result`, but for the Anthropic-on-Bedrock backend, not the OpenAI bridge.

## What happened?

When LiteLLM proxies an **Anthropic `/v1/messages`** request to an **OpenAI-compatible chat/completions backend** (`hosted_vllm/…` or `openai/…`), any `image` block that is nested inside a `tool_result` content block is **silently dropped** during translation. The outgoing `/v1/chat/completions` payload to the backend contains **zero** `image_url` parts, so a vision-capable backend never receives the image and the model hallucinates a description.

This is the standard shape produced by **Claude Code's `Read` tool**, which returns image files as:

```json
{ "role": "user",
  "content": [ { "type": "tool_result", "tool_use_id": "toolu_…",
                 "content": [ { "type": "image",
                                "source": { "type": "base64", "media_type": "image/png", "data": "…" } } ] } ] }
```

The same image placed **directly in a user message** (not wrapped in `tool_result`) translates correctly and the model sees it — so the bug is specific to the `tool_result` nesting. Root cause appears to be the Anthropic→OpenAI converter in
`litellm/llms/anthropic/experimental_pass_through/adapters/transformation.py` only forwarding text-type sub-blocks of a `tool_result` (the exact pattern #6953/#6965 fixed for the opposite direction, and that #23844 is extending to `input_text` — but not to `image`).

**Impact:** the Claude Code → LiteLLM → vLLM/OpenAI path cannot do image analysis at all; every image request returns a confident hallucination with no error surfaced (see also the umbrella report #30043).

## Steps to reproduce

1. **`config.yaml`** — any OpenAI-compatible, vision-capable backend (vLLM shown):

   ```yaml
   model_list:
     - model_name: claude-sonnet-4-5
       litellm_params:
         model: hosted_vllm/qwen3.6-35b     # also reproduces with openai/<model>
         api_base: http://<your-openai-compatible-server>/v1
         api_key: dummy
       model_info:
         supports_vision: true
   litellm_settings:
     drop_params: true
   ```

   ```bash
   litellm --config config.yaml --port 4000
   ```

2. **Control — image in a USER message (works).** Returns a correct description:

   ```bash
   curl -s http://localhost:4000/v1/messages \
     -H "Content-Type: application/json" -H "anthropic-version: 2023-06-01" -H "x-api-key: dummy" \
     -d '{"model":"claude-sonnet-4-5","max_tokens":300,"messages":[
           {"role":"user","content":[
             {"type":"image","source":{"type":"base64","media_type":"image/png","data":"<BASE64_PNG>"}},
             {"type":"text","text":"What do you see?"}]}]}'
   ```

3. **Bug — same image inside a `tool_result` (image dropped).** Model hallucinates because no image reaches the backend:

   ```bash
   curl -s http://localhost:4000/v1/messages \
     -H "Content-Type: application/json" -H "anthropic-version: 2023-06-01" -H "x-api-key: dummy" \
     -d '{"model":"claude-sonnet-4-5","max_tokens":300,
          "tools":[{"name":"Read","description":"Read a file","input_schema":{"type":"object","properties":{"file_path":{"type":"string"}},"required":["file_path"]}}],
          "messages":[
            {"role":"user","content":[{"type":"text","text":"Read /img.png and tell me what you see."}]},
            {"role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/img.png"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":[
              {"type":"image","source":{"type":"base64","media_type":"image/png","data":"<BASE64_PNG>"}}]}]}]}'
   ```

4. With `LITELLM_LOG=DEBUG`, inspect the translated request LiteLLM sends to the backend. For step 2 it contains an `image_url` part; for step 3 there are **0** `image_url` / image parts — the image is gone.

**Expected:** the `image` block inside `tool_result` is forwarded to the backend (e.g. hoisted into an adjacent user message, the way clients/proxies handle the OpenAI limitation that tool-role messages can't carry images), so the backend receives `image_url` in both cases.

**Actual:** step 3's outgoing payload has no image; a vision backend gets text only and hallucinates.

## Relevant log output

```shell
# DEBUG: outgoing translated payload to the backend for step 3 (truncated) — note no image parts
{
  "role": "tool",
  "tool_call_id": "toolu_01",
  "content": "[image content omitted by translation]"
}
# grep over the outgoing /chat/completions body:
#   image_url   -> 0 matches
#   input_image -> 0 matches
```

## What LiteLLM component is this bug for?

Proxy   <!-- dropdown options: SDK (Python package) · Proxy · UI Dashboard · Docs · Other -->

## What LiteLLM version are you on?

v1.89.3 (`ghcr.io/berriai/litellm:v1.89.3`)

## Twitter / LinkedIn details

_(optional — leave blank or add your handle)_

---

### Suggested fix

Mirror the existing image handling in the OpenAI→Anthropic converter (#6965) for the Anthropic→OpenAI direction in
`litellm/llms/anthropic/experimental_pass_through/adapters/transformation.py`: when a `tool_result` contains `image` sub-blocks, emit the image as an `image_url` part in an adjacent user message instead of discarding it (OpenAI tool-role messages cannot carry images). This is the same place PR #23844 adds `input_text` support; extending it to `image` would close this gap.

### Current workaround

A small reverse proxy in front of LiteLLM that lifts `image` blocks out of `tool_result` into a trailing user message (the placement proven to translate correctly) before forwarding to `/v1/messages`, streaming everything else verbatim. This fully restores image analysis through the Claude Code → LiteLLM → OpenAI/vLLM path.

Reference implementation: <!-- TODO: paste the committed permalink to claude-shim.js here before posting --> `<INSERT_SCRIPT_LINK_AFTER_COMMIT>`
