# Stage 1: Build Sccache natively caching S2S dependency graph compilation
FROM rust:1.80-slim-bookworm AS builder

# Install required build dependencies including SSL and pkg-config for reqwest/sqlx
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    gcc \
    make \
    perl

WORKDIR /usr/src/app

# Only copy in Cargo manifests to cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY api/Cargo.toml api/
COPY runtime/Cargo.toml runtime/
COPY cli/Cargo.toml cli/

# Create dummy source files so `cargo fetch` builds standard dependencies
RUN mkdir -p api/src runtime/src cli/src \
    && echo "fn main() {}" > api/src/main.rs \
    && echo "fn main() {}" > runtime/src/main.rs \
    && echo "fn main() {}" > cli/src/main.rs \
    && cargo build --release --bin api \
    && rm -rf api/src

# Copy the actual API source code (which triggers a rebuild of the binary only)
COPY api/ api/

# Ensure macro evaluation can use offline mode using the .sqlx cache
ENV SQLX_OFFLINE=true

# Rebuild the final binary natively over the dependency caches
RUN touch api/src/main.rs && cargo build --release --bin api

# Stage 2: Create a tiny minimal runtime image
FROM debian:bookworm-slim

# Install OpenSSL CA certificates needed by reqwest/SQLx at runtime
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

# Copy the compiled binary from the builder environment
COPY --from=builder /usr/src/app/target/release/api ./fluxbase-api

# Optionally create a directory for migrations or templates if needed later
# RUN mkdir -p /var/lib/fluxbase

ENV PORT=3000

# Open the port our Axum server binds against
EXPOSE 3000

CMD ["./fluxbase-api"]
