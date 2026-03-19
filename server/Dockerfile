FROM rust:1.80-slim-bookworm AS builder
WORKDIR /app

# Install required build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    git \
 && rm -rf /var/lib/apt/lists/*

# Copy the entire workspace
COPY . .

# Build the server binary
RUN cargo build --release -p server

# ── Runtime Image ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies for executing and making outbound web requests
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
