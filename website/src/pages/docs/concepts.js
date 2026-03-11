/**
 * Core Concepts — mental model every developer needs before the CLI makes sense.
 */
import { docsLayout } from '../../layouts/docs.js';
import { codeWindow, c } from '../../components/code-window.js';
import { eyebrow, sectionHeader } from '../../components/section.js';

export const meta = {
  title:       'Core Concepts — Fluxbase',
  description: 'Understand the three ideas behind Fluxbase: requests are executions, data changes are mutations, and everything is traceable. The mental model that makes the CLI intuitive.',
  path:        'docs/concepts.html',
};

function conceptBlock({ anchor, number, title, tagline, body, window: win }) {
  return `<div id="${anchor}" style="padding:56px 0;border-bottom:1px solid var(--border);">
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;">
    <div>
      <div style="display:flex;align-items:center;gap:10px;margin-bottom:16px;">
        <span style="width:28px;height:28px;border-radius:50%;background:var(--accent);color:#000;font-size:.78rem;font-weight:800;display:inline-flex;align-items:center;justify-content:center;flex-shrink:0;">${number}</span>
        <span style="font-size:.72rem;font-weight:600;text-transform:uppercase;letter-spacing:.1em;color:var(--accent);">${tagline}</span>
      </div>
      <h2 style="font-size:1.5rem;font-weight:800;margin-bottom:16px;">${title}</h2>
      ${body}
    </div>
    <div style="position:sticky;top:24px;">${win}</div>
  </div>
</div>`;
}

function concept1() {
  const win = codeWindow({
    title: 'execution record',
    content: `${c.cmd('$')} flux why ${c.id('550e8400')}

  ${c.white('Execution')}  ${c.id('550e8400')}
  ${c.dim('POST /signup  →  function: create_user')}
  ${c.dim('2026-03-10 14:22:31 UTC  ·  44ms')}

  ${c.white('Spans')}
  ${c.fn('create_user')}          ${c.ms('44ms')}
  ${c.db('  db.insert(users)')}    ${c.ms('3ms')}
  ${c.db('  stripe.create')}       ${c.err('timeout')}

  ${c.white('Status')}  ${c.err('500 — Stripe API timeout')}`,
  });

  return conceptBlock({
    anchor: 'execution',
    number: '1',
    tagline: 'Requests are executions',
    title: 'Every HTTP request is a recorded execution',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">When a request hits Fluxbase, the gateway captures it and the runtime executes your function inside a V8 isolate. That entire round-trip — inputs, outputs, spans, tool calls, DB queries — is stored as a single <strong style="color:var(--text);">execution record</strong>.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:20px;">The execution record is identified by a <code>request_id</code>. Every CLI command that debugs a request takes this ID.</p>
<div style="background:var(--accent-dim);border:1px solid var(--accent);border-radius:8px;padding:16px 18px;">
  <div style="font-size:.78rem;font-weight:700;color:var(--accent);margin-bottom:10px;text-transform:uppercase;letter-spacing:.08em;">CLI commands that use request_id</div>
  <div style="display:flex;flex-direction:column;gap:6px;font-family:var(--font-mono);font-size:.8rem;">
    <div><span style="color:var(--green);">flux why</span> <span style="color:var(--muted);">&lt;id&gt;        — root cause</span></div>
    <div><span style="color:var(--green);">flux trace</span> <span style="color:var(--muted);">&lt;id&gt;      — full span tree</span></div>
    <div><span style="color:var(--green);">flux trace debug</span> <span style="color:var(--muted);">&lt;id&gt; — step through spans</span></div>
    <div><span style="color:var(--green);">flux trace diff</span> <span style="color:var(--muted);">&lt;a&gt; &lt;b&gt; — compare two</span></div>
  </div>
</div>`,
    window: win,
  });
}

function concept2() {
  const win = codeWindow({
    title: 'mutation log',
    content: `${c.cmd('$')} flux state history ${c.db('users')} --id 42

  ${c.white('Row')}  users / id = 42
  ${c.dim('────────────────────────────────────────')}

  ${c.dim('2026-03-10 14:20:11')}  ${c.id('req:4f9a3b2c')}
  ${c.ok('+')} email    → alice@example.com
  ${c.ok('+')} plan     → free

  ${c.dim('2026-03-10 14:22:31')}  ${c.id('req:550e8400')}
  ${c.err('✗')} plan     → ${c.err('null (Stripe timeout rolled back)')}

  ${c.dim('2026-03-10 14:23:18')}  ${c.id('req:7b1d3f9a')}
  ${c.ok('✔')} plan     → pro`,
  });

  return conceptBlock({
    anchor: 'mutation-log',
    number: '2',
    tagline: 'Data changes are mutations',
    title: 'Every database write is a logged mutation',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">When your function writes to PostgreSQL, Fluxbase's Data Engine intercepts the write and records it in the <strong style="color:var(--text);">mutation log</strong>. Each entry carries the row that changed, the old value, the new value, and — critically — the <code>request_id</code> that caused it.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:20px;">This makes your database auditable by default. You never need to write <code>audit_log</code> tables manually.</p>
<div style="background:var(--accent-dim);border:1px solid var(--accent);border-radius:8px;padding:16px 18px;">
  <div style="font-size:.78rem;font-weight:700;color:var(--accent);margin-bottom:10px;text-transform:uppercase;letter-spacing:.08em;">CLI commands that query mutations</div>
  <div style="display:flex;flex-direction:column;gap:6px;font-family:var(--font-mono);font-size:.8rem;">
    <div><span style="color:var(--green);">flux state history</span> <span style="color:var(--muted);">&lt;table&gt; — row history</span></div>
    <div><span style="color:var(--green);">flux state blame</span> <span style="color:var(--muted);">&lt;table&gt;  — who changed it</span></div>
  </div>
</div>`,
    window: win,
  });
}

function concept3() {
  const win = codeWindow({
    title: 'trace graph',
    content: `${c.cmd('$')} flux trace ${c.id('91a3f')}

  ${c.white('Trace Graph')}  ${c.id('91a3f')}
  ${c.dim('POST /create_order · 200 OK · 194ms')}

  ${c.fn('gateway')}                         ${c.ms('2ms')}
  ${c.fn('└─ create_order')}                 ${c.ms('8ms')}
  ${c.db('   ├─ db.insert(orders)')}          ${c.ms('4ms')}
  ${c.fn('   ├─')} ${c.purple('stripe.charge')}             ${c.ms('180ms')}
  ${c.fn('   └─')} ${c.err('slack.notify')}                 ${c.err('429')}

  ${c.dim('── Spans: 5  ·  Errors: 1  ·  DB writes: 1')}`,
  });

  return conceptBlock({
    anchor: 'trace-graph',
    number: '3',
    tagline: 'Everything is traceable',
    title: 'Every layer emits spans — automatically',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">The gateway, runtime, data engine, tool calls, async jobs — every layer in Fluxbase emits spans without any instrumentation from you. Those spans are assembled into a <strong style="color:var(--text);">trace graph</strong> keyed by <code>request_id</code>.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:20px;">The trace graph is not just for reading. You can replay it, diff it against a different request, bisect your commit history with it, or step through it interactively.</p>
<div style="background:var(--accent-dim);border:1px solid var(--accent);border-radius:8px;padding:16px 18px;">
  <div style="font-size:.78rem;font-weight:700;color:var(--accent);margin-bottom:10px;text-transform:uppercase;letter-spacing:.08em;">CLI commands that use the trace graph</div>
  <div style="display:flex;flex-direction:column;gap:6px;font-family:var(--font-mono);font-size:.8rem;">
    <div><span style="color:var(--green);">flux incident replay</span> <span style="color:var(--muted);">— safe re-execution</span></div>
    <div><span style="color:var(--green);">flux bug bisect</span> <span style="color:var(--muted);">    — find breaking commit</span></div>
    <div><span style="color:var(--green);">flux explain</span> <span style="color:var(--muted);">       — AI analysis</span></div>
  </div>
</div>`,
    window: win,
  });
}

function concept4() {
  const win = codeWindow({
    title: 'replay engine',
    content: `${c.cmd('$')} flux incident replay --window 14:20-14:25

  ${c.white('Replay Window')}  2026-03-10 14:20 → 14:25
  ${c.dim('3 requests captured')}

  ${c.ok('✔')} Replaying ${c.id('4f9a3b2c')} … ${c.ok('200')}  ${c.ms('82ms')}
  ${c.err('✗')} Replaying ${c.id('550e8400')} … ${c.err('500')}  ${c.ms('44ms')}
  ${c.ok('✔')} Replaying ${c.id('7b1d3f9a')} … ${c.ok('201')}  ${c.ms('91ms')}

  ${c.dim('Side-effects disabled: email, Slack, Stripe')}
  ${c.dim('x-flux-replay: true sent on each request')}
  ${c.ok('→')} 1 failure reproduced. Run ${c.white('flux why 550e8400')} to debug.`,
  });

  return conceptBlock({
    anchor: 'replay-engine',
    number: '4',
    tagline: 'Deterministic replay',
    title: 'Production can be replayed safely',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">Because every execution is recorded — inputs, state, secrets, timing — Fluxbase can re-run a production time window against your current code with side-effects disabled. Emails won't send. Payments won't process. Slack won't fire.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:20px;">This is the <strong style="color:var(--text);">Replay Engine</strong>. It is how you test a fix against exactly the production traffic that caused an incident, without touching production.</p>
<div style="background:var(--accent-dim);border:1px solid var(--accent);border-radius:8px;padding:16px 18px;">
  <div style="font-size:.78rem;font-weight:700;color:var(--accent);margin-bottom:10px;text-transform:uppercase;letter-spacing:.08em;">How replay safety works</div>
  <div style="display:flex;flex-direction:column;gap:6px;font-family:var(--font-mono);font-size:.8rem;color:var(--muted);">
    <div>Each replayed request carries <span style="color:var(--text);">x-flux-replay: true</span></div>
    <div>Your functions can check this header to disable side-effects</div>
    <div>The runtime also suppresses outbound webhooks automatically</div>
  </div>
</div>`,
    window: win,
  });
}

function systemDiagram() {
  const nodes = [
    { label: 'Client Request',      sub: 'HTTP / webhook / event',        color: 'var(--border)',  accent: 'var(--muted)' },
    { label: 'Gateway',             sub: 'captures request + metadata',    color: 'var(--accent)',  accent: 'var(--accent)', tag: 'trace_requests' },
    { label: 'Runtime',             sub: 'executes function in V8 isolate', color: 'var(--accent)',  accent: 'var(--accent)', tag: 'runtime spans' },
    { label: 'Data Engine',         sub: 'intercepts DB writes',            color: 'var(--accent)',  accent: 'var(--accent)', tag: 'state_mutations' },
    { label: 'Your PostgreSQL',     sub: 'source of truth',                color: 'var(--border)',  accent: 'var(--muted)' },
  ];

  const arrow = `<div style="display:flex;align-items:center;gap:0;padding:0 32px;">
    <div style="flex:1;height:1px;background:var(--border);"></div>
    <span style="color:var(--muted);font-size:.75rem;">&#9660;</span>
  </div>`;

  const boxes = nodes.map((n, i) => {
    const isHighlighted = i > 0 && i < 4;
    return `<div style="display:flex;align-items:center;justify-content:space-between;padding:14px 20px;border:1px solid ${n.color};border-radius:8px;background:${isHighlighted ? 'var(--accent-dim)' : 'var(--bg-elevated)'}">
      <div>
        <div style="font-size:.88rem;font-weight:700;color:${isHighlighted ? 'var(--text)' : 'var(--muted)'}">${n.label}</div>
        <div style="font-size:.74rem;color:var(--muted);margin-top:2px;">${n.sub}</div>
      </div>
      ${n.tag ? `<span style="font-family:var(--font-mono);font-size:.68rem;color:${n.accent};background:var(--accent-dim);border:1px solid var(--accent);padding:2px 10px;border-radius:20px;white-space:nowrap;">${n.tag}</span>` : ''}
    </div>`;
  }).join(`\n    ${arrow}\n    `);

  const dataFlows = [
    { table: 'trace_requests',  color: 'var(--accent)',         desc: 'One row per request: id, tenant, function, status, duration, spans JSON' },
    { table: 'state_mutations', color: '#60a5fa',               desc: 'One row per DB write: table, row id, old value, new value, request_id' },
    { table: 'runtime spans',   color: '#f9a8d4',               desc: 'Nested span tree: function calls, tool calls, async steps, latencies' },
  ];

  const flowItems = dataFlows.map(f =>
    `<div style="display:flex;align-items:flex-start;gap:10px;padding:12px 16px;background:var(--bg-elevated);border:1px solid var(--border);border-radius:8px;">
      <span style="font-family:var(--font-mono);font-size:.75rem;color:${f.color};white-space:nowrap;padding-top:1px;">${f.table}</span>
      <span style="font-size:.8rem;color:var(--muted);line-height:1.6;">${f.desc}</span>
    </div>`
  ).join('\n    ');

  return `<div style="margin:32px 0;padding:32px;background:var(--bg-surface);border:1px solid var(--border);border-radius:14px;">
  <div style="font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--muted);margin-bottom:20px;">System shape</div>
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:40px;align-items:start;">
    <div style="display:flex;flex-direction:column;gap:0;">
      ${boxes}
    </div>
    <div>
      <div style="font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--muted);margin-bottom:14px;">Data recorded at each layer</div>
      <div style="display:flex;flex-direction:column;gap:8px;">
        ${flowItems}
      </div>
      <div style="margin-top:20px;padding:14px 16px;background:var(--accent-dim);border:1px solid var(--accent);border-radius:8px;">
        <div style="font-size:.78rem;font-weight:700;color:var(--accent);margin-bottom:6px;">All data is keyed by request_id</div>
        <div style="font-size:.8rem;color:var(--muted);line-height:1.6;">Every row in every table carries the <code>request_id</code> that caused it. That single ID unlocks the entire debugging workflow.</div>
      </div>
    </div>
  </div>
</div>`;
}

export function render() {
  const content = `
<div style="padding:48px 0 24px;">
  <div style="max-width:640px;">
    ${eyebrow({ text: 'Core Concepts' })}
    <h1 style="font-size:clamp(1.8rem,4vw,2.8rem);font-weight:800;margin-bottom:16px;">The mental model.</h1>
    <p style="font-size:1.05rem;color:var(--muted);line-height:1.8;">Three ideas explain how Fluxbase works. Once you have them, every CLI command becomes obvious.</p>
  </div>

  ${systemDiagram()}

  <div style="display:flex;flex-direction:column;gap:0;margin-top:32px;">
    <div style="display:flex;gap:16px;flex-wrap:wrap;">
      ${['Requests are executions', 'Data changes are mutations', 'Everything is traceable', 'Deterministic replay'].map((label, i) => {
        const anchors = ['execution', 'mutation-log', 'trace-graph', 'replay-engine'];
        return `<a href="#${anchors[i]}" style="display:inline-flex;align-items:center;gap:8px;padding:8px 16px;border:1px solid var(--border);border-radius:20px;font-size:.82rem;color:var(--muted);text-decoration:none;transition:border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)';this.style.color='var(--text)'" onmouseleave="this.style.borderColor='var(--border)';this.style.color='var(--muted)'">
          <span style="width:18px;height:18px;border-radius:50%;background:var(--accent);color:#000;font-size:.65rem;font-weight:800;display:inline-flex;align-items:center;justify-content:center;flex-shrink:0;">${i + 1}</span>
          ${label}
        </a>`;
      }).join('\n      ')}
    </div>
  </div>
</div>

${concept1()}
${concept2()}
${concept3()}
${concept4()}

<div style="padding:48px 0 0;">
  <div style="background:var(--bg-surface);border:1px solid var(--border);border-radius:12px;padding:32px;">
    <h3 style="font-size:1.1rem;font-weight:700;margin-bottom:8px;">Ready to see it in action?</h3>
    <p style="color:var(--muted);margin-bottom:20px;line-height:1.7;">The quickstart puts all four concepts into practice in about 5 minutes.</p>
    <div style="display:flex;gap:10px;flex-wrap:wrap;">
      <a class="btn-primary" href="/docs/quickstart">Quickstart tutorial →</a>
      <a class="btn-secondary" href="/cli">CLI reference</a>
      <a class="btn-secondary" href="/docs/debugging-production">Debugging guide</a>
    </div>
  </div>
</div>
`;

  return docsLayout({
    meta,
    activePath: '/docs/concepts',
    content,
  });
}
