# harness-proxy — multi-stage build.
# Builder: Ubuntu (NOT alpine), same lineage as the repo's ubuntu:26.04. Produces a fully
# static musl binary so the final stage can be FROM scratch with zero runtime deps.
# Built for the NATIVE build arch (amd64 or arm64) — no cross-compile. Fallback if a fully
# static build ever breaks: ship a chiselled Ubuntu rootfs instead of scratch — see PLAN.md §3.

FROM ubuntu:26.04@sha256:f3d28607ddd78734bb7f71f117f3c6706c666b8b76cbff7c9ff6e5718d46ff64 AS builder

# Provided by BuildKit from the build platform (amd64/arm64). Falls back to dpkg if unset.
ARG TARGETARCH

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        musl-tools \
    && rm -rf /var/lib/apt/lists/*

# Map docker arch -> rust musl triple, install rustup + the matching musl target.
RUN set -eux; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
      amd64) TRIPLE=x86_64-unknown-linux-musl ;; \
      arm64) TRIPLE=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported arch: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    echo "$TRIPLE" > /triple; \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable; \
    rustup target add "$TRIPLE"

WORKDIR /build
# ponytail: skip the cargo-chef/dummy-main dep-caching dance — copy sources and build once.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN set -eux; TRIPLE="$(cat /triple)"; \
    cargo build --locked --release --target "$TRIPLE"; \
    cp "target/$TRIPLE/release/harness-proxy" /harness-proxy

# Final: scratch + the static binary only, running as a non-root numeric UID.
FROM scratch
COPY --from=builder /harness-proxy /harness-proxy
USER 65532:65532
EXPOSE 4000
ENTRYPOINT ["/harness-proxy"]
