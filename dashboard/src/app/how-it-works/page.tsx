import type { Metadata } from 'next'
import Link from 'next/link'
import { MarketingLayout } from '@/components/marketing/MarketingLayout'
import { CodeWindow } from '@/components/marketing/CodeWindow'

export const metadata: Metadata = {
  title: 'How It Works — Fluxbase',
  description: 'Request capture, mutation logging, trace graph, and deterministic replay. How the Fluxbase runtime turns every request into a queryable production record.',
}

const inner: React.CSSProperties = { maxWidth: 1040, margin: '0 auto', padding: '0 24px' }
const muted: React.CSSProperties = { color: 'var(--mg-muted)' }
const section = (bg?: string): React.CSSProperties => ({
  borderTop: '1px solid var(--mg-border)', padding: '80px 0',
  ...(bg ? { background: bg } : {}),
})

export default function HowItWorksPage() {
  return (
    <MarketingLayout>
      {/* ── Hero ─────────────────────────────────────────────── */}
      <section className="hero" style={{ paddingBottom: 48 }}>
        <span className="eyebrow">How It Works</span>
        <h1 style={{ fontSize: 'clamp(2rem,5vw,3rem)' }}>
          One request ID.<br />
          <span className="gradient-text">The entire stack.</span>
        </h1>
        <p style={{ maxWidth: 560, margin: '0 auto 32px', color: 'var(--mg-muted)' }}>
          Fluxbase adds a recording and replay layer around your execution stack. Every layer — from the gateway to the database — emits structured spans tied to one request ID. The CLI reassembles them on demand.
        </p>
        <div style={{ display: 'flex', gap: 12, justifyContent: 'center', flexWrap: 'wrap' }}>
          <Link href="/docs/quickstart" className="btn-primary">Try it →</Link>
          <Link href="/product" className="btn-secondary">See the features</Link>
        </div>
      </section>

      {/* ── The Architecture ─────────────────────────────────── */}
      <section style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <span className="section-label">The Architecture</span>
          <h2 className="section-h2">Five layers. One trace.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            Every layer is instrumented at the runtime level — no application-level tracing hooks needed. The span data is stored in the Data Engine alongside mutation logs.
          </p>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 48, alignItems: 'center' }}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 0 }}>
              {[
                { icon: '👤', title: 'Client',          sub: 'Any HTTP client',                      hi: false, span: false },
                { icon: '🛡️', title: 'Gateway',         sub: 'auth · rate limit · route → span',     hi: true,  span: true  },
                { icon: '⚡', title: 'Runtime',         sub: 'your TypeScript code → span',          hi: true,  span: true  },
                { icon: '🗄️', title: 'Data Engine',     sub: 'query compiler · policy · SQL → span', hi: true,  span: true  },
                { icon: '🐘', title: 'Your PostgreSQL', sub: 'standard Postgres, you own the data',  hi: false, span: false },
              ].map(({ icon, title, sub, hi }, i, arr) => (
                <div key={title}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 16, padding: '16px 20px', border: `1px solid ${hi ? 'var(--mg-accent)' : 'var(--mg-border)'}`, borderRadius: 8, background: hi ? 'var(--mg-accent-dim)' : 'var(--mg-bg-surface)' }}>
                    <span style={{ fontSize: '1.2rem' }}>{icon}</span>
                    <div>
                      <div style={{ fontSize: '.9rem', fontWeight: 700, color: 'var(--mg-text)' }}>{title}</div>
                      <div style={{ fontSize: '.76rem', color: 'var(--mg-muted)' }}>{sub}</div>
                    </div>
                  </div>
                  {i < arr.length - 1 && <div style={{ display: 'flex', padding: '0 28px', alignItems: 'stretch', height: 20 }}><div style={{ width: 2, background: 'var(--mg-border)' }} /></div>}
                </div>
              ))}
              <p style={{ fontSize: '.78rem', color: 'var(--mg-muted)', marginTop: 16 }}>* Each hop produces a span stored in the trace store.</p>
            </div>
            <CodeWindow label="flux trace 4f9a3b2c">{`<span style="color:var(--mg-green);">$</span> flux trace <span style="color:var(--mg-accent);">4f9a3b2c</span>\n\n  Trace <span style="color:var(--mg-accent);">4f9a3b2c</span>  <span style="color:var(--mg-muted);">POST /create_user  200</span>\n\n  <span style="color:#f9a8d4;">▸ gateway</span>                     <span style="color:var(--mg-yellow);">3ms</span>\n    <span style="color:var(--mg-muted);">auth ✔  rate_limit ✔  cors ✔</span>\n\n  <span style="color:#f9a8d4;">▸ create_user</span>                 <span style="color:var(--mg-yellow);">81ms</span>\n    <span style="color:#60a5fa;">▸ db:select(users)</span>           <span style="color:var(--mg-yellow);">11ms</span>\n    <span style="color:#60a5fa;">▸ db:insert(users)</span>           <span style="color:var(--mg-yellow);">14ms</span>\n\n  <span style="color:#f9a8d4;">▸ send_welcome</span>  <span style="color:var(--mg-muted);">async →</span>  <span style="color:var(--mg-yellow);">queued</span>\n\n  <span style="color:var(--mg-muted);">── total: 98ms ─────────────────────</span>`}</CodeWindow>
          </div>
        </div>
      </section>

      {/* ── Step by Step ─────────────────────────────────────── */}
      <section style={section()}>
        <div style={inner}>
          <span className="section-label">Step by Step</span>
          <h2 className="section-h2" style={{ marginBottom: 40 }}>What happens when a request runs.</h2>
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            {[
              {
                n: 1, title: 'Request capture', label: 'gateway → span',
                desc: 'When a request arrives at the Gateway, Fluxbase assigns it a globally unique request ID (UUID v4). This ID is propagated via internal headers to every downstream service. The gateway records auth result, rate-limit decision, matched route, and timing as the first span.',
                code: `<span style="color:var(--mg-muted);"># Gateway emits:</span>\n{\n  request_id: <span style="color:var(--mg-accent);">"4f9a3b2c"</span>,\n  span: <span style="color:var(--mg-green);">"gateway"</span>,\n  method: <span style="color:var(--mg-green);">"POST"</span>,\n  path: <span style="color:var(--mg-green);">"/create_user"</span>,\n  auth: <span style="color:var(--mg-green);">"ok"</span>,\n  duration_ms: 3\n}`,
              },
              {
                n: 2, title: 'Function execution', label: 'runtime → spans',
                desc: 'The Runtime receives the request with the forwarded request ID. It executes your TypeScript function in a sandboxed V8 isolate. Every ctx.db, ctx.tool, and ctx.workflow call is intercepted and recorded as a child span under the function span.',
                code: `<span style="color:var(--mg-muted);"># Runtime emits per call:</span>\n{\n  request_id: <span style="color:var(--mg-accent);">"4f9a3b2c"</span>,\n  span: <span style="color:var(--mg-green);">"create_user"</span>,\n  children: [\n    { span: <span style="color:#60a5fa;">"db:select(users)"</span>, <span style="color:var(--mg-yellow);">11ms</span> },\n    { span: <span style="color:#60a5fa;">"db:insert(users)"</span>, <span style="color:var(--mg-yellow);">14ms</span> }\n  ],\n  duration_ms: 81\n}`,
              },
              {
                n: 3, title: 'Mutation logging', label: 'data engine → mutation log',
                desc: 'Every database write goes through the Data Engine, which applies schema validation, column policies, and row-level security before executing the SQL. After execution, it writes a mutation record: which table, which row, old value, new value, and the request ID that caused it.',
                code: `<span style="color:var(--mg-muted);"># Mutation record:</span>\n{\n  request_id: <span style="color:var(--mg-accent);">"4f9a3b2c"</span>,\n  table: <span style="color:var(--mg-green);">"users"</span>,\n  row_id: 42,\n  operation: <span style="color:var(--mg-green);">"insert"</span>,\n  data: { email: <span style="color:var(--mg-green);">"a@b.com"</span>, plan: <span style="color:var(--mg-green);">"free"</span> },\n  timestamp: <span style="color:var(--mg-muted);">"2026-03-10T14:22:01Z"</span>\n}`,
              },
              {
                n: 4, title: 'Trace graph', label: 'trace store → rendered',
                desc: 'All spans for a request ID are stored in an ordered graph. flux trace <id> retrieves them and renders the full tree — gateway, function, database queries, tool calls — in execution order with latencies.',
                code: `<span style="color:var(--mg-green);">$</span> flux trace 4f9a3b2c\n\n  <span style="color:#f9a8d4;">gateway</span>           <span style="color:var(--mg-yellow);">3ms</span>\n  <span style="color:#f9a8d4;">create_user</span>      <span style="color:var(--mg-yellow);">81ms</span>\n    <span style="color:#60a5fa;">db:select</span>       <span style="color:var(--mg-yellow);">11ms</span>\n    <span style="color:#60a5fa;">db:insert</span>       <span style="color:var(--mg-yellow);">14ms</span>\n  <span style="color:#f9a8d4;">send_welcome</span>  <span style="color:var(--mg-muted);">async → queued</span>\n\n  <span style="color:var(--mg-muted);">total: 98ms</span>`,
              },
              {
                n: 5, title: 'Deterministic replay', label: 'replay — side-effects off',
                desc: 'Because every span includes its full input and output, any request can be replayed deterministically. flux incident replay re-executes against the current code with side-effects disabled. flux bug bisect replays across your git history to find regressions.',
                code: `<span style="color:var(--mg-green);">$</span> flux incident replay 14:00..14:05\n\n  <span style="color:var(--mg-muted);">hooks: off · events: off · cron: off</span>\n  <span style="color:var(--mg-muted);">db writes: on · mutation log: on</span>\n\n  <span style="color:var(--mg-green);">✔</span> 22/23 passing\n  <span style="color:var(--mg-red);">✗</span>  req:550e8400 still fails\n     <span style="color:var(--mg-red);">Stripe timeout at payments/create.ts:42</span>`,
              },
            ].map(({ n, title, label, desc, code }, i, arr) => (
              <div key={n} style={{ display: 'grid', gridTemplateColumns: '48px 1fr 1fr', gap: 32, alignItems: 'start', padding: '40px 0', borderBottom: i < arr.length - 1 ? '1px solid var(--mg-border)' : 'none' }}>
                <div style={{ width: 36, height: 36, borderRadius: '50%', background: 'var(--mg-accent)', color: '#fff', fontSize: '.85rem', fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, marginTop: 2 }}>{n}</div>
                <div>
                  <h3 style={{ fontSize: '1.05rem', fontWeight: 700, marginBottom: 10, color: 'var(--mg-text)' }}>{title}</h3>
                  <p style={{ fontSize: '.9rem', color: 'var(--mg-muted)', lineHeight: 1.65, margin: 0 }}>{desc}</p>
                </div>
                <CodeWindow label={label}>{code}</CodeWindow>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Technology ───────────────────────────────────────── */}
      <section style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <span style={{ display: 'inline-block', fontSize: '.72rem', fontWeight: 700, letterSpacing: '.1em', textTransform: 'uppercase', color: 'var(--mg-muted)', background: 'var(--mg-bg-elevated)', padding: '4px 12px', borderRadius: 20, marginBottom: 20 }}>Technology</span>
          <h2 className="section-h2">Open foundations, high-performance core.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 600, margin: '0 0 40px' }}>
            Every service is written in Rust for predictable latency and memory safety. Your TypeScript functions run in Deno V8 isolates. Your data stays in standard Postgres.
          </p>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit,minmax(260px,1fr))', gap: 16 }}>
            {[
              { icon: '🛡️', title: 'Gateway', tech: 'Rust (Axum)', desc: 'Auth, rate limit (per-tenant token bucket), query guard, semaphore budget. Routes requests to Runtime or Data Engine.' },
              { icon: '⚡', title: 'Runtime', tech: 'Rust + Deno V8', desc: 'Executes TypeScript in sandboxed V8 isolates per tenant. Warm isolates for low latency; per-tenant affinity to prevent cross-tenant heap contamination.' },
              { icon: '🗄️', title: 'Data Engine', tech: 'Rust (Axum)', desc: 'Query compiler (JSON → SQL), column policies, row-level security, BYODB (Bring Your Own Database). Mutation log writer and explain endpoint.' },
              { icon: '📬', title: 'Queue', tech: 'Rust', desc: 'Durable async job queue. Stores job payloads in Postgres. Workers poll and execute functions from the Runtime. Fully traced.' },
              { icon: '🔌', title: 'API', tech: 'Rust (Axum)', desc: 'Management API: deploy functions, manage schemas, API keys, tenant config. Consumed by the CLI and Dashboard.' },
            ].map(({ icon, title, tech, desc }) => (
              <div key={title} style={{ background: 'var(--mg-bg-surface)', border: '1px solid var(--mg-border)', borderRadius: 10, padding: 24 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 12 }}>
                  <span style={{ fontSize: '1.2rem' }}>{icon}</span>
                  <div>
                    <div style={{ fontWeight: 700, fontSize: '.95rem', color: 'var(--mg-text)' }}>{title}</div>
                    <div style={{ fontSize: '.75rem', fontFamily: 'var(--font-geist-mono,monospace)', color: 'var(--mg-accent)' }}>{tech}</div>
                  </div>
                </div>
                <p style={{ fontSize: '.85rem', color: 'var(--mg-muted)', lineHeight: 1.6, margin: 0 }}>{desc}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── CTA ─────────────────────────────────────────────── */}
      <section className="cta-strip">
        <h2>See it in action.</h2>
        <p style={{ maxWidth: 480, margin: '0 auto 32px', color: 'var(--mg-muted)' }}>
          The quickstart takes 5 minutes. You deploy a function, trigger a request, and trace it end to end from the CLI.
        </p>
        <div style={{ display: 'flex', gap: 12, justifyContent: 'center', flexWrap: 'wrap' }}>
          <Link href="/docs/quickstart" className="btn-primary">Start the Quickstart →</Link>
          <Link href="/cli" className="btn-secondary">CLI Reference</Link>
        </div>
      </section>
    </MarketingLayout>
  )
}
