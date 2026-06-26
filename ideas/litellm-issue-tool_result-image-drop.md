<!--
  Draft GitHub issue for BerriAI/litellm — fill into the "Bug Report" form at
  https://github.com/BerriAI/litellm/issues/new?template=bug_report.yml
  Each H2 below maps to a field in that template. Suggested title:

  [Bug]: Anthropic /v1/messages → OpenAI chat/completions drops images nested in tool_result
-->

## Title

[Bug]: Anthropic `/v1/messages` → OpenAI chat/completions drops images nested in `tool_result`

## Are you searching for a similar issue?

- [x] I have searched the existing issues and discussions.

The closest existing reports don't cover this exact case:
- #6953 / #6965 fixed the opposite direction (OpenAI → Anthropic: `convert_to_anthropic_tool_result` was text-only and dropped images). The Anthropic → OpenAI side never got the same handling.
- #23841 / #23844 (open) add `input_text` text blocks to this adapter, including inside `tool_result`, but don't touch `image` blocks.
- #16195 is a similar image drop from `tool_result`, but for the Anthropic-on-Bedrock backend rather than the OpenAI bridge.

## What happened?

If you proxy an Anthropic `/v1/messages` request to an OpenAI-compatible backend (`hosted_vllm/…` or `openai/…`) and an image is nested inside a `tool_result` block, the image gets dropped in translation. The `/v1/chat/completions` payload that reaches the backend has no `image_url`, so a vision model never sees the image and just makes something up.

This is exactly the shape Claude Code's `Read` tool produces. It returns images as:

```json
{ "role": "user",
  "content": [ { "type": "tool_result", "tool_use_id": "toolu_…",
                 "content": [ { "type": "image",
                                "source": { "type": "base64", "media_type": "image/png", "data": "…" } } ] } ] }
```

The same image in a plain user message works fine and the model sees it, so the problem is specific to the `tool_result` nesting. It looks like the converter in `litellm/llms/anthropic/experimental_pass_through/adapters/transformation.py` only forwards text sub-blocks of a `tool_result` — the same gap #6965 fixed for the other direction and #23844 is extending to `input_text`.

This still happens on the latest `main`, not just the stable tag. On `main-latest` (digest `sha256:f792e404f0db3b8c8d841e25e8c7b373df6079b74d660068a6d4974345c8d43d`, pulled 2026-06-25) the adapter does translate the image into an `image_url`, but it puts it inside the `role:"tool"` message. OpenAI and vLLM don't accept images in tool-role messages, so it gets stripped before the request leaves LiteLLM. To survive, the `image_url` has to go in a `role:"user"` message instead. I confirmed this against a vision-capable vLLM: the `tool_result` request hallucinates on both `v1.89.3` and `main-latest`, while the same image in (or hoisted into) a user message is described correctly.

End result: the Claude Code → LiteLLM → vLLM/OpenAI path can't do image analysis at all. Every image request comes back as a confident hallucination with no error surfaced (related: #30043).

## Steps to reproduce

1. `config.yaml` for any OpenAI-compatible, vision-capable backend (vLLM here):

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

2. Control: image in a user message. Works, returns a correct description.

   ```bash
   curl -s http://localhost:4000/v1/messages \
     -H "Content-Type: application/json" -H "anthropic-version: 2023-06-01" -H "x-api-key: dummy" \
     -d '{"model":"claude-sonnet-4-5","max_tokens":300,"messages":[
           {"role":"user","content":[
             {"type":"image","source":{"type":"base64","media_type":"image/png","data":"<BASE64_PNG>"}},
             {"type":"text","text":"What do you see?"}]}]}'
   ```

3. Same image inside a `tool_result`. Image is dropped, model hallucinates.

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

4. With `LITELLM_LOG=DEBUG`, look at what LiteLLM sends the backend. Step 2 includes an `image_url`; step 3 has none.

5. Move the image out of the `tool_result` and into a trailing user message (`{"role":"user","content":[{"type":"text","text":"Image returned by the tool:"},{"type":"image","source":{…}}]}`), leaving a text placeholder in the `tool_result`. Now it works. Confirmed on `v1.89.3` and `main-latest`.

Expected: the image inside `tool_result` reaches the backend (for example hoisted into an adjacent user message, since tool-role messages can't carry images), so both cases send an `image_url`.

Actual: step 3 sends no image. On `main-latest` the adapter builds an `image_url` but puts it in the `role:"tool"` message, and it's gone by the time the request reaches the backend.

## Relevant log output

```shell
# DEBUG: outgoing payload to the backend for step 3 (truncated) — no image parts
{
  "role": "tool",
  "tool_call_id": "toolu_01",
  "content": "[image content omitted by translation]"
}
# grep over the outgoing /chat/completions body that reaches the backend:
#   image_url   -> 0 matches   (v1.89.3 and main-latest)
#   input_image -> 0 matches
# On main-latest the adapter builds an image_url inside the role:"tool" message,
# but it's stripped before the request hits the backend (tool messages can't carry images).
```

## What LiteLLM component is this bug for?

Proxy   <!-- dropdown options: SDK (Python package) · Proxy · UI Dashboard · Docs · Other -->

## What LiteLLM version are you on?

v1.89.3 (`ghcr.io/berriai/litellm:v1.89.3`). Also reproduced on `main-latest` (`ghcr.io/berriai/litellm@sha256:f792e404f0db3b8c8d841e25e8c7b373df6079b74d660068a6d4974345c8d43d`, pulled 2026-06-25).

## Twitter / LinkedIn details

_(optional — leave blank or add your handle)_

---

### Suggested fix

In the `elif content.get("type") == "tool_result":` branch of `translate_anthropic_messages_to_openai()` (`litellm/llms/anthropic/experimental_pass_through/adapters/transformation.py`): when a `tool_result`'s content list has image sub-blocks, don't keep the `image_url` in the `role:"tool"` message (that's what `main` does now, and it gets dropped at the backend). Keep the tool message text-only and put the image in a `role:"user"` message right after it. Same idea as the #6965 fix for the other direction, same branch #23844 touches for `input_text`.

To avoid regressions, only do this when an image is actually present, so text-only `tool_result`s stay byte-for-byte identical. Keep the `tool_use` → `tool` ordering (the new user message comes after the tool message; OpenAI has no 1:1 tool pairing requirement on its side) and keep `_add_cache_control_if_applicable` on the tool message. The same change applies to the Responses-API twin in `…/responses_adapters/transformation.py`.

Sketch of the `isinstance(content.get("content"), list)` branch:

```python
elif isinstance(content.get("content"), list):
    text_parts, image_parts = [], []
    for c in content.get("content", []):
        if c.get("type") == "image":
            # existing helper already produces {"type": "image_url", "image_url": {...}}
            image_parts.append(self._translate_anthropic_image_to_openai(cast(dict, c["source"])))
        elif c.get("type") in ("text", "input_text"):
            text_parts.append({"type": "text", "text": c.get("text", "")})

    # Tool message stays text-only (string content = maximum backend compatibility).
    tool_result = ChatCompletionToolMessage(
        role="tool",
        tool_call_id=content.get("tool_use_id", ""),
        content="".join(p["text"] for p in text_parts)
        or ("[image returned by tool — see following message]" if image_parts else ""),
    )
    self._add_cache_control_if_applicable(content, tool_result, model)
    tool_message_list.append(tool_result)

    # OpenAI/vLLM can't carry images in a tool-role message: hoist them into a user turn.
    if image_parts:
        tool_message_list.append(
            ChatCompletionUserMessage(
                role="user",
                content=[{"type": "text", "text": "Image(s) returned by the tool call above:"}, *image_parts],
            )
        )
```

The trailing user message is only added when `image_parts` is non-empty, so the text-only path is unchanged.

### Current workaround

A small reverse proxy in front of LiteLLM that moves image blocks out of `tool_result` into a trailing user message before forwarding to `/v1/messages`, and streams everything else through unchanged. That restores image analysis on the Claude Code → LiteLLM → vLLM/OpenAI path.

Reference implementation: <!-- TODO: paste the committed permalink to claude-shim.js here before posting --> `<INSERT_SCRIPT_LINK_AFTER_COMMIT>`
