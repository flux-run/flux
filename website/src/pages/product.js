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
  const pills = [
    { label: '~1–3 ms overhead',         href: '/how-it-works#performance' },
    { label: '~3–5 KB/request',           href: '/docs/security-and-privacy#storage' },
    { label: 'async, fire-and-forget',    href: '/how-it-works#performance' },
    { label: 'self-hosted available',     href: '/docs/deployment' },
  ].map(p => `<a href="${p.href}" style="display:inline-block;font-size:.75rem;font-family:var(--font-mono);color:var(--muted);background:var(--bg-elevated);border:1px solid var(--border);border-radius:20px;padding:3px 12px;text-decoration:none;transition:border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)';this.style.color='var(--accent)';" onmouseleave="this.style.borderColor='var(--border)';this.style.color='var(--muted)';">${p.label}</a>`).join('\n    ');

  return `<section class="hero" style="padding-bottom:48px;">
  <span class="eyebrow">Product</span>
  <h1 style="font-size:clamp(2rem,5vw,3rem);">Every production question,<br><span class="gradient-text">answered in one command.</span></h1>
  <p style="max-width:580px;margin:0 auto 20px;">Fluxbase captures a deterministic record of every request and every database mutation. Then gives you tools to query that record from the terminal.</p>
  <div style="display:flex;flex-wrap:wrap;gap:8px;justify-content:center;margin-bottom:28px;">
    ${pills}
  </div>
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

// ── Trust section ────────────────────────────────────────────────────────────
function trustSection() {
  const cards = [
    {
      icon: '🔒',
      title: 'Privacy',
      body: 'Fluxbase records execution metadata — not your database. Mutation diffs capture <code>plan: free → pro</code>, not the entire row. Sensitive fields can be masked. Request bodies are never stored.',
      link: { href: '/docs/security-and-privacy#data-privacy', label: 'Privacy model →' },
    },
    {
      icon: '💾',
      title: 'Storage cost',
      body: 'Typical request: ~3–5 KB (spans + mutation diffs). 1M requests/day ≈ 4 GB/day. Retention defaults to 7 days with automatic purge. Archive to S3/GCS for long-term compliance.',
      link: { href: '/docs/security-and-privacy#storage', label: 'Storage model →' },
    },
    {
      icon: '📈',
      title: 'Scalability',
      body: 'Gateway is stateless and horizontally scalable. All trace writes are append-only — no cross-request locking or coordination. Span emission is fire-and-forget and does not delay HTTP responses.',
      link: { href: '/docs/security-and-privacy#scalability', label: 'Scale details →' },
    },
  ];

  const cardHtml = cards.map(card => `<div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;padding:24px 26px;display:flex;flex-direction:column;gap:10px;">
    <div style="font-size:1.5rem;">${card.icon}</div>
    <h3 style="font-size:1rem;font-weight:700;margin:0;color:var(--text);">${card.title}</h3>
    <p style="font-size:.86rem;color:var(--muted);line-height:1.7;margin:0;flex:1;">${card.body}</p>
    <a href="${card.link.href}" style="font-size:.82rem;color:var(--accent);text-decoration:none;font-family:var(--font-mono);">${card.link.label}</a>
  </div>`).join('\n  ');

  return section({
    bg: 'var(--bg-surface)',
    content: `<div style="text-align:center;margin-bottom:40px;">
  <span style="display:inline-block;font-family:var(--font-mono);font-size:.75rem;color:var(--accent);background:var(--accent-dim);border:1px solid var(--accent);border-radius:20px;padding:4px 14px;text-transform:uppercase;letter-spacing:.08em;margin-bottom:16px;">Common Questions</span>
  <h2 style="font-size:1.8rem;font-weight:800;margin:0 0 10px;">Privacy, storage, and scale</h2>
  <p style="color:var(--muted);max-width:500px;margin:0 auto 8px;">Your code. Your database. Your infrastructure. Fluxbase records the execution history — not the data.</p>
</div>
<div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:16px;">
  ${cardHtml}
</div>`,
  });
}

// ── Why a runtime callout ─────────────────────────────────────────────────────
function whyRuntimeCallout() {
  const invariants = [
    { icon: '🔌', label: 'Execution interception', desc: 'Every db, tool, and workflow call is intercepted at the runtime level. Nothing is invisible.' },
    { icon: '📝', label: 'Mutation completeness', desc: 'All writes pass through the Data Engine in the same transaction. The mutation log is never partial.' },
    { icon: '♻️', label: 'Replay safety', desc: 'Side-effects are suppressed at the call site — not at the network layer. Only possible from inside the runtime.' },
  ];

  const items = invariants.map(inv => `<div style="display:flex;gap:14px;align-items:start;">
    <span style="font-size:1.2rem;flex-shrink:0;margin-top:2px;">${inv.icon}</span>
    <div>
      <div style="font-size:.9rem;font-weight:700;margin-bottom:4px;">${inv.label}</div>
      <p style="font-size:.84rem;color:var(--muted);line-height:1.6;margin:0;">${inv.desc}</p>
    </div>
  </div>`).join('\n  ');

  return section({
    content: `${eyebrow({ text: 'Common Question' })}
${sectionHeader({
  heading: 'Why must this be a runtime?',
  sub: 'Senior engineers will ask: "Why can\'t this work as a library or proxy on my existing backend?" Three invariants require runtime ownership to hold.',
  maxWidth: '580px',
})}
<div style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;max-width:960px;margin:0 auto;" class="grid-2col">
  <div style="display:flex;flex-direction:column;gap:24px;">
    ${items}
    <p style="font-size:.84rem;color:var(--muted);line-height:1.65;padding:16px 18px;background:var(--bg-elevated);border-left:3px solid var(--border);border-radius:0 6px 6px 0;margin:0;">
      The adoption step is real — you run your functions on Fluxbase rather than integrating into an existing backend. The guarantee in return is a complete, reliable execution history that a library or proxy cannot provide.
    </p>
    <a href="/how-it-works#why-runtime" style="font-size:.85rem;color:var(--accent);font-family:var(--font-mono);text-decoration:none;">Full technical explanation →</a>
  </div>
  <div style="background:var(--bg-surface);border:1px solid var(--border);border-radius:12px;padding:28px;">
    <p style="font-size:.78rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--muted);margin:0 0 16px;">What runtime ownership enables</p>
    <div style="display:flex;flex-direction:column;gap:10px;">
      ${[
        ['flux why &lt;id&gt;',             'complete span graph, no gaps'],
        ['flux state history',               'every mutation traced back to a request'],
        ['flux trace diff',                  'span-level diff between any two requests'],
        ['flux incident replay',             'side-effects suppressed at call site'],
        ['flux bug bisect',                  'replay across git history reliably'],
      ].map(([cmd, note]) => `<div style="display:flex;justify-content:space-between;align-items:baseline;gap:12px;padding:8px 0;border-bottom:1px solid var(--border);">
        <code style="font-size:.8rem;color:var(--accent);">${cmd}</code>
        <span style="font-size:.78rem;color:var(--muted);text-align:right;">${note}</span>
      </div>`).join('\n      ')}
    </div>
  </div>
</div>`,
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
    trustSection(),
    whyRuntimeCallout(),
    cta(),
  ].join('\n\n');

  return landingLayout({ meta, active: 'product', extraHead, content });
}
