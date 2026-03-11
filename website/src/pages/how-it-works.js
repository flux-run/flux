/**
 * How It Works — architecture walkthrough.
 */
import { landingLayout }    from '../layouts/landing.js';
import { codeWindow, c }    from '../components/code-window.js';
import { eyebrow, section, sectionHeader } from '../components/section.js';

export const meta = {
  title:       'How It Works — Fluxbase',
  description: 'Request capture, mutation logging, trace graph, and deterministic replay. How the Fluxbase runtime turns every request into a queryable production record.',
  path:        'how-it-works.html',
};

// ── Hero ──────────────────────────────────────────────────────────────────────
function hero() {
  return `<section class="hero" style="padding-bottom:48px;">
  <span class="eyebrow">How It Works</span>
  <h1 style="font-size:clamp(2rem,5vw,3rem);">One request ID.<br><span class="gradient-text">The entire stack.</span></h1>
  <p style="max-width:560px;margin:0 auto 32px;">Fluxbase adds a recording and replay layer around your execution stack. Every layer — from the gateway to the database — emits structured spans tied to one request ID. The CLI reassembles them on demand.</p>
  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="/docs/quickstart">Try it →</a>
    <a class="btn-secondary" href="/product">See the features</a>
  </div>
</section>`;
}

// ── Architecture diagram ──────────────────────────────────────────────────────
function architectureDiagram() {
  const layers = [
    { emoji: '👤', label: 'Client',         note: 'Any HTTP client',                      isUser: true  },
    { emoji: '🛡️', label: 'Gateway',        note: 'auth · rate limit · route → span',      isUser: false },
    { emoji: '⚡', label: 'Runtime',         note: 'your TypeScript code → span',           isUser: false },
    { emoji: '🗄️', label: 'Data Engine',    note: 'query compiler · policy · SQL → span',  isUser: false },
    { emoji: '🐘', label: 'Your PostgreSQL', note: 'standard Postgres, you own the data',   isUser: true  },
  ];

  const nodes = layers.map((layer, i) => {
    const border = layer.isUser ? 'var(--border)' : 'var(--accent)';
    const bg     = layer.isUser ? 'var(--bg-surface)' : 'var(--accent-dim)';

    const node = `<div style="display:flex;align-items:center;gap:16px;padding:16px 20px;border:1px solid ${border};border-radius:8px;background:${bg};">
      <span style="font-size:1.2rem;">${layer.emoji}</span>
      <div>
        <div style="font-size:.9rem;font-weight:700;">${layer.label}</div>
        <div style="font-size:.76rem;color:var(--muted);">${layer.note}</div>
      </div>
    </div>`;

    const arrow = i < layers.length - 1
      ? `<div style="display:flex;padding:0 28px;align-items:stretch;height:20px;"><div style="width:2px;background:var(--border);"></div></div>`
      : '';

    return node + (arrow ? '\n    ' + arrow : '');
  }).join('\n    ');

  const traceWindow = codeWindow({
    title: 'flux trace 4f9a3b2c',
    content: `${c.cmd('$')} flux trace ${c.id('4f9a3b2c')}

  Trace ${c.id('4f9a3b2c')}  ${c.dim('POST /create_user  200')}

  ${c.fn('▸ gateway')}                     ${c.ms('3ms')}
    ${c.dim('auth ✔  rate_limit ✔  cors ✔')}

  ${c.fn('▸ create_user')}                 ${c.ms('81ms')}
    ${c.db('▸ db:select(users)')}           ${c.ms('11ms')}
    ${c.db('▸ db:insert(users)')}           ${c.ms('14ms')}

  ${c.fn('▸ send_welcome')}  ${c.dim('async →')}  ${c.ms('queued')}

  ${c.dim('── total: 98ms ─────────────────────')}`,
  });

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'The Architecture' })}
${sectionHeader({
  heading: 'Five layers. One trace.',
  sub: 'Every layer is instrumented at the runtime level — no application-level tracing hooks needed. The span data is stored in the Data Engine alongside mutation logs.',
})}

<div class="grid-2col" style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:center;">
  <div style="display:flex;flex-direction:column;gap:0;">
    ${nodes}
    <p style="font-size:.78rem;color:var(--muted);margin-top:16px;">* Arrows represent HTTP/internal calls. Each hop produces a span stored in the trace store.</p>
  </div>
  <div>${traceWindow}</div>
</div>`,
  });
}

// ── Step-by-step ──────────────────────────────────────────────────────────────
function stepByStep() {
  const steps = [
    {
      num: '1',
      heading: 'Request capture',
      body: 'When a request arrives at the Gateway, Fluxbase assigns it a globally unique request ID (UUID v4). This ID is propagated via internal headers to every downstream service. The gateway records auth result, rate-limit decision, matched route, and timing as the first span.',
      code: codeWindow({
        title: 'gateway → span',
        content: `${c.dim('# Gateway emits:')}
{
  request_id: ${c.id('"4f9a3b2c"')},
  span: ${c.ok('"gateway"')},
  method: ${c.ok('"POST"')},
  path: ${c.ok('"/create_user"')},
  auth: ${c.ok('"ok"')},
  duration_ms: 3
}`,
      }),
    },
    {
      num: '2',
      heading: 'Function execution',
      body: 'The Runtime receives the request with the forwarded request ID. It executes your TypeScript function in a sandboxed V8 isolate. Every <code>ctx.db</code>, <code>ctx.tool</code>, and <code>ctx.workflow</code> call is intercepted and recorded as a child span under the function span.',
      code: codeWindow({
        title: 'runtime → spans',
        content: `${c.dim('# Runtime emits per call:')}
{
  request_id: ${c.id('"4f9a3b2c"')},
  span: ${c.ok('"create_user"')},
  children: [
    { span: ${c.db('"db:select(users)"')}, ${c.ms('11ms')} },
    { span: ${c.db('"db:insert(users)"')}, ${c.ms('14ms')} }
  ],
  duration_ms: 81
}`,
      }),
    },
    {
      num: '3',
      heading: 'Mutation logging',
      body: 'Every database write goes through the Data Engine, which applies schema validation, column policies, and row-level security before executing the SQL. After execution, it writes a mutation record: which table, which row, old value, new value, and the request ID that caused it.<br><br><span style="font-size:.85rem;color:var(--muted);"><strong style="color:var(--text);">Invariant:</strong> All database writes must pass through the Data Engine for mutation history to remain complete. Writes made directly to Postgres — bypassing the Data Engine — are not recorded.</span>',
      code: codeWindow({
        title: 'data engine → mutation log',
        content: `${c.dim('# Mutation record:')}
{
  request_id: ${c.id('"4f9a3b2c"')},
  table: ${c.ok('"users"')},
  row_id: 42,
  operation: ${c.ok('"insert"')},
  data: { email: ${c.ok('"a@b.com"')}, plan: ${c.ok('"free"')} },
  timestamp: ${c.dim('"2026-03-10T14:22:01Z"')}
}`,
      }),
    },
    {
      num: '4',
      heading: 'Trace graph',
      body: 'All spans for a request ID are stored in an ordered graph. <code>flux trace &lt;id&gt;</code> retrieves them and renders the full tree — gateway, function, database queries, tool calls — in execution order with latencies.',
      code: codeWindow({
        title: 'trace store → rendered',
        content: `${c.cmd('$')} flux trace 4f9a3b2c

  ${c.fn('gateway')}           ${c.ms('3ms')}
  ${c.fn('create_user')}      ${c.ms('81ms')}
    ${c.db('db:select')}       ${c.ms('11ms')}
    ${c.db('db:insert')}       ${c.ms('14ms')}
  ${c.fn('send_welcome')}  ${c.dim('async → queued')}

  ${c.dim('total: 98ms')}`,
      }),
    },
    {
      num: '5',
      heading: 'Deterministic replay',
      body: 'Because every span includes its full input and output, any request can be replayed against the current code. <code>flux incident replay</code> re-executes with side-effects disabled. <code>flux bug bisect</code> replays across your git history to find regressions.<br><br><span style="font-size:.85rem;color:var(--muted);"><strong style="color:var(--text);">What replay guarantees:</strong> database state transitions are reproduced exactly. External side effects — emails, webhooks, third-party API calls — are skipped during replay. Non-deterministic values like <code>random()</code> or <code>Date.now()</code> inside your own code may differ; the recorded data state will not.</span>',
      code: codeWindow({
        title: 'replay — side-effects off',
        content: `${c.cmd('$')} flux incident replay 14:00..14:05

  ${c.dim('hooks: off · events: off · cron: off')}
  ${c.dim('db writes: on · mutation log: on')}

  ${c.ok('✔')} 22/23 passing
  ${c.err('✗')}  req:550e8400 still fails
     ${c.err('Stripe timeout at payments/create.ts:42')}`,
      }),
    },
  ];

  const stepEls = steps.map(step => `
  <div style="display:grid;grid-template-columns:48px 1fr 1fr;gap:32px;align-items:start;padding:40px 0;border-bottom:1px solid var(--border);">
    <div style="width:36px;height:36px;border-radius:50%;background:var(--accent);color:#fff;font-size:.85rem;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0;margin-top:2px;">${step.num}</div>
    <div>
      <h3 style="font-size:1.05rem;font-weight:700;margin-bottom:10px;">${step.heading}</h3>
      <p style="font-size:.9rem;color:var(--muted);line-height:1.65;margin:0;">${step.body}</p>
    </div>
    <div>${step.code}</div>
  </div>`).join('');

  return section({
    content: `${eyebrow({ text: 'Step by Step' })}
${sectionHeader({ heading: 'What happens when a request runs.' })}
<div style="display:flex;flex-direction:column;">${stepEls}
</div>`,
  });
}

// ── Why a runtime? ────────────────────────────────────────────────────────────
function whyRuntime() {
  const invariants = [
    {
      num: '1',
      color: 'var(--accent)',
      title: 'Execution interception',
      body: 'Fluxbase must intercept every <code>ctx.db</code>, <code>ctx.tool</code>, <code>ctx.workflow</code>, and <code>ctx.event</code> call to produce a complete trace. If arbitrary code can run outside the runtime, calls are invisible — and the execution graph has gaps.',
      code: codeWindow({
        title: 'intercepted at the runtime level',
        content: `${c.dim('# Every call is intercepted — no SDK needed')}

  ctx.db.insert(users, { email })
  ${c.fn('→')} compile + validate     ${c.dim('data engine')}
  ${c.fn('→')} execute SQL            ${c.dim('your postgres')}
  ${c.fn('→')} write mutation diff    ${c.dim('trace store')}
  ${c.fn('→')} emit span              ${c.dim('trace store')}

${c.dim('# If this call were outside the runtime:')}

  ${c.err('→ no diff recorded')}
  ${c.err('→ no span emitted')}
  ${c.err('→ flux state history broken')}
  ${c.err('→ flux incident replay incomplete')}`,
      }),
    },
    {
      num: '2',
      color: '#60a5fa',
      title: 'Mutation completeness',
      body: 'The mutation log is only trustworthy if <em>every</em> write passes through the Data Engine. A proxy or library can guarantee this for code that uses it — but not for direct writes, migration scripts, or other services hitting the same Postgres. Runtime ownership closes that gap.',
      code: codeWindow({
        title: 'the mutation invariant',
        content: `${c.dim('# Required for state history + replay:')}

  ALL DB writes → Data Engine → Postgres
                           ↓
                    mutation diff written

${c.ok('✔')} same-transaction: your write + diff
    both commit or both roll back

${c.dim('# What breaks without this:')}

  ${c.err('flux state history')}   — incomplete row timeline
  ${c.err('flux state blame')}     — attribution missing
  ${c.err('flux incident replay')} — replays against wrong state`,
      }),
    },
    {
      num: '3',
      color: '#c084fc',
      title: 'Replay safety',
      body: 'Replay must re-execute production requests against your current code with all external side-effects disabled — no emails sent, no webhooks fired, no Stripe charges. This is only possible if the runtime controls execution. A sidecar or agent cannot safely intercept and suppress arbitrary outbound calls.',
      code: codeWindow({
        title: 'replay mode — runtime-controlled',
        content: `${c.cmd('$')} flux incident replay 14:00..14:05

  ${c.dim('Runtime flags active for this replay:')}

  hooks:     ${c.err('off')}   ${c.dim('← email, Slack, webhooks')}
  events:    ${c.err('off')}   ${c.dim('← external event bus')}
  cron:      ${c.err('off')}   ${c.dim('← scheduled triggers')}
  db writes: ${c.ok('on')}    ${c.dim('← mutations recorded')}
  spans:     ${c.ok('on')}    ${c.dim('← new trace produced')}

${c.dim('# Side-effects are suppressed at the call site,')}
${c.dim('# not at the network layer. Only runtime')}
${c.ok('# interception makes this reliable.')}`,
      }),
    },
  ];

  const cards = invariants.map(inv => `<div style="display:grid;grid-template-columns:1fr 1fr;gap:32px;align-items:start;padding:40px 0;border-top:1px solid var(--border);"> 
    <div>
      <div style="display:flex;align-items:center;gap:12px;margin-bottom:12px;">
        <span style="width:32px;height:32px;border-radius:50%;background:${inv.color};color:#fff;font-size:.8rem;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0;">${inv.num}</span>
        <h3 style="font-size:1.05rem;font-weight:700;margin:0;color:${inv.color};">${inv.title}</h3>
      </div>
      <p style="font-size:.9rem;color:var(--muted);line-height:1.7;margin:0;">${inv.body}</p>
    </div>
    <div>${inv.code}</div>
  </div>`).join('\n  ');

  return section({
    id: 'why-runtime',
    content: `${eyebrow({ text: 'Why a Runtime?' })}
${sectionHeader({
  heading: 'Why Fluxbase must own the runtime.',
  sub: 'The most common question from senior engineers: "Why can\'t this work as a library or proxy?" Three invariants require runtime ownership to be guaranteed.',
  maxWidth: '620px',
})}
<div style="display:flex;flex-direction:column;">
  ${cards}
</div>
<div style="margin-top:40px;padding:22px 26px;background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;max-width:720px;">
  <p style="font-weight:700;margin:0 0 8px;font-size:.95rem;">The tradeoff is intentional.</p>
  <p style="color:var(--muted);font-size:.88rem;line-height:1.7;margin:0 0 12px;">Runtime ownership introduces adoption friction — you must run your functions on Fluxbase rather than integrating with an existing backend. In exchange, you get guarantees that a library or proxy cannot provide: complete execution history, reliable mutation tracking, and safe deterministic replay. The bet is that the debugging power is worth the deployment step.</p>
  <p style="font-size:.85rem;margin:0;"><strong>You can self-host the entire stack.</strong> All Fluxbase services are open source. Run them on your own infrastructure with Docker Compose, in your own region, with your own Postgres. <a href="/docs/deployment" style="color:var(--accent);">Deployment options →</a></p>
</div>`,
  });
}

// ── Performance overhead ─────────────────────────────────────────────────────
function performanceOverhead() {
  const perfWindow = codeWindow({
    title: 'per-request overhead breakdown',
    content: `${c.dim('# Time added by Fluxbase per request')}

  span recording      ${c.ms('~0.3 ms')}   ${c.dim('in-memory, async write')}
  mutation logging    ${c.ms('~0.7 ms')}   ${c.dim('one row, same tx as user write')}
  trace storage       ${c.ms('~0.4 ms')}   ${c.dim('fire-and-forget, non-blocking')}
                      ─────────
  total               ${c.ok('~1–3 ms')}   ${c.dim('p95 overhead')}

${c.dim('# Your user write and the mutation diff')}
${c.dim('# commit in the same Postgres transaction.')}

  user write     ─────────────────► Postgres
  mutation diff  ─── same tx  ────► trace store

${c.ok('# Slow trace write does not delay HTTP response.')}`,
  });

  const properties = [
    { icon: '⚡', label: 'In-memory spans', desc: 'Spans are assembled in-memory during execution. A single batch insert commits them after the response is sent.' },
    { icon: '📝', label: 'Same-transaction diffs', desc: 'Mutation diffs are appended in the same Postgres transaction as the user write — no extra round-trip to the database.' },
    { icon: '🔥', label: 'Fire-and-forget trace write', desc: 'The span batch insert is non-blocking. A slow trace write cannot delay the HTTP response to your client.' },
    { icon: '🚫', label: 'No cross-request locking', desc: 'Each request writes its own rows independently. There is no global lock, no shared state between concurrent requests.' },
  ];

  const cards = properties.map(p => `<div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;padding:20px 22px;">
    <div style="display:flex;align-items:center;gap:10px;margin-bottom:8px;">
      <span style="font-size:1.1rem;">${p.icon}</span>
      <strong style="font-size:.9rem;">${p.label}</strong>
    </div>
    <p style="font-size:.84rem;color:var(--muted);line-height:1.6;margin:0;">${p.desc}</p>
  </div>`).join('\n    ');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Performance' })}
${sectionHeader({
  heading: 'Typical overhead: 1–3 ms per request.',
  sub: 'Span recording is in-memory and non-blocking. Mutation logging shares the same Postgres transaction as your own write. No synchronous tracing pipeline. No global locks.',
  maxWidth: '620px',
})}
<div style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;" class="grid-2col">
  <div>${perfWindow}</div>
  <div style="display:grid;gap:12px;">
    ${cards}
  </div>
</div>`,
  });
}

// ── Technology stack ──────────────────────────────────────────────────────────
function techStack() {
  const modules = [
    { icon: '🛡️', name: 'Gateway',     tech: 'Rust (Axum)',    desc: 'Auth, rate limit (per-tenant token bucket), query guard, semaphore budget. Routes requests to Runtime or Data Engine.' },
    { icon: '⚡', name: 'Runtime',      tech: 'Rust + Deno V8', desc: 'Executes TypeScript in sandboxed V8 isolates per tenant. Warm isolates for low latency; per-tenant affinity to prevent cross-tenant heap contamination.' },
    { icon: '🗄️', name: 'Data Engine', tech: 'Rust (Axum)',    desc: 'Query compiler (JSON → SQL), column policies, row-level security, BYODB (Bring Your Own Database). Mutation log writer and explain endpoint.' },
    { icon: '📬', name: 'Queue',        tech: 'Rust',           desc: 'Durable async job queue. Stores job payloads in Postgres. Workers poll and execute functions from the Runtime. Fully traced.' },
    { icon: '🔌', name: 'API',          tech: 'Rust (Axum)',    desc: 'Management API: deploy functions, manage schemas, API keys, tenant config. Consumed by the CLI and Dashboard.' },
  ];

  const cards = modules.map(m => `<div style="background:var(--bg-surface);border:1px solid var(--border);border-radius:10px;padding:24px;">
    <div style="display:flex;align-items:center;gap:12px;margin-bottom:12px;">
      <span style="font-size:1.2rem;">${m.icon}</span>
      <div>
        <div style="font-weight:700;font-size:.95rem;">${m.name}</div>
        <div style="font-size:.75rem;font-family:var(--font-mono);color:var(--accent);">${m.tech}</div>
      </div>
    </div>
    <p style="font-size:.85rem;color:var(--muted);line-height:1.6;margin:0;">${m.desc}</p>
  </div>`).join('\n  ');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Technology', color: 'muted' })}
${sectionHeader({
  heading: 'Open foundations, high-performance core.',
  sub: 'Every service is written in Rust for predictable latency and memory safety. Your TypeScript functions run in Deno V8 isolates. Your data stays in standard Postgres.',
  maxWidth: '600px',
})}
<div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:16px;">
  ${cards}
</div>`,
  });
}

// ── CTA ───────────────────────────────────────────────────────────────────────
function cta() {
  return `<section class="cta-strip">
  <h2>See it in action.</h2>
  <p style="max-width:480px;margin:0 auto 32px;">The quickstart takes 5 minutes. You deploy a function, trigger a request, and trace it end to end from the CLI.</p>
  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="/docs/quickstart">Start the Quickstart →</a>
    <a class="btn-secondary" href="/cli">CLI Reference</a>
  </div>
</section>`;
}

// ── Page styles ───────────────────────────────────────────────────────────────
const extraHead = `<style>
  @media (max-width: 760px) {
    [style*="grid-template-columns:48px"] { grid-template-columns: 1fr !important; }
    [style*="grid-template-columns:48px"] > div:first-child { display: none; }
  }
</style>`;

// ── Render ────────────────────────────────────────────────────────────────────
export function render() {
  const content = [
    hero(),
    architectureDiagram(),
    stepByStep(),
    whyRuntime(),
    performanceOverhead(),
    techStack(),
    cta(),
  ].join('\n\n');

  return landingLayout({ meta, active: 'how-it-works', extraHead, content });
}
