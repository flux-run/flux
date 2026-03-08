.PHONY: dev api dashboard build migrate install clean

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
	./scripts/build.sh

# Builds all services as Docker images.
build-docker:
	./scripts/build.sh --docker

# ── Deploy ──────────────────────────────────────────────────────────────────
# Deploys all services to the specified environment.
# Usage: make deploy ENV=staging
deploy:
	./scripts/deploy.sh --env $(ENV)

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
