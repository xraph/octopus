# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.toml
COPY crates crates

# Build dependencies (cached layer)
RUN mkdir -p octopus-cli/src && \
    echo "fn main() {}" > octopus-cli/src/main.rs && \
    cargo build --release && \
    rm -rf octopus-cli/src

# Copy source and build
COPY . .
RUN cargo build --release --bin octopus

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash octopus

# Copy binary from builder
COPY --from=builder /app/target/release/octopus /usr/local/bin/octopus

# Create directories
RUN mkdir -p /etc/octopus /var/log/octopus && \
    chown -R octopus:octopus /etc/octopus /var/log/octopus

USER octopus

EXPOSE 8080 9090

ENTRYPOINT ["octopus"]
CMD ["--config", "/etc/octopus/config.yaml"]


