FROM ubuntu:26.04@sha256:f3d28607ddd78734bb7f71f117f3c6706c666b8b76cbff7c9ff6e5718d46ff64

ARG ENABLE_NODEJS=true
ARG ENABLE_PYTHON=false
ARG ENABLE_RUST=false
ARG PYTHON_VERSION=3.13

ENV DEBIAN_FRONTEND=noninteractive \
    TZ=Europe/Berlin \
    PUID=1000 \
    PGID=1000 \
    # Bake ARG values into the image so they're available at runtime too
    ENABLE_NODEJS=${ENABLE_NODEJS} \
    ENABLE_PYTHON=${ENABLE_PYTHON} \
    ENABLE_RUST=${ENABLE_RUST} \
    PYTHON_VERSION=${PYTHON_VERSION} \
    # Pin tool data dirs explicitly — survives the HOME override in entrypoint.sh
    CARGO_HOME=/home/opencode/.cargo \
    RUSTUP_HOME=/home/opencode/.rustup \
    # All user tool bins in PATH — inherited by every subprocess after exec gosu
    PATH="/home/opencode/.local/bin:/home/opencode/.cargo/bin:/home/opencode/.opencode/bin:${PATH}"


# Install basic tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    git \
    gosu \
    openssh-client \
    && apt-get autoremove -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# Optional: Node.js — system-wide, stays as root
RUN if [ "$ENABLE_NODEJS" = "true" ]; then \
    apt-get update && apt-get install -y --no-install-recommends --no-install-suggests nodejs npm \
    && apt-get autoremove -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*; \
    fi

# Ubuntu 26.04 ships with a default 'ubuntu' user at 1000:1000 — reuse it
RUN usermod -l opencode ubuntu && \
    groupmod -n opencode ubuntu && \
    usermod -d /home/opencode -m opencode

RUN mkdir -p /home/opencode/.config/opencode \
    /home/opencode/.local/share/opencode \
    /home/opencode/workspace && \
    chown -R opencode:opencode /home/opencode

# Switch to opencode user — HOME is now /home/opencode, installs land in the right place
USER opencode
WORKDIR /home/opencode

# Optional: uv + Python
RUN if [ "$ENABLE_PYTHON" = "true" ]; then \
    curl -LsSf https://astral.sh/uv/install.sh | sh \
    && uv python install ${PYTHON_VERSION}; \
    fi

# Optional: Rust — --no-modify-path because PATH is managed via ENV above
RUN if [ "$ENABLE_RUST" = "true" ]; then \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path; \
    fi

# Always: opencode itself
RUN curl -fsSL https://opencode.ai/install | bash

USER root
WORKDIR /

COPY --chmod=744 entrypoint.sh /

ENTRYPOINT ["./entrypoint.sh"]
