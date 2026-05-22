#!/usr/bin/env bash
# https://stackoverflow.com/questions/27669950/difference-between-euid-and-uid
set -e

APP_USER=opencode
APP_GROUP=opencode
APP_HOME=/home/$APP_USER
CURRENT_GID=$(getent group "$APP_GROUP" 2>/dev/null | cut -d: -f3)
CURRENT_UID=$(id -u "$APP_USER" 2>/dev/null || echo "")

if [[ "${EUID}" -ne 0 ]]; then
    echo ">>> [Entrypoint] Requires root to run setup (creating users, fixing file ownership)."
    echo "    The container process is currently running as EUID=${EUID}. Please start the container without a --user override."
    exit 1
fi

if [[ "${PUID}" -eq 0 ]] || [[ "${PGID}" -eq 0 ]]; then
    echo ">>> [Config] PUID=${PUID} PGID=${PGID} — Running the application user as root is not supported."
    echo "    This container is designed to drop privileges after setup. Please set non-zero values for PUID and PGID."
    exit 1
fi

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
fi

# HOME → workspace so opencode session state lands on the mounted volume
# CARGO_HOME/RUSTUP_HOME are pinned in the image ENV, so tools still find their data
OPENCODE_WORKSPACE="/home/opencode/workspace"
echo "> Set HOME to $OPENCODE_WORKSPACE (mounted workspace volume)"
export HOME="$OPENCODE_WORKSPACE"

exec gosu $APP_USER:$APP_GROUP "opencode"
