# Local E2E Testing Guide

To verify the E2E suite locally on your Mac, you have two options:

## Option 1: Native (Fastest)

This runs the core E2E flow against your local environment (no Docker).

1. **Build the CLI/Server/Runtime**:
   ```bash
   cargo build
   ```
2. **Run the Test Flow**:
   ```bash
   ./tests/e2e/test-flow.sh
   ```
   *Note: This script automatically starts the server, building the project, and runs a series of `exec`, `run`, `logs`, and `replay` commands.*

---

## Option 2: Docker Compose (CI Parity)

This replicates the CI environment exactly. Since you are on Mac, the binaries must be built for Linux.

1. **Build Linux Binaries** (requires `cross` or a Linux builder):
   ```bash
   # If you have 'cross' installed:
   cross build --target x86_64-unknown-linux-gnu --release
   ```
   *Note: If you don't have cross-compilation set up, it's easier to let the CI handle the Docker build.*

2. **Build the E2E Image**:
   ```bash
   docker build -t flux-e2e:ci -f scripts/e2e/Dockerfile.e2e . \
     --build-arg FLUX_BIN=target/x86_64-unknown-linux-gnu/release/flux \
     --build-arg FLUX_SERVER_BIN=target/x86_64-unknown-linux-gnu/release/flux-server \
     --build-arg FLUX_RUNTIME_BIN=target/x86_64-unknown-linux-gnu/release/flux-runtime
   ```

3. **Run the Suite**:
   ```bash
   # Minimal (Postgres only)
   docker compose -f scripts/e2e/docker-compose.minimal.yml run --rm e2e

   # Full (Postgres + Redis)
   docker compose -f scripts/e2e/docker-compose.full.yml run --rm e2e
   ```

---

## Option 3: Full Docker Build (Easiest CI Parity)

This builds the entire project **inside** a Docker container, ensuring Linux compatibility without needing local cross-compilation tools.

1. **Run everything with one command**:
   ```bash
   docker compose -f scripts/e2e/docker-compose.local.yml run --rm e2e
   ```
   *Note: This will build the binaries inside the container, which might take a few minutes the first time, but it's the most reliable "one-click" verification.*

---

## Recommendation
- Use **Option 1** for rapid iteration on your code logic.
- Use **Option 3** for a final "pre-flight" check before pushing, as it matches the CI environment exactly.
