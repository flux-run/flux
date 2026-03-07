# Stage 1: Build the rust application safely
FROM rust:1.80-slim-bookworm AS builder

# Install essential dependencies needed by networking traits
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    gcc \
    make \
    perl

WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./
COPY api/Cargo.toml api/
COPY runtime/Cargo.toml runtime/
COPY cli/Cargo.toml cli/

RUN mkdir -p api/src runtime/src cli/src \
    && echo "fn main() {}" > api/src/main.rs \
    && echo "fn main() {}" > runtime/src/main.rs \
    && echo "fn main() {}" > cli/src/main.rs \
    && cargo build --release --bin runtime \
    && rm -rf runtime/src

# Bring over the real code to compile
COPY runtime/ runtime/

# Touch main.rs to invalidate the cache marker and rebuild the actual target
RUN touch runtime/src/main.rs && cargo build --release --bin runtime

# Stage 2: Tiny runner mapping
FROM debian:bookworm-slim

# The core certificates required for making webhook/auth requests outward natively
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

COPY --from=builder /usr/src/app/target/release/runtime ./fluxbase-runtime

ENV PORT=4000

EXPOSE 4000

CMD ["./fluxbase-runtime"]
