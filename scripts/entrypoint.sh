#!/usr/bin/env bash
# https://stackoverflow.com/questions/27669950/difference-between-euid-and-uid
set -euo pipefail
umask 0027

APP_USER=agent
APP_GROUP=agent
APP_HOME=/home/$APP_USER
readonly APP_USER APP_GROUP APP_HOME

CURRENT_GID=$(getent group "$APP_GROUP" 2>/dev/null | cut -d: -f3)
CURRENT_UID=$(id -u "$APP_USER" 2>/dev/null || echo "")

if [[ "${EUID}" -ne 0 ]]; then
    echo ">>> [Entrypoint] Requires root to run setup (creating users, fixing file ownership)."
    echo "    The container process is currently running as EUID=${EUID}. Please start the container without a --user override."
    exit 1
fi

# Validate PUID/PGID for positive integer values
if ! [[ "${PUID:-}" =~ ^[1-9][0-9]*$ ]] || ! [[ "${PGID:-}" =~ ^[1-9][0-9]*$ ]]; then
    echo ">>> [Config] PUID=${PUID:-<unset>} PGID=${PGID:-<unset>} — Must be positive integers"
    echo "    Also running the application user as root is not supported."
    echo "    This container is designed to drop privileges after setup. Please set positive integer values for PUID and PGID."
    exit 1
fi

# Preflight — fail fast with a clear message rather than letting services start
# and fail mysteriously at inference time.
: "${VLLM_URL:?VLLM_URL is not set. Add it to compose.yml, e.g. VLLM_URL=http://<host>:8000/v1}"

NEEDS_CHOWN=false

if [[ -z "$CURRENT_GID" ]]; then
    echo "> Group '$APP_GROUP' not found — creating with GID=${PGID}"
    groupadd "$APP_GROUP" --gid "${PGID}"
    NEEDS_CHOWN=true
elif [[ "$CURRENT_GID" -ne "${PGID}" ]]; then
    echo "> Group '$APP_GROUP' found with GID=${CURRENT_GID} — updating to GID=${PGID}"
    groupmod -g "${PGID}" "$APP_GROUP" > /dev/null
    NEEDS_CHOWN=true
else
    echo "> Group '$APP_GROUP' found with correct GID=${PGID} — skipping"
fi

if [[ -z "$CURRENT_UID" ]]; then
    echo "> User '$APP_USER' not found — creating with UID=${PUID}"
    useradd -g "$APP_GROUP" -m -d "$APP_HOME" -s /bin/bash "$APP_USER" --uid "${PUID}"
    NEEDS_CHOWN=true
elif [[ "$CURRENT_UID" -ne "${PUID}" ]]; then
    echo "> User '$APP_USER' found with UID=${CURRENT_UID} — updating to UID=${PUID}"
    usermod -u "${PUID}" -g "${PGID}" "$APP_USER" > /dev/null
    NEEDS_CHOWN=true
else
    echo "> User '$APP_USER' found with correct UID=${PUID} — skipping"
fi

if [[ "$NEEDS_CHOWN" = "true" ]]; then
    # -xdev: stay on the same filesystem, skip bind mounts (avoids EPERM on :ro mounts)
    find "$APP_HOME" -xdev -exec chown "$APP_USER":"$APP_GROUP" {} +
    # Explicitly re-own bind-mounted data dirs that -xdev skips
    chown -R "$APP_USER":"$APP_GROUP" "$APP_HOME/.claude" 2>/dev/null || true
fi

# Sync Claude Code config files on every start so config changes always take effect.
# Sources are mounted read-only at /home/agent/.config/claude-* by compose.yml.
# ~/.claude/ is a rw volume (session state lives there alongside the synced files).
# ~/.claude.json is in the writable container layer — written here so onboarding/trust
# state is always correct even after a container restart or recreation.
CLAUDE_CFG_SRC_SETTINGS="$APP_HOME/.config/claude-settings.json"
CLAUDE_CFG_SRC_CLAUDE_MD="$APP_HOME/.config/claude-CLAUDE.md"
CLAUDE_CFG_SRC_AGENTS="$APP_HOME/.config/claude-agents"
CLAUDE_DIR="$APP_HOME/.claude"
CLAUDE_JSON_SRC="$APP_HOME/.config/claude.json"
CLAUDE_JSON="$APP_HOME/.claude.json"

if [[ -f "$CLAUDE_CFG_SRC_SETTINGS" ]]; then
    mkdir -p "$CLAUDE_DIR/agents"
    chown "$APP_USER":"$APP_GROUP" "$CLAUDE_DIR" "$CLAUDE_DIR/agents"
    install -m644 -o "$APP_USER" -g "$APP_GROUP" "$CLAUDE_CFG_SRC_SETTINGS" "$CLAUDE_DIR/settings.json"
    install -m644 -o "$APP_USER" -g "$APP_GROUP" "$CLAUDE_CFG_SRC_CLAUDE_MD" "$CLAUDE_DIR/CLAUDE.md"
    if [[ -d "$CLAUDE_CFG_SRC_AGENTS" ]]; then
        rm -f "$CLAUDE_DIR/agents/"*.md 2>/dev/null || true
        find "$CLAUDE_CFG_SRC_AGENTS" -name '*.md' | while IFS= read -r f; do
            install -m644 -o "$APP_USER" -g "$APP_GROUP" "$f" "$CLAUDE_DIR/agents/$(basename "$f")"
        done
    fi
    [[ -f "$CLAUDE_JSON_SRC" ]] && install -m600 -o "$APP_USER" -g "$APP_GROUP" "$CLAUDE_JSON_SRC" "$CLAUDE_JSON"
    echo "> Claude Code config synced to $CLAUDE_DIR and $CLAUDE_JSON"
fi

OPENCODE_WORKSPACE="/home/agent/workspace"
readonly OPENCODE_WORKSPACE

if [[ ! -d "$OPENCODE_WORKSPACE" ]]; then
    echo ">>> [Entrypoint] Workspace directory '$OPENCODE_WORKSPACE' not found — is the volume mounted?"
    exit 1
fi

# Build the ordered list of available tools from TOOLS (defined in Dockerfile, overridable in compose.yml).
# First entry is the default. Each name is validated and checked against installed binaries.
AVAILABLE_TOOLS=()
IFS=',' read -ra _TOOLS_LIST <<< "$TOOLS"
for _t in "${_TOOLS_LIST[@]}"; do
    _t="${_t// /}"  # trim whitespace
    # Validate tool name is safe (alphanumeric, hyphens, underscores only)
    if ! [[ "$_t" =~ ^[a-zA-Z0-9_-]+$ ]]; then
        echo "> [Warning] Skipping invalid tool name '$_t' in TOOLS"
        continue
    fi
    if gosu "$APP_USER":"$APP_GROUP" bash -c "command -v '$_t'" &>/dev/null; then
        AVAILABLE_TOOLS+=("$_t")
    else
        echo "> [Warning] Tool '$_t' listed in TOOLS but not found — skipping"
    fi
done

if [[ ${#AVAILABLE_TOOLS[@]} -eq 0 ]]; then
    echo ">>> [Entrypoint] No tools available. Ensure at least one tool binary is installed in the image."
    exit 1
fi

# If DEFAULT_TOOL is set, validate it now so we fail fast before wetty starts.
if [[ -n "${DEFAULT_TOOL:-}" ]]; then
    _VALID=false
    for _t in "${AVAILABLE_TOOLS[@]}"; do
        if [[ "$_t" = "$DEFAULT_TOOL" ]]; then
            _VALID=true
            break
        fi
    done
    if [[ "$_VALID" = "false" ]]; then
        echo ">>> [Entrypoint] DEFAULT_TOOL='$DEFAULT_TOOL' not available. Available: ${AVAILABLE_TOOLS[*]}"
        exit 1
    fi
fi

# Export available tools as a space-separated string — inherited by agent-session.sh via wetty.
export AVAILABLE_TOOLS_ENV="${AVAILABLE_TOOLS[*]}"

# _supervise <name> <restart_delay_s> <cmd...>  — restarts cmd on crash without touching wetty/sessions.
_supervise() {
    local name="$1" delay="$2"; shift 2
    local _rc=0
    while true; do
        # || _rc=$? keeps set -e from exiting the subshell when the sidecar crashes.
        "$@" || _rc=$?
        echo "> [Supervisor] $name exited (exit ${_rc}) — restarting in ${delay} s" >&2
        _rc=0
        sleep "$delay"
    done
}

# Start the image upload companion server (addon — 30 s restart delay)
( _supervise upload-server 30 gosu agent node /upload-server.js ) &
echo "> Upload server started on port 1112 — https://<your-server-ip>:1112"

# Start the Claude→LiteLLM rewrite proxy (critical for image analysis — 5 s restart delay).
# It lifts images out of Claude Code's tool_result blocks so LiteLLM forwards them to vLLM
# instead of dropping them. Claude Code reaches it via ANTHROPIC_BASE_URL=http://127.0.0.1:4001.
( _supervise claude-shim 5 gosu agent node /claude-shim.js ) &
echo "> Claude image-rewrite proxy started on 127.0.0.1:4001 → ${LITELLM_UPSTREAM:-http://agentic-litellm:4000}"

echo "> Starting WeTTY browser terminal on port 1111..."
echo "> Connect at: https://<your-server-ip>:1111  (accept the self-signed cert warning once)"
# wetty must run as root so it detects localhost and uses local/command mode instead of SSH.
# agent-session.sh drops to the agent user immediately on startup.
exec wetty \
    --port 1111 \
    --host 0.0.0.0 \
    --command /agent-session.sh \
    --title "Agentic Harness Sandbox" \
    --allow-iframe \
    --ssl-key /etc/wetty/key.pem \
    --ssl-cert /etc/wetty/cert.pem
