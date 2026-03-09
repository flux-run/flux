# Build Stage
FROM rust:1.93-bookworm AS builder

WORKDIR /usr/src/app
COPY . .

# Build the Data Engine binary
ENV SQLX_OFFLINE=true
RUN cargo build --release -p data-engine

# Final Stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/data-engine .

# Set the environment variables and expose port
ENV PORT=8080
EXPOSE 8080

# Run the Data Engine
CMD ["./data-engine"]
