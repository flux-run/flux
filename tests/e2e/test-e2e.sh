#!/usr/bin/env bash

set -e

# --- Configuration ---
E2E_DIR="/tmp/flux-e2e"
SERVICE_TOKEN="modern-e2e-token-888"
APP_PORT=3002
GRPC_PORT=50053
REDIS_PORT=6379

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# Allow local connections for E2E
export FLOWBASE_ALLOW_LOOPBACK_REDIS=1
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1
export FLOWBASE_ALLOW_LOOPBACK_TCP=1

echo "========================================"
echo "🚀 Flux Ultimate E2E (MODERN STACK)"
echo "   Hono + Zod + Drizzle + Redis"
echo "========================================"

# 1. Setup Workspace
echo "👉 Cleaning up and creating workspace: $E2E_DIR"
rm -rf "$E2E_DIR"
mkdir -p "$E2E_DIR"
cd "$E2E_DIR"

echo "👉 Clearing ports $GRPC_PORT, $APP_PORT, and $REDIS_PORT..."
lsof -ti :$GRPC_PORT | xargs kill -9 2>/dev/null || true
lsof -ti :$APP_PORT | xargs kill -9 2>/dev/null || true
docker stop flux-e2e-redis > /dev/null 2>&1 || true
docker rm flux-e2e-redis > /dev/null 2>&1 || true

# 2. Infrastructure (Redis + DB URL)
echo "👉 Ensuring Redis container is running..."
docker run -d --name flux-e2e-redis -p $REDIS_PORT:6379 --rm redis:alpine > /dev/null 2>&1 || true
REDIS_URL="redis://localhost:$REDIS_PORT"

echo "👉 Extracting DATABASE_URL..."
if [ -f /Users/shashisharma/code/my-app/.env ]; then
  DB_URL=$(grep "^DATABASE_URL=" /Users/shashisharma/code/my-app/.env | cut -d'=' -f2-)
else
  DB_URL=$DATABASE_URL
fi

# 3. Flux Init
echo "👉 Initializing new Flux project..."
flux init

# 4. Starting Flux Server
echo "👉 Starting Flux Server (port $GRPC_PORT)..."
flux server start --port "$GRPC_PORT" --service-token "$SERVICE_TOKEN" --database-url "$DB_URL" > flux-server.log 2>&1 &
SERVER_PID=$!

echo "👉 Waiting for server..."
for i in {1..20}; do
  if lsof -i :$GRPC_PORT > /dev/null; then
    echo "✅ Server is ready."
    break
  fi
  sleep 1
done

# 5. Authenticate CLI
flux auth --url "http://localhost:$GRPC_PORT" --token "$SERVICE_TOKEN" --skip-verify

# 6. Inject Modern Application Code
echo "👉 Injecting Modern Stack code (Hono + Zod + Drizzle + Redis)..."
mkdir -p src
cat > src/index.ts <<EOF
// @ts-nocheck
import { Hono } from "https://esm.sh/hono@3.11.7";
import { z } from "https://esm.sh/zod@3.22.4";
import { drizzle } from "https://esm.sh/drizzle-orm@0.31.0/pg-proxy";
import { pgTable, serial, text, integer } from "https://esm.sh/drizzle-orm@0.31.0/pg-core";
import pg from "flux:pg";
import { createClient } from "flux:redis";

const app = new Hono();

// 1. Drizzle Schema
const products = pgTable("products", {
  id: serial("id").primaryKey(),
  name: text("name").notNull(),
  price: integer("price").notNull(),
});

// 2. Client Setup
const DB_URL = Deno.env.get("DATABASE_URL") || "$DB_URL";
const pool = new pg.Pool({ connectionString: DB_URL });
const redis = createClient({ url: "$REDIS_URL" });

// 3. Drizzle Proxy Adapter
const db = drizzle(async (sql, params, method) => {
  try {
    const res = await pool.query(sql, params);
    return { rows: res.rows };
  } catch (e) {
    console.error("❌ Drizzle Proxy Error:", e);
    throw e;
  }
});

// 4. Zod Validation
const CreateProductSchema = z.object({
  name: z.string().min(1),
  price: z.number().positive().int(),
});

// 5. Routes
app.post("/products", async (c) => {
  const body = await c.req.json();
  const result = CreateProductSchema.safeParse(body);
  
  if (!result.success) {
    return c.json({ error: "Validation Failed", details: result.error }, 400);
  }
  
  const { name, price } = result.data;
  
  // Drizzle Insert
  const res = await db.insert(products).values({ name, price }).returning();
  const newProduct = res[0];
  
  // Clear redis cache
  await redis.del("flux:e2e:all_products");
  
  return c.json(newProduct);
});

app.get("/products", async (c) => {
  // Cache check
  const cached = await redis.get("flux:e2e:all_products");
  if (cached) {
    return c.json({ data: JSON.parse(cached), source: "cache" });
  }
  
  // Drizzle Select
  const allProducts = await db.select().from(products).orderBy(products.id);
  
  // Update cache
  await redis.set("flux:e2e:all_products", JSON.stringify(allProducts), { EX: 60 });
  
  return c.json({ data: allProducts, source: "database" });
});

// Init Table
app.get("/init", async (c) => {
  await pool.query(\`CREATE TABLE IF NOT EXISTS products (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    price INTEGER NOT NULL
  )\`);
  return c.json({ ok: true });
});

Deno.serve({ port: 3002 }, app.fetch);
EOF

# 7. Build & Run
echo "👉 Building modern application..."
flux build

echo "👉 Starting modern app on port $APP_PORT..."
flux run --artifact src/.flux/artifact.json --port "$APP_PORT" > flux-app.log 2>&1 &
APP_PID=$!

echo "👉 Waiting for app..."
for i in {1..20}; do
  if lsof -i :$APP_PORT > /dev/null; then
    echo "✅ App is ready."
    break
  fi
  sleep 1
done

# 8. Verify End-to-End Flow
echo "👉 Initializing database table..."
curl -s "http://localhost:$APP_PORT/init" | jq

echo "👉 Testing Zod Validation (Failure Case)..."
curl -s -X POST "http://localhost:$APP_PORT/products" -H "Content-Type: application/json" -d '{"name": "", "price": -10}' | jq

echo "👉 Creating Product (Hono + Drizzle + Zod)..."
PRODUCT=$(curl -s -X POST "http://localhost:$APP_PORT/products" -H "Content-Type: application/json" -d '{"name": "Flux Core", "price": 999}')
echo "$PRODUCT" | jq

echo "👉 Fetching Products (Source: Database)..."
curl -s "http://localhost:$APP_PORT/products" | jq

echo "👉 Fetching Products (Source: Redis Cache)..."
curl -s "http://localhost:$APP_PORT/products" | jq

# 9. Verify Observability
echo "👉 Auditing observability for latest execution..."
# Skip header and any footer text, extract last column of the actual log line, and remove any whitespace/newlines
EXEC_ID=$(flux logs --limit 1 | grep -v "TIME" | grep -v "showing" | awk '{print $NF}' | tr -d '[:space:]')

if [ -n "$EXEC_ID" ]; then
    echo "👉 Running flux trace for $EXEC_ID..."
    flux trace "$EXEC_ID" --verbose | head -n 30
    
    echo "👉 Verifying replay-safety (safe-caching)..."
    flux replay "$EXEC_ID"
fi

# 10. Cleanup
echo "👉 Shutting down Modern E2E processes..."
kill $APP_PID 2>/dev/null || true
kill $SERVER_PID 2>/dev/null || true
docker stop flux-e2e-redis > /dev/null 2>&1 || true

echo "========================================"
echo -e "${GREEN}✅ Modern Stack E2E Complete${NC}"
echo "========================================"
