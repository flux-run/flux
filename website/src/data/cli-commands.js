/**
 * CLI commands reference data.
 * Used by the CLI reference page and docs.
 */
export const CLI_COMMANDS = [
  {
    cmd:     'flux deploy',
    summary: 'Deploy functions to production',
    desc:    'Bundles all functions in the current project, uploads them, and makes them live behind the gateway in ~20 seconds. Returns a deploy ID and the public URL for each function.',
    example: `$ flux deploy

  Deploying 3 functions…

  ✔  create_user   → gw.fluxbase.co/create_user
  ✔  list_users    → gw.fluxbase.co/list_users
  ✔  send_welcome  (async)

  ✔  Deployed in 18s  deploy:d_7f3a9`,
  },
  {
    cmd:     'flux tail',
    summary: 'Stream live request logs',
    desc:    'Opens a real-time log stream from the gateway. Every incoming request prints its method, path, status, and latency. Errors are highlighted red. Press Ctrl-C to stop.',
    example: `$ flux tail

  Streaming logs…

  ✔  POST /create_user   201   98ms  req:4f9a3b2c
  ✔  GET  /list_users    200   12ms  req:a3c91ef0
  ✗  POST /signup        500   44ms  req:550e8400
     └─ Error: Stripe timeout`,
  },
  {
    cmd:     'flux why <request-id>',
    summary: 'Root cause a failed request',
    desc:    "Fetches the full trace for a request, identifies the first failing span, and explains the cause in plain language — plus shows what request ran just before it.",
    example: `$ flux why 550e8400

✗  POST /signup → create_user  (142ms, 500 FAILED)
    request_id:  550e8400-e29b-41d4-a716-446655440000
    error:       TypeError: Cannot read properties of undefined (reading 'id')

─── State changes (1 mutation) ─────────────────────────────────────────────
  users  v1  INSERT  by api-key  id=7f3a…
    email:   user@example.com
    plan:    free

─── Previous request ───────────────────────────────────────────────────────
  ✔ 3c9f1a2b  POST /login  38ms
  ⚠ also modified  users.id=7f3a…

─── Suggested next steps ───────────────────────────────────────────────────
  flux debug 550e8400-e29b  deep-dive the full trace + logs
  flux state history users 7f3a  full row version history`,
  },
  {
    cmd:     'flux trace <request-id>',
    summary: 'Full trace for any request',
    desc:    'Shows every span for a request in order: gateway auth, function execution, each database query, tool calls, async jobs. Latencies in yellow, errors in red.',
    example: `$ flux trace 4f9a3b2c

  Trace 4f9a3b2c  POST /create_user  200

  ▸ gateway                    3ms
    auth ✔  rate_limit ✔
  ▸ create_user               81ms
    ▸ db:select(users)        11ms
    ▸ db:insert(users)        14ms
  ▸ send_welcome  async →   queued

  ── total: 98ms ──────────────────`,
  },
  {
    cmd:     'flux trace diff <id-a> <id-b>',
    summary: 'Compare two request traces',
    desc:    'Diffs two traces side by side. Changed spans are highlighted. Useful for comparing a passing and a failing request to find what changed.',
    example: `$ flux trace diff 4f9a3b2c 550e8400

  SPAN                  A        B        DELTA
  gateway               3ms      4ms      +1ms
  create_user          81ms     44ms     -37ms
  stripe.charge        12ms   10002ms  +9990ms  ✗
  send_welcome       queued    SKIP

  → stripe.charge regressed by 9990ms`,
  },
  {
    cmd:     'flux trace debug <request-id>',
    summary: 'Step through a trace interactively',
    desc:    'Opens an interactive step-by-step view of a request. Use arrow keys to navigate spans. Each span shows its input, output, duration, and any errors.',
    example: `$ flux trace debug 550e8400

  Step 1/4  gateway
  ─────────────────────────────────────
  Input:  POST /signup  { email: "a@b.com" }
  Output: { tenant_id: "t_123", passed: true }
  Time:   4ms

  ↓  next span  ↑  prev  q quit`,
  },
  {
    cmd:     'flux state history <table> --id <row-id>',
    summary: 'See every mutation to a row',
    desc:    'Shows a timestamped history of every INSERT, UPDATE, and DELETE that touched a specific row. Each entry links back to the request that caused it.',
    example: `$ flux state history users --id 42

  users id=42  (7 mutations)

  2026-03-10 14:21:55  INSERT   email=a@b.com, plan=free
  2026-03-10 14:22:01  UPDATE   plan: free → pro   req:4f9a3b2c
  2026-03-10 14:22:01  UPDATE   plan: pro  → null  req:550e8400  ✗ rolled back`,
  },
  {
    cmd:     'flux state blame <table> --id <row-id>',
    summary: 'Find which request owns a row\'s current state',
    desc:    "Shows the last mutation per column for a row, annotated with the request ID and timestamp. Answers 'who set this field to this value?'",
    example: `$ flux state blame users --id 42

  users id=42

  email    a@b.com         req:4f9a3b2c  2026-03-10 14:21:55
  plan     free            req:550e8400  2026-03-10 14:22:01  ✗ rolled back
  name     Alice Smith     req:a3c91ef0  2026-03-10 12:00:00`,
  },
  {
    cmd:     'flux incident replay <from>..<to>',
    summary: 'Replay production requests safely',
    desc:    "Re-executes all requests from a time window against the current code. Side-effects (emails, Slack, webhooks, cron) are disabled. Database writes and mutation logs are enabled. Use this to test a fix against the exact requests that caused an incident.",
    example: `$ flux incident replay 14:00..14:05

  Replaying 23 requests from 14:00–14:05…

  ✔  req:4f9a3b2c  POST /create_user   200  81ms
  ✔  req:a3c91ef0  GET  /list_users    200  12ms
  ✗  req:550e8400  POST /signup        500  44ms
     └─ Still failing: Stripe timeout

  23 replayed · 1 still failing`,
  },
  {
    cmd:     'flux bug bisect',
    summary: 'Find the commit that introduced a bug',
    desc:    "Binary-searches your git history comparing trace behaviour before/after each commit. Automatically identifies the first commit where a specified request started failing.",
    example: `$ flux bug bisect --request 550e8400

  Bisecting 42 commits (2026-03-01..2026-03-10)…

  Testing commit abc123…  ✔ passes
  Testing commit def456…  ✗ fails

  FIRST BAD COMMIT
  def456  "feat: add retry logic to stripe.charge"
  2026-03-08 by alice@example.com`,
  },
  {
    cmd:     'flux explain <query-file>',
    summary: 'Dry-run a database query',
    desc:    "Sends a query to the Data Engine dry-run endpoint. Returns the compiled SQL, applied row policies, column access rules, and query complexity score — without executing anything.",
    example: `$ flux explain query.json

  ── Query Plan ──────────────────────────────
  Table:      users
  Operation:  select
  Schema:     public

  ── Policies Applied ────────────────────────
  Role:       admin
  Columns:    id, email, plan, created_at
  Row filter: tenant_id = $1

  ── Compiled SQL ────────────────────────────
  SELECT id, email, plan, created_at
  FROM users
  WHERE tenant_id = $1
  LIMIT 100

  ── QueryGuard Score ────────────────────────
  Complexity: 3/100  ✓
  Depth:      1/6    ✓
  Filters:    1/20   ✓`,
  },
];
