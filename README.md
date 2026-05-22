# OpenCode + vLLM (Gemma 4 26B MoE) — Docker Sandbox Setup

Run OpenCode as a sandboxed, non-root Docker container connected to a self-hosted vLLM inference server. No cloud API keys required.

---

## Prerequisites

- Docker + Docker Compose installed on your machine
- Access to a running vLLM server exposing an OpenAI-compatible API (e.g. `http://10.0.0.13:8000`)
- Your vLLM server must have the model loaded and `/v1/models` responding

Verify your vLLM is reachable before starting:

```bash
curl http://10.0.0.13:8000/v1/models
```

You should see your model ID in the response (e.g. `gemma4-26b-a4b`).

Use the exact `"id"` value from the response — e.g. `gemma4-26b-a4b`.

**Finding your context size:**
The `max_model_len` field in the `/v1/models` response is your context limit. Use that value for `"context"`.

---

## Directory Structure

Create the following layout on your machine:

```
opencode-sandbox/
├── Dockerfile
├── compose.yml
├── start.sh
├── config/
│   ├── opencode.json
│   └── auth.json       ← provider auth tokens (mounted read-only)
├── data/               ← opencode session state, persisted across runs
├── .opencode/          ← skills and agent config (optional)
└── workspace/          ← put your code projects here
```

---

## Get, Build & Run

```bash
# Get the code
git clone git@github.com:jammsen/docker-opencode-sandbox.git

# Build and launch
./start.sh

# Force a full rebuild (no layer cache) — useful when changing Dockerfile or feature toggles
./start.sh --no-cache
```

On first launch OpenCode opens the TUI. Press `/` to open the command palette.

---

## Verify Everything Works

Inside the TUI:

1. Press `/model` — your model should appear under your provider name with an orange dot
2. Type `hello, what model are you?` — the response should mention your model ID
3. Check the status bar at the bottom — it should show `Gemma 4 26B MoE · vLLM (Gemma4 local)`
4. Check the right panel — `$0.00 spent` confirms no cloud API is being used

---

## Usage Tips

### Working with files

Drop files into `./workspace/` on your host. They appear at `~/workspace/` inside the container. OpenCode operates within this directory and cannot access anything outside it.

```bash
# Copy a project into the sandbox
cp -r ~/myproject ./workspace/myproject
```

### Modes

| Mode  | Shortcut | Token overhead | Best for                               |
| ----- | -------- | -------------- | -------------------------------------- |
| Build | default  | ~10k tokens    | Agentic file editing, multi-step tasks |
| Ask   | `tab`    | ~3-5k tokens   | Questions, code review, explanations   |

With a 32k context limit, **Ask mode** leaves significantly more room for your actual code and conversation.

### Context window awareness

The status bar shows `X tokens (Y% used)`. Build mode consumes ~10,000 tokens just for the system prompt before you type anything. For large codebases, open only the files you need or use Ask mode.

---

## Troubleshooting

**Config not loading / provider picker appears on every launch**

```bash
docker compose run --rm --entrypoint bash opencode -c \
  "cat /home/opencode/.config/opencode/opencode.json"
```

If this returns an error, check that `docker compose` is run from the same directory as `docker-compose.yml` and that `./config/opencode.json` exists.

**`GID already exists` error during build**

Ubuntu 26.04 ships with a default user at UID/GID 1000. The Dockerfile handles this by renaming the existing user instead of creating a new one. Ensure you are using the Dockerfile exactly as provided above.

**Model not responding / timeout**

```bash
# Test vLLM connectivity from inside the container
docker compose run --rm --entrypoint bash opencode -c \
  "curl -s http://YOUR_VLLM_IP:8000/v1/models"
```

If this fails, your vLLM IP is unreachable from the container. Use the actual host IP — not `localhost`.

**Tool calling loops or model halts mid-task**

This is a known Gemma 4 behavior with agentic tool use. Mitigations:

- Prefer **Ask mode** for questions and code review that don't require file editing
- For Build mode, give explicit step-by-step instructions rather than open-ended goals
- Keep tasks scoped to one file or one function at a time

---

## Security Notes

The container starts as root to handle setup (creating the user, fixing file ownership on mounted volumes), then permanently drops to an unprivileged user via `gosu` before your session begins. There is no way back to root after that point.

**Restrictions in place:**

- **`no-new-privileges`** — once the container drops to the unprivileged user, no process inside the container can ever gain more permissions, even if it tries to run a `sudo` binary or a binary with special file capabilities. The kernel enforces this hard, before any code in such a binary even runs.
- **`cap_drop: ALL`** — Linux capabilities are fine-grained units of root power (e.g. "change file ownership", "bind to privileged ports", "load kernel modules"). By default Docker grants containers a subset of these even without full root. Dropping all of them removes every one of those powers.
- **`cap_add: CHOWN, SETUID, SETGID, DAC_OVERRIDE`** — only the four capabilities the entrypoint actually needs for its setup phase are added back. Once `gosu` drops to the non-root user, the kernel automatically clears the effective capability set on the UID transition, and `no-new-privileges` blocks any path to reclaiming them.
- **`PUID` / `PGID`** — the in-container user is created at runtime with the same UID/GID as your host user. This ensures bind-mounted files in `./workspace` and `./data` have correct ownership on both sides of the mount.
- Bridge networking only — isolated from the host network
- Filesystem access limited to `./workspace` and `./data` on the host

The model runs entirely on your local vLLM server. No data leaves your network.

## Feature Toggles

The image supports optional language runtimes controlled via build arguments. All toggles default to `false`.

| ARG | Default | Effect |
| --- | ------- | ------ |
| `ENABLE_NODEJS` | `false` | Installs Node.js and npm via apt |
| `ENABLE_PYTHON` | `false` | Installs `uv` and the configured Python version |
| `ENABLE_RUST` | `false` | Installs Rust via `rustup` |
| `PYTHON_VERSION` | `3.13` | Python version passed to `uv python install` |

Enable a toggle at build time Dockerfile rewrite or via command-line args:

```bash
docker compose build --build-arg ENABLE_PYTHON=true --build-arg PYTHON_VERSION=3.12
./start.sh --no-cache  # rebuild with new toggles
```

All runtimes are installed at **build time** under the `opencode` user, so the container starts instantly with no network downloads at runtime. The tool binaries are on `PATH` and their data directories (`CARGO_HOME`, `RUSTUP_HOME`) are pinned via environment variables so they survive the `HOME` override that redirects opencode's session state to the mounted workspace.

---

## Using Tools and Skills

### Skills

Skills a special capabilities for an Agent that tells him how to do things or how to handle tools.
They have a specific Format and a SKILL.md file is mandatory.
Read more about Skills here https://agentskills.io/home

- just copy over the `.opencode` folder into your workspace. Opencode will then recognize them
