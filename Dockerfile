# syntax=docker/dockerfile:1

# ==============================================================================
# Octopus API Gateway - multi-stage, multi-arch image
# ==============================================================================

# ---- chef: shared base with the build toolchain -----------------------------
FROM rust:1.88-slim AS chef
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        protobuf-compiler \
        git \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef --locked

# ---- planner: compute the dependency recipe ---------------------------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---- builder: cook deps (cached), then build the binary ---------------------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build & cache dependencies only — this layer is reused until deps change.
RUN cargo chef cook --release --bin octopus --recipe-path recipe.json
COPY . .
ARG VERSION=dev
ARG COMMIT=unknown
ARG BUILD_DATE
ENV OCTOPUS_VERSION=${VERSION} \
    OCTOPUS_COMMIT=${COMMIT} \
    OCTOPUS_BUILD_DATE=${BUILD_DATE}
RUN cargo build --release --bin octopus

# ---- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 -s /bin/bash octopus \
    && mkdir -p /etc/octopus /var/log/octopus \
    && chown -R octopus:octopus /etc/octopus /var/log/octopus

COPY --from=builder /app/target/release/octopus /usr/local/bin/octopus
COPY config.example.yaml /etc/octopus/config.example.yaml

ARG VERSION=dev
ARG COMMIT=unknown
ARG BUILD_DATE
LABEL org.opencontainers.image.title="octopus" \
      org.opencontainers.image.description="Octopus API Gateway" \
      org.opencontainers.image.source="https://github.com/xraph/octopus" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${COMMIT}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

USER octopus

EXPOSE 8080 9090

# HTTP liveness on the gateway's /livez probe (config default 0.0.0.0:8080).
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/livez || exit 1

ENTRYPOINT ["octopus"]
CMD ["serve", "--config", "/etc/octopus/config.yaml"]
