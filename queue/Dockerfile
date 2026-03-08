# Build Stage
FROM rust:1.93-bookworm AS builder

WORKDIR /usr/src/app
COPY . .

# Build the Queue binary
ENV SQLX_OFFLINE=true
RUN cargo build --release -p queue

# Final Stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/queue .

# Set the environment variables
ENV PORT=8080
EXPOSE 8080

# Run the Queue service
CMD ["./queue"]
