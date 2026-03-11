/**
 * Homepage — "Git for Backend Execution"
 */
import { landingLayout }    from '../layouts/landing.js';
import { codeWindow, c }    from '../components/code-window.js';
import { eyebrow, section, sectionHeader, featureCard, comparisonGrid } from '../components/section.js';

export const meta = {
  title:       'Fluxbase — Git for Backend Execution',
  description: 'Debug production systems faster than local development. Every request is recorded. Replay, diff, and root-cause any production issue with a single CLI command.',
  path:        'home.html',
};

// ── Hero ──────────────────────────────────────────────────────────────────────
function hero() {
  const cliMoment = codeWindow({
    title: 'production debugging in 2 commands',
    content: `${c.cmd('$')} flux tail

  Streaming live requests…

  ${c.ok('✔')}  POST /signup      201  ${c.ms('88ms')}  ${c.dim('req:4f9a3b2c')}
  ${c.err('✗')}  POST /signup      500  ${c.ms('44ms')}  ${c.id('req:550e8400')}
     ${c.err('└─ Error: Stripe API timeout')}

${c.cmd('$')} flux why ${c.id('550e8400')}

  ${c.white('ROOT CAUSE')}    Stripe API timeout
  ${c.white('LOCATION')}     payments/create.ts:42
  ${c.white('DATA CHANGES')}  users.id=42  plan: free ${c.err('→ null')}  ${c.dim('(rolled back)')}
  ${c.white('SUGGESTION')}   ${c.ok('→')} Add 5s timeout + idempotency key retry`,
  });

  return `<section class="hero" style="padding-bottom:60px;">
  <span class="eyebrow">Git for Backend Execution</span>
  <h1>Debug production systems<br><span class="gradient-text">faster than local development.</span></h1>
  <p style="max-width:580px;margin:0 auto 10px;font-size:1.05rem;">Fluxbase records every backend execution — requests, data mutations, and runtime spans — so you can debug production systems the way Git debugs code.</p>
  <p style="max-width:520px;margin:0 auto 36px;font-size:.9rem;color:var(--muted);">Root-cause any incident in seconds. Replay it safely. Find the exact commit that broke it.</p>

  <div style="max-width:660px;margin:0 auto 40px;text-align:left;">${cliMoment}</div>

  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;margin-bottom:28px;">
    <a class="btn-primary" href="/docs/quickstart">Start Building →</a>
    <a class="btn-secondary" href="/docs/">View Docs</a>
  </div>

  <div class="install-hint">
    <span class="prompt">$</span>
    curl -fsSL https://fluxbase.co/install | bash
  </div>
</section>`;
}

// ── Demo section ──────────────────────────────────────────────────────────────
function demoSection() {
  const tailWindow = codeWindow({
    title: 'flux tail',
    content: `${c.cmd('$')} flux tail

  Streaming live requests…

  ${c.ok('✔')}  POST /signup      201   88ms  ${c.dim('req:4f9a3b2c')}
  ${c.ok('✔')}  GET  /users       200   12ms  ${c.dim('req:a3c91ef0')}
  ${c.err('✗')}  POST /signup      500   44ms  ${c.id('req:550e8400')}
     ${c.err('└─ Error: Stripe API timeout')}`,
  });

  const whyWindow = codeWindow({
    title: 'flux why 550e8400',
    content: `${c.cmd('$')} flux why ${c.id('550e8400')}

  ${c.white('ROOT CAUSE')}
  Stripe API timeout after 10s

  ${c.white('LOCATION')}
  payments/create.ts:42

  ${c.white('DATA CHANGES')}
  ${c.db('users')} id=42  plan: free ${c.err('→ null')}  ${c.dim('(rolled back)')}

  ${c.white('SUGGESTION')}
  ${c.ok('→')} Add 5s timeout + idempotency key retry`,
  });

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'The Debugging Workflow' })}
${sectionHeader({
  heading: 'From alert to root cause in 30 seconds.',
  sub: 'A user reports "signup failed". You have a request ID from <code>flux tail</code>. One more command and you know exactly what happened.',
})}

<div class="grid-2col" style="display:grid;grid-template-columns:1fr 1fr;gap:24px;align-items:start;">
  <div>
    <p style="font-size:.78rem;font-weight:600;text-transform:uppercase;letter-spacing:.08em;color:var(--muted);margin-bottom:14px;">Step 1 — spot the failure</p>
    ${tailWindow}
  </div>
  <div>
    <p style="font-size:.78rem;font-weight:600;text-transform:uppercase;letter-spacing:.08em;color:var(--muted);margin-bottom:14px;">Step 2 — understand it</p>
    ${whyWindow}
  </div>
</div>

<p style="margin-top:20px;text-align:center;font-size:.85rem;color:var(--muted);">
  Want to go deeper? <a href="/cli" style="color:var(--accent);">flux trace diff</a>, <a href="/cli" style="color:var(--accent);">flux state history</a>, and <a href="/cli" style="color:var(--accent);">flux incident replay</a> give you full production time-travel.
</p>`,
  });
}

// ── Comparison ────────────────────────────────────────────────────────────────
function comparisonSection() {
  return section({
    content: `${eyebrow({ text: 'The Difference' })}
${sectionHeader({
  heading: 'You shouldn\'t need 5 tools to debug one request.',
  sub: 'Traditional backends scatter evidence across logs, metrics, and traces — each in a different tool without shared context. Fluxbase records everything in one place, queryable by request ID.',
})}

${comparisonGrid({
  leftTitle: 'Traditional backend debugging',
  leftItems: [
    'logs — scattered across services',
    'metrics dashboard — no request context',
    'trace UI — requires manual SDK instrumentation',
    'DB console — query by query',
    'queue monitor — separate tool',
  ],
  rightTitle: 'Fluxbase',
  rightItems: [
    '<code>flux why &lt;id&gt;</code> — root cause, one command',
    '<code>flux trace &lt;id&gt;</code> — full span tree, latencies',
    '<code>flux state history</code> — every row mutation',
    '<code>flux incident replay</code> — safe re-execution',
    '<code>flux bug bisect</code> — which commit broke it',
  ],
})}`,
  });
}

// ── Feature grid ──────────────────────────────────────────────────────────────
function featuresSection() {
  const cards = [
    featureCard({ icon: '🔍', title: 'Time-Travel Debugging',   badge: 'flux trace debug', body: 'Step through a production request span by span. See the exact input, output, and state at every point in execution.' }),
    featureCard({ icon: '📜', title: 'Mutation History',         badge: 'flux state history', body: 'Every database write is logged with its request ID. See the full history of any row — what changed, when, and which request caused it.' }),
    featureCard({ icon: '♻️', title: 'Incident Replay',          badge: 'flux incident replay', body: 'Re-run a production time window with side-effects disabled. Test your fix against exactly the requests that caused the incident.' }),
    featureCard({ icon: '🔎', title: 'Regression Detection',     badge: 'flux bug bisect', body: 'Binary-searches your git history to find the first commit where a request started failing. Like <code>git bisect</code>, but for production behaviour.' }),
    featureCard({ icon: '🛡️', title: 'Deterministic Execution',  badge: 'recorded by default', body: 'Every request captures a complete trace automatically — no instrumentation, no SDKs, no config. The runtime produces the trace.' }),
    featureCard({ icon: '🔷', title: 'Observable by Construction', badge: 'zero config', body: 'Gateway, functions, database queries, tool calls, async jobs — every layer emits spans automatically. <code>flux trace</code> reconstructs the full picture.' }),
  ].join('\n    ');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Capabilities' })}
${sectionHeader({
  heading: 'Production debugging, not just monitoring.',
  sub: 'Monitoring tells you something is wrong. Fluxbase tells you why, shows you what changed, and lets you replay it.',
})}
<div class="feature-grid" style="padding-bottom:0;">
  ${cards}
</div>`,
  });
}

// ── Architecture teaser ───────────────────────────────────────────────────────
function architectureTeaser() {
  const window = codeWindow({
    title: 'flux trace 91a3f',
    content: `${c.cmd('$')} flux trace ${c.id('91a3f')}

  Trace ${c.id('91a3f')}  ${c.dim('2026-03-10 14:22 UTC')}
  ${c.dim('POST /create_order · 200 OK')}

  ${c.fn('gateway')}                      ${c.ms('2ms')}
  ${c.fn('└─ create_order')}              ${c.ms('8ms')}
  ${c.db('   ├─ db.insert(orders)')}       ${c.ms('4ms')}
  ${c.db('   ├─ stripe.charge')}           ${c.ms('180ms')}
  ${c.err('   └─ send_slack')}              ${c.err('error: rate limited')}

  ${c.dim('── Suggestion ──────────────────────────')}
  ${c.ok('→ Move send_slack to async background step')}`,
  });

  return section({
    content: `${eyebrow({ text: 'How It Works', color: 'muted' })}
${sectionHeader({
  heading: 'One request ID covers the entire stack.',
  sub: 'Client → Gateway → Runtime → Data Engine → Your PostgreSQL. Every hop records a span. <code>flux trace</code> reassembles them in order.',
})}

<div class="grid-2col" style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:center;">
  <div style="display:flex;flex-direction:column;gap:12px;">
    ${['Client', 'Gateway', 'Runtime', 'Data Engine', 'Your PostgreSQL'].map((layer, i) => {
      const isMiddle = i > 0 && i < 4;
      const border = isMiddle ? 'var(--border)' : 'var(--accent)';
      const bg = isMiddle ? 'var(--bg-surface)' : 'var(--accent-dim)';
      return `    <div style="display:flex;align-items:center;justify-content:space-between;padding:14px 18px;border:1px solid ${border};border-radius:8px;background:${bg};">
      <span style="font-size:.9rem;font-weight:600;">${layer}</span>
      ${i > 0 ? `<span style="font-size:.73rem;font-family:var(--font-mono);color:var(--accent);background:var(--accent-dim);padding:3px 10px;border-radius:20px;">→ span</span>` : ''}
    </div>`;
    }).join('\n')}
    <a class="btn-secondary" href="/how-it-works" style="display:inline-block;margin-top:8px;font-size:.85rem;text-align:center;">Full architecture →</a>
  </div>
  <div>${window}</div>
</div>`,
  });
}

// ── Why Fluxbase Works ─────────────────────────────────────────────────────────
function whyItWorks() {
  const points = [
    {
      icon: '📝',
      title: 'Every request is recorded.',
      body: 'The gateway captures inputs, outputs, and metadata for every HTTP request. No SDK. No instrumentation. No config.',
    },
    {
      icon: '📃',
      title: 'Every mutation is logged.',
      body: 'When your function writes to PostgreSQL, the Data Engine intercepts it and stores the row diff with its <code>request_id</code>. Your database is auditable by default.',
    },
    {
      icon: '🔬',
      title: 'Every execution span is traced.',
      body: 'Gateway, runtime, DB queries, tool calls, async jobs — each layer emits spans automatically. <code>flux trace</code> reassembles the full picture from a single ID.',
    },
    {
      icon: '♻️',
      title: 'Production can be replayed safely.',
      body: 'Because inputs and state are captured, any time window can be re-executed against your current code. Side-effects are disabled. Your fix is tested against real production traffic.',
    },
  ];

  const cards = points.map(p => `<div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;padding:22px 24px;">
    <div style="font-size:1.4rem;margin-bottom:10px;">${p.icon}</div>
    <h3 style="font-size:.95rem;font-weight:700;margin-bottom:8px;">${p.title}</h3>
    <p style="font-size:.85rem;color:var(--muted);line-height:1.7;">${p.body}</p>
  </div>`).join('\n  ');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Why It Works', color: 'green' })}
${sectionHeader({
  heading: 'The system is designed to be observable.',
  sub: 'There is no "add tracing later" checkbox. Observability is how Fluxbase executes your code, not a feature you bolt on.',
  maxWidth: '560px',
})}
<div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:16px;">
  ${cards}
</div>`,
  });
}

// ── CTA ──────────────────────────────────────────────────────────────────────
function ctaSection() {
  const steps = [
    { label: 'Homepage',           note: 'understand the product',      href: '/' },
    { label: 'Quickstart',         note: 'deploy + debug in 5 minutes',  href: '/docs/quickstart' },
    { label: 'Debugging Guide',    note: 'the 4-command workflow',       href: '/docs/debugging-production' },
    { label: 'Core Concepts',      note: 'understand why it works',      href: '/docs/concepts' },
    { label: 'CLI Reference',      note: 'every command, with examples', href: '/cli' },
  ];

  const ladder = steps.map((s, i) =>
    `<div style="display:flex;align-items:center;gap:10px;">
      <a href="${s.href}" style="display:flex;align-items:center;gap:10px;text-decoration:none;color:inherit;flex:1;">
        <span style="width:22px;height:22px;border-radius:50%;background:rgba(255,255,255,.12);color:#fff;font-size:.68rem;font-weight:800;display:inline-flex;align-items:center;justify-content:center;flex-shrink:0;">${i + 1}</span>
        <span style="font-weight:600;font-size:.9rem;">${s.label}</span>
        <span style="font-size:.8rem;opacity:.55;">— ${s.note}</span>
      </a>
    </div>${i < steps.length - 1 ? '<div style="padding-left:11px;height:16px;border-left:1px dashed rgba(255,255,255,.2);"></div>' : ''}`
  ).join('\n    ');

  return `<section class="cta-strip" style="text-align:left;">
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:64px;align-items:start;max-width:900px;margin:0 auto;">
    <div>
      <h2 style="text-align:left;">Start debugging production in 5 minutes.</h2>
      <p style="max-width:400px;margin:0 0 28px;opacity:.75;">Install the CLI, deploy your first function, and get a full trace end-to-end before you finish the quickstart.</p>
      <div style="display:flex;gap:12px;flex-wrap:wrap;margin-bottom:24px;">
        <a class="btn-primary" href="/docs/quickstart">Start the quickstart →</a>
        <a class="btn-secondary" href="/product">See all features</a>
      </div>
      <div class="install-hint" style="margin:0;">
        <span class="prompt">$</span>
        curl -fsSL https://fluxbase.co/install | bash
      </div>
    </div>
    <div>
      <div style="font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;opacity:.5;margin-bottom:16px;">Learning path</div>
      ${ladder}
    </div>
  </div>
</section>`;
}

// ── Page styles ───────────────────────────────────────────────────────────────
const extraHead = '';

// ── Render ────────────────────────────────────────────────────────────────────
export function render() {
  const content = [
    hero(),
    demoSection(),
    comparisonSection(),
    featuresSection(),
    whyItWorks(),
    architectureTeaser(),
    ctaSection(),
  ].join('\n\n');

  return landingLayout({
    meta,
    active: 'home',
    extraHead,
    content,
  });
}
