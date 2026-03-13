# syntax=docker/dockerfile:1
#
# Single-binary Flux server — all five services in one process, one port.
# Build context: workspace root (pass via `docker build -f server/Dockerfile .`)

# ── Build stage ───────────────────────────────────────────────────────────────
FROM rust:1.93-bookworm AS builder

WORKDIR /usr/src/app
COPY . .

# Build the monolithic server binary.
ENV SQLX_OFFLINE=true
RUN cargo build --release -p server

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends libssl3 ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin
COPY --from=builder /usr/src/app/target/release/server ./flux-server

# Default port for the monolithic server.
ENV PORT=4000
EXPOSE 4000

CMD ["./flux-server"]
