.PHONY: dev api dashboard build migrate install clean test-async-wiring

# ── Full stack ──────────────────────────────────────────────────────────────
# Starts API + dashboard in parallel, printing labelled output.
dev:
	@echo "Starting Fluxbase dev stack…"
	@make -j2 api dashboard

# ── Individual services ─────────────────────────────────────────────────────
api:
	cd api && SQLX_OFFLINE=true cargo run

dashboard:
	cd dashboard && npm run dev

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
	./scripts/deploy.sh --env production --project fluxbase-app --region asia-south1 $(if $(SERVICE),--service $(SERVICE))

# Deploys with a dry-run.
deploy-dry-run:
	./scripts/deploy.sh --env $(ENV) --dry-run


# ── Database ─────────────────────────────────────────────────────────────────
# Regenerate sqlx offline cache — run against direct (non-pooler) DB connection.
# Usage: make sqlx-prepare DB_URL="postgresql://..."
sqlx-prepare:
	cd api && DATABASE_URL="$(DB_URL)" cargo sqlx prepare

migrate:
	cd api && sqlx migrate run

# ── Setup ────────────────────────────────────────────────────────────────────
install:
	cd dashboard && npm install

# ── Clean ────────────────────────────────────────────────────────────────────
clean:
	cd api && cargo clean
	cd dashboard && rm -rf dist node_modules/.vite

# ── Async Wiring Test ───────────────────────────────────────────────────────
# Runs deterministic staging wiring test for Gateway -> Queue -> Worker -> Runtime.
# Required env vars are documented in scripts/test_async_wiring.sh
test-async-wiring:
	./scripts/test_async_wiring.sh
