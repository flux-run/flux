#!/usr/bin/env python3
"""
scripts/setup_demo.py — One-shot setup for the landing page demo.

Creates:
  - Tenant  "Demo Org"
  - Project "Demo Project"  under that tenant
  - Function "create_user"  (deno runtime)  under that project
  - Active deployment with the code from test_functions/create_user.js

Then patches api/env.yaml with the generated DEMO_TENANT_SLUG.

Usage:
  python3 scripts/setup_demo.py
"""

import os, sys, uuid, re, datetime
import urllib.request, urllib.error, json

# ── Resolve repo root ──────────────────────────────────────────────────────────
SCRIPT_DIR  = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT   = os.path.dirname(SCRIPT_DIR)

def load_env_yaml(path):
    result = {}
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line.startswith("#") or ":" not in line:
                continue
            key, _, val = line.partition(":")
            val = val.strip().strip('"')
            result[key.strip()] = val
    return result

API_ENV   = load_env_yaml(os.path.join(REPO_ROOT, "api", "env.yaml"))
DB_URL    = API_ENV["DATABASE_URL"]

FUNCTION_CODE = open(
    os.path.join(REPO_ROOT, "test_functions", "create_user.js"), "rb"
).read()

# ── DB (psycopg2) ──────────────────────────────────────────────────────────────
try:
    import psycopg2
    import psycopg2.extras
except ImportError:
    print("Installing psycopg2-binary …")
    os.system(f"{sys.executable} -m pip install psycopg2-binary -q")
    import psycopg2
    import psycopg2.extras

conn = psycopg2.connect(DB_URL)
conn.autocommit = True
cur  = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)

def pg(sql, *args):
    cur.execute(sql, args)
    try:    return cur.fetchall()
    except: return []

def pg1(sql, *args):
    rows = pg(sql, *args)
    return rows[0] if rows else None

# ── Slug helper ────────────────────────────────────────────────────────────────
import random, string

def generate_slug(name: str) -> str:
    """Mirrors slug_service::generate_slug in Rust."""
    base = "".join(c if c.isalnum() else "-" for c in name.lower() if c.isascii())
    parts = [p for p in base.split("-") if p]
    trimmed = "-".join(parts) if parts else ""
    suffix = "".join(random.choices(string.ascii_lowercase + string.digits, k=6))
    if not trimmed:
        return f"demo-{suffix}-org"
    return f"{trimmed}-{suffix}-org"

# ─────────────────────────────────────────────────────────────────────────────
print("=== Flux Demo Setup ===\n")

# 1. Tenant
row = pg1("SELECT id, slug FROM tenants WHERE name = 'Demo Org' LIMIT 1")
if row:
    tenant_id   = row["id"]
    tenant_slug = row["slug"]
    print(f"[skip] Tenant already exists: {tenant_slug}  ({tenant_id})")
else:
    tenant_id   = uuid.uuid4()
    tenant_slug = generate_slug("Demo Org")
    # Use a fixed system user for owner (pick first user in DB)
    owner_row = pg1("SELECT id FROM users LIMIT 1")
    owner_id  = owner_row["id"] if owner_row else uuid.uuid4()
    pg("INSERT INTO tenants (id, name, slug, owner_id) VALUES (%s, %s, %s, %s)",
       tenant_id, "Demo Org", tenant_slug, owner_id)
    pg("INSERT INTO tenant_members (tenant_id, user_id, role) VALUES (%s, %s, 'owner') ON CONFLICT DO NOTHING",
       tenant_id, owner_id)
    print(f"[create] Tenant:  Demo Org  slug={tenant_slug}  id={tenant_id}")

# 2. Project
row = pg1("SELECT id FROM projects WHERE tenant_id = %s AND name = 'Demo Project' LIMIT 1", tenant_id)
if row:
    project_id = row["id"]
    print(f"[skip] Project already exists: {project_id}")
else:
    project_id   = uuid.uuid4()
    project_slug = generate_slug("Demo Project")
    pg("INSERT INTO projects (id, tenant_id, name, slug) VALUES (%s, %s, %s, %s)",
       project_id, tenant_id, "Demo Project", project_slug)
    print(f"[create] Project: Demo Project  slug={project_slug}  id={project_id}")

# 3. Function
row = pg1("SELECT id FROM functions WHERE tenant_id = %s AND project_id = %s AND name = 'create_user' LIMIT 1",
          tenant_id, project_id)
if row:
    function_id = row["id"]
    print(f"[skip] Function already exists: {function_id}")
else:
    function_id = uuid.uuid4()
    pg("INSERT INTO functions (id, tenant_id, project_id, name, runtime) VALUES (%s, %s, %s, %s, 'deno')",
       function_id, tenant_id, project_id, "create_user")
    print(f"[create] Function: create_user  id={function_id}")

# 4. Deployment
deployment_id = uuid.uuid4()
storage_key   = f"deployments/{function_id}_{deployment_id}.js"
# Deactivate any existing
pg("UPDATE deployments SET is_active = false WHERE function_id = %s", function_id)

row = pg1("SELECT COALESCE(MAX(version),0) as v FROM deployments WHERE function_id = %s", function_id)
next_version = (row["v"] if row else 0) + 1

bundle_code_str = FUNCTION_CODE.decode("utf-8")
pg("""INSERT INTO deployments
       (id, function_id, storage_key, bundle_code, version, status, is_active)
     VALUES (%s, %s, %s, %s, %s, 'ready', true)""",
   deployment_id, function_id, storage_key,
   bundle_code_str,
   next_version)
print(f"[deploy] Deployment v{next_version}: {deployment_id}  active=true")

# 6. Gateway route — ensure POST /create_user routes to this function
row = pg1("""SELECT id FROM routes
             WHERE project_id = %s AND path = '/create_user' AND method = 'POST'
             LIMIT 1""", project_id)
if row:
    pg("UPDATE routes SET function_id = %s WHERE id = %s", function_id, row["id"])
    print("[upsert] Gateway route /create_user → create_user")
else:
    route_id = uuid.uuid4()
    pg("""INSERT INTO routes (id, project_id, function_id, path, method, auth_type, cors_enabled)
          VALUES (%s, %s, %s, '/create_user', 'POST', 'none', false)""",
       route_id, project_id, function_id)
    print(f"[create] Gateway route: POST /create_user → create_user  id={route_id}")

# 7. Patch api/env.yaml
env_path = os.path.join(REPO_ROOT, "api", "env.yaml")
content  = open(env_path).read()
new_content = re.sub(
    r'^DEMO_TENANT_SLUG:\s*"[^"]*"',
    f'DEMO_TENANT_SLUG: "{tenant_slug}"',
    content,
    flags=re.MULTILINE,
)
open(env_path, "w").write(new_content)
print(f'\n[patch] api/env.yaml  DEMO_TENANT_SLUG="{tenant_slug}"')

cur.close()
conn.close()

print("\n=== DONE ===")
print(f"\nTenant slug: {tenant_slug}")
print(f"Tenant ID:   {tenant_id}")
print(f"Project ID:  {project_id}")
print(f"Function ID: {function_id}")
print(f"\nNext steps:")
print(f"  1. Connect Outlook in Composio dashboard → entity: flux-demo")
print(f"  2. Deploy api:  make deploy-gcp SERVICE=api")
print(f"  3. Set COMPOSIO_API_KEY on Cloud Run if not already set")
print(f"  4. Test: curl -s -X POST https://api.fluxbase.co/demo/signup \\")
print(f"              -H 'Content-Type: application/json' \\")
print(f"              -d '{{\"name\":\"Alex\",\"email\":\"you@example.com\"}}' | python3 -m json.tool")
