import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Documentation — Fluxbase',
  description: 'Fluxbase documentation. Get started with deployment, understand the architecture, and master the CLI debugging tools.',
}

export default function Page() {
  return (
    <div
      dangerouslySetInnerHTML={{ __html: `<h1>Fluxbase Documentation</h1>
<p class="page-subtitle">Git for Backend Execution — every request recorded, every bug reproducible.</p>

<p>Fluxbase is a backend runtime where every request automatically produces a complete execution trace. No instrumentation, no SDK, no configuration. Deploy TypeScript functions and gain instant observability across the entire stack.</p>

<h2>Start here</h2>

<div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:16px;margin:24px 0 40px;">
  <a href="/docs/quickstart" style="display:block;padding:20px 24px;border:1px solid var(--border);border-radius:10px;background:var(--bg-surface);color:var(--text);text-decoration:none;transition:border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)'" onmouseleave="this.style.borderColor='var(--border)'">
    <div style="font-size:1.2rem;margin-bottom:8px;">🚀</div>
    <div style="font-weight:700;margin-bottom:4px;">Quickstart</div>
    <div style="font-size:.85rem;color:var(--muted);">Deploy your first function and trace it in 5 minutes.</div>
  </a>
  <a href="/cli" style="display:block;padding:20px 24px;border:1px solid var(--border);border-radius:10px;background:var(--bg-surface);color:var(--text);text-decoration:none;transition:border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)'" onmouseleave="this.style.borderColor='var(--border)'">
    <div style="font-size:1.2rem;margin-bottom:8px;">⌨️</div>
    <div style="font-weight:700;margin-bottom:4px;">CLI Reference</div>
    <div style="font-size:.85rem;color:var(--muted);">flux deploy, tail, why, trace, state, replay, bisect.</div>
  </a>
  <a href="/how-it-works" style="display:block;padding:20px 24px;border:1px solid var(--border);border-radius:10px;background:var(--bg-surface);color:var(--text);text-decoration:none;transition:border-color .15s;" onmouseenter="this.style.borderColor='var(--accent)'" onmouseleave="this.style.borderColor='var(--border)'">
    <div style="font-size:1.2rem;margin-bottom:8px;">🏗️</div>
    <div style="font-weight:700;margin-bottom:4px;">How It Works</div>
    <div style="font-size:.85rem;color:var(--muted);">Architecture: gateway, runtime, data engine, replay.</div>
  </a>
</div>

<h2>Core Concepts</h2>

<p>Everything in Fluxbase revolves around three ideas:</p>

<h3>1. Deterministic Execution</h3>
<p>Every request is executed and recorded atomically. The runtime captures every span — gateway, function, database queries, tool calls — and stores them indexed by request ID. There is no setup required; recording happens at the infrastructure level.</p>

<h3>2. Mutation Logging</h3>
<p>Every database write goes through the Data Engine, which logs the mutation — table, row, old value, new value, and the request ID that caused it. <code>flux state history</code> and <code>flux state blame</code> query this log directly.</p>

<h3>3. Replay Safety</h3>
<p>Because the complete input to every request is recorded, any request can be deterministically re-executed. <code>flux incident replay</code> disables outbound side-effects (emails, webhooks, Slack, cron) while re-running database writes against the current code. This makes testing production fixes safe and exact.</p>

<hr>

<h2>Module Docs</h2>

<table>
  <thead>
    <tr>
      <th>Module</th>
      <th>Purpose</th>
      <th>Link</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Gateway</td>
      <td>Auth, rate limiting, routing, query guard</td>
      <td><a href="/docs/gateway">docs/gateway</a></td>
    </tr>
    <tr>
      <td>Runtime</td>
      <td>TypeScript execution in sandboxed V8 isolates</td>
      <td><a href="/docs/runtime">docs/runtime</a></td>
    </tr>
    <tr>
      <td>Data Engine</td>
      <td>Query compiler, policy enforcement, mutation log</td>
      <td><a href="/docs/data-engine">docs/data-engine</a></td>
    </tr>
    <tr>
      <td>Queue</td>
      <td>Durable async job processing</td>
      <td><a href="/docs/queue">docs/queue</a></td>
    </tr>
    <tr>
      <td>CLI</td>
      <td>Developer toolchain (deploy, trace, debug, replay)</td>
      <td><a href="/cli">CLI Reference</a></td>
    </tr>
  </tbody>
</table>

<hr>

<h2>Debugging Reference</h2>

<table>
  <thead>
    <tr>
      <th>Question</th>
      <th>Command</th>
    </tr>
  </thead>
  <tbody>
    <tr><td>Why did this request fail?</td>            <td><code>flux why &lt;id&gt;</code></td></tr>
    <tr><td>What happened in this request?</td>        <td><code>flux trace &lt;id&gt;</code></td></tr>
    <tr><td>Step through it interactively?</td>        <td><code>flux trace debug &lt;id&gt;</code></td></tr>
    <tr><td>How did two requests differ?</td>          <td><code>flux trace diff &lt;a&gt; &lt;b&gt;</code></td></tr>
    <tr><td>What changed in the database?</td>         <td><code>flux state history &lt;table&gt; --id &lt;row&gt;</code></td></tr>
    <tr><td>Who set this field?</td>                   <td><code>flux state blame &lt;table&gt; --id &lt;row&gt;</code></td></tr>
    <tr><td>Replay the incident safely?</td>           <td><code>flux incident replay &lt;from&gt;..&lt;to&gt;</code></td></tr>
    <tr><td>Which commit broke it?</td>                <td><code>flux bug bisect --request &lt;id&gt;</code></td></tr>
    <tr><td>Preview a query before running?</td>       <td><code>flux explain &lt;query.json&gt;</code></td></tr>
  </tbody>
</table>` }}
    />
  )
}
