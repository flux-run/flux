/**
 * Security & Privacy — data model, storage, scalability.
 */
import { docsLayout } from '../../layouts/docs.js';
import { codeWindow, c } from '../../components/code-window.js';

export const meta = {
  title:       'Security & Privacy — Fluxbase',
  description: 'What Fluxbase records, where it is stored, how much storage it uses, and how the system scales.',
  path:        'docs/security-and-privacy.html',
};

function content() {
  const privacyWindow = codeWindow({
    title: 'what is recorded',
    content: `${c.dim('# Recorded by Fluxbase')}

  trace_requests   request_id, method, path, duration, status
  execution_spans  span_id, request_id, name, duration, error
  state_mutations  table, operation, ${c.ok('before_state')}, ${c.ok('after_state')}, request_id

${c.dim('# A recorded mutation looks like this:')}

  table:       users
  operation:   UPDATE
  before:      ${c.ok('{ plan: "free" }')}
  after:       ${c.ok('{ plan: "pro"  }')}
  request_id:  ${c.id('550e8400')}

${c.dim('# Not this:')}

  ${c.err('full users table')}
  ${c.err('request body / passwords / tokens')}
  ${c.err('your PostgreSQL database')}`,
  });

  const storageWindow = codeWindow({
    title: 'storage per request',
    content: `${c.dim('# Typical request breakdown')}

  trace_requests row     ~1 KB
  execution spans        ~2 KB   ${c.dim('(3–5 spans typical)')}
  mutation diffs         ~1 KB   ${c.dim('(1–3 mutations typical)')}
                         ─────
  total                  ${c.ok('~3–5 KB per request')}

${c.dim('# At scale')}

  100k req/day    →   ${c.ok('~400 MB/day')}
  1M   req/day    →   ${c.ok('~4 GB/day')}
  10M  req/day    →   ${c.ok('~40 GB/day')}

${c.dim('# Retention')}

  default        7 days   ${c.dim('(configurable)')}
  archive        S3/GCS   ${c.dim('(optional)')}
  auto-purge     enabled  ${c.dim('(older than retention window)')}`,
  });

  const scaleWindow = codeWindow({
    title: 'request path — no coordination',
    content: `HTTP request
  │
  ▼
Gateway (horizontal)
  │  emits: trace_requests row
  ▼
Runtime (isolated per tenant)
  │  emits: execution_spans rows
  ▼
Data Engine (append-only writes)
  │  emits: state_mutations rows
  ▼
Your PostgreSQL

${c.dim('# Every step is an independent append-only write.')}
${c.dim('# No cross-request locking.')}
${c.dim('# No coordination between concurrent requests.')}
${c.ok('# Horizontal scale: add more Gateway / Runtime pods.')}`,
  });

  return `<article class="doc-content">

<h1>Security &amp; Privacy</h1>

<p class="doc-lead">Fluxbase records execution history — not your database. Here is exactly what is stored, how much space it uses, and how the system handles high traffic.</p>

<hr class="doc-divider">

<h2 id="data-privacy">Data Privacy</h2>

<p>Fluxbase does not store your application database. Your PostgreSQL data stays in your own Postgres instance, under your own cloud account.</p>

<p>What Fluxbase records is <strong>execution metadata</strong>:</p>

<ul>
  <li><strong>Request envelopes</strong> — method, path, duration, status, tenant, project</li>
  <li><strong>Execution spans</strong> — which function ran, latency of each step, any error message</li>
  <li><strong>Mutation diffs</strong> — before-state and after-state for each row that changed, linked back to the request that caused it</li>
</ul>

<p>Mutation diffs are field-level deltas, not table snapshots. A plan upgrade records <code>plan: "free" → "pro"</code> — not the entire users table.</p>

${privacyWindow}

<h3 id="masking">Sensitive field masking</h3>

<p>Fields that should never appear in mutation records can be marked as masked in your schema definition. Masked fields are replaced with <code>"[REDACTED]"</code> before the diff is written. Typical candidates: <code>password_hash</code>, <code>ssn</code>, <code>credit_card_number</code>.</p>

<pre style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:8px;padding:16px 20px;font-size:.82rem;line-height:1.7;overflow-x:auto;"><code>-- schema definition
ALTER TABLE users ENABLE FLUXBASE AUDIT MASK (password_hash, ssn);</code></pre>

<p>Request bodies are <strong>not</strong> recorded by default. Only structured span data (duration, error string, status code) is captured at the gateway layer.</p>

<hr class="doc-divider">

<h2 id="storage">Storage &amp; Retention</h2>

<p>Because Fluxbase records diffs rather than snapshots, storage is small relative to traffic volume.</p>

${storageWindow}

<h3 id="retention">Retention policies</h3>

<p>Retention is configurable per project:</p>

<table style="width:100%;border-collapse:collapse;font-size:.88rem;margin:0 0 20px;">
  <thead>
    <tr>
      <th style="text-align:left;padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">Tier</th>
      <th style="text-align:left;padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">Default</th>
      <th style="text-align:left;padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">Notes</th>
    </tr>
  </thead>
  <tbody>
    <tr><td style="padding:8px 14px;border-bottom:1px solid var(--border);">Hot (fully queryable)</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);">7 days</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">all CLI commands work</td></tr>
    <tr><td style="padding:8px 14px;border-bottom:1px solid var(--border);">Warm (exportable)</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);">30 days</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">downloadable, not live-queried</td></tr>
    <tr><td style="padding:8px 14px;">Archive</td><td style="padding:8px 14px);">custom</td><td style="padding:8px 14px;color:var(--muted);">export to S3 / GCS</td></tr>
  </tbody>
</table>

<p>Records older than the hot window are automatically purged or archived. You can lower the window to 1 day to minimise storage, or extend it for compliance requirements.</p>

<hr class="doc-divider">

<h2 id="scalability">Scalability</h2>

<p>Fluxbase is designed to run in front of high-traffic production backends without becoming a bottleneck.</p>

${scaleWindow}

<h3 id="scale-properties">Key properties</h3>

<ul>
  <li><strong>Gateway is horizontally scalable.</strong> Stateless; add pods to handle more traffic. Each pod emits spans independently with no shared state.</li>
  <li><strong>Runtime uses per-tenant isolate affinity.</strong> One V8 isolate is kept warm per worker thread and reused across requests from the same tenant. New tenant = new isolate. No cross-tenant coordination.</li>
  <li><strong>Mutation logs are append-only.</strong> The Data Engine writes one row per mutation in the same transaction as the user's write. No locking beyond that transaction.</li>
  <li><strong>PostgreSQL handles storage efficiently.</strong> Append-only write patterns are among the most scalable Postgres workloads. TimescaleDB continuous aggregates can be used for large volumes.</li>
  <li><strong>No synchronous tracing pipeline.</strong> Span emission is fire-and-forget. A slow trace write does not delay the HTTP response.</li>
</ul>

<h3 id="limits">Practical limits</h3>

<table style="width:100%;border-collapse:collapse;font-size:.88rem;margin:0 0 20px;">
  <thead>
    <tr>
      <th style="text-align:left;padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">Traffic</th>
      <th style="text-align:left;padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">Gateway pods</th>
      <th style="text-align:left;padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">Storage (7-day hot)</th>
    </tr>
  </thead>
  <tbody>
    <tr><td style="padding:8px 14px;border-bottom:1px solid var(--border);">10k req/day</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);">1</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">~350 MB</td></tr>
    <tr><td style="padding:8px 14px;border-bottom:1px solid var(--border);">100k req/day</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);">1–2</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">~3.5 GB</td></tr>
    <tr><td style="padding:8px 14px;border-bottom:1px solid var(--border);">1M req/day</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);">3–5</td><td style="padding:8px 14px;border-bottom:1px solid var(--border);color:var(--muted);">~35 GB</td></tr>
    <tr><td style="padding:8px 14px;">10M req/day</td><td style="padding:8px 14px;">10+</td><td style="padding:8px 14px;color:var(--muted);">~350 GB (archive recommended)</td></tr>
  </tbody>
</table>

<hr class="doc-divider">

<h2 id="replay-guarantees">Replay Guarantees</h2>

<p>Replay re-executes a recorded request against the current version of your code using the stored span inputs. It is designed to reproduce the <strong>data state</strong> of the original request — not every non-deterministic value your code might compute.</p>

<h3>What replay does guarantee</h3>

<ul>
  <li><strong>Database state transitions are reproduced.</strong> The same rows are written, the same mutations are applied. The before/after diff recorded in the original request is used to drive the re-execution path.</li>
  <li><strong>External side effects are disabled.</strong> Email sends, webhook calls, Stripe charges, and any registered tool calls are intercepted and skipped. Your data store changes; the outside world does not.</li>
  <li><strong>All spans are re-recorded.</strong> The replay run produces a fresh trace that can be compared span-by-span against the original — including latency, errors, and per-step output.</li>
</ul>

<h3>What replay does not guarantee</h3>

<ul>
  <li><strong>Non-deterministic values.</strong> If your function calls <code>Date.now()</code>, <code>Math.random()</code>, or reads from an external source, those values will differ in the replay run. This is expected and does not affect data state reproduction.</li>
  <li><strong>Reads from untracked external state.</strong> If your function reads from a third-party API or a cache that is not part of the Fluxbase trace, replay uses the live value at the time it runs.</li>
</ul>

<p style="margin-top:16px;padding:14px 18px;background:var(--bg-elevated);border-left:3px solid var(--accent);border-radius:0 6px 6px 0;font-size:.88rem;line-height:1.65;">
  <strong>In short:</strong> replay guarantees data state reproduction, not perfect code determinism. For debugging — which is the primary use case — reproducing the data transitions is what matters.
</p>

<hr class="doc-divider">

<h2 id="data-residency">Data residency</h2>

<p>Fluxbase is deployed in GCP <code>asia-south1</code> by default. The trace and mutation data is stored in your own PostgreSQL database — which you can host in any region on any cloud.</p>

<p>Because your database is BYODB (bring your own), data residency, encryption at rest, and backup policies are under your control.</p>

<hr class="doc-divider">

<div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;padding:24px 28px;margin-top:8px;">
  <p style="font-weight:700;color:var(--text);margin:0 0 8px;">In short</p>
  <p style="color:var(--muted);margin:0;line-height:1.7;"><strong style="color:var(--text);">Your code.</strong> Your database. Your infrastructure. Fluxbase records the execution history — not the data.</p>
</div>

</article>`;
}

export function render() {
  return docsLayout({
    meta,
    activePath: '/docs/security-and-privacy',
    content: content(),
  });
}
