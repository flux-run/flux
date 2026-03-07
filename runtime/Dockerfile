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

RUN mkdir -p src \
    && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Bring over the real code to compile
COPY src/ src/

# Touch main.rs to invalidate the cache marker and rebuild the actual target
RUN touch src/main.rs && cargo build --release

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
