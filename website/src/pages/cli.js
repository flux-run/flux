/**
 * CLI Reference — top-level page, developers live here.
 */
import { landingLayout }  from '../layouts/landing.js';
import { codeWindow, c }  from '../components/code-window.js';
import { eyebrow, section, sectionHeader } from '../components/section.js';
import { CLI_COMMANDS }   from '../data/cli-commands.js';

export const meta = {
  title:       'CLI Reference — Fluxbase',
  description: 'Complete reference for the flux CLI: deploy, tail, why, trace, state, incident replay, bug bisect, and explain. Examples and output for every command.',
  path:        'cli.html',
};

// ── Hero ──────────────────────────────────────────────────────────────────────
function hero() {
  const whyWindow = codeWindow({
    title: 'flux why 550e8400',
    content: `${c.cmd('$')} flux why ${c.id('550e8400')}

  ${c.err('✗')}  POST /signup → create_user  (${c.ms('3200ms')}, 500)

  ${c.dim('─── Root cause ──────────────────────────────────────────')}
  ${c.err('Stripe timeout after 10000ms')}

  ${c.dim('─── Execution graph ─────────────────────────────────────')}
  ${c.fn('gateway')}     POST /signup           ${c.ms('2ms')}
  ${c.fn('runtime')}     create_user            ${c.ms('1ms')}
  ${c.db('db')}          users (SELECT)        ${c.ms('12ms')}
  ${c.fn('tool')}        stripe.charge       ${c.ms('3200ms')}  ${c.err('⚠ slow')}

  ${c.dim('─── State changes ───────────────────────────────────────')}
  ${c.db('users')}  INSERT  email=user@example.com  plan=free

  ${c.dim('─── Suggested next steps ────────────────────────────────')}
  ${c.ok('flux doctor')} ${c.id('550e8400')}        full diagnosis
  ${c.ok('flux trace diff')} ${c.id('550e8400')} ${c.id('4f9a3b2c')}  compare traces`,
  });

  return `<section class="hero" style="padding-bottom:64px;">
  <span class="eyebrow">CLI Reference</span>
  <h1 style="font-size:clamp(2rem,5vw,3rem);">Debug production<br><span class="gradient-text">in one command.</span></h1>
  <p style="max-width:560px;margin:0 auto 40px;font-size:1.05rem;">Fluxbase records every backend execution. The CLI lets you inspect production requests like Git commits — right from your terminal.</p>

  <div style="max-width:680px;margin:0 auto 40px;text-align:left;">${whyWindow}</div>

  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="#install">Install the CLI →</a>
    <a class="btn-secondary" href="/docs/quickstart">Quickstart</a>
  </div>
</section>`;
}

// ── Installation ──────────────────────────────────────────────────────────────
function installation() {
  const w = codeWindow({
    title: 'install',
    content: `<span style="color:var(--green);">$</span> curl -fsSL https://fluxbase.co/install | bash

  Installing flux CLI…

  <span style="color:var(--green);">✔</span>  Downloaded flux v1.0.0
  <span style="color:var(--green);">✔</span>  Installed to /usr/local/bin/flux

  <span style="color:var(--green);">$</span> flux --version
  flux 1.0.0

  <span style="color:var(--green);">$</span> flux login
  Opening browser… <span style="color:var(--muted);">(or set FLUX_API_KEY env var)</span>
  <span style="color:var(--green);">✔</span>  Logged in as alice@example.com`,
  });

  return `<div id="install">${section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Installation' })}
${sectionHeader({
  heading: 'One-line install.',
  sub: 'Installs a single static binary. No Node.js, no Python, no dependencies.',
})}
<div style="max-width:600px;">${w}</div>`,
  })}</div>`;
}

// ── Command sections ──────────────────────────────────────────────────────────
function commandSection(cmd) {
  const anchor = cmd.cmd.split(' ')[1];
  // Escape HTML in code example
  const escaped = cmd.example
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  const w = `<div style="background:#0a0a0c;border:1px solid var(--border);border-radius:10px;overflow:hidden;">
    <div style="background:var(--bg-elevated);border-bottom:1px solid var(--border);padding:9px 14px;display:flex;align-items:center;gap:7px;">
      <span style="width:10px;height:10px;border-radius:50%;background:#f87171;display:inline-block;flex-shrink:0;"></span>
      <span style="width:10px;height:10px;border-radius:50%;background:var(--yellow);display:inline-block;flex-shrink:0;"></span>
      <span style="width:10px;height:10px;border-radius:50%;background:var(--green);display:inline-block;flex-shrink:0;"></span>
      <span style="font-family:var(--font-mono);font-size:.73rem;color:var(--muted);margin-left:8px;">${cmd.cmd}</span>
    </div>
    <pre style="margin:0;border:none;border-radius:0;padding:20px 24px;background:#0a0a0c;"><code style="font-size:.82rem;line-height:1.85;">${escaped}</code></pre>
  </div>`;

  return `<div id="${anchor}" style="padding:56px 0;border-top:1px solid var(--border);">
  <div style="max-width:1040px;margin:0 auto;padding:0 24px;">
    <div class="grid-2col" style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;">
      <div>
        <div style="font-family:var(--font-mono);font-size:1rem;font-weight:700;color:var(--accent);margin-bottom:10px;">${cmd.cmd}</div>
        <h3 style="font-size:1.2rem;font-weight:700;margin-bottom:12px;">${cmd.summary}</h3>
        <p style="color:var(--muted);font-size:.92rem;line-height:1.7;">${cmd.desc}</p>
      </div>
      <div>${w}</div>
    </div>
  </div>
</div>`;
}

// ── Core three commands ───────────────────────────────────────────────────────
function coreThreeCommands() {
  const items = [
    {
      phase: 'WATCH',
      color: 'var(--green)',
      cmd: 'flux tail',
      summary: 'Stream live requests',
      window: codeWindow({
        title: 'flux tail',
        content: `${c.cmd('$')} flux tail

  METHOD   ROUTE          DURATION  STATUS
  ${c.ok('POST')}     /login         ${c.ms('38ms')}     ${c.ok('✔')}
  ${c.ok('POST')}     /checkout      ${c.ms('121ms')}    ${c.ok('✔')}
  ${c.ok('POST')}     /signup        ${c.ms('3.2s')}     ${c.err('✗ 500')}
    ${c.err('Stripe timeout after 10000ms')}
    ${c.dim('→ flux why')} ${c.id('550e8400')}`,
      }),
    },
    {
      phase: 'UNDERSTAND',
      color: 'var(--accent)',
      cmd: 'flux why',
      summary: 'Root-cause a failed request',
      window: codeWindow({
        title: 'flux why 550e8400',
        content: `${c.cmd('$')} flux why ${c.id('550e8400')}

  ${c.err('✗')}  POST /signup  ${c.ms('3200ms')}  500

  ${c.err('Root cause: Stripe timeout after 10000ms')}

  ${c.fn('gateway')}   ${c.ms('2ms')}
  ${c.fn('runtime')}   ${c.ms('1ms')}
  ${c.db('db')}        ${c.ms('12ms')}
  ${c.fn('stripe')}    ${c.ms('3200ms')}  ${c.err('⚠')}`,
      }),
    },
    {
      phase: 'DIAGNOSE',
      color: '#c084fc',
      cmd: 'flux doctor',
      summary: 'Automatic incident diagnosis',
      window: codeWindow({
        title: 'flux doctor 550e8400',
        content: `${c.cmd('$')} flux doctor ${c.id('550e8400')}

  ROOT CAUSE
  ${c.err('⚡ stripe.charge timed out after 10000ms')}

  LIKELY ISSUE
  ${c.ok('External tool latency exceeded threshold')}

  SUGGESTED ACTIONS
  ${c.ok('•')} Increase timeout above 11000ms
  ${c.ok('•')} Add retry with exponential backoff`,
      }),
    },
  ];

  const cards = items.map(item => `<div style="display:flex;flex-direction:column;gap:16px;">
    <div style="display:flex;align-items:center;gap:10px;">
      <span style="display:inline-block;width:3px;height:20px;background:${item.color};border-radius:2px;flex-shrink:0;"></span>
      <span style="font-size:.65rem;font-weight:700;text-transform:uppercase;letter-spacing:.14em;color:${item.color};">${item.phase}</span>
    </div>
    <div>
      <div style="font-family:var(--font-mono);font-size:1rem;font-weight:700;color:${item.color};margin-bottom:6px;">${item.cmd}</div>
      <p style="font-size:.88rem;color:var(--muted);margin:0 0 14px;">${item.summary}</p>
    </div>
    ${item.window}
  </div>`).join('\n  ');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Core Workflow' })}
${sectionHeader({
  heading: 'Three commands. Every incident resolved.',
  sub: 'The entire debugging workflow — from spotting a failure to understanding its cause — is three commands.',
  maxWidth: '580px',
})}
<div style="display:grid;grid-template-columns:repeat(3,1fr);gap:32px;" class="grid-3col">
  ${cards}
</div>`,
  });
}

// ── Deep debugging ─────────────────────────────────────────────────────────────
function deepDebugging() {
  const tools = [
    {
      icon: '🔬',
      label: 'Step through a request',
      cmd: 'flux trace debug 550e8400',
      desc: 'Navigate spans with arrow keys. See exact inputs, outputs, and timing for every step of execution.',
      window: codeWindow({
        title: 'flux trace debug',
        content: `${c.dim('Step 1/4')}  ${c.fn('gateway')}
  ${c.dim('──────────────────────────────────')}
  Input:  POST /signup  { email: "a@b.com" }
  Output: { passed: true }
  Time:   ${c.ms('4ms')}

  ${c.dim('↓ next  ↑ prev  q quit')}`,
      }),
    },
    {
      icon: '⚖️',
      label: 'Compare two executions',
      cmd: 'flux trace diff reqA reqB',
      desc: 'Diff any two request traces side by side. Changed spans highlighted — useful for before/after a deploy.',
      window: codeWindow({
        title: 'flux trace diff',
        content: `  SPAN              A        B        DELTA
  gateway           ${c.ms('3ms')}     ${c.ms('4ms')}     ${c.dim('+1ms')}
  create_user      ${c.ms('81ms')}    ${c.ms('44ms')}    ${c.dim('-37ms')}
  stripe.charge    ${c.ms('12ms')}  ${c.err('10002ms')}  ${c.err('+9990ms ✗')}

  ${c.err('→ stripe.charge regressed')}`,
      }),
    },
    {
      icon: '⏪',
      label: 'Replay a production incident',
      cmd: 'flux incident replay 14:00..14:05',
      desc: 'Re-execute any time window against your current code. Side-effects off. Database writes on.',
      window: codeWindow({
        title: 'flux incident replay',
        content: `  ${c.ok('✔')}  ${c.id('4f9a3b2c')}  POST /create_user   ${c.ok('200')}
  ${c.ok('✔')}  ${c.id('a3c91ef0')}  GET  /list_users    ${c.ok('200')}
  ${c.err('✗')}  ${c.id('550e8400')}  POST /signup        ${c.err('500')}
     ${c.err('└─ Still failing: Stripe timeout')}

  ${c.dim('23 replayed · 1 still failing')}`,
      }),
    },
    {
      icon: '🔎',
      label: 'Find the bad commit',
      cmd: 'flux bug bisect --request 550e8400',
      desc: 'Binary-searches your git history. Finds the exact commit and author that introduced the regression.',
      window: codeWindow({
        title: 'flux bug bisect',
        content: `  Testing ${c.id('abc123')}…  ${c.ok('✔ passes')}
  Testing ${c.id('def456')}…  ${c.err('✗ fails')}

  FIRST BAD COMMIT
  ${c.err('def456')}  "feat: add retry to stripe.charge"
  ${c.dim('2026-03-08  alice@example.com')}`,
      }),
    },
  ];

  const cards = tools.map(t => `<div style="background:var(--bg-surface);border:1px solid var(--border);border-radius:12px;padding:28px;display:flex;flex-direction:column;gap:16px;">
    <div>
      <div style="font-size:1.4rem;margin-bottom:10px;">${t.icon}</div>
      <div style="font-size:.95rem;font-weight:700;margin-bottom:6px;">${t.label}</div>
      <div style="font-family:var(--font-mono);font-size:.78rem;color:var(--accent);margin-bottom:10px;">${t.cmd}</div>
      <p style="font-size:.85rem;color:var(--muted);line-height:1.6;margin:0;">${t.desc}</p>
    </div>
    ${t.window}
  </div>`).join('\n  ');

  return section({
    content: `${eyebrow({ text: 'Deep Debugging' })}
${sectionHeader({
  heading: 'Superpowers that no other tool has.',
  sub: 'Traditional APM shows you that a request failed. Fluxbase lets you step through it, diff it, replay it, and bisect it — all from the CLI.',
  maxWidth: '600px',
})}
<div style="display:grid;grid-template-columns:repeat(2,1fr);gap:24px;" class="grid-2col">
  ${cards}
</div>`,
  });
}

// ── Git analogy ───────────────────────────────────────────────────────────────
function gitAnalogy() {
  const rows = [
    { git: 'git log',    flux: 'flux tail',              meaning: 'See what happened' },
    { git: 'git show',   flux: 'flux why',               meaning: 'Inspect one execution' },
    { git: 'git diff',   flux: 'flux trace diff',        meaning: 'Compare two executions' },
    { git: 'git bisect', flux: 'flux bug bisect',        meaning: 'Find the bad commit' },
    { git: 'git blame',  flux: 'flux state blame',       meaning: 'Who changed this row?' },
  ];

  const tableRows = rows.map(r => `<tr>
      <td style="padding:12px 16px;border-bottom:1px solid var(--border);font-family:var(--font-mono);font-size:.84rem;color:#facc15;">${r.git}</td>
      <td style="padding:12px 16px;border-bottom:1px solid var(--border);font-family:var(--font-mono);font-size:.84rem;color:var(--accent);">${r.flux}</td>
      <td style="padding:12px 16px;border-bottom:1px solid var(--border);font-size:.85rem;color:var(--muted);">${r.meaning}</td>
    </tr>`).join('\n    ');

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Mental Model', color: 'muted' })}
${sectionHeader({
  heading: 'If Git is version history for code, Fluxbase is version history for production.',
  sub: 'The same mental model. The same commands. Applied to your production execution graph instead of your source tree.',
  maxWidth: '620px',
})}
<div style="max-width:680px;margin:0 auto;">
  <table style="width:100%;border-collapse:collapse;font-size:.9rem;border:1px solid var(--border);border-radius:10px;overflow:hidden;">
    <thead>
      <tr style="background:var(--bg-elevated);">
        <th style="text-align:left;padding:12px 16px;font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:#facc15;border-bottom:1px solid var(--border);">Git</th>
        <th style="text-align:left;padding:12px 16px;font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--accent);border-bottom:1px solid var(--border);">Fluxbase</th>
        <th style="text-align:left;padding:12px 16px;font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--muted);border-bottom:1px solid var(--border);">What it answers</th>
      </tr>
    </thead>
    <tbody>
    ${tableRows}
    </tbody>
  </table>
</div>`,
  });
}

// ── Command surface table ─────────────────────────────────────────────────────
function commandSurfaceTable() {
  const rows = [
    { cmd: 'flux tail',                        q: 'What is happening right now?' },
    { cmd: 'flux why &lt;id&gt;',              q: 'Why did this request fail?' },
    { cmd: 'flux doctor &lt;id&gt;',           q: 'What is the likely cause?' },
    { cmd: 'flux trace debug &lt;id&gt;',      q: 'What happened step-by-step?' },
    { cmd: 'flux trace diff &lt;a&gt; &lt;b&gt;', q: 'What changed between two runs?' },
    { cmd: 'flux state history &lt;table&gt; --id &lt;row&gt;', q: 'How did this row change over time?' },
    { cmd: 'flux state blame &lt;table&gt; --id &lt;row&gt;',   q: 'Who modified this field last?' },
    { cmd: 'flux incident replay &lt;from&gt;..&lt;to&gt;',    q: 'What happens if we replay this window?' },
    { cmd: 'flux bug bisect --request &lt;id&gt;',              q: 'Which deploy introduced the bug?' },
    { cmd: 'flux explain &lt;query-file&gt;',  q: 'What SQL will run? What policies apply?' },
    { cmd: 'flux deploy',                       q: 'Deploy functions to production' },
  ];

  const tableRows = rows.map((r, i) => {
    const bg = i % 2 === 1 ? 'background:var(--bg-elevated);' : '';
    return `<tr style="${bg}">
      <td style="padding:11px 16px;border-bottom:1px solid var(--border);font-family:var(--font-mono);font-size:.81rem;color:var(--accent);white-space:nowrap;">${r.cmd}</td>
      <td style="padding:11px 16px;border-bottom:1px solid var(--border);font-size:.85rem;color:var(--muted);">${r.q}</td>
    </tr>`;
  }).join('\n    ');

  return section({
    content: `${eyebrow({ text: 'Full Command Surface' })}
${sectionHeader({
  heading: 'Every command answers a developer question.',
  sub: 'The CLI has 11 commands. Each one maps to a question you\'d ask when something goes wrong in production.',
  maxWidth: '560px',
})}
<div style="max-width:780px;margin:0 auto;border:1px solid var(--border);border-radius:10px;overflow:hidden;">
  <table style="width:100%;border-collapse:collapse;">
    <thead>
      <tr style="background:var(--bg-elevated);">
        <th style="text-align:left;padding:12px 16px;font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--muted);border-bottom:1px solid var(--border);">Command</th>
        <th style="text-align:left;padding:12px 16px;font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--muted);border-bottom:1px solid var(--border);">What it answers</th>
      </tr>
    </thead>
    <tbody>
    ${tableRows}
    </tbody>
  </table>
</div>`,
  });
}

// ── Tutorial callout ──────────────────────────────────────────────────────────
function tutorialCallout() {
  const workflowWindow = codeWindow({
    title: 'debug in 30 seconds',
    content: `${c.dim('# 1. Deploy')}
${c.cmd('$')} flux deploy
  ${c.ok('✔')}  create_user → gw.fluxbase.co/create_user

${c.dim('# 2. Watch for failures')}
${c.cmd('$')} flux tail
  POST /signup  ${c.ms('3.2s')}  ${c.err('✗ 500')}
  ${c.err('Stripe timeout after 10000ms')}
  ${c.dim('→ flux why')} ${c.id('550e8400')}

${c.dim('# 3. Root-cause immediately')}
${c.cmd('$')} flux why ${c.id('550e8400')}
  ${c.err('Root cause: Stripe timeout after 10000ms')}
  ${c.fn('stripe.charge')}  ${c.ms('3200ms')}  ${c.err('⚠ slow')}

${c.dim('# 4. Auto-diagnose')}
${c.cmd('$')} flux doctor ${c.id('550e8400')}
  ${c.ok('•')} Increase timeout above 11000ms
  ${c.ok('•')} Add retry with exponential backoff`,
  });

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: '30-Second Workflow', color: 'green' })}
${sectionHeader({
  heading: 'Deploy. Tail. Why. Fixed.',
  sub: 'The core debugging workflow: spot a failure in <code>flux tail</code>, take the request ID, hand it to <code>flux why</code>. Everything else is optional depth.',
  maxWidth: '540px',
})}
<div style="display:grid;grid-template-columns:1fr 1fr;gap:48px;align-items:start;max-width:960px;margin:0 auto;" class="grid-2col">
  <div>
    ${workflowWindow}
  </div>
  <div>
    <ol style="padding:0;margin:0;list-style:none;display:flex;flex-direction:column;gap:24px;">
      <li style="display:flex;gap:16px;align-items:start;">
        <span style="width:28px;height:28px;border-radius:50%;background:var(--green);color:#fff;font-size:.8rem;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0;margin-top:2px;">1</span>
        <div><h3 style="font-size:.95rem;font-weight:700;margin:0 0 4px;">Deploy your function</h3><p style="font-size:.85rem;color:var(--muted);margin:0;line-height:1.6;"><code>flux deploy</code> bundles and ships your TypeScript in ~20s. Returns a URL per function.</p></div>
      </li>
      <li style="display:flex;gap:16px;align-items:start;">
        <span style="width:28px;height:28px;border-radius:50%;background:var(--green);color:#fff;font-size:.8rem;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0;margin-top:2px;">2</span>
        <div><h3 style="font-size:.95rem;font-weight:700;margin:0 0 4px;">Watch for failures</h3><p style="font-size:.85rem;color:var(--muted);margin:0;line-height:1.6;"><code>flux tail</code> streams live requests. Errors appear in red with their request ID inline.</p></div>
      </li>
      <li style="display:flex;gap:16px;align-items:start;">
        <span style="width:28px;height:28px;border-radius:50%;background:var(--accent);color:#fff;font-size:.8rem;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0;margin-top:2px;">3</span>
        <div><h3 style="font-size:.95rem;font-weight:700;margin:0 0 4px;">Root-cause immediately</h3><p style="font-size:.85rem;color:var(--muted);margin:0;line-height:1.6;"><code>flux why &lt;id&gt;</code> shows root cause, execution graph, and state changes in one output.</p></div>
      </li>
      <li style="display:flex;gap:16px;align-items:start;">
        <span style="width:28px;height:28px;border-radius:50%;background:#c084fc;color:#fff;font-size:.8rem;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0;margin-top:2px;">4</span>
        <div><h3 style="font-size:.95rem;font-weight:700;margin:0 0 4px;">Go deeper if needed</h3><p style="font-size:.85rem;color:var(--muted);margin:0;line-height:1.6;"><code>flux doctor</code>, <code>flux trace diff</code>, <code>flux incident replay</code>, <code>flux bug bisect</code> — each adds a layer of precision.</p></div>
      </li>
    </ol>
    <div style="margin-top:32px;">
      <a class="btn-primary" href="/docs/quickstart">Follow the full quickstart →</a>
    </div>
  </div>
</div>`,
  });
}

// ── Page styles ───────────────────────────────────────────────────────────────
const extraHead = `<style>
  @media (max-width: 900px) {
    .grid-3col { grid-template-columns: 1fr !important; }
  }
  @media (max-width: 760px) {
    .grid-2col { grid-template-columns: 1fr !important; }
  }
</style>`;

// ── Command groups ────────────────────────────────────────────────────────────
const CMD_GROUPS = [
  {
    label: 'Deploy & Runtime',
    color: 'var(--green)',
    commands: ['flux deploy', 'flux tail'],
  },
  {
    label: 'Debugging',
    color: 'var(--accent)',
    commands: ['flux why', 'flux trace debug', 'flux trace diff', 'flux trace'],
  },
  {
    label: 'Data History',
    color: 'var(--blue,#60a5fa)',
    commands: ['flux state history', 'flux state blame'],
  },
  {
    label: 'Incident Analysis',
    color: 'var(--purple,#c084fc)',
    commands: ['flux incident replay', 'flux bug bisect', 'flux explain'],
  },
];

function groupedCommandSections() {
  return CMD_GROUPS.map(group => {
    const matchedCmds = group.commands
      .map(name => CLI_COMMANDS.find(c => c.cmd === name))
      .filter(Boolean);

    const sections = matchedCmds.map(commandSection).join('\n');

    return `<div style="padding:56px 0 0;">
  <div style="max-width:1040px;margin:0 auto;padding:0 24px;">
    <div style="display:flex;align-items:center;gap:12px;margin-bottom:0;">
      <span style="display:inline-block;width:3px;height:24px;background:${group.color};border-radius:2px;"></span>
      <span style="font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.12em;color:${group.color};">${group.label}</span>
    </div>
  </div>
  ${sections}
</div>`;
  }).join('\n\n');
}

// ── Render ────────────────────────────────────────────────────────────────────
export function render() {
  const content = [
    hero(),
    coreThreeCommands(),
    deepDebugging(),
    gitAnalogy(),
    commandSurfaceTable(),
    tutorialCallout(),
    installation(),
    groupedCommandSections(),
    `<section class="cta-strip">
  <h2>Ready to try it?</h2>
  <p style="max-width:480px;margin:0 auto 32px;">Install the CLI, deploy your first function, and trace it end to end in 5 minutes.</p>
  <div style="display:flex;gap:12px;justify-content:center;flex-wrap:wrap;">
    <a class="btn-primary" href="/docs/quickstart">Quickstart →</a>
    <a class="btn-secondary" href="/product">Product overview</a>
  </div>
</section>`,
  ].join('\n\n');

  return landingLayout({ meta, active: 'cli', extraHead, content });
}
