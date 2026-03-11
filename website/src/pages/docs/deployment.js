/**
 * Deployment — cloud vs self-hosted, docker-compose, BYODB.
 */
import { docsLayout } from '../../layouts/docs.js';
import { codeWindow, c } from '../../components/code-window.js';

export const meta = {
  title:       'Deployment — Fluxbase',
  description: 'Cloud-managed or fully self-hosted. How to run Fluxbase in your own infrastructure with Docker Compose, what each service does, and how BYODB works.',
  path:        'docs/deployment.html',
};

function content() {
  const cloudWindow = codeWindow({
    title: 'cloud: deploy in 20 seconds',
    content: `${c.dim('# Install CLI')}
${c.cmd('$')} curl -fsSL https://fluxbase.co/install | bash

${c.dim('# Authenticate')}
${c.cmd('$')} flux login
  ${c.ok('✔')}  Logged in as alice@example.com

${c.dim('# Deploy your functions')}
${c.cmd('$')} flux deploy
  ${c.ok('✔')}  create_user  → gw.fluxbase.co/create_user
  ${c.ok('✔')}  list_users   → gw.fluxbase.co/list_users
  ${c.ok('✔')}  Deployed in 18s

${c.dim('# Start debugging')}
${c.cmd('$')} flux tail`,
  });

  const composeWindow = codeWindow({
    title: 'self-hosted: docker compose up',
    content: `${c.dim('# Clone the repo')}
${c.cmd('$')} git clone https://github.com/fluxbase/fluxbase
${c.cmd('$')} cd fluxbase

${c.dim('# Configure environment')}
${c.cmd('$')} cp .env.dev.example .env.dev
${c.cmd('$')} $EDITOR .env.dev   ${c.dim('# set DATABASE_URL, etc.')}

${c.dim('# Start all services')}
${c.cmd('$')} flux stack up --build

  ${c.ok('✔')}  db            postgres:16  :5432
  ${c.ok('✔')}  api           :8080
  ${c.ok('✔')}  gateway       :8081
  ${c.ok('✔')}  data-engine   :8082
  ${c.ok('✔')}  runtime       :8083
  ${c.ok('✔')}  queue         :8084

${c.dim('# Point CLI at local stack')}
${c.cmd('$')} flux login --endpoint http://localhost:8080`,
  });

  const byodbWindow = codeWindow({
    title: 'BYODB — your postgres, always',
    content: `${c.dim('# Point Fluxbase at your existing Postgres')}
${c.dim('# in .env.dev or cloud project settings:')}

  DATABASE_URL=postgresql://user:pass@your-db.host:5432/myapp

${c.dim('# Fluxbase creates two schemas:')}

  ${c.ok('public')}          ${c.dim('← your application tables (unchanged)')}
  ${c.ok('fluxbase_trace')}  ${c.dim('← spans, mutations, request log')}

${c.dim('# Your tables are never modified.')}
${c.dim('# Trace data lives in its own schema.')}
${c.ok('# You control backups, region, encryption.')}`,
  });

  const services = [
    { port: '8080', name: 'API',         tech: 'Rust (Axum)',    desc: 'Management plane — deploy functions, manage API keys, tenant config, schema definitions. Consumed by the CLI and Dashboard.' },
    { port: '8081', name: 'Gateway',     tech: 'Rust (Axum)',    desc: 'Public execution edge — auth, rate limiting, request routing. Assigns request IDs and emits the first span. Horizontally scalable.' },
    { port: '8082', name: 'Data Engine', tech: 'Rust (Axum)',    desc: 'Query compiler and mutation logger — validates writes, applies row-level security and column policies, writes mutation diffs in the same transaction as user writes.' },
    { port: '8083', name: 'Runtime',     tech: 'Rust + Deno V8', desc: 'TypeScript execution — runs your functions in sandboxed V8 isolates per tenant. Warm isolates for low latency. All ctx.db / ctx.tool calls are intercepted here.' },
    { port: '8084', name: 'Queue',       tech: 'Rust',           desc: 'Async job queue — stores job payloads in Postgres, polls and executes async functions via the Runtime. Fully traced.' },
  ];

  const serviceRows = services.map(s => `<tr>
    <td style="padding:10px 14px;border-bottom:1px solid var(--border);font-family:var(--font-mono);font-size:.8rem;color:var(--muted);">:${s.port}</td>
    <td style="padding:10px 14px;border-bottom:1px solid var(--border);font-weight:600;font-size:.88rem;">${s.name}</td>
    <td style="padding:10px 14px;border-bottom:1px solid var(--border);font-family:var(--font-mono);font-size:.77rem;color:var(--accent);">${s.tech}</td>
    <td style="padding:10px 14px;border-bottom:1px solid var(--border);font-size:.84rem;color:var(--muted);line-height:1.5;">${s.desc}</td>
  </tr>`).join('\n  ');

  return `<article class="doc-content">

<h1>Deployment</h1>

<p class="doc-lead">Fluxbase can run as a managed cloud service or as a fully self-hosted stack on your own infrastructure. Both options use the same CLI and the same BYODB (Bring Your Own Database) model.</p>

<hr class="doc-divider">

<h2 id="options">Deployment options</h2>

<div style="display:grid;grid-template-columns:1fr 1fr;gap:24px;margin:0 0 24px;">
  <div style="background:var(--bg-elevated);border:1px solid var(--accent);border-radius:10px;padding:22px 24px;">
    <div style="font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.12em;color:var(--accent);margin-bottom:10px;">Cloud (managed)</div>
    <p style="font-size:.88rem;line-height:1.65;margin:0 0 12px;">Fluxbase runs the infrastructure at <strong>fluxbase.co</strong>. You install the CLI, <code>flux login</code>, and deploy. Zero infrastructure to manage.</p>
    <ul style="font-size:.85rem;color:var(--muted);line-height:1.7;margin:0;padding-left:18px;">
      <li>Deploy in ~20 seconds</li>
      <li>Automatic scaling</li>
      <li>Your Postgres, connected via <code>DATABASE_URL</code></li>
      <li>Trace data stored in your own DB</li>
    </ul>
  </div>
  <div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;padding:22px 24px;">
    <div style="font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.12em;color:var(--muted);margin-bottom:10px;">Self-hosted</div>
    <p style="font-size:.88rem;line-height:1.65;margin:0 0 12px;">All Fluxbase services are open source. Run the full stack with Docker Compose on your own machines, VMs, or Kubernetes cluster.</p>
    <ul style="font-size:.85rem;color:var(--muted);line-height:1.7;margin:0;padding-left:18px;">
      <li>Full control over all services</li>
      <li>Your network, your region, your certs</li>
      <li>Same BYODB model</li>
      <li>Same CLI, pointed at your endpoint</li>
    </ul>
  </div>
</div>

<hr class="doc-divider">

<h2 id="cloud">Cloud setup</h2>

<p>The managed cloud option requires no infrastructure. Install the CLI, authenticate, connect your Postgres, and deploy.</p>

${cloudWindow}

<p>Trace and mutation data is written to a <code>fluxbase_trace</code> schema inside the Postgres database you provide. It is never stored on Fluxbase infrastructure.</p>

<hr class="doc-divider">

<h2 id="self-hosted">Self-hosted setup</h2>

<p>The self-hosted stack runs five Rust services plus Postgres. All source code is in the <a href="https://github.com/fluxbase/fluxbase" style="color:var(--accent);">fluxbase/fluxbase</a> repository.</p>

${composeWindow}

<p>The CLI is identical in both modes. Point it at your local gateway with <code>--endpoint</code> or the <code>FLUX_ENDPOINT</code> environment variable.</p>

<h3 id="services">Services</h3>

<div style="overflow-x:auto;margin:0 0 24px;">
  <table style="width:100%;border-collapse:collapse;font-size:.88rem;border:1px solid var(--border);border-radius:10px;overflow:hidden;">
    <thead>
      <tr style="background:var(--bg-elevated);">
        <th style="text-align:left;padding:10px 14px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.72rem;text-transform:uppercase;letter-spacing:.08em;">Port</th>
        <th style="text-align:left;padding:10px 14px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.72rem;text-transform:uppercase;letter-spacing:.08em;">Service</th>
        <th style="text-align:left;padding:10px 14px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.72rem;text-transform:uppercase;letter-spacing:.08em;">Tech</th>
        <th style="text-align:left;padding:10px 14px;border-bottom:1px solid var(--border);color:var(--muted);font-size:.72rem;text-transform:uppercase;letter-spacing:.08em;">Role</th>
      </tr>
    </thead>
    <tbody>
    ${serviceRows}
    </tbody>
  </table>
</div>

<p>All services communicate over internal HTTP. The only public-facing port is the Gateway (<code>:8081</code>).</p>

<hr class="doc-divider">

<h2 id="byodb">BYODB — Bring Your Own Database</h2>

<p>Both cloud and self-hosted use the same database model: <strong>you provide the Postgres instance</strong>. Fluxbase never stores application data on its own infrastructure.</p>

${byodbWindow}

<p>Fluxbase creates one additional schema (<code>fluxbase_trace</code>) in your Postgres for spans, mutation diffs, and the request log. Your application tables are never modified. The trace schema can be in the same cluster or a separate one.</p>

<h3>What this means for compliance and residency</h3>

<ul>
  <li><strong>Data residency</strong> is determined by where you host Postgres — your choice of cloud and region.</li>
  <li><strong>Encryption at rest</strong> follows your Postgres configuration.</li>
  <li><strong>Backups</strong> are your responsibility (same as any other data in your DB).</li>
  <li><strong>Deletion</strong> — dropping the <code>fluxbase_trace</code> schema removes all trace data immediately.</li>
</ul>

<hr class="doc-divider">

<h2 id="faq">Common questions</h2>

<h3>Can I run only some services?</h3>
<p>Yes, with some caveats. The Runtime, Data Engine, and Gateway are co-dependent — removing any one breaks the execution and mutation recording pipeline. The Queue service is optional if you don't use async functions. The API service is optional for read-only CLI operations but required for deploys and key management.</p>

<h3>Does self-hosted support multi-tenancy?</h3>
<p>Yes. Tenant isolation is enforced inside the services via <code>tenant_id</code> on every table and V8 isolate affinity in the Runtime. The cloud and self-hosted stacks use identical isolation logic.</p>

<h3>What about auth in self-hosted mode?</h3>
<p>The managed cloud uses Firebase Auth. In self-hosted mode you can configure an alternative OIDC provider, or use API key auth with <code>FLUX_API_KEY</code> for CLI access. See the <code>.env.dev.example</code> in the repository for all configuration options.</p>

<h3>Is the self-hosted stack production-ready?</h3>
<p>The self-hosted Dockerfiles are the same images used in the managed cloud. For production we recommend running the Gateway and Runtime in at least two replicas, with Postgres behind a connection pooler (e.g. PgBouncer).</p>

<hr class="doc-divider">

<div style="background:var(--bg-elevated);border:1px solid var(--border);border-radius:10px;padding:22px 26px;">
  <p style="font-weight:700;margin:0 0 8px;">In short</p>
  <p style="color:var(--muted);margin:0;line-height:1.7;">Cloud or self-hosted, the guarantees are identical: your Postgres, your data, your infrastructure. Fluxbase records the execution history in a schema you own and can delete at any time.</p>
</div>

</article>`;
}

export function render() {
  return docsLayout({
    meta,
    activePath: '/docs/deployment',
    content: content(),
  });
}
