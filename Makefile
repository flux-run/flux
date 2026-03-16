.PHONY: dev api server build migrate install install-cli install-server clean test-async-wiring test-platform test-product-loop test-system deploy-with-migrate generate-types

# ── Full stack ──────────────────────────────────────────────────────────────
# Starts API.
dev:
	@echo "Starting Flux dev API…"
	@make api

# ── Individual services ─────────────────────────────────────────────────────
api:
	cd api && SQLX_OFFLINE=true cargo run

# Monolith server.
server:
	SQLX_OFFLINE=true cargo build -p server

# ── Build (production artefacts) ────────────────────────────────────────────
# Builds all services using the new build script.
build:
	./scripts/build.sh $(if $(SERVICE),--service $(SERVICE))

# Builds all services as Docker images.
build-docker:
	./scripts/build.sh --docker $(if $(SERVICE),--service $(SERVICE))

build-gcp:
	./scripts/build.sh --docker --platform linux/amd64 --registry asia-south1-docker.pkg.dev/fluxbase-app/fluxbase $(if $(SERVICE),--service $(SERVICE)) --parallel

# ── Deploy ──────────────────────────────────────────────────────────────────
# Deploys all services to the specified environment.
# Usage: make deploy ENV=staging
deploy:
	./scripts/deploy.sh --env production $(if $(SERVICE),--service $(SERVICE))

deploy-gcp:
	TAG=$$(git rev-parse --short HEAD); \
	./scripts/build.sh --docker --platform linux/amd64 --registry asia-south1-docker.pkg.dev/fluxbase-app/fluxbase --tag $$TAG $(if $(SERVICE),--service $(SERVICE)); \
	./scripts/deploy.sh --env production --project fluxbase-app --region asia-south1 --platform linux/amd64 --tag $$TAG $(if $(SERVICE),--service $(SERVICE))

# Full deploy workflow: migrate DB first, then build + push + deploy.
# Use this whenever your commit includes new migration files.
# Usage: make deploy-with-migrate SERVICE=api
deploy-with-migrate:
	$(MAKE) migrate
	$(MAKE) deploy-gcp SERVICE=$(SERVICE)

# Deploys with a dry-run.
deploy-dry-run:
	./scripts/deploy.sh --env $(ENV) --dry-run


# ── Database ─────────────────────────────────────────────────────────────────
# Regenerate sqlx offline cache — run against direct (non-pooler) DB connection.
# Usage: make sqlx-prepare DB_URL="postgresql://..."
sqlx-prepare:
	cd api && DATABASE_URL="$(DB_URL)" cargo sqlx prepare

migrate:
	sqlx migrate run --source schemas/api --ignore-missing

# ── Setup ────────────────────────────────────────────────────────────────────
install:
	@echo "No Node install step required."

# Install flux CLI and server binaries to ~/.cargo/bin
install-local:
	./scripts/install-local.sh

install-cli:
	./scripts/install-local.sh --cli

install-server:
	./scripts/install-local.sh --server

# ── Clean ────────────────────────────────────────────────────────────────────
clean:
	cd api && cargo clean

# ── API Types (TypeScript codegen) ──────────────────────────────────────────
# Generates TypeScript bindings from Rust types via ts-rs.
# Output: shared/api_contract/bindings/*.ts
generate-types:
	cargo test -p api_contract --features ts
	@echo "TypeScript bindings written to shared/api_contract/bindings/"

# ── Async Wiring Test ───────────────────────────────────────────────────────
# Runs deterministic staging wiring test for API/Runtime queue execution path.
# Required env vars are documented in scripts/test_async_wiring.sh
test-async-wiring:
	./scripts/test_async_wiring.sh

test-platform:
	./scripts/platform-tests/run_all.sh

test-product-loop:
	./scripts/platform-tests/execution_record_test.sh
	./scripts/platform-tests/state_audit_test.sh

test-system:
	./scripts/platform-tests/run_all.sh
