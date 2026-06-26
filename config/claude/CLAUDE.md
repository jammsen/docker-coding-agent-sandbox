# Claude Code Sandbox Rules

You are running inside a hardened Docker sandbox. You are a client to a dedicated vLLM server accessible only via its API endpoint — you have no direct access to the model weights. If you need information about yourself (capabilities, training cutoff, context size, etc.), look it up online based on your model ID.

All projects are located under /home/agent/workspace. Work only inside this directory, with no exceptions, unless the user explicitly asks for access outside the sandbox.

When starting work in an existing project directory, check whether WORKLOG.md exists and read it for prior context. Before finishing any task that changes files, append a concise entry to WORKLOG.md with the current Europe/Berlin timestamp from:
  TZ=Europe/Berlin date "+%d.%m.%Y, %H:%M (%Z)"
Include: changed files, concrete findings, and pending follow-ups.

## Running Background Servers

Port **3000** is the externally reachable port for agent-hosted servers. Ports 1111 and 1112 are reserved (WeTTY terminal, image upload). Bind servers to 0.0.0.0:3000.

Always check before starting a server:
  ss -tlnp | grep 3000

Use nohup with a PID file — do not use bare & without nohup.

## Using Playwright

Playwright with Chromium is pre-installed. Due to cap_drop:ALL, always launch with:
  --no-sandbox --disable-setuid-sandbox

## Web Search

The built-in `WebSearch` and `WebFetch` tools are disabled. `curl` and `wget` work fine for fetching a known URL directly. For search queries (no URL), use the `searxng_web_search` MCP tool — search engines block automated curl requests, so curl will return nothing useful on search pages. `web_url_read` is also available to fetch and convert a URL to markdown.

## Image Analysis
The model behind this sandbox is vision-capable. To analyse an image, use the **Read tool**
directly on the image file path (png, jpg, jpeg, gif, webp), e.g.:
  Read /home/agent/workspace/uploads/filename.png
Claude Code natively encodes the file and sends it to the model as a real image block — this is
the only mechanism that delivers actual pixels to the model. Do NOT base64-encode the file by hand
and paste the string into a prompt; raw base64 text is not an image and the model cannot see it.
