# ── Chef Stage ────────────────────────────────────────────────────────────────
FROM rustlang/rust:nightly-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /app

# ── Planner Stage ─────────────────────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Cacher Stage ──────────────────────────────────────────────────────────────
FROM chef AS cacher
# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    git \
 && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
# Cook dependencies
RUN cargo chef cook --release --recipe-path recipe.json

# ── Builder Stage ─────────────────────────────────────────────────────────────
FROM cacher AS builder
# cacher already has dependencies in /app/target and /usr/local/cargo
COPY . .
# Build the server binary
RUN cargo build --release -p server

# ── Runtime Image ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
 && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/flux-server /usr/local/bin/

# Expose standard listen port
ENV PORT=4000
EXPOSE 4000

# Start the gRPC orchestration server
CMD ["flux-server"]
