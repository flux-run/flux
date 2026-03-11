/**
 * "Building Git for Backend Execution" — launch article.
 * Published at /git-for-backend
 */
import { landingLayout } from '../layouts/landing.js';
import { codeWindow, c } from '../components/code-window.js';

export const meta = {
  title:       'Building Git for Backend Execution — Fluxbase',
  description: 'Every backend failure leaves evidence. The problem is that execution history disappears the moment a request finishes. Here\'s what it looks like when it doesn\'t.',
  path:        'blog/git-for-backend.html',
};

// ── Reusable prose helpers ────────────────────────────────────────────────────
const prose = (html) =>
  `<div style="max-width:680px;margin:0 auto;color:var(--muted);line-height:1.85;font-size:1rem;">${html}</div>`;

const h2 = (text) =>
  `<h2 style="font-size:1.55rem;font-weight:800;color:var(--text);margin:64px 0 20px;">${text}</h2>`;

const h3 = (text) =>
  `<h3 style="font-size:1.1rem;font-weight:700;color:var(--text);margin:40px 0 12px;">${text}</h3>`;

const p = (text) =>
  `<p style="margin:0 0 20px;">${text}</p>`;

const ul = (...items) =>
  `<ul style="padding-left:24px;margin:0 0 20px;">${items.map(i => `<li style="margin-bottom:8px;">${i}</li>`).join('')}</ul>`;

const code = (t) => `<code style="font-family:var(--font-mono);font-size:.88em;background:var(--bg-elevated);border:1px solid var(--border);border-radius:4px;padding:1px 5px;">${t}</code>`;

const pre = (text) =>
  `<pre style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:8px;padding:20px 24px;overflow-x:auto;font-family:var(--font-mono);font-size:.82rem;line-height:1.7;margin:0 0 24px;">${text}</pre>`;

const table = (headers, rows) => `<div style="overflow-x:auto;margin:0 0 28px;">
<table style="width:100%;border-collapse:collapse;font-size:.88rem;">
  <thead><tr>${headers.map(h => `<th style="text-align:left;padding:10px 14px;border-bottom:1px solid var(--border);color:var(--text);font-weight:700;">${h}</th>`).join('')}</tr></thead>
  <tbody>${rows.map(row => `<tr>${row.map((cell, i) => `<td style="padding:9px 14px;border-bottom:1px solid var(--border);${i === 0 ? 'font-family:var(--font-mono);font-size:.82rem;color:var(--accent);' : ''}">${cell}</td>`).join('')}</tr>`).join('')}</tbody>
</table></div>`;

const aside = (html) =>
  `<div style="border-left:3px solid var(--accent);padding:16px 20px;background:var(--bg-elevated);border-radius:0 8px 8px 0;margin:0 0 28px;">${html}</div>`;

// ── Article sections ──────────────────────────────────────────────────────────

function hero() {
  return `<section style="padding:96px 24px 64px;text-align:center;border-bottom:1px solid var(--border);">
  <div style="display:inline-block;font-family:var(--font-mono);font-size:.78rem;color:var(--accent);background:var(--accent-dim);border:1px solid var(--accent);border-radius:20px;padding:4px 14px;margin-bottom:24px;text-transform:uppercase;letter-spacing:.08em;">Launch Article</div>
  <h1 style="font-size:clamp(2rem,5vw,3.2rem);font-weight:900;max-width:820px;margin:0 auto 24px;line-height:1.1;">Building Git for<br><span class="gradient-text">Backend Execution</span></h1>
  <p style="color:var(--muted);font-size:1.1rem;max-width:580px;margin:0 auto 40px;line-height:1.7;">Every backend failure leaves evidence. The problem is that execution history disappears the moment a request finishes. Here's what it looks like when it doesn't.</p>
  <div style="display:flex;gap:16px;justify-content:center;flex-wrap:wrap;font-family:var(--font-mono);font-size:.82rem;color:var(--muted);">
    <span>March 11, 2026</span>
    <span style="color:var(--border);">·</span>
    <span>12 min read</span>
    <span style="color:var(--border);">·</span>
    <a href="https://news.ycombinator.com" style="color:var(--accent);text-decoration:none;">Discuss on HN</a>
  </div>
</section>`;
}

function section1() {
  return `<section style="padding:64px 24px 0;">
${prose(`
${h2('The problem')}
${p(`When a backend fails in production, a typical debugging session looks like this:`)}
${ul(
  'An alert fires (or a customer reports it)',
  'You open the log aggregator and start grepping',
  'You find a vague error message without context',
  'You add more logging and redeploy',
  'You wait for the bug to happen again',
  'You repeat',
)}
${p(`This is the state of the art in 2026. Not because we lack tooling — we have more observability infrastructure than ever. We have logs, metrics, and traces. But there's something fundamental missing.`)}
${p(`When your function runs in production, processes a Stripe webhook, touches three database rows, sends an email and crashes at line 87 — you can see <em>that</em> it failed. You cannot reconstruct <em>the execution</em> that caused it.`)}
${p(`There is no equivalent of:`)}
${pre(`git show &lt;commit&gt;\ngit diff HEAD~1\ngit bisect run test`)}
${p(`…for a backend request. Every execution disappears the moment it finishes.`)}
`)}
</section>`;
}

function section2() {
  return `<section style="padding:0 24px;">
${prose(`
${h2('The insight: treat execution like version control')}
${p(`Git changed software development by making every code change permanent and inspectable. Before Git, "what changed?" was a hard question. After Git, it's trivial.`)}
${p(`The same shift is possible for backend execution.`)}
${aside(`<strong style="color:var(--text);">The key insight:</strong><br><span style="color:var(--muted);">Every request should leave behind a complete forensic record — execution spans, data mutations, and request metadata — all tied to a single <code>request_id</code>. Once you have that, production debugging becomes deterministic.</span>`)}
${p(`Instead of reconstructing what happened from scattered logs, you can <em>retrieve</em> what happened, directly. A failure from three days ago at 2am has the same inspectability as one from five seconds ago.`)}
`)}
</section>`;
}

function section3() {
  const win = codeWindow({
    title: 'execution chain for one request',
    content: `POST /signup  →  request_id: ${c.id('550e8400')}
│
├─ span: gateway          ${c.ms('2ms')}   auth ✔  rate_limit ✔
├─ span: create_user      ${c.ms('1ms')}   function start
├─ span: db.users SELECT  ${c.ms('12ms')}  rows: 0
├─ span: stripe.charge  ${c.ms('3200ms')}  ${c.err('⚠ SLOW  status: timeout')}
│
└─ state_mutations
     users  id=${c.id('7f3a')}
       plan: free  email: user@acme.com`,
  });

  return `<section style="padding:0 24px;">
${prose(`${h2('What Fluxbase records')}`)}
<div style="max-width:680px;margin:0 auto;">${win}</div>
${prose(`
${p(`Every request produces three linked records:`)}
${h3('1. Trace spans')}
${p(`Every layer of the stack emits a structured span: the gateway records auth and routing, the runtime records function calls and tool calls, the data engine records each database query. All share the same ${code('request_id')}.`)}
${h3('2. State mutations')}
${p(`Every INSERT, UPDATE, and DELETE is recorded with full before/after snapshots in the same transaction as the write. If the write rolls back, the record rolls back too. The mutation log is never optional and never out of sync.`)}
${pre(`-- state_mutations (simplified)
request_id   TEXT       -- join key to the trace
table_name   TEXT       -- which table
operation    TEXT       -- INSERT / UPDATE / DELETE
before_state JSONB      -- row before the change
after_state  JSONB      -- row after the change
version      INT        -- per-row monotonic counter
span_id      TEXT       -- links to the exact execution span`)}
${h3('3. Request envelope')}
${p(`The gateway records the full HTTP context: method, path, function name, started_at, duration, and status. This is the entry point the CLI uses to look up any past request by ID.`)}
`)}
</section>`;
}

function section4() {
  return `<section style="padding:0 24px;">
${prose(`
${h2('The Git-style debugging workflow')}
${p(`Once execution is recorded, the CLI works like Git against that history.`)}

${h3('1. Watch production requests')}
`)}
<div style="max-width:680px;margin:0 auto;">${codeWindow({ title: 'flux tail', content:
`${c.cmd('$')} flux tail

METHOD   ROUTE              FUNCTION       DURATION  STATUS
${c.ok('POST')}     /login             auth_user      ${c.ms('38ms')}     ${c.ok('✔')}
   ${c.dim('users.id=7f3a  last_login_at → 2026-03-11T09:40:52Z')}

${c.err('POST')}     /signup            create_user    ${c.ms('3.2s')}     ${c.err('✗ 500')}
   ${c.err('error: Stripe timeout after 10000ms')}
   ${c.id('→ flux why 550e8400')}
   ${c.dim('users.id=7f3a  plan free → pro')}`,
})}</div>
${prose(`
${p(`Errors surface immediately with inline error messages, data mutations, and the next command to run.`)}

${h3('2. Inspect an execution')}
`)}
<div style="max-width:680px;margin:0 auto;">${codeWindow({ title: 'flux why 550e8400', content:
`${c.cmd('$')} flux why ${c.id('550e8400')}

${c.err('✗')}  POST /signup → create_user  (3200ms, ${c.err('500 FAILED')})
    request_id:  ${c.id('550e8400-e29b-41d4-a716-446655440000')}
    ${c.err('error:')}       Stripe timeout after 10000ms

─── Execution graph ──────────────────────────────────
  ${c.dim('gateway')}     POST /signup          ${c.ms('2ms')}
  ${c.dim('runtime')}     create_user           ${c.ms('1ms')}
  ${c.dim('db')}          users (SELECT)        ${c.ms('12ms')}
  ${c.dim('tool')}        stripe.charge      ${c.ms('3200ms')}  ${c.err('⚠ slow')}

─── State changes ────────────────────────────────────
  ${c.db('users')}  v1  INSERT  id=${c.id('7f3a')}
    email:  user@acme.com
    plan:   free

─── Previous request ─────────────────────────────────
  ${c.ok('✔')} POST /login  ${c.ms('38ms')}  (0.2s before)
  ${c.err('⚠ also modified')}  users.id=${c.id('7f3a')}`,
})}</div>
${prose(`
${h3('3. Let the system diagnose the failure')}
`)}
<div style="max-width:680px;margin:0 auto;">${codeWindow({ title: 'flux doctor 550e8400', content:
`${c.cmd('$')} flux doctor ${c.id('550e8400')}

ROOT CAUSE
────────────────────────────────
  ${c.err('⚡ stripe.charge timed out after 10000ms')}

LIKELY ISSUE
────────────────────────────────
  External tool latency exceeded threshold.

EVIDENCE
────────────────────────────────
  stripe.charge     ${c.err('3200ms  ⚠ slow')}
  db.users          ${c.ms('12ms')}
  runtime           ${c.ms('1ms')}

SUGGESTED ACTIONS
────────────────────────────────
  • Increase timeout above 11000ms
  • Add retry with exponential backoff for stripe.charge
  • ${c.id('flux why 550e8400')}
  • ${c.id('flux trace debug 550e8400')}`,

})}</div>
${prose(`
${p(`${code('flux doctor')} runs a rules engine over the spans and mutations. It doesn't need an LLM — it just reads the data that's already there. The most common failures are pattern-matchable.`)}

${h3('4. Step through the execution')}
${h3('5. Compare two executions')}
${h3('6. Find the commit that broke it')}
`)}
<div style="max-width:680px;margin:0 auto;">${codeWindow({ title: 'flux bug bisect', content:
`${c.cmd('$')} flux bug bisect --function create_user --period 24h

  Bisecting 47 commits...
  Testing a1f9c3e... ${c.ok('✔ pass')}   (avg ${c.ms('94ms')}, 0 errors)
  Testing 4d8a2b1... ${c.err('✗ fail')}   (avg ${c.ms('3200ms')}, 12% error rate)

  First bad commit: ${c.id('4d8a2b1')}
  Author: dev@acme.com
  diff: stripe timeout 5000 → 10000`,
})}</div>
</section>`;
}

function section5() {
  return `<section style="padding:0 24px;">
${prose(`
${h2('The full command table')}
${table(
  ['Command', 'Git equivalent', 'What it does'],
  [
    [code('flux tail'), code('git log -f'), 'Stream live requests with inline mutations'],
    [code('flux why &lt;id&gt;'), code('git show'), 'Full execution: spans + mutations + context'],
    [code('flux doctor &lt;id&gt;'), '—', 'Automatic diagnosis (no Git equivalent)'],
    [code('flux trace debug'), code('git show -p'), 'Interactive step-through'],
    [code('flux trace diff'), code('git diff'), 'Compare two executions side by side'],
    [code('flux bug bisect'), code('git bisect'), 'Find the commit that introduced a failure'],
    [code('flux state blame'), code('git blame'), 'Who mutated this row and when'],
    [code('flux incident replay'), code('git stash apply'), 'Deterministic reproduction'],
    [code('flux state history'), code('git log -- &lt;file&gt;'), 'Full version history for a DB row'],
  ]
)}
${p(`The last row — ${code('flux doctor')} — has no Git equivalent because Git only knows about code. Fluxbase knows about code <em>and</em> runtime state, which is what makes automated diagnosis possible.`)}
`)}
</section>`;
}

function section6() {
  return `<section style="padding:0 24px;">
${prose(`
${h2('Why existing tools can\'t do this')}
${p(`The reason logs, metrics, and traces are insufficient isn't a tooling failure — it's a data model problem.`)}
${table(
  ['Signal', 'Records', 'Missing'],
  [
    ['Logs', 'Messages', 'Causal chain, state context'],
    ['Metrics', 'Counters, percentiles', 'Individual request detail'],
    ['Traces', 'Timing per span', 'What data changed'],
    ['<strong style="color:var(--accent);">Fluxbase</strong>', '<strong style="color:var(--accent);">Execution + state transitions</strong>', '—'],
  ]
)}
${p(`None of the standard signals tell you <em>what state the system was in</em> when the failure happened. Fluxbase records execution and state transitions together. That combination is what makes deterministic replay possible.`)}

${h2('The architecture behind this')}
`)}
<div style="max-width:680px;margin:0 auto;">${codeWindow({ title: 'request flow', content:
`HTTP client
     │
     ▼
┌──────────────────────────────────────────────────┐
│  Gateway                                         │
│  auth · rate limit · span emit · request_id      │
└──────────────────────────────────────────────────┘
     │  request_id propagated through every layer
     ▼
┌──────────────────────────────────────────────────┐
│  Runtime  (V8 isolate per tenant)                │
│  your TypeScript · tool calls · spans            │
└──────────────────────────────────────────────────┘
     │  mutations intercepted before they hit PG
     ▼
┌──────────────────────────────────────────────────┐
│  Data Engine                                     │
│  type-safe query API · policies · mutation log   │
└──────────────────────────────────────────────────┘
     │  standard SQL
     ▼
  PostgreSQL  (you own this)`,
})}</div>
${prose(`
${p(`Three properties make the recording reliable:`)}
${ul(
  `<strong style="color:var(--text);">Atomic writes</strong> — the mutation log entry commits in the same transaction as the data. No mutation record is ever missing or out of sync.`,
  `<strong style="color:var(--text);">Span IDs on mutations</strong> — every mutation row carries the span_id that caused it. You can trace any DB change back to the exact line of function code.`,
  `<strong style="color:var(--text);">Immutable history</strong> — state_mutations is append-only. There is no UPDATE or DELETE on audit records.`,
)}
`)}
</section>`;
}

function closing() {
  return `<section style="padding:0 24px 96px;">
${prose(`
${h2('The key insight, stated plainly')}
${p(`Git made code history permanent. Every change is recorded, every commit is inspectable, every regression is bisectable.`)}
${p(`Fluxbase applies the same model to backend execution. Every request is recorded. Every mutation is inspectable. Every regression is bisectable.`)}
${aside(`<strong style="color:var(--text);">The biggest problem in production debugging isn't lack of logging.</strong><br><span style="color:var(--muted);">It's that execution history disappears the moment a request finishes. Fluxbase keeps that history.</span>`)}
<div style="text-align:center;margin-top:56px;padding-top:48px;border-top:1px solid var(--border);">
  <p style="color:var(--muted);margin-bottom:28px;font-size:1.05rem;">The quickstart takes five minutes and works with any TypeScript function and a Postgres database you already have.</p>
  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="/docs/quickstart" style="text-decoration:none;">Try the quickstart →</a>
    <a class="btn-secondary" href="/cli" style="text-decoration:none;">See the CLI</a>
  </div>
</div>
`)}
</section>`;
}

export function render() {
  const body = [
    hero(),
    section1(),
    section2(),
    section3(),
    section4(),
    section5(),
    section6(),
    closing(),
  ].join('\n');

  return landingLayout({ meta, body });
}
