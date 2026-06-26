#!/usr/bin/env bash
# Runs inside the browser terminal — spawned by wetty for each browser connection.
# Self-wraps in GNU screen so closing the browser tab detaches rather than kills the session.
# On reconnect, screen reattaches to the same running agent.
# Inherits AVAILABLE_TOOLS_ENV, DEFAULT_TOOL, TOOLS from entrypoint.sh.

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

    # Present picker with re-prompt on invalid input.
    # Always shown — even with no sessions — so the terminal has time to size
    # correctly before screen starts. Use screen -x (multiattach) so multiple
    # browser tabs can share a session.
    _MAX=$((${#SESSION_IDS[@]} + 1))
    while true; do
        echo ""
        echo "Existing sessions:"
        for i in "${!SESSION_LABELS[@]}"; do
            if [[ $i -eq 0 ]]; then
                echo "  $((i+1)). ${SESSION_LABELS[$i]}  (default)"
            else
                echo "  $((i+1)). ${SESSION_LABELS[$i]}"
            fi
        done
        echo "  $_MAX. Start a new session"
        echo ""
        read -r -p "Enter selection [1]: " _SEL
        _SEL="${_SEL:-1}"

        if [[ "$_SEL" =~ ^[0-9]+$ ]] && [[ "$_SEL" -ge 1 ]] && [[ "$_SEL" -le "${#SESSION_IDS[@]}" ]]; then
            # -x = multiattach: works whether the session is Detached or Attached.
            exec screen -x "${SESSION_IDS[$((${_SEL}-1))]}"
        elif [[ "$_SEL" =~ ^[0-9]+$ ]] && [[ "$_SEL" -eq "$_MAX" ]]; then
            exec screen -S "sandbox-started-$(date +%Y-%m-%d-%H:%M:%S)" "$0"
        else
            echo "  Invalid — enter a number between 1 and $_MAX"
        fi
    done
fi
# --- From here on we are inside a screen session ---

# Tool list is built and validated once by entrypoint.sh, exported as AVAILABLE_TOOLS_ENV.
if [[ -z "${AVAILABLE_TOOLS_ENV:-}" ]]; then
    echo ">>> AVAILABLE_TOOLS_ENV is not set — was this session started through entrypoint.sh?"
    exit 1
fi
read -ra AVAILABLE_TOOLS <<< "$AVAILABLE_TOOLS_ENV"

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
                echo ">>> Invalid selection '$SELECTION' — defaulting to ${AVAILABLE_TOOLS[0]} in 3 seconds..."
                sleep 3
                TOOL="${AVAILABLE_TOOLS[0]}"
            fi
            ;;
    esac
    echo ""
fi

# Start all tools from the workspace directory.
# omp auto-switches away from ~ unless --allow-home is passed, and opencode
# uses CWD as its project root — both need a proper starting directory.
cd /home/agent/workspace

exec "$TOOL"
