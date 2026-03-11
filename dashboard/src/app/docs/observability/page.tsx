import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Observability — Fluxbase Docs',
  description: '',
}

export default function Page() {
  return (
    <div
      dangerouslySetInnerHTML={{ __html: `<h1>Observability</h1>
<p class="page-subtitle">Distributed traces, N+1 detection, slow query alerts, and automatic index suggestions — all built in, zero configuration.</p>

<h2>Overview</h2>
<p>Every request flowing through Fluxbase is automatically instrumented. You don't add libraries or configure exporters. Just deploy and observe.</p>

<div class="callout callout-success">
  <div class="callout-title">Zero-config observability</div>
  All traces, spans, and diagnostics are captured automatically. No SDK calls, no OpenTelemetry setup.
</div>

<h2>Distributed traces</h2>
<p>Every request gets a unique <code>x-request-id</code> that is propagated across the gateway, runtime, and data-engine. All spans for a request are correlated under this ID.</p>

<p>View a trace in your terminal:</p>
<pre><code><span class="shell-prompt">$</span> flux trace &lt;trace-id&gt;

  Trace 4f9a3b2c   2026-03-10 14:22:01 UTC

  ▸ function:create_todo           312 ms
    ▸ db:query(todos)               28 ms  [MISS]
      table: todos
      op:    insert
    ▸ db:query(todos)               15 ms  [HIT]  ← cache

  ─── Slow Queries ────────────────────────────────
    todos.user_id   284 ms

  ─── Index Suggestions ───────────────────────────
    → todos.user_id   run: CREATE INDEX ON todos(user_id);</code></pre>

<p>Trace IDs appear in:</p>
<ul>
  <li><code>x-request-id</code> response header on every API call</li>
  <li><code>flux logs</code> output</li>
  <li>The Fluxbase dashboard under <strong>Logs → Traces</strong></li>
</ul>

<h2>Span types</h2>
<table>
  <thead><tr><th>Span kind</th><th>What it tracks</th></tr></thead>
  <tbody>
    <tr><td><code>function</code></td><td>Total function execution time</td></tr>
    <tr><td><code>db:query</code></td><td>Individual database queries — table, operation, duration, cache status</td></tr>
    <tr><td><code>gateway</code></td><td>Auth, routing, rate-limit checks</td></tr>
  </tbody>
</table>

<h2>N+1 query detection</h2>
<p>Fluxbase automatically detects N+1 query patterns — when the same table is queried inside a loop, turning one request into dozens of database round-trips.</p>

<p>Example warning in a trace response:</p>
<pre><code>{
  "n1_warnings": [
    {
      "table": "users",
      "count": 47,
      "message": "N+1 detected: 'users' queried 47 times in one request"
    }
  ]
}</code></pre>

<p>Fix example — batch with a single <code>IN</code> filter instead:</p>
<pre><code><span class="cm">// ✗ N+1 — one query per item</span>
<span class="kw">for</span> (<span class="kw">const</span> item <span class="kw">of</span> items) {
  <span class="kw">const</span> [user] = <span class="kw">await</span> ctx.db.<span class="fn">query</span>({
    table: <span class="str">"users"</span>, operation: <span class="str">"select"</span>,
    filters: [{ column: <span class="str">"id"</span>, op: <span class="str">"eq"</span>, value: item.user_id }],
  });
}

<span class="cm">// ✔ Batched — one query for all items</span>
<span class="kw">const</span> users = <span class="kw">await</span> ctx.db.<span class="fn">query</span>({
  table: <span class="str">"users"</span>, operation: <span class="str">"select"</span>,
  filters: [{ column: <span class="str">"id"</span>, op: <span class="str">"in"</span>, value: items.<span class="fn">map</span>(i => i.user_id) }],
});</code></pre>

<h2>Slow query detection</h2>
<p>Any database query that takes longer than <strong>100 ms</strong> is flagged as slow. The trace shows which table and operation was slow, helping you prioritize optimization.</p>

<h2>Automatic index suggestions</h2>
<p>When slow queries repeatedly filter on the same column, Fluxbase generates a <code>CREATE INDEX</code> suggestion:</p>

<pre><code>{
  "suggested_indexes": [
    {
      "table": "todos",
      "column": "user_id",
      "ddl": "CREATE INDEX ON todos(user_id);"
    }
  ]
}</code></pre>

<p>Apply it:</p>
<pre><code><span class="cm">-- Run against your project's database</span>
CREATE INDEX ON todos(user_id);</code></pre>

<div class="callout callout-info">
  <div class="callout-title">How it works</div>
  The API aggregates all slow spans (&gt;100 ms) for a trace. When the same <code>(table, column)</code> pair appears
  2 or more times in slow filter positions, a <code>CREATE INDEX</code> DDL statement is generated and returned
  alongside the trace response.
</div>

<h2>Logs</h2>
<p>All <code>ctx.log()</code> calls emit structured JSON logs correlated with the trace ID.</p>
<pre><code><span class="shell-prompt">$</span> flux logs --tail
<span class="shell-prompt">$</span> flux logs --function create_todo --since 1h --limit 100</code></pre>

<h2>Dashboard</h2>
<p>All observability data is also visible in the Fluxbase dashboard at <a href="/dashboard">/dashboard</a> under <strong>Logs</strong> and <strong>Traces</strong>.</p>

<hr>
<p style="display:flex;gap:16px;font-size:.875rem;">
  <a href="/docs/database">← Database</a>
  <a href="/docs/cli">Next: CLI Reference →</a>
</p>` }}
    />
  )
}
