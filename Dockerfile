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
    pkg-config \
    postgresql-client \
    procps \
    ripgrep \
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

USER root
WORKDIR /

COPY --chmod=744 entrypoint.sh /

ENTRYPOINT ["./entrypoint.sh"]
