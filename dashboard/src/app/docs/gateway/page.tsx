import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'API Gateway — Fluxbase Docs',
  description: 'Fluxbase API Gateway: authentication, rate limiting, CORS, routing, and request tracing — all configured automatically for your project.',
}

export default function Page() {
  return (
    <div
      dangerouslySetInnerHTML={{ __html: `<h1>API Gateway</h1>
<p class="page-subtitle">Authentication, rate limiting, CORS, routing, and distributed tracing — automatically applied to every function you deploy.</p>

<h2>Overview</h2>
<p>The Fluxbase API Gateway sits in front of all your deployed functions. It handles every concern that should be resolved before your business logic runs — authentication, routing, rate limiting, CORS — and stamps every request with a unique trace ID that flows through the entire system.</p>

<p>You don't configure the gateway manually. Routes are derived from your <code>flux.json</code> files. Auth is enforced using your project API keys. CORS is handled automatically. Nothing to set up.</p>

<div class="callout callout-info">
  <div class="callout-title">Request flow</div>
  Client → Gateway (auth · route · rate-limit · CORS · x-request-id) → Runtime (your function) → Response
</div>

<h2>Authentication</h2>
<p>Every request to a deployed function must carry a valid credential. The gateway supports two modes:</p>

<h3>Project API keys</h3>
<p>The primary authentication method for server-to-server and client-to-server calls. Generate keys in the dashboard or CLI.</p>
<pre><code><span class="cm"># Using a project API key in the Authorization header</span>
<span class="shell-prompt">$</span> curl https://gw.fluxbase.co/create_order \\
    -H <span class="str">"Authorization: Bearer flx_live_abc123..."</span> \\
    -H <span class="str">"Content-Type: application/json"</span> \\
    -d <span class="str">'{"userId": "u_881", "total": 49.99}'</span></code></pre>

<h3>Managing API keys</h3>
<pre><code><span class="cm"># Create a new API key for your project</span>
<span class="shell-prompt">$</span> flux api-keys create --name "production-server"
<span class="str">flx_live_abc123...xyz789</span>

<span class="cm"># List all keys</span>
<span class="shell-prompt">$</span> flux api-keys list

  production-server   flx_live_abc1...   created 2 days ago
  staging             flx_test_xyz9...   created 1 week ago

<span class="cm"># Revoke a key</span>
<span class="shell-prompt">$</span> flux api-keys revoke production-server</code></pre>

<h2>Routing</h2>
<p>The gateway builds its routing table from the <code>flux.json</code> files of all deployed functions in your project. Each function declares its own route and HTTP method.</p>

<pre><code><span class="cm">// functions/create_order/flux.json</span>
{
  <span class="str">"name"</span>: <span class="str">"create_order"</span>,
  <span class="str">"runtime"</span>: <span class="str">"deno"</span>,
  <span class="str">"route"</span>: <span class="str">"/create_order"</span>,
  <span class="str">"method"</span>: <span class="str">"POST"</span>
}</code></pre>

<p>After running <code>flux deploy</code>, this function is reachable at:</p>
<pre><code>POST https://gw.fluxbase.co/create_order</code></pre>

<h3>Route configuration fields</h3>
<table>
  <thead><tr><th>Field</th><th>Default</th><th>Description</th></tr></thead>
  <tbody>
    <tr><td><code>route</code></td><td><code>/&lt;name&gt;</code></td><td>URL path for this function</td></tr>
    <tr><td><code>method</code></td><td><code>POST</code></td><td>HTTP method: <code>GET</code>, <code>POST</code>, <code>PUT</code>, <code>PATCH</code>, <code>DELETE</code></td></tr>
    <tr><td><code>async</code></td><td><code>false</code></td><td>If <code>true</code>, invocation is queued; HTTP response returns immediately</td></tr>
  </tbody>
</table>

<h2>Rate limiting</h2>
<p>The gateway enforces per-project rate limits to protect your functions from traffic spikes and abuse. Limits are applied per API key.</p>

<table>
  <thead><tr><th>Plan</th><th>Requests / minute</th><th>Burst</th></tr></thead>
  <tbody>
    <tr><td>Free</td><td>60</td><td>120</td></tr>
    <tr><td>Pro</td><td>1,000</td><td>2,000</td></tr>
    <tr><td>Team</td><td>10,000</td><td>30,000</td></tr>
  </tbody>
</table>

<p>When a rate limit is exceeded, the gateway returns:</p>
<pre><code>HTTP 429 Too Many Requests
Retry-After: 15</code></pre>

<h2>CORS</h2>
<p>CORS is handled automatically at the gateway level. By default, all origins are allowed for GET requests. For POST/PUT/DELETE, restrict origins in your project settings.</p>

<p>To configure allowed origins:</p>
<pre><code><span class="cm"># Allow only your frontend domain</span>
<span class="shell-prompt">$</span> flux project update --cors-origin https://your-app.com</code></pre>

<div class="callout callout-info">
  <div class="callout-title">Preflight requests</div>
  The gateway handles <code>OPTIONS</code> preflight requests automatically. You do not need to write a preflight handler function.
</div>

<h2>Request tracing</h2>
<p>The gateway stamps every inbound request with:</p>
<ul>
  <li><code>x-request-id</code> — a unique ID for the request, propagated to the runtime and data engine</li>
  <li>Gateway auth result, rate-limit check outcome, and routing decision are all recorded as the first span in the trace</li>
</ul>

<p>The trace ID is also returned in the HTTP response:</p>
<pre><code>HTTP/2 200
x-request-id: 9c3e7f1a
content-type: application/json

{"orderId": "ord_xyz"}</code></pre>

<p>Use this ID with <code>flux trace</code>:</p>
<pre><code><span class="shell-prompt">$</span> flux trace 9c3e7f1a</code></pre>

<h2>Middleware pipeline</h2>
<p>The gateway processes middleware in a fixed order. Each step either passes the request forward or returns an error response:</p>
<ol>
  <li><strong>API key authentication</strong> — validates the <code>Authorization</code> header</li>
  <li><strong>Rate limit check</strong> — counts against the key's quota</li>
  <li><strong>Route lookup</strong> — matches the path + method to a deployed function</li>
  <li><strong>CORS headers</strong> — applies configured origin allowlist</li>
  <li><strong>x-request-id injection</strong> — stamps a unique trace ID</li>
  <li><strong>Function dispatch</strong> — forwards to the runtime or queue (for async)</li>
</ol>

<h2>Error responses</h2>
<table>
  <thead><tr><th>Status</th><th>Cause</th></tr></thead>
  <tbody>
    <tr><td><code>401 Unauthorized</code></td><td>Missing or invalid API key</td></tr>
    <tr><td><code>403 Forbidden</code></td><td>Key does not have access to this project</td></tr>
    <tr><td><code>404 Not Found</code></td><td>No function deployed at this route/method</td></tr>
    <tr><td><code>429 Too Many Requests</code></td><td>Rate limit exceeded</td></tr>
    <tr><td><code>502 Bad Gateway</code></td><td>Runtime failed to start or crashed</td></tr>
    <tr><td><code>504 Gateway Timeout</code></td><td>Function exceeded execution time limit (30s)</td></tr>
  </tbody>
</table>

<hr>
<p style="display:flex;gap:16px;font-size:.875rem;">
  <a href="/docs/architecture">← Architecture</a>
  <a href="/docs/secrets">Next: Secrets →</a>
</p>` }}
    />
  )
}
