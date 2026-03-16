#!/usr/bin/env python3
"""
Replace every hardcoded API URL format!() string in CLI source files
with the corresponding api_contract::routes constant.

Usage:
    python3 scripts/migrate_routes.py [--dry-run]
"""

import re
import sys
import os

DRY_RUN = "--dry-run" in sys.argv
CLI = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "cli", "src")

R = "api_contract::routes"

# ---------------------------------------------------------------------------
# Mapping: exact old string -> new string
# Keys are the exact text that appears inside format!(...) arguments,
# i.e. the first two items: the template string + the base_url argument.
# We match the full format!(...) expression.
# ---------------------------------------------------------------------------

# ── helpers ─────────────────────────────────────────────────────────────────

def url(mod, const, base="client.base_url"):
    return f"{R}::{mod}::{const}.url(&{base})"

def url_with(mod, const, base, *pairs):
    ps = ", ".join(f'("{k}", {v})' for k, v in pairs)
    return f'{R}::{mod}::{const}.url_with(&{base}, &[{ps}])'

# ── simple replacements (no path params) ────────────────────────────────────

SIMPLE = {
    # api-keys
    'format!("{}/api-keys", client.base_url)':
        url("api_keys", "LIST"),
    # auth / health
    'format!("{}/auth/status", client.base_url)':
        url("auth", "STATUS"),
    'format!("{}/health", client.base_url)':
        url("health", "HEALTH"),
    'format!("{}/auth/setup", client.base_url)':
        url("auth", "SETUP"),
    'format!("{}/auth/login", client.base_url)':
        url("auth", "LOGIN"),
    'format!("{}/auth/me", client.base_url)':
        url("auth", "ME"),
    # environments
    'format!("{}/environments", client.base_url)':
        url("environments", "LIST"),
    'format!("{}/environments/clone", client.base_url)':
        url("environments", "CLONE"),
    # events
    'format!("{}/events", client.base_url)':
        url("events", "PUBLISH"),
    'format!("{}/events/subscriptions", client.base_url)':
        url("events", "SUBSCRIPTIONS_LIST"),
    # functions
    'format!("{}/functions", client.base_url)':
        url("functions", "LIST"),
    # gateway
    'format!("{}/gateway/routes", client.base_url)':
        url("gateway", "ROUTES_LIST"),
    'format!("{}/gateway/middleware", client.base_url)':
        url("gateway", "MIDDLEWARE_CREATE"),
    # monitor
    'format!("{}/monitor/status", client.base_url)':
        url("monitor", "STATUS"),
    'format!("{}/monitor/alerts", client.base_url)':
        url("monitor", "ALERTS_LIST"),
    # queues
    'format!("{}/queues", client.base_url)':
        url("queues", "LIST"),
    # records
    'format!("{}/records/export", client.base_url)':
        url("records", "EXPORT"),
    'format!("{}/records/count", client.base_url)':
        url("records", "COUNT"),
    'format!("{}/records/prune", client.base_url)':
        url("records", "PRUNE"),
    # schedules
    'format!("{}/schedules", client.base_url)':
        url("schedules", "LIST"),
    # sdk
    'format!("{}/sdk/schema", client.base_url)':
        url("sdk", "SDK_SCHEMA"),
    'format!("{}/sdk/typescript", client.base_url)':
        url("sdk", "SDK_TS"),
    'format!("{}/sdk/manifest", client.base_url)':
        url("sdk", "MANIFEST"),
    # secrets (uses api_url)
    'format!("{}/secrets", api_url)':
        url("secrets", "LIST", "api_url"),
    # db proxy
    'format!("{}/db/databases", client.base_url)':
        url("db", "DATABASES_LIST"),
    'format!("{}/db/tables", client.base_url)':
        url("db", "TABLES_CREATE"),
    'format!("{}/db/query", client.base_url)':
        url("db", "QUERY"),
    'format!("{}/db/explain", client.base_url)':
        url("db", "EXPLAIN"),
    # tenants
    'format!("{}/tenants", client.base_url)':
        url("tenants", "LIST"),
    # deployments
    'format!("{}/deployments", client.base_url)':
        url("deployments", "CREATE"),
}

# ── single path-param replacements ──────────────────────────────────────────
# Pattern: format!("{}/path/{}", base, var)  →  CONST.url_with(&base, &[("param", var.as_str())])

SINGLE_PARAM = [
    # api-keys
    (r'format!\("{}/api-keys/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("api_keys", "DELETE", "client.base_url", ("id", f"&{m.group(1)}"))),
    (r'format!\("{}/api-keys/\{\}/rotate", client\.base_url, (\w+)\)',
     lambda m: url_with("api_keys", "ROTATE", "client.base_url", ("id", f"&{m.group(1)}"))),
    # environments
    (r'format!\("{}/environments/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("environments", "DELETE", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    # events subscriptions
    (r'format!\("{}/events/subscriptions/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("events", "SUBSCRIPTIONS_DELETE", "client.base_url", ("id", f"{m.group(1)}.as_str()"))),
    # functions
    (r'format!\("{}/functions/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("functions", "GET", "client.base_url", ("id", f"&{m.group(1)}"))),
    (r'format!\("{}/functions/\{\}/deployments", client\.base_url, (\w+)\)',
     lambda m: url_with("functions", "DEPLOYMENTS_LIST", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    # gateway routes  
    (r'format!\("{}/gateway/routes/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("gateway", "ROUTES_GET", "client.base_url", ("id", f"&{m.group(1)}"))),
    (r'format!\("{}/gateway/routes/\{\}/rate-limit", client\.base_url, (\w+)\)',
     lambda m: url_with("gateway", "RATE_LIMIT_SET", "client.base_url", ("id", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/gateway/routes/\{\}/cors", client\.base_url, (\w+)\)',
     lambda m: url_with("gateway", "CORS_SET", "client.base_url", ("id", f"{m.group(1)}.as_str()"))),
    # monitor alerts
    (r'format!\("{}/monitor/alerts/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("monitor", "ALERTS_DELETE", "client.base_url", ("id", f"&{m.group(1)}"))),
    # queues
    (r'format!\("{}/queues/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("queues", "GET", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/queues/\{\}/messages", client\.base_url, (\w+)\)',
     lambda m: url_with("queues", "PUBLISH", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/queues/\{\}/bindings", client\.base_url, (\w+)\)',
     lambda m: url_with("queues", "BINDINGS_LIST", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/queues/\{\}/purge", client\.base_url, (\w+)\)',
     lambda m: url_with("queues", "PURGE", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/queues/\{\}/dlq", client\.base_url, (\w+)\)',
     lambda m: url_with("queues", "DLQ_LIST", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/queues/\{\}/dlq/replay", client\.base_url, (\w+)\)',
     lambda m: url_with("queues", "DLQ_REPLAY", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    # schedules
    (r'format!\("{}/schedules/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("schedules", "DELETE", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/schedules/\{\}/pause", client\.base_url, (\w+)\)',
     lambda m: url_with("schedules", "PAUSE", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/schedules/\{\}/resume", client\.base_url, (\w+)\)',
     lambda m: url_with("schedules", "RESUME", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/schedules/\{\}/run", client\.base_url, (\w+)\)',
     lambda m: url_with("schedules", "RUN", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/schedules/\{\}/history", client\.base_url, (\w+)\)',
     lambda m: url_with("schedules", "HISTORY", "client.base_url", ("name", f"{m.group(1)}.as_str()"))),
    # secrets (api_url)
    (r'format!\("{}/secrets/\{\}", api_url, (\w+)\)',
     lambda m: url_with("secrets", "UPDATE", "api_url", ("key", f"{m.group(1)}.as_str()"))),
    # traces
    (r'format!\("{}/traces/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("logs", "TRACE_GET", "client.base_url", ("request_id", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/traces/\{\}/replay", client\.base_url, (\w+)\)',
     lambda m: url_with("logs", "TRACE_REPLAY", "client.base_url", ("request_id", f"{m.group(1)}.as_str()"))),
    # db proxy
    (r'format!\("{}/db/tables/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("db", "TABLES_LIST", "client.base_url", ("database", f"{m.group(1)}.as_str()"))),
    (r'format!\("{}/deployments/list/\{\}", client\.base_url, (\w+)\)',
     lambda m: url_with("deployments", "LIST", "client.base_url", ("id", f"{m.group(1)}.as_str()"))),
]

# ── two path-param replacements ──────────────────────────────────────────────

TWO_PARAM = [
    # gateway middleware delete
    (r'format!\("{}/gateway/middleware/\{\}/\{\}", client\.base_url, (\w+), (r#\w+|\w+)\)',
     lambda m: url_with("gateway", "MIDDLEWARE_DELETE", "client.base_url",
                        ("route", f"{m.group(1)}.as_str()"),
                        ("type",  f"{m.group(2)}.as_str()"))),
    # db history
    (r'format!\("{}/db/history/\{\}/\{\}", client\.base_url, (\w+), (\w+)\)',
     lambda m: url_with("db", "HISTORY", "client.base_url",
                        ("database", f"{m.group(1)}.as_str()"),
                        ("table",    f"{m.group(2)}.as_str()"))),
    # db blame
    (r'format!\("{}/db/blame/\{\}/\{\}", client\.base_url, (\w+), (\w+)\)',
     lambda m: url_with("db", "BLAME", "client.base_url",
                        ("database", f"{m.group(1)}.as_str()"),
                        ("table",    f"{m.group(2)}.as_str()"))),
    # version_cmd two-param
    (r'format!\("{}/functions/\{\}/deployments/\{\}", client\.base_url, (\w+), (\w+)\)',
     lambda m: url_with("functions", "DEPLOYMENTS_GET", "client.base_url",
                        ("name",    f"{m.group(1)}.as_str()"),
                        ("version", f"{m.group(2)}.as_str()"))),
    (r'format!\("{}/functions/\{\}/deployments/\{\}/activate", client\.base_url, (\w+), (\w+)\)',
     lambda m: url_with("functions", "DEPLOYMENTS_ACTIVATE", "client.base_url",
                        ("name",    f"{m.group(1)}.as_str()"),
                        ("version", f"{m.group(2)}.as_str()"))),
    (r'format!\("{}/functions/\{\}/deployments/\{\}/promote", client\.base_url, (\w+), (\w+)\)',
     lambda m: url_with("functions", "DEPLOYMENTS_PROMOTE", "client.base_url",
                        ("name",    f"{m.group(1)}.as_str()"),
                        ("version", f"{m.group(2)}.as_str()"))),
]

# ── files list ───────────────────────────────────────────────────────────────

FILES = [
    "api_key.rs",
    "auth.rs",
    "debug.rs",
    "deployments.rs",
    "env_cmd.rs",
    "event.rs",
    "explain.rs",
    "functions.rs",
    "gateway.rs",
    "generate.rs",
    "incident.rs",
    "logs.rs",
    "monitor.rs",
    "queue.rs",
    "records.rs",
    "schedule.rs",
    "sdk.rs",
    "secrets.rs",
    "tail.rs",
    "tenant.rs",
    "trace.rs",
    "trace_debug.rs",
    "trace_diff.rs",
    "version_cmd.rs",
    "whoami.rs",
    "why.rs",
    "doctor.rs",
    "db.rs",
]


def process_file(path):
    with open(path, "r") as f:
        src = f.read()

    original = src
    changes = 0

    # 1. Simple exact replacements
    for old, new in SIMPLE.items():
        if old in src:
            src = src.replace(old, new)
            changes += src.count(new) - original.count(new)

    # 2. Single-param regex replacements
    for pattern, fn in SINGLE_PARAM:
        new_src, n = re.subn(pattern, fn, src)
        if n:
            src = new_src
            changes += n

    # 3. Two-param regex replacements
    for pattern, fn in TWO_PARAM:
        new_src, n = re.subn(pattern, fn, src)
        if n:
            src = new_src
            changes += n

    if src != original:
        # Add import at top of file if not present
        if "use api_contract::routes" not in src:
            # Insert after the last `use crate::...;` line or first `use ` line
            lines = src.split("\n")
            insert_at = 0
            for i, line in enumerate(lines):
                if line.startswith("use "):
                    insert_at = i + 1
            lines.insert(insert_at, "use api_contract::routes as R;")
            src = "\n".join(lines)
            # Update the R:: references if we added "R"
            # (but we wrote them as api_contract::routes:: in simple map, fix to R::)
            src = src.replace("api_contract::routes::", "R::")

        if DRY_RUN:
            print(f"[DRY-RUN] {path}: {changes} change(s)")
        else:
            with open(path, "w") as f:
                f.write(src)
            print(f"✓ {os.path.basename(path)}: rewrote {changes} URL(s)")
    else:
        print(f"  {os.path.basename(path)}: no changes")


def main():
    for fname in FILES:
        fpath = os.path.join(CLI, fname)
        if os.path.exists(fpath):
            process_file(fpath)
        else:
            print(f"  {fname}: not found, skipping")


if __name__ == "__main__":
    main()
