# Stage 1: Build Sccache natively caching S2S dependency graph compilation
FROM rust:slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    gcc \
    make \
    perl \
    curl \
    python3

WORKDIR /usr/src/app

# Copy the API-specific Cargo lockfiles directly from the current build context (the `api/` folder)
COPY Cargo.toml Cargo.lock* ./

# Create dummy source wrapper to strictly cache dependency builds
RUN mkdir -p src \
    && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Copy the actual API source code (which triggers a rebuild of the binary only)
COPY src/ src/
COPY .sqlx/ .sqlx/

# Ensure macro evaluation can use offline mode using the .sqlx cache
ENV SQLX_OFFLINE=true

# Rebuild the final binary natively over the dependency caches
RUN touch src/main.rs && cargo build --release

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
