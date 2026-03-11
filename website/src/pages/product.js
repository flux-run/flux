/**
 * Product page — capabilities in depth.
 */
import { landingLayout }    from '../layouts/landing.js';
import { codeWindow, c }    from '../components/code-window.js';
import { eyebrow, section, sectionHeader } from '../components/section.js';

export const meta = {
  title:       'Product — Fluxbase',
  description: 'Time-travel debugging, mutation history, incident replay, regression detection. Every tool a developer needs to understand and fix production systems fast.',
  path:        'product.html',
};

// ── Hero ──────────────────────────────────────────────────────────────────────
function hero() {
  return `<section class="hero" style="padding-bottom:48px;">
  <span class="eyebrow">Product</span>
  <h1 style="font-size:clamp(2rem,5vw,3rem);">Every production question,<br><span class="gradient-text">answered in one command.</span></h1>
  <p style="max-width:580px;margin:0 auto 24px;">Fluxbase captures a deterministic record of every request and every database mutation. Then gives you tools to query that record from the terminal.</p>
  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="/docs/quickstart">Get Started →</a>
    <a class="btn-secondary" href="/cli">CLI Reference</a>
  </div>
</section>`;
}

// ── Task-oriented table ───────────────────────────────────────────────────────
function taskTable() {
  const rows = [
    ['Why did my request fail?',              '<code>flux why &lt;id&gt;</code>',        'Root cause, span tree, suggestions'],
    ['Which commit introduced this bug?',     '<code>flux bug bisect</code>',             'Binary-searches git history'],
    ['What changed in the database?',         '<code>flux state history</code>',          'Every row mutation, linked to request'],
    ['Who set this field to this value?',     '<code>flux state blame</code>',            'Per-column last-write attribution'],
    ['What happens if I replay this?',        '<code>flux incident replay</code>',        'Safe re-run, side-effects off'],
    ['How do two requests differ?',           '<code>flux trace diff</code>',             'Span-by-span comparison'],
    ['How does my query get compiled?',       '<code>flux explain</code>',                'Dry-run with policy + SQL preview'],
  ];

  const tableRows = rows.map(([q, cmd, desc]) =>
    `<tr>
      <td style="padding:12px 16px;border-bottom:1px solid var(--border);color:var(--text);">${q}</td>
      <td style="padding:12px 16px;border-bottom:1px solid var(--border);white-space:nowrap;">${cmd}</td>
      <td style="padding:12px 16px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.87rem;">${desc}</td>
    </tr>`
  ).join('\n');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Task-Oriented Design' })}
${sectionHeader({
  heading: 'Start with the question, not the tool.',
  sub: 'Fluxbase CLI commands map directly to the questions developers ask when something breaks in production.',
})}

<div style="overflow-x:auto;">
<table style="width:100%;border-collapse:collapse;font-size:.9rem;">
  <thead>
    <tr>
      <th style="text-align:left;padding:8px 16px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.75rem;text-transform:uppercase;letter-spacing:.05em;">Developer Question</th>
      <th style="text-align:left;padding:8px 16px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.75rem;text-transform:uppercase;letter-spacing:.05em;">Command</th>
      <th style="text-align:left;padding:8px 16px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.75rem;text-transform:uppercase;letter-spacing:.05em;">What it does</th>
    </tr>
  </thead>
  <tbody>
    ${tableRows}
  </tbody>
</table>
</div>`,
  });
}

// ── Feature deep-dive ─────────────────────────────────────────────────────────
function featureBlock({ id = '', eyebrowText, eyebrowColor = 'accent', heading, sub, window, reverse = false }) {
  const cols = reverse
    ? `<div>${window}</div><div><div>${eyebrow({ text: eyebrowText, color: eyebrowColor })}</div><h3 style="font-size:1.4rem;font-weight:800;letter-spacing:-.02em;margin-bottom:12px;">${heading}</h3><p style="color:var(--muted);font-size:.95rem;line-height:1.7;">${sub}</p></div>`
    : `<div><div>${eyebrow({ text: eyebrowText, color: eyebrowColor })}</div><h3 style="font-size:1.4rem;font-weight:800;letter-spacing:-.02em;margin-bottom:12px;">${heading}</h3><p style="color:var(--muted);font-size:.95rem;line-height:1.7;">${sub}</p></div><div>${window}</div>`;

  return section({
    id,
    bg: reverse ? 'var(--bg-surface)' : '',
    content: `<div class="grid-2col" style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:center;">
  ${cols}
</div>`,
  });
}

// ── Deterministic Execution ───────────────────────────────────────────────────
function deterministicExecution() {
  const w = codeWindow({
    title: 'automatic recording',
    content: `${c.dim('# Every request produces:')}

  trace_requests      ${c.ok('→')} span tree (gateway to db)
  state_mutations     ${c.ok('→')} every row change + request link
  execution_spans     ${c.ok('→')} timing, errors, tool calls

${c.dim('# Nothing to configure. Zero SDK changes.')}
${c.dim('# The runtime records it all.')}`,
  });

  return featureBlock({
    id: 'deterministic-execution',
    eyebrowText: 'Deterministic Execution',
    heading: 'Every request is recorded automatically.',
    sub: `The Fluxbase runtime captures a complete record of every request as it happens — gateway auth, function spans, every database query, tool latencies, async job hand-offs. No instrumentation, no SDK, no config. If the request ran, it's recorded.`,
    window: w,
  });
}

// ── Time-Travel Debugging ─────────────────────────────────────────────────────
function timeTravelDebugging() {
  const w = codeWindow({
    title: 'flux trace debug 550e8400',
    content: `${c.cmd('$')} flux trace debug ${c.id('550e8400')}

  ${c.dim('Step 1/4  gateway')}
  ${c.dim('─────────────────────────────────────')}
  Input:   POST /signup  ${c.ok('{ email: "a@b.com" }')}
  Output:  ${c.ok('{ tenant_id: "t_123", passed: true }')}
  Time:    4ms

  ${c.dim('Step 2/4  create_user')}
  ${c.dim('─────────────────────────────────────')}
  Input:   ${c.ok('{ email: "a@b.com" }')}
  Output:  ${c.ok('{ userId: "u_42" }')}
  Time:    81ms

  ${c.dim('↓ next  ↑ prev  e expand  q quit')}`,
  });

  return featureBlock({
    id: 'time-travel-debugging',
    eyebrowText: 'Time-Travel Debugging',
    heading: 'Step through any production request.',
    sub: `<code>flux trace debug &lt;id&gt;</code> opens an interactive terminal UI where you can navigate each span of a production request. See the exact input and output at every step. What the gateway received. What the function returned. What the database wrote. All from the actual production execution.`,
    window: w,
    reverse: true,
  });
}

// ── Data Mutation History ─────────────────────────────────────────────────────
function mutationHistory() {
  const w = codeWindow({
    title: 'flux state history users --id 42',
    content: `${c.cmd('$')} flux state history users --id 42

  ${c.white('users id=42')}  (7 mutations)

  ${c.dim('2026-03-10 12:00:00')}  INSERT  ${c.ok('email=a@b.com, plan=free')}
  ${c.dim('2026-03-10 14:21:59')}  UPDATE  name: null → Alice Smith  ${c.id('req:a3c91ef0')}
  ${c.dim('2026-03-10 14:22:01')}  UPDATE  plan: free → pro           ${c.id('req:4f9a3b2c')}
  ${c.dim('2026-03-10 14:22:01')}  UPDATE  plan: pro → null  ${c.dim('(rolled back)')}  ${c.err('req:550e8400')}

${c.dim('$')} flux state blame users --id 42

  email    a@b.com     ${c.id('req:4f9a3b2c')}  12:00:00
  plan     free        ${c.err('req:550e8400')}  14:22:01  ${c.err('✗ rolled back')}`,
  });

  return featureBlock({
    id: 'mutation-history',
    eyebrowText: 'Data Mutation History',
    heading: 'See every change ever made to a row.',
    sub: `<code>flux state history</code> shows every INSERT, UPDATE, and DELETE on any row, linked back to the request that caused it. <code>flux state blame</code> shows which request owns each column's current value. Instantly answer "who or what set this field to this value?"`,
    window: w,
  });
}

// ── Incident Replay ───────────────────────────────────────────────────────────
function incidentReplay() {
  const w = codeWindow({
    title: 'flux incident replay 14:00..14:05',
    content: `${c.cmd('$')} flux incident replay 14:00..14:05

  Replaying 23 requests from 14:00–14:05…

  ${c.dim('Side-effects: hooks off · events off · cron off')}
  ${c.dim('Database writes: on · mutation log: on')}

  ${c.ok('✔')}  ${c.id('req:4f9a3b2c')}  POST /create_user   200  81ms
  ${c.ok('✔')}  ${c.id('req:a3c91ef0')}  GET  /list_users    200  12ms
  ${c.err('✗')}  ${c.id('req:550e8400')}  POST /signup        500  44ms
     ${c.err('└─ Still failing: Stripe timeout')}

  23 replayed · 22 passing · 1 still failing`,
  });

  return featureBlock({
    id: 'incident-replay',
    eyebrowText: 'Incident Replay',
    heading: 'Test your fix against the exact incident.',
    sub: `<code>flux incident replay</code> re-executes all requests from a time window against your current code. Outbound side-effects are disabled — no emails, no webhooks, no Slack. Database writes and mutation logs run normally. After your commit, replay the incident to confirm the fix before deploying.`,
    window: w,
    reverse: true,
  });
}

// ── Regression Detection ──────────────────────────────────────────────────────
function regressionDetection() {
  const w = codeWindow({
    title: 'flux bug bisect',
    content: `${c.cmd('$')} flux bug bisect --request ${c.id('550e8400')}

  Bisecting 42 commits (2026-03-01..2026-03-10)…

  Testing ${c.dim('abc123')}…  ${c.ok('✔ passes')}
  Testing ${c.dim('fde789')}…  ${c.ok('✔ passes')}
  Testing ${c.dim('def456')}…  ${c.err('✗ fails')}

  ${c.white('FIRST BAD COMMIT')}
  ${c.id('def456')}  "feat: add retry logic to stripe.charge"
  ${c.dim('2026-03-08 by alice@example.com')}

  ${c.ok('→')} Compare before/after:
     flux trace diff ${c.dim('abc123:550e8400 def456:550e8400')}`,
  });

  return featureBlock({
    id: 'regression-detection',
    eyebrowText: 'Regression Detection',
    heading: 'Find the commit that introduced the bug.',
    sub: `<code>flux bug bisect</code> binary-searches your git history comparing trace behaviour before and after each commit. It automatically identifies the first commit where a given request started failing. Like <code>git bisect</code>, but for production behaviour rather than a test suite.`,
    window: w,
  });
}

// ── Page styles ───────────────────────────────────────────────────────────────
const extraHead = '';

// ── CTA ───────────────────────────────────────────────────────────────────────
function cta() {
  return `<section class="cta-strip">
  <h2>Ready to debug production like it's local?</h2>
  <p style="max-width:480px;margin:0 auto 32px;">Everything on this page is available immediately after <code>flux deploy</code>. No configuration, no setup, no SDK changes.</p>
  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="/docs/quickstart">Start Building →</a>
    <a class="btn-secondary" href="/how-it-works">How It Works</a>
  </div>
</section>`;
}

// ── Render ────────────────────────────────────────────────────────────────────
export function render() {
  const content = [
    hero(),
    taskTable(),
    deterministicExecution(),
    timeTravelDebugging(),
    mutationHistory(),
    incidentReplay(),
    regressionDetection(),
    cta(),
  ].join('\n\n');

  return landingLayout({ meta, active: 'product', extraHead, content });
}
