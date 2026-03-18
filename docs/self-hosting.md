# Self-Hosting Flux

Flux consists of one Postgres database and one orchestration server binary. There are no external caching, message-queue, or service mesh dependencies.

## 1. With Docker (Quickstart)

The fastest and most common way to run Flux in production or locally is using `docker-compose`.

1. Download the reference `docker-compose.yml` file from the repository, or simply curl it:
   ```bash
   curl -fsSL https://raw.githubusercontent.com/flux-run/flux/main/docker-compose.yml -o docker-compose.yml
   ```
2. Start the database and server in the background:
   ```bash
   docker-compose up -d
   ```
3. Deploy your Flux functions from your CLI, pointing to the default local address:
   ```bash
   flux init my-app && cd my-app
   flux dev
   ```

*Note: The `flux` service container will auto-run schema migrations on startup.*

---

## 2. Without Docker (Single Binary)

If you prefer to run Flux directly on the host machine or a bare VPS (like an EC2 instance or Droplet), you just need the binary and a Postgres connection URL.

1. Ensure you have a running PostgreSQL 14+ database instance.
2. Install the Flux CLI and server binaries:
   ```bash
   curl -fsSL https://fluxbase.co/install | bash
   ```
3. Set your internal database connection string:
   ```bash
   export DATABASE_URL=postgres://user:password@localhost:5432/flux
   ```
4. Start the server daemon:
   ```bash
   flux server start
   ```

*You can also override the port the server listens to using `export PORT=8080` (defaults to 4000).*
