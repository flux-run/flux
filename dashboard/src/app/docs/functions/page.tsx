import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Functions — Fluxbase Docs',
  description: '',
}

export default function Page() {
  return (
    <div
      dangerouslySetInnerHTML={{ __html: `<h1>Functions</h1>
<p class="page-subtitle">Serverless TypeScript functions — the core building block of Fluxbase.</p>

<h2>Overview</h2>
<p>A Fluxbase function is a TypeScript module that exports a single handler via <code>defineFunction()</code>.
Functions are deployed as isolated serverless units, auto-scaled on demand, and exposed as HTTP endpoints via the API Gateway.</p>

<h2>Defining a function</h2>
<pre><code><span class="kw">import</span> { <span class="fn">defineFunction</span> } <span class="kw">from</span> <span class="str">"@fluxbase/functions"</span>;

<span class="kw">export default</span> <span class="fn">defineFunction</span>({
  name: <span class="str">"greet"</span>,

  handler: <span class="kw">async</span> ({ input, ctx }) => {
    ctx.<span class="fn">log</span>(<span class="str">"invoked with"</span>, input);
    <span class="kw">return</span> { message: <span class="str">\`Hello, \${input.name}!\`</span> };
  },
});</code></pre>

<h2>The handler signature</h2>
<pre><code><span class="kw">handler</span>: <span class="kw">async</span> ({ input, ctx }: HandlerArgs) => Result</code></pre>

<table>
  <thead><tr><th>Parameter</th><th>Type</th><th>Description</th></tr></thead>
  <tbody>
    <tr><td><code>input</code></td><td><code>Record&lt;string,unknown&gt;</code></td><td>JSON body parsed from the HTTP request</td></tr>
    <tr><td><code>ctx</code></td><td><code>FluxContext</code></td><td>Runtime context — logging, secrets, env, db</td></tr>
  </tbody>
</table>

<h2>FluxContext API</h2>

<h3><code>ctx.log(...args)</code></h3>
<p>Structured log that appears in <code>flux logs</code> with the current request ID attached.</p>
<pre><code>ctx.<span class="fn">log</span>(<span class="str">"processing"</span>, { user_id: input.user_id });</code></pre>

<h3><code>ctx.env</code></h3>
<p>Environment variables configured for the project (set via <code>flux secrets set</code>).</p>
<pre><code><span class="kw">const</span> apiUrl = ctx.env.API_URL;
<span class="kw">const</span> region = ctx.env.REGION ?? <span class="str">"us-east-1"</span>;</code></pre>

<h3><code>ctx.secrets.get(key)</code></h3>
<p>Retrieves a secret value from the encrypted secrets store.</p>
<pre><code><span class="kw">const</span> apiKey = <span class="kw">await</span> ctx.secrets.<span class="fn">get</span>(<span class="str">"OPENAI_API_KEY"</span>);</code></pre>

<h3><code>ctx.db.query(params)</code></h3>
<p>Execute a structured database query. See the <a href="/docs/database">Database</a> guide for full reference.</p>
<pre><code><span class="kw">const</span> rows = <span class="kw">await</span> ctx.db.<span class="fn">query</span>({
  table: <span class="str">"todos"</span>,
  operation: <span class="str">"select"</span>,
  filters: [{ column: <span class="str">"done"</span>, op: <span class="str">"eq"</span>, value: <span class="kw">false</span> }],
});</code></pre>

<h2>flux.json — function config</h2>
<p>Each function directory contains a <code>flux.json</code> that declares its metadata:</p>
<pre><code>{
  <span class="str">"name"</span>: <span class="str">"create_todo"</span>,
  <span class="str">"runtime"</span>: <span class="str">"deno"</span>,
  <span class="str">"route"</span>: <span class="str">"/create_todo"</span>,
  <span class="str">"method"</span>: <span class="str">"POST"</span>
}</code></pre>

<table>
  <thead><tr><th>Field</th><th>Required</th><th>Description</th></tr></thead>
  <tbody>
    <tr><td><code>name</code></td><td>✔</td><td>Unique function name within the project</td></tr>
    <tr><td><code>runtime</code></td><td>✔</td><td><code>"deno"</code> (default) or <code>"node"</code></td></tr>
    <tr><td><code>route</code></td><td></td><td>HTTP route path (defaults to <code>/&lt;name&gt;</code>)</td></tr>
    <tr><td><code>method</code></td><td></td><td>HTTP method (defaults to <code>POST</code>)</td></tr>
    <tr><td><code>async</code></td><td></td><td>If <code>true</code>, function runs asynchronously (fire-and-forget)</td></tr>
  </tbody>
</table>

<h2>Project structure</h2>
<pre><code>my-project/
  functions/
    my_function/
      index.ts      ← handler
      flux.json     ← config
      package.json  ← npm dependencies for this function (optional)
  schema/
    *.sql           ← database migrations
  .env.example</code></pre>

<h2>Local development</h2>
<pre><code><span class="cm"># Run a function locally (watches for changes)</span>
<span class="shell-prompt">$</span> flux dev

<span class="cm"># Invoke directly</span>
<span class="shell-prompt">$</span> flux invoke create_todo --payload '{"title":"test"}'</code></pre>

<h2>Deploying</h2>
<pre><code><span class="cm"># Deploy all functions in the project</span>
<span class="shell-prompt">$</span> flux deploy

<span class="cm"># Deploy a single function (from inside its directory)</span>
<span class="shell-prompt">$</span> cd functions/create_todo
<span class="shell-prompt">$</span> flux deploy</code></pre>

<hr>
<p style="display:flex;gap:16px;font-size:.875rem;">
  <a href="/docs/quickstart">← Quickstart</a>
  <a href="/docs/database">Next: Database →</a>
</p>` }}
    />
  )
}
