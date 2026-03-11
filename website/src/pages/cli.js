/**
 * CLI Reference — top-level page, developers live here.
 */
import { landingLayout }  from '../layouts/landing.js';
import { codeWindow }     from '../components/code-window.js';
import { eyebrow, section, sectionHeader } from '../components/section.js';
import { CLI_COMMANDS }   from '../data/cli-commands.js';

export const meta = {
  title:       'CLI Reference — Fluxbase',
  description: 'Complete reference for the flux CLI: deploy, tail, why, trace, state, incident replay, bug bisect, and explain. Examples and output for every command.',
  path:        'cli.html',
};

// ── Hero ──────────────────────────────────────────────────────────────────────
function hero() {
  const cmds = CLI_COMMANDS.map(cmd =>
    `<a href="#${cmd.cmd.split(' ')[1]}" style="display:block;padding:10px 16px;border:1px solid var(--border);border-radius:8px;background:var(--bg-surface);color:var(--text);font-size:.85rem;font-family:var(--font-mono);transition:border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)'" onmouseleave="this.style.borderColor='var(--border)'" title="${cmd.summary}">
      <span style="color:var(--green);">flux</span> <span style="color:var(--accent);">${cmd.cmd.replace('flux ', '')}</span>
      <span style="display:block;font-family:var(--font);font-size:.78rem;color:var(--muted);margin-top:2px;">${cmd.summary}</span>
    </a>`
  ).join('\n    ');

  return `<section class="hero" style="padding-bottom:48px;">
  <span class="eyebrow">CLI Reference</span>
  <h1 style="font-size:clamp(2rem,5vw,3rem);">Developers live<br><span class="gradient-text">in the terminal.</span></h1>
  <p style="max-width:560px;margin:0 auto 36px;">Every debugging operation — from deploying a function to bisecting a production regression — is a single <code>flux</code> command.</p>

  <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:10px;max-width:900px;margin:0 auto;text-align:left;">
    ${cmds}
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

  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'Installation' })}
${sectionHeader({
  heading: 'One-line install.',
  sub: 'Installs a single static binary. No Node.js, no Python, no dependencies.',
})}
<div style="max-width:600px;">${w}</div>`,
  });
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

// ── Tutorial callout ──────────────────────────────────────────────────────────
function tutorialCallout() {
  return section({
    bg: 'var(--bg-surface)',
    content: `${eyebrow({ text: 'First Tutorial', color: 'green' })}
${sectionHeader({
  heading: 'Debug a production bug in 30 seconds.',
  sub: 'The fastest way to understand these commands is to see them in sequence against a real failure.',
  maxWidth: '520px',
})}

<ol class="steps" style="max-width:640px;">
  <li>
    <div class="step-num">1</div>
    <div class="step-body">
      <h3>Deploy your function</h3>
      <p><code>flux deploy</code> — bundles and deploys your TypeScript functions. Returns a URL per function.</p>
    </div>
  </li>
  <li>
    <div class="step-num">2</div>
    <div class="step-body">
      <h3>Watch for failures</h3>
      <p><code>flux tail</code> — streams live requests. Errors appear in red with their request ID.</p>
    </div>
  </li>
  <li>
    <div class="step-num">3</div>
    <div class="step-body">
      <h3>Root-cause immediately</h3>
      <p><code>flux why &lt;request-id&gt;</code> — takes the ID from <code>flux tail</code> and shows root cause, location, and data changes.</p>
    </div>
  </li>
  <li>
    <div class="step-num">4</div>
    <div class="step-body">
      <h3>Compare before/after your fix</h3>
      <p><code>flux trace diff &lt;id-before&gt; &lt;id-after&gt;</code> — shows which spans changed and by how much after your code change.</p>
    </div>
  </li>
</ol>

<a class="btn-primary" href="/docs/quickstart" style="display:inline-flex;">Follow the full quickstart →</a>`,
  });
}

// ── Page styles ───────────────────────────────────────────────────────────────
const extraHead = '';

// ── Render ────────────────────────────────────────────────────────────────────
export function render() {
  const cmds = CLI_COMMANDS.map(commandSection).join('\n');

  const content = [
    hero(),
    installation(),
    tutorialCallout(),
    cmds,
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
