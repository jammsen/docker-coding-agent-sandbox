FROM ubuntu:26.04@sha256:f3d28607ddd78734bb7f71f117f3c6706c666b8b76cbff7c9ff6e5718d46ff64

ENV DEBIAN_FRONTEND=noninteractive \
    TZ=Europe/Berlin \
    PUID=1000 \
    PGID=1000 \
    # Tool selection — order defines menu order, first entry is the default
    TOOLS=opencode,omp \
    # Pin tool data dirs explicitly so subprocesses find language toolchains reliably
    CARGO_HOME=/home/agent/.cargo \
    RUSTUP_HOME=/home/agent/.rustup \
    # Absolute path so Playwright finds its browsers even when opencode redirects HOME at runtime
    PLAYWRIGHT_BROWSERS_PATH=/home/agent/.cache/ms-playwright \
    # All user tool bins in PATH — inherited by every subprocess after exec gosu
    PATH="/home/agent/.local/bin:/home/agent/.cargo/bin:/home/agent/.opencode/bin:${PATH}"


# Install basic tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    dnsutils \
    git \
    git-lfs \
    gosu \
    iproute2 \
    iputils-ping \
    jq \
    lsof \
    netcat-openbsd \
    openssh-client \
    openssl \
    pkg-config \
    postgresql-client \
    procps \
    ripgrep \
    screen \
    sqlite3 \
    tree \
    tzdata \
    unzip \
    wget \
    zip \
    && apt-get autoremove -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# Node.js — system-wide, available for workspace projects
RUN apt-get update && apt-get install -y --no-install-recommends --no-install-suggests nodejs npm \
    && apt-get autoremove -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# WeTTY — browser-based terminal served on port 1111
RUN npm install -g wetty@3.1.0

# Patch wetty's env.js which assumes "env (GNU coreutils)" version string.
# Ubuntu 26.04 ships uutils coreutils whose version output is "env (uutils coreutils) 0.8.0",
# causing an uncaughtException when the string split returns undefined.
# Returning 0 is safe: env version >=9 is only needed for the -S flag, which we never use.
RUN sed -i \
    '/resolve(parseInt(stdout\.split/s/.*/        resolve(0);/' \
    /usr/local/lib/node_modules/wetty/build/server/spawn/env.js \
    && grep -q 'resolve(0)' /usr/local/lib/node_modules/wetty/build/server/spawn/env.js \
    && echo 'env.js patched OK'

# Inject upload icon into wetty's page (links to upload server on port 1112).
# Port resolved at browser runtime via JS — no host hardcoded in the image.
RUN node -e 'const fs=require("fs"),\
    f="/usr/local/lib/node_modules/wetty/build/server/socketServer/html.js",\
    a="<a id=\"ub\" href=\"#\" target=\"_blank\" rel=\"noopener\" class=\"toggler\" title=\"Upload Image\" style=\"top:36px;\"><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 448 512\" fill=\"currentColor\" width=\"1em\" height=\"1em\"><path d=\"M246.6 9.4c-12.5-12.5-32.8-12.5-45.3 0l-128 128c-12.5 12.5-12.5 32.8 0 45.3s32.8 12.5 45.3 0L192 109.3 192 320c0 17.7 14.3 32 32 32s32-14.3 32-32l0-210.7 73.4 73.4c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3l-128-128zM64 352c0-17.7-14.3-32-32-32s-32 14.3-32 32l0 64c0 53 43 96 96 96l256 0c53 0 96-43 96-96l0-64c0-17.7-14.3-32-32-32s-32 14.3-32 32l0 64c0 17.7-14.3 32-32 32L96 448c-17.7 0-32-14.3-32-32l0-64z\"/></svg></a>",\
    s="<scr"+"ipt>document.getElementById(\"ub\").href=location.protocol+\"//\"+location.hostname+\":1112\";</"+"script>",\
    h=fs.readFileSync(f,"utf8");\
    fs.writeFileSync(f,h.replace("<iframe class=\"editor\"",a+s+"<iframe class=\"editor\""));\
    console.log("wetty upload link injected OK")' \
    && grep -q 'id="ub"' /usr/local/lib/node_modules/wetty/build/server/socketServer/html.js

# Self-signed TLS cert for WeTTY — avoids browser HTTPS-upgrade blocking on HTTP
# Browsers show a one-time "proceed anyway" warning, then work fine.
RUN mkdir -p /etc/wetty \
    && openssl req -x509 -newkey rsa:4096 \
       -keyout /etc/wetty/key.pem \
       -out /etc/wetty/cert.pem \
       -days 3650 -nodes \
       -subj "/CN=agentic-harness-sandbox" \
    && chmod 644 /etc/wetty/key.pem /etc/wetty/cert.pem

# Chromium headless system libraries — required for Playwright
RUN apt-get update && apt-get install -y --no-install-recommends \
    fonts-liberation \
    fonts-noto-color-emoji \
    libasound2t64 \
    libatk-bridge2.0-0t64 \
    libatk1.0-0t64 \
    libatspi2.0-0t64 \
    libcairo2 \
    libcups2t64 \
    libdbus-1-3 \
    libdrm2 \
    libgbm1 \
    libglib2.0-0t64 \
    libnspr4 \
    libnss3 \
    libpango-1.0-0 \
    libx11-6 \
    libxcb1 \
    libxcomposite1 \
    libxdamage1 \
    libxext6 \
    libxfixes3 \
    libxkbcommon0 \
    libxrandr2 \
    xdg-utils \
    && apt-get autoremove -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# Ubuntu 26.04 ships with a default 'ubuntu' user at 1000:1000 — reuse it
RUN usermod -l agent ubuntu && \
    groupmod -n agent ubuntu && \
    usermod -d /home/agent -m agent

RUN mkdir -p /home/agent/.config/opencode \
    /home/agent/.local/share/opencode \
    /home/agent/.omp/agent \
    /home/agent/.omp/logs \
    /home/agent/workspace && \
    chown -R agent:agent /home/agent

# Switch to agent user — HOME is now /home/agent, installs land in the right place
USER agent
WORKDIR /home/agent

# opencode
RUN curl -fsSL https://opencode.ai/install | bash

# omp
RUN curl -fsSL https://omp.sh/install | sh

# Rust — --no-modify-path because PATH is managed via ENV above
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path

# uv + Python
# ARG here (not at top) so changing PYTHON_VERSION only busts cache from this layer onward
ARG PYTHON_VERSION=3.13
ENV PYTHON_VERSION=${PYTHON_VERSION}
RUN curl -LsSf https://astral.sh/uv/install.sh | sh \
    && uv python install ${PYTHON_VERSION}

# Playwright — install Chromium browser binary at build time (system libs already installed above)
# --no-sandbox is required at runtime due to cap_drop: ALL — see AGENTS.md
RUN npx -y playwright install chromium

# claude — installs to ~/.local/bin/claude which is already on PATH via ENV above
RUN curl -fsSL https://claude.ai/install.sh | bash

USER root
WORKDIR /

COPY --chmod=744 scripts/entrypoint.sh /
COPY --chmod=755 scripts/agent-session.sh /agent-session.sh
COPY --chmod=644 scripts/upload-server.js /upload-server.js
COPY --chmod=644 scripts/claude-shim.js /claude-shim.js
COPY --chmod=755 scripts/agent-task.sh /usr/local/bin/agent-task

ENTRYPOINT ["./entrypoint.sh"]
