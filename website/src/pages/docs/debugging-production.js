/**
 * Debugging Production Systems — the core product workflow narrative.
 * Shows flux tail → why → trace debug → trace diff in one story.
 */
import { docsLayout } from '../../layouts/docs.js';
import { codeWindow, c } from '../../components/code-window.js';
import { eyebrow } from '../../components/section.js';

export const meta = {
  title:       'Debugging Production Systems — Fluxbase',
  description: 'Debug production backend failures end-to-end: stream live errors with flux tail, root-cause them with flux why, step through spans with flux trace debug, and verify your fix with flux trace diff.',
  path:        'docs/debugging-production.html',
};

function step({ number, title, cmd, body, win }) {
  return `<div style="padding:56px 0;border-bottom:1px solid var(--border);">
  <div style="display:flex;gap:10px;align-items:flex-start;margin-bottom:24px;">
    <span style="width:32px;height:32px;border-radius:50%;background:var(--accent);color:#000;font-size:.85rem;font-weight:800;display:inline-flex;align-items:center;justify-content:center;flex-shrink:0;margin-top:2px;">${number}</span>
    <div>
      <div style="font-family:var(--font-mono);font-size:.8rem;color:var(--accent);margin-bottom:4px;">${cmd}</div>
      <h2 style="font-size:1.4rem;font-weight:800;">${title}</h2>
    </div>
  </div>
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;">
    <div>${body}</div>
    <div style="position:sticky;top:24px;">${win}</div>
  </div>
</div>`;
}

function step1() {
  const win = codeWindow({
    title: 'flux tail',
    content: `${c.cmd('$')} flux tail

  Streaming live requests…

  ${c.ok('✔')}  POST /signup    201  ${c.ms('88ms')}   ${c.dim('req:4f9a3b2c')}
  ${c.ok('✔')}  GET  /users     200  ${c.ms('12ms')}   ${c.dim('req:a3c91ef0')}
  ${c.ok('✔')}  POST /checkout  200  ${c.ms('192ms')}  ${c.dim('req:b7e3d12f')}
  ${c.err('✗')}  POST /signup    500  ${c.ms('44ms')}   ${c.id('req:550e8400')}
     ${c.err('└─ Error: Stripe API timeout')}
  ${c.ok('✔')}  GET  /products  200  ${c.ms('9ms')}    ${c.dim('req:cc4a8e71')}`,
  });

  return step({
    number: '1',
    cmd: 'flux tail',
    title: 'Stream live requests — spot failures instantly',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;"><code>flux tail</code> streams every request hitting your Fluxbase functions in real time. Successes appear in green. Failures appear in red with an inline error summary.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">The <strong style="color:var(--text);">request ID on failures is your debugging handle</strong>. Copy it — you'll pass it to the next command.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:20px;">Unlike log tailing, <code>flux tail</code> shows you the full execution context: method, path, status, latency, and error summary — all in one line.</p>
<div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:8px;padding:14px 18px;">
  <div style="font-size:.78rem;font-weight:700;color:var(--muted);margin-bottom:8px;text-transform:uppercase;letter-spacing:.08em;">Useful flags</div>
  <div style="display:flex;flex-direction:column;gap:6px;font-family:var(--font-mono);font-size:.78rem;color:var(--muted);">
    <div><span style="color:var(--text);">--fn payments</span>  &nbsp;filter to one function</div>
    <div><span style="color:var(--text);">--errors-only</span>  &nbsp;hide 2xx requests</div>
    <div><span style="color:var(--text);">--since 30m</span>    &nbsp;replay recent requests</div>
  </div>
</div>`,
    win,
  });
}

function step2() {
  const win = codeWindow({
    title: 'flux why 550e8400',
    content: `${c.cmd('$')} flux why ${c.id('550e8400')}

  ${c.white('ROOT CAUSE')}
  Stripe API timeout after 10s

  ${c.white('LOCATION')}
  payments/create.ts : line 42

  ${c.white('DATA CHANGES')}
  ${c.db('users')} id=42
    plan : free ${c.err('→ null')}   ${c.dim('(rolled back)')}

  ${c.white('SUGGESTION')}
  ${c.ok('→')} Add 5s timeout + idempotency key retry
  ${c.ok('→')} Consider moving to async background step`,
  });

  return step({
    number: '2',
    cmd: 'flux why <request-id>',
    title: 'Root-cause the failure — one command',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;"><code>flux why</code> takes the request ID from <code>flux tail</code> and gives you everything you need to understand the failure:</p>
<ul style="color:var(--muted);line-height:2;padding-left:0;list-style:none;margin-bottom:20px;">
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> <strong style="color:var(--text);">Root cause</strong> — the first error in the span tree</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> <strong style="color:var(--text);">Location</strong> — exact file and line in your code</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> <strong style="color:var(--text);">Data changes</strong> — every row that was mutated (including rolled-back writes)</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> <strong style="color:var(--text);">Suggestion</strong> — AI-generated fix hint based on the error pattern</li>
</ul>
<p style="color:var(--muted);line-height:1.8;">This is enough to fix most failures. If you need more detail, continue to step 3.</p>`,
    win,
  });
}

function step3() {
  const win = codeWindow({
    title: 'flux trace debug 550e8400',
    content: `${c.cmd('$')} flux trace debug ${c.id('550e8400')}

  ${c.white('Span 1/5')}  ${c.fn('gateway')}             ${c.ms('2ms')}
  in  : ${c.dim('POST /signup  {email, plan}')}
  out : ${c.dim('→ runtime  request_id=550e8400')}

  ${c.white('Span 2/5')}  ${c.fn('create_user')}         ${c.ms('4ms')}
  in  : ${c.db('{email: "alice@..", plan: "pro"}')}
  out : ${c.db('users.id = 42  (inserted)')}

  ${c.white('Span 3/5')}  ${c.purple('stripe.create')}           ${c.err('timeout')}
  in  : ${c.dim('{customer_id: "cus_123", amount: 2900}')}
  out : ${c.err('Error: Request timeout after 10011ms')}

  ${c.dim('[n]ext  [p]rev  [q]uit')}`,
  });

  return step({
    number: '3',
    cmd: 'flux trace debug <request-id>',
    title: 'Step through spans interactively',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;"><code>flux trace debug</code> puts you in an interactive span-by-span walkthrough of the execution. Think of it like <code>git bisect</code> but for a single request's execution steps.</p>
<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">Each span shows you:</p>
<ul style="color:var(--muted);line-height:2;padding-left:0;list-style:none;margin-bottom:20px;">
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> The exact input that was passed to that span</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> The exact output (or error) it returned</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> The duration and any DB state changes it caused</li>
</ul>
<p style="color:var(--muted);line-height:1.8;">Use this when <code>flux why</code> identifies the failure but you want to understand <em>exactly</em> what the function received and returned at each step.</p>`,
    win,
  });
}

function step4() {
  const win = codeWindow({
    title: 'flux trace diff',
    content: `${c.cmd('$')} flux trace diff ${c.id('550e8400')} ${c.id('7b1d3f9a')}

  Comparing traces…

  ${c.white('Span diff')}
  ${c.db('stripe.create')}
  ${c.err('−')} duration  : 10011ms  (timeout)
  ${c.ok('+')} duration  : 142ms    (success)
  ${c.ok('+')} idempotency_key: ik_1234  ${c.dim('(new)')}

  ${c.white('Mutation diff')}
  ${c.db('users.id=42')}
  ${c.err('−')} plan: null  ${c.dim('(rolled back)')}
  ${c.ok('+')} plan: pro   ${c.dim('(committed)')}

  ${c.ok('→')} Fix verified: idempotency key + 5s timeout')}`,
  });

  return step({
    number: '4',
    cmd: 'flux trace diff <before> <after>',
    title: 'Verify your fix — diff two traces',
    body: `<p style="color:var(--muted);line-height:1.8;margin-bottom:16px;">After you fix the bug and deploy, trigger the same scenario and capture the new request ID. Then <code>flux trace diff</code> compares the two executions side-by-side:</p>
<ul style="color:var(--muted);line-height:2;padding-left:0;list-style:none;margin-bottom:20px;">
  <li style="display:flex;gap:10px;"><span style="color:var(--green);flex-shrink:0;">+</span> New spans that were added</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--err,#f87171);flex-shrink:0;">−</span> Spans that changed or were removed</li>
  <li style="display:flex;gap:10px;"><span style="color:var(--accent);flex-shrink:0;">→</span> Mutation changes (what the DB looks like now)</li>
</ul>
<p style="color:var(--muted);line-height:1.8;">This is how you <strong style="color:var(--text);">prove</strong> your fix worked — not just "it seems fine" but "here is exactly how the execution changed".</p>`,
    win,
  });
}

function beyondBasics() {
  return `<div style="padding:48px 0;">
  ${eyebrow({ text: 'Going Deeper' })}
  <h2 style="font-size:1.3rem;font-weight:800;margin-bottom:24px;">For harder incidents, go further.</h2>

  <div style="display:grid;grid-template-columns:1fr 1fr;gap:16px;">
    ${[
      {
        cmd: 'flux state blame <table> --id <row>',
        title: 'Which request changed this row?',
        body: 'When you find corrupted data and don\'t know which request caused it, <code>flux state blame</code> gives you the full history of any database row.',
      },
      {
        cmd: 'flux incident replay --window <start>-<end>',
        title: 'Replay the incident safely',
        body: 'Re-runs a production time window against your fixed code with all side-effects disabled. Emails won\'t send. Payments won\'t process.',
      },
      {
        cmd: 'flux bug bisect <failing-id>',
        title: 'Find the commit that broke it',
        body: 'Binary-searches your git history to find the first commit where a request started failing. Like <code>git bisect</code> but for production behaviour.',
      },
      {
        cmd: 'flux explain <request-id>',
        title: 'Ask AI for analysis',
        body: 'Sends a trace to an LLM with full context — spans, mutations, errors — and returns a detailed diagnosis and suggested fix. Dry-run safe by default.',
      },
    ].map(card => `<div style="background:var(--bg-surface);border:1px solid var(--border);border-radius:10px;padding:20px 22px;">
      <div style="font-family:var(--font-mono);font-size:.75rem;color:var(--accent);margin-bottom:8px;">${card.cmd}</div>
      <h3 style="font-size:.95rem;font-weight:700;margin-bottom:8px;">${card.title}</h3>
      <p style="font-size:.85rem;color:var(--muted);line-height:1.6;">${card.body}</p>
    </div>`).join('\n    ')}
  </div>
</div>`;
}

export function render() {
  const content = `
<div style="padding:48px 0 24px;">
  <div style="max-width:680px;">
    ${eyebrow({ text: 'Debugging Guide' })}
    <h1 style="font-size:clamp(1.8rem,4vw,2.8rem);font-weight:800;margin-bottom:16px;">Debugging Production Systems</h1>
    <p style="font-size:1.05rem;color:var(--muted);line-height:1.8;margin-bottom:20px;">Four commands cover 95% of production debugging. Here's how they work together as a complete workflow — from noticing a failure to proving the fix.</p>
    <div style="display:flex;gap:8px;flex-wrap:wrap;">
      ${[
        ['#flux-tail', 'flux tail'],
        ['#flux-why', 'flux why'],
        ['#flux-trace-debug', 'flux trace debug'],
        ['#flux-trace-diff', 'flux trace diff'],
      ].map(([href, label]) =>
        `<a href="${href}" style="display:inline-block;padding:6px 14px;border:1px solid var(--border);border-radius:20px;font-family:var(--font-mono);font-size:.78rem;color:var(--accent);text-decoration:none;transition:background .15s;" onmouseenter="this.style.background='var(--accent-dim)'" onmouseleave="this.style.background=''">${label}</a>`
      ).join('\n      ')}
    </div>
  </div>
</div>

${step1()}
${step2()}
${step3()}
${step4()}
${beyondBasics()}

<div style="padding:0 0 48px;">
  <div style="background:var(--accent-dim);border:1px solid var(--accent);border-radius:12px;padding:28px 32px;">
    <h3 style="font-size:1rem;font-weight:700;margin-bottom:8px;">Want the hands-on version?</h3>
    <p style="color:var(--muted);margin-bottom:18px;font-size:.9rem;line-height:1.7;">The quickstart tutorial walks through all four steps with a real function that deliberately fails.</p>
    <div style="display:flex;gap:10px;flex-wrap:wrap;">
      <a class="btn-primary" href="/docs/quickstart">Quickstart tutorial →</a>
      <a class="btn-secondary" href="/cli">Full CLI reference</a>
    </div>
  </div>
</div>
`;

  return docsLayout({
    meta,
    activePath: '/docs/debugging-production',
    content,
  });
}
