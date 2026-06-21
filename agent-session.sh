#!/usr/bin/env bash
# Runs inside the browser terminal — spawned by wetty for each browser connection.
# Self-wraps in GNU screen so closing the browser tab detaches rather than kills the session.
# On reconnect, screen reattaches to the same running agent.
# Inherits AVAILABLE_TOOLS_ENV, DEFAULT_TOOL, TOOLS, OPENCODE_WORKSPACE from entrypoint.sh.

# wetty must run as root for local/command mode; drop to agent user immediately.
# gosu preserves environment variables so all exported vars from entrypoint.sh carry through.
if [[ "${EUID}" -eq 0 ]]; then
    exec /usr/sbin/gosu agent "$0" "$@"
fi

set -euo pipefail

# --- Screen session persistence ---
# STY is set by screen when already inside a session — skip this block if so.
if [[ -z "${STY:-}" ]]; then

    # Clean up dead/zombie screen sessions before listing.
    screen -wipe &>/dev/null || true

    # Parse running screen sessions for this user.
    SESSION_IDS=()    # "12345.sandbox" — used for screen -x
    SESSION_LABELS=() # "sandbox  (Detached)" — shown to user

    while IFS= read -r line; do
        # Match lines like: "    12345.sandbox    (date)    (Detached)"
        # Capture the full PID.name and the final status token.
        # Use [(] [)] instead of \( \) — bash 5.3+ rejects escaped parens in [[ =~ ]] patterns.
        _SCREEN_RE='^[[:space:]]+([0-9]+[.][^[:space:]]+).*[(]([^)]+)[)][[:space:]]*$'
        if [[ "$line" =~ $_SCREEN_RE ]]; then
            FULL_ID="${BASH_REMATCH[1]}"          # "12345.sandbox"
            STATUS="${BASH_REMATCH[2]}"           # "Detached" or "Attached"
            NAME="${FULL_ID#*.}"                  # "sandbox"
            SESSION_IDS+=("$FULL_ID")
            SESSION_LABELS+=("$NAME  ($STATUS)")
        fi
    done < <(screen -ls 2>/dev/null || true)

    if [[ ${#SESSION_IDS[@]} -eq 0 ]]; then
        # No existing sessions — start fresh with a timestamped name.
        exec screen -S "sandbox-started-$(date +%Y-%m-%d-%H:%M:%S)" "$0"
    fi

    # Sessions exist — present picker.
    # Use screen -x (multiattach) so multiple browser tabs can share a session.
    echo ""
    echo "Existing sessions:"
    for i in "${!SESSION_LABELS[@]}"; do
        if [[ $i -eq 0 ]]; then
            echo "  $((i+1)). ${SESSION_LABELS[$i]}  (default)"
        else
            echo "  $((i+1)). ${SESSION_LABELS[$i]}"
        fi
    done
    echo "  $((${#SESSION_IDS[@]}+1)). Start a new session"
    echo ""
    read -r -p "Enter selection [1]: " _SEL
    _SEL="${_SEL:-1}"

    if [[ "$_SEL" =~ ^[0-9]+$ ]] && [[ "$_SEL" -ge 1 ]] && [[ "$_SEL" -le "${#SESSION_IDS[@]}" ]]; then
        # -x = multiattach: works whether the session is Detached or Attached.
        exec screen -x "${SESSION_IDS[$((${_SEL}-1))]}"
    elif [[ "$_SEL" =~ ^[0-9]+$ ]] && [[ "$_SEL" -eq "$((${#SESSION_IDS[@]}+1))" ]]; then
        exec screen -S "sandbox-started-$(date +%Y-%m-%d-%H:%M:%S)" "$0"
    else
        echo "Invalid selection — attaching to ${SESSION_LABELS[0]}"
        exec screen -x "${SESSION_IDS[0]}"
    fi
fi
# --- From here on we are inside a screen session ---

OPENCODE_WORKSPACE="${OPENCODE_WORKSPACE:-/home/agent/workspace}"

# Rebuild available tool list from AVAILABLE_TOOLS_ENV (space-separated) exported by entrypoint.sh.
# Fall back to scanning TOOLS env var directly if env var is missing (defensive).
AVAILABLE_TOOLS=()
if [[ -n "${AVAILABLE_TOOLS_ENV:-}" ]]; then
    read -ra AVAILABLE_TOOLS <<< "$AVAILABLE_TOOLS_ENV"
else
    IFS=',' read -ra _TOOLS_LIST <<< "${TOOLS:-opencode}"
    for _t in "${_TOOLS_LIST[@]}"; do
        _t="${_t// /}"
        if [[ "$_t" =~ ^[a-zA-Z0-9_-]+$ ]] && command -v "$_t" &>/dev/null; then
            AVAILABLE_TOOLS+=("$_t")
        fi
    done
fi

if [[ ${#AVAILABLE_TOOLS[@]} -eq 0 ]]; then
    echo ">>> No agent tools available. Check the TOOLS environment variable."
    exit 1
fi

# Select tool — skip menu if DEFAULT_TOOL is set or only one tool exists.
TOOL=""
if [[ -n "${DEFAULT_TOOL:-}" ]]; then
    for _t in "${AVAILABLE_TOOLS[@]}"; do
        if [[ "$_t" = "$DEFAULT_TOOL" ]]; then
            TOOL="$_t"
            break
        fi
    done
    if [[ -z "$TOOL" ]]; then
        echo ">>> DEFAULT_TOOL='$DEFAULT_TOOL' not available. Available: ${AVAILABLE_TOOLS[*]}"
        exit 1
    fi
elif [[ ${#AVAILABLE_TOOLS[@]} -eq 1 ]]; then
    TOOL="${AVAILABLE_TOOLS[0]}"
else
    echo ""
    echo "Select which tool to start:"
    for i in "${!AVAILABLE_TOOLS[@]}"; do
        if [[ $i -eq 0 ]]; then
            echo "  $((i+1)). ${AVAILABLE_TOOLS[$i]}  (default)"
        else
            echo "  $((i+1)). ${AVAILABLE_TOOLS[$i]}"
        fi
    done
    echo ""
    read -r -p "Enter selection [1]: " SELECTION
    case "$SELECTION" in
        ""|1) TOOL="${AVAILABLE_TOOLS[0]}" ;;
        *)
            if [[ "$SELECTION" =~ ^[0-9]+$ ]] && \
               [[ "$SELECTION" -ge 2 ]] && \
               [[ "$((SELECTION-1))" -lt "${#AVAILABLE_TOOLS[@]}" ]]; then
                TOOL="${AVAILABLE_TOOLS[$((SELECTION-1))]}"
            else
                echo ">>> Invalid selection '$SELECTION' — defaulting to ${AVAILABLE_TOOLS[0]}"
                TOOL="${AVAILABLE_TOOLS[0]}"
            fi
            ;;
    esac
    echo ""
fi

# HOME → workspace so opencode session state lands on the mounted volume.
# omp keeps the real HOME (/home/agent) so it finds its config/logs there.
if [[ "$TOOL" = "opencode" ]]; then
    export HOME="$OPENCODE_WORKSPACE"
fi

exec "$TOOL"
