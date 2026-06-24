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

## MANDATORY: Image Analysis
NEVER use the Read tool on image files (png, jpg, jpeg, gif, webp).
ALWAYS delegate to the vision-analyzer agent instead.
