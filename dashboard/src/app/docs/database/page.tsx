import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Database — Fluxbase Docs',
  description: '',
}

export default function Page() {
  return (
    <div
      dangerouslySetInnerHTML={{ __html: `<h1>Database</h1>
<p class="page-subtitle">Hosted Postgres — query from functions using a simple, structured API.</p>

<h2>Overview</h2>
<p>Every Fluxbase project has a dedicated Postgres database. You interact with it via <code>ctx.db.query()</code> inside your functions — no boilerplate, no connection pooling, no ORM configuration.</p>

<h2>ctx.db.query(params)</h2>
<pre><code><span class="kw">const</span> rows = <span class="kw">await</span> ctx.db.<span class="fn">query</span>(params);</code></pre>

<table>
  <thead><tr><th>Field</th><th>Type</th><th>Required</th><th>Description</th></tr></thead>
  <tbody>
    <tr><td><code>table</code></td><td><code>string</code></td><td>✔</td><td>Table name</td></tr>
    <tr><td><code>operation</code></td><td><code>"select" | "insert" | "update" | "delete"</code></td><td>✔</td><td>Query type</td></tr>
    <tr><td><code>filters</code></td><td><code>Filter[]</code></td><td></td><td>WHERE conditions</td></tr>
    <tr><td><code>columns</code></td><td><code>string[]</code></td><td></td><td>Columns to return (SELECT)</td></tr>
    <tr><td><code>data</code></td><td><code>object</code></td><td></td><td>Row data (INSERT / UPDATE)</td></tr>
    <tr><td><code>limit</code></td><td><code>number</code></td><td></td><td>Max rows returned</td></tr>
    <tr><td><code>offset</code></td><td><code>number</code></td><td></td><td>Pagination offset</td></tr>
  </tbody>
</table>

<h2>Filter operators</h2>
<table>
  <thead><tr><th>op</th><th>SQL equivalent</th></tr></thead>
  <tbody>
    <tr><td><code>eq</code></td><td><code>= value</code></td></tr>
    <tr><td><code>neq</code></td><td><code>!= value</code></td></tr>
    <tr><td><code>gt</code></td><td><code>&gt; value</code></td></tr>
    <tr><td><code>lt</code></td><td><code>&lt; value</code></td></tr>
    <tr><td><code>gte</code></td><td><code>&gt;= value</code></td></tr>
    <tr><td><code>lte</code></td><td><code>&lt;= value</code></td></tr>
    <tr><td><code>like</code></td><td><code>LIKE value</code></td></tr>
    <tr><td><code>in</code></td><td><code>IN (values)</code></td></tr>
    <tr><td><code>is_null</code></td><td><code>IS NULL</code></td></tr>
  </tbody>
</table>

<h2>Examples</h2>

<h3>SELECT — fetch rows</h3>
<pre><code><span class="kw">const</span> todos = <span class="kw">await</span> ctx.db.<span class="fn">query</span>({
  table: <span class="str">"todos"</span>,
  operation: <span class="str">"select"</span>,
  filters: [{ column: <span class="str">"done"</span>, op: <span class="str">"eq"</span>, value: <span class="kw">false</span> }],
  columns: [<span class="str">"id"</span>, <span class="str">"title"</span>, <span class="str">"created_at"</span>],
  limit: <span class="num">50</span>,
});</code></pre>

<h3>INSERT — create a row</h3>
<pre><code><span class="kw">const</span> [todo] = <span class="kw">await</span> ctx.db.<span class="fn">query</span>({
  table: <span class="str">"todos"</span>,
  operation: <span class="str">"insert"</span>,
  data: { title: input.title, done: <span class="kw">false</span>, user_id: input.userId },
});</code></pre>

<h3>UPDATE — modify a row</h3>
<pre><code><span class="kw">await</span> ctx.db.<span class="fn">query</span>({
  table: <span class="str">"todos"</span>,
  operation: <span class="str">"update"</span>,
  filters: [{ column: <span class="str">"id"</span>, op: <span class="str">"eq"</span>, value: input.id }],
  data: { done: <span class="kw">true</span> },
});</code></pre>

<h3>DELETE — remove a row</h3>
<pre><code><span class="kw">await</span> ctx.db.<span class="fn">query</span>({
  table: <span class="str">"todos"</span>,
  operation: <span class="str">"delete"</span>,
  filters: [{ column: <span class="str">"id"</span>, op: <span class="str">"eq"</span>, value: input.id }],
});</code></pre>

<h2>Schema migrations</h2>
<p>Define your schema as SQL files in <code>schema/</code>. Apply them with:</p>
<pre><code><span class="shell-prompt">$</span> flux db migrate</code></pre>
<p>Example <code>schema/todos.sql</code>:</p>
<pre><code><span class="kw">CREATE TABLE IF NOT EXISTS</span> todos (
  id         UUID <span class="kw">DEFAULT</span> gen_random_uuid() <span class="kw">PRIMARY KEY</span>,
  title      TEXT <span class="kw">NOT NULL</span>,
  done       BOOLEAN <span class="kw">DEFAULT</span> <span class="kw">FALSE</span>,
  user_id    TEXT,
  created_at TIMESTAMPTZ <span class="kw">DEFAULT</span> now()
);</code></pre>

<h2>Row-level security</h2>
<p>You can enable tenant isolation by filtering on a <code>tenant_id</code> column. The gateway injects tenant context automatically when using project API keys.</p>

<h2>Observability</h2>
<p>All database queries are automatically traced. Slow queries (&gt;100ms) are flagged, N+1 patterns are detected, and index suggestions are generated based on repeated filter columns. See <a href="/docs/observability">Observability</a>.</p>

<hr>
<p style="display:flex;gap:16px;font-size:.875rem;">
  <a href="/docs/functions">← Functions</a>
  <a href="/docs/observability">Next: Observability →</a>
</p>` }}
    />
  )
}
