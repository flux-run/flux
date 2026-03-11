import type { Metadata } from 'next'
import Link from 'next/link'
import { MarketingLayout } from '@/components/marketing/MarketingLayout'
import { CodeWindow } from '@/components/marketing/CodeWindow'

export const metadata: Metadata = {
  title: 'Fluxbase — Git for Backend Execution',
  description: 'Debug production systems faster than local development. Every request is recorded. Replay, diff, and root-cause any production issue with a single CLI command.',
}

const inner: React.CSSProperties = { maxWidth: 1040, margin: '0 auto', padding: '0 24px' }
const muted: React.CSSProperties = { color: 'var(--mg-muted)' }
const section = (bg?: string): React.CSSProperties => ({
  borderTop: '1px solid var(--mg-border)', padding: '80px 0',
  ...(bg ? { background: bg } : {}),
})

export default function HomePage() {
  return (
    <MarketingLayout>
      {/* ── Hero ─────────────────────────────────────────────── */}
      <section className="hero" style={{ paddingBottom: 60 }}>
        <span className="eyebrow">Git for Backend Execution</span>
        <h1>
          Debug production systems<br />
          <span className="gradient-text">faster than local development.</span>
        </h1>
        <p style={{ maxWidth: 580, margin: '0 auto 10px', fontSize: '1.05rem', color: 'var(--mg-muted)' }}>
          Fluxbase records every backend execution — requests, data mutations, and runtime spans — so you can debug production systems the way Git debugs code.
        </p>
        <p style={{ maxWidth: 520, margin: '0 auto 36px', fontSize: '.9rem', color: 'var(--mg-muted)' }}>
          Root-cause any incident in seconds. Replay it safely. Find the exact commit that broke it.
        </p>
        <div style={{ maxWidth: 660, margin: '0 auto 40px' }}>
          <CodeWindow label="production debugging in 2 commands">{
            `<span style="color:var(--mg-green);">$</span> flux tail\n\n  Streaming live requests…\n\n  <span style="color:var(--mg-green);">✔</span>  POST /signup      201  <span style="color:var(--mg-yellow);">88ms</span>  <span style="color:var(--mg-muted);">req:4f9a3b2c</span>\n  <span style="color:var(--mg-red);">✗</span>  POST /signup      500  <span style="color:var(--mg-yellow);">44ms</span>  <span style="color:var(--mg-accent);">req:550e8400</span>\n     <span style="color:var(--mg-red);">└─ Error: Stripe API timeout</span>\n\n<span style="color:var(--mg-green);">$</span> flux why <span style="color:var(--mg-accent);">550e8400</span>\n\n  <span style="color:#f8f8f2;">ROOT CAUSE</span>    Stripe API timeout\n  <span style="color:#f8f8f2;">LOCATION</span>     payments/create.ts:42\n  <span style="color:#f8f8f2;">DATA CHANGES</span>  users.id=42  plan: free <span style="color:var(--mg-red);">→ null</span>  <span style="color:var(--mg-muted);">(rolled back)</span>\n  <span style="color:#f8f8f2;">SUGGESTION</span>   <span style="color:var(--mg-green);">→</span> Add 5s timeout + idempotency key retry`
          }</CodeWindow>
        </div>
        <div style={{ display: 'flex', gap: 12, justifyContent: 'center', flexWrap: 'wrap', marginBottom: 28 }}>
          <Link href="/docs/quickstart" className="btn-primary">Start Building →</Link>
          <Link href="/docs" className="btn-secondary">View Docs</Link>
        </div>
        <div className="install-hint">
          <span className="prompt">$</span>
          curl -fsSL https://fluxbase.co/install | bash
        </div>
      </section>

      {/* ── The Debugging Workflow ──────────────────────────── */}
      <section style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <span className="section-label">The Debugging Workflow</span>
          <h2 className="section-h2">From alert to root cause in 30 seconds.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            A user reports &quot;signup failed&quot;. You have a request ID from <code>flux tail</code>. One more command and you know exactly what happened.
          </p>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 24 }}>
            <div>
              <p style={{ fontSize: '.78rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: '.08em', color: 'var(--mg-muted)', marginBottom: 14 }}>Step 1 — spot the failure</p>
              <CodeWindow label="flux tail">{`<span style="color:var(--mg-green);">$</span> flux tail\n\n  Streaming live requests…\n\n  <span style="color:var(--mg-green);">✔</span>  POST /signup      201   88ms  <span style="color:var(--mg-muted);">req:4f9a3b2c</span>\n  <span style="color:var(--mg-green);">✔</span>  GET  /users       200   12ms  <span style="color:var(--mg-muted);">req:a3c91ef0</span>\n  <span style="color:var(--mg-red);">✗</span>  POST /signup      500   44ms  <span style="color:var(--mg-accent);">req:550e8400</span>\n     <span style="color:var(--mg-red);">└─ Error: Stripe API timeout</span>`}</CodeWindow>
            </div>
            <div>
              <p style={{ fontSize: '.78rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: '.08em', color: 'var(--mg-muted)', marginBottom: 14 }}>Step 2 — understand it</p>
              <CodeWindow label="flux why 550e8400">{`<span style="color:var(--mg-green);">$</span> flux why <span style="color:var(--mg-accent);">550e8400</span>\n\n  <span style="color:#f8f8f2;">ROOT CAUSE</span>\n  Stripe API timeout after 10s\n\n  <span style="color:#f8f8f2;">LOCATION</span>\n  payments/create.ts:42\n\n  <span style="color:#f8f8f2;">DATA CHANGES</span>\n  <span style="color:#60a5fa;">users</span> id=42  plan: free <span style="color:var(--mg-red);">→ null</span>  <span style="color:var(--mg-muted);">(rolled back)</span>\n\n  <span style="color:#f8f8f2;">SUGGESTION</span>\n  <span style="color:var(--mg-green);">→</span> Add 5s timeout + idempotency key retry`}</CodeWindow>
            </div>
          </div>
          <p style={{ marginTop: 20, textAlign: 'center', fontSize: '.85rem', color: 'var(--mg-muted)' }}>
            Want to go deeper?{' '}
            <Link href="/cli" style={{ color: 'var(--mg-accent)' }}>flux trace diff</Link>,{' '}
            <Link href="/cli" style={{ color: 'var(--mg-accent)' }}>flux state history</Link>, and{' '}
            <Link href="/cli" style={{ color: 'var(--mg-accent)' }}>flux incident replay</Link> give you full production time-travel.
          </p>
        </div>
      </section>

      {/* ── The Difference ─────────────────────────────────── */}
      <section style={section()}>
        <div style={inner}>
          <span className="section-label">The Difference</span>
          <h2 className="section-h2">You shouldn&apos;t need 5 tools to debug one request.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            Traditional backends scatter evidence across logs, metrics, and traces — each in a different tool without shared context.
          </p>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 48, alignItems: 'start' }}>
            <div style={{ border: '1px solid var(--mg-border)', borderRadius: 10, overflow: 'hidden' }}>
              <div style={{ background: 'var(--mg-bg-elevated)', borderBottom: '1px solid var(--mg-border)', padding: '14px 20px' }}>
                <span style={{ fontSize: '.8rem', fontWeight: 700, color: 'var(--mg-muted)', textTransform: 'uppercase', letterSpacing: '.06em' }}>Traditional backend debugging</span>
              </div>
              <div style={{ padding: '24px 20px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                {['logs — scattered across services', 'metrics dashboard — no request context', 'trace UI — requires manual SDK instrumentation', 'DB console — query by query', 'queue monitor — separate tool'].map(t => (
                  <div key={t} style={{ display: 'flex', alignItems: 'center', gap: 12, fontSize: '.88rem', color: 'var(--mg-muted)' }}>
                    <span style={{ color: 'var(--mg-red)' }}>✗</span> {t}
                  </div>
                ))}
              </div>
            </div>
            <div style={{ border: '1px solid var(--mg-accent)', borderRadius: 10, overflow: 'hidden' }}>
              <div style={{ background: 'var(--mg-accent-dim)', borderBottom: '1px solid var(--mg-accent)', padding: '14px 20px' }}>
                <span style={{ fontSize: '.8rem', fontWeight: 700, color: 'var(--mg-accent)', textTransform: 'uppercase', letterSpacing: '.06em' }}>Fluxbase</span>
              </div>
              <div style={{ padding: '24px 20px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                {[
                  ['flux why <id>', '— root cause, one command'],
                  ['flux trace <id>', '— full span tree, latencies'],
                  ['flux state history', '— every row mutation'],
                  ['flux incident replay', '— safe re-execution'],
                  ['flux bug bisect', '— which commit broke it'],
                ].map(([cmd, desc]) => (
                  <div key={cmd} style={{ display: 'flex', alignItems: 'center', gap: 12, fontSize: '.88rem', color: 'var(--mg-muted)' }}>
                    <span style={{ color: 'var(--mg-green)' }}>✓</span>
                    <code>{cmd}</code> {desc}
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* ── Capabilities ───────────────────────────────────── */}
      <section style={section('var(--mg-bg-surface)')}>
        <div style={{ ...inner, paddingBottom: 0 }}>
          <span className="section-label">Capabilities</span>
          <h2 className="section-h2">Production debugging, not just monitoring.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            Monitoring tells you something is wrong. Fluxbase tells you why, shows you what changed, and lets you replay it.
          </p>
        </div>
        <div className="feature-grid">
          {[
            { icon: '🔍', title: 'Time-Travel Debugging', cmd: 'flux trace debug', desc: 'Step through a production request span by span. See the exact input, output, and state at every point in execution.' },
            { icon: '📜', title: 'Mutation History', cmd: 'flux state history', desc: 'Every database write is logged with its request ID. See the full history of any row — what changed, when, and which request caused it.' },
            { icon: '♻️', title: 'Incident Replay', cmd: 'flux incident replay', desc: 'Re-run a production time window with side-effects disabled. Test your fix against exactly the requests that caused the incident.' },
            { icon: '🔎', title: 'Regression Detection', cmd: 'flux bug bisect', desc: 'Binary-searches your git history to find the first commit where a request started failing. Like git bisect, but for production behaviour.' },
            { icon: '🛡️', title: 'Deterministic Execution', cmd: 'recorded by default', desc: 'Every request captures a complete trace automatically — no instrumentation, no SDKs, no config. The runtime produces the trace.' },
            { icon: '🔷', title: 'Observable by Construction', cmd: 'zero config', desc: 'Gateway, functions, database queries, tool calls, async jobs — every layer emits spans automatically. flux trace reconstructs the full picture.' },
          ].map(({ icon, title, cmd, desc }) => (
            <div key={title} className="feature-card">
              <div className="icon">{icon}</div>
              <h3>
                {title}
                <span style={{ fontSize: '.72rem', fontFamily: 'var(--font-geist-mono,monospace)', color: 'var(--mg-accent)', fontWeight: 400, marginLeft: 6 }}>{cmd}</span>
              </h3>
              <p>{desc}</p>
            </div>
          ))}
        </div>
      </section>

      {/* ── Why It Works ───────────────────────────────────── */}
      <section style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <span style={{ display: 'inline-block', fontSize: '.72rem', fontWeight: 700, letterSpacing: '.1em', textTransform: 'uppercase', color: 'var(--mg-green)', background: 'rgba(61,214,140,.1)', padding: '4px 12px', borderRadius: 20, marginBottom: 20 }}>Why It Works</span>
          <h2 className="section-h2">The system is designed to be observable.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            There is no &quot;add tracing later&quot; checkbox. Observability is how Fluxbase executes your code, not a feature you bolt on.
          </p>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit,minmax(240px,1fr))', gap: 16 }}>
            {[
              { icon: '📝', title: 'Every request is recorded.', desc: 'The gateway captures inputs, outputs, and metadata for every HTTP request. No SDK. No instrumentation. No config.' },
              { icon: '📃', title: 'Every mutation is logged.', desc: 'When your function writes to PostgreSQL, the Data Engine intercepts it and stores the row diff with its request_id. Your database is auditable by default.' },
              { icon: '🔬', title: 'Every execution span is traced.', desc: 'Gateway, runtime, DB queries, tool calls, async jobs — each layer emits spans automatically. flux trace reassembles the full picture from a single ID.' },
              { icon: '♻️', title: 'Production can be replayed safely.', desc: 'Because inputs and state are captured, any time window can be re-executed against your current code. Side-effects are disabled. Your fix is tested against real production traffic.' },
            ].map(({ icon, title, desc }) => (
              <div key={title} style={{ background: 'var(--mg-bg-elevated)', border: '1px solid var(--mg-border)', borderRadius: 10, padding: '22px 24px' }}>
                <div style={{ fontSize: '1.4rem', marginBottom: 10 }}>{icon}</div>
                <h3 style={{ fontSize: '.95rem', fontWeight: 700, marginBottom: 8, color: 'var(--mg-text)' }}>{title}</h3>
                <p style={{ fontSize: '.85rem', color: 'var(--mg-muted)', lineHeight: 1.7 }}>{desc}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Architecture ───────────────────────────────────── */}
      <section style={section()}>
        <div style={inner}>
          <span style={{ display: 'inline-block', fontSize: '.72rem', fontWeight: 700, letterSpacing: '.1em', textTransform: 'uppercase', color: 'var(--mg-muted)', background: 'var(--mg-bg-elevated)', padding: '4px 12px', borderRadius: 20, marginBottom: 20 }}>How It Works</span>
          <h2 className="section-h2">One request ID covers the entire stack.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            Client → Gateway → Runtime → Data Engine → Your PostgreSQL. Every hop records a span. <code>flux trace</code> reassembles them in order.
          </p>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 48, alignItems: 'center' }}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              {[
                { label: 'Client', badge: null, hi: true },
                { label: 'Gateway', badge: '→ span', hi: false },
                { label: 'Runtime', badge: '→ span', hi: false },
                { label: 'Data Engine', badge: '→ span', hi: false },
                { label: 'Your PostgreSQL', badge: '→ span', hi: true },
              ].map(({ label, badge, hi }) => (
                <div key={label} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '14px 18px', border: `1px solid ${hi ? 'var(--mg-accent)' : 'var(--mg-border)'}`, borderRadius: 8, background: hi ? 'var(--mg-accent-dim)' : 'var(--mg-bg-surface)' }}>
                  <span style={{ fontSize: '.9rem', fontWeight: 600 }}>{label}</span>
                  {badge && <span style={{ fontSize: '.73rem', fontFamily: 'var(--font-geist-mono,monospace)', color: 'var(--mg-accent)', background: 'var(--mg-accent-dim)', padding: '3px 10px', borderRadius: 20 }}>{badge}</span>}
                </div>
              ))}
              <Link href="/how-it-works" className="btn-secondary" style={{ marginTop: 8, justifyContent: 'center', fontSize: '.85rem' }}>Full architecture →</Link>
            </div>
            <CodeWindow label="flux trace 91a3f">{`<span style="color:var(--mg-green);">$</span> flux trace <span style="color:var(--mg-accent);">91a3f</span>\n\n  Trace <span style="color:var(--mg-accent);">91a3f</span>  <span style="color:var(--mg-muted);">2026-03-10 14:22 UTC</span>\n  <span style="color:var(--mg-muted);">POST /create_order · 200 OK</span>\n\n  <span style="color:#f9a8d4;">gateway</span>                      <span style="color:var(--mg-yellow);">2ms</span>\n  <span style="color:#f9a8d4;">└─ create_order</span>              <span style="color:var(--mg-yellow);">8ms</span>\n  <span style="color:#60a5fa;">   ├─ db.insert(orders)</span>       <span style="color:var(--mg-yellow);">4ms</span>\n  <span style="color:#60a5fa;">   ├─ stripe.charge</span>           <span style="color:var(--mg-yellow);">180ms</span>\n  <span style="color:var(--mg-red);">   └─ send_slack</span>              <span style="color:var(--mg-red);">error: rate limited</span>\n\n  <span style="color:var(--mg-muted);">── Suggestion ───────────────────────</span>\n  <span style="color:var(--mg-green);">→ Move send_slack to async background step</span>`}</CodeWindow>
          </div>
        </div>
      </section>

      {/* ── CTA ─────────────────────────────────────────────── */}
      <section style={{ borderTop: '1px solid var(--mg-border)', padding: '80px 24px', textAlign: 'left' }}>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 64, alignItems: 'start', maxWidth: 900, margin: '0 auto' }}>
          <div>
            <h2 style={{ fontSize: '1.6rem', fontWeight: 700, marginBottom: 8, color: 'var(--mg-text)' }}>Start debugging production in 5 minutes.</h2>
            <p style={{ maxWidth: 400, margin: '0 0 28px', color: 'var(--mg-muted)' }}>Install the CLI, deploy your first function, and get a full trace end-to-end before you finish the quickstart.</p>
            <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', marginBottom: 24 }}>
              <Link href="/docs/quickstart" className="btn-primary">Start the quickstart →</Link>
              <Link href="/product" className="btn-secondary">See all features</Link>
            </div>
            <div className="install-hint" style={{ marginTop: 0 }}>
              <span className="prompt">$</span>
              curl -fsSL https://fluxbase.co/install | bash
            </div>
          </div>
          <div>
            <div style={{ fontSize: '.7rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: '.1em', color: 'var(--mg-muted)', marginBottom: 16 }}>Learning path</div>
            {[
              { n: 1, href: '/',                         label: 'Homepage',        hint: '— understand the product' },
              { n: 2, href: '/docs/quickstart',          label: 'Quickstart',      hint: '— deploy + debug in 5 minutes' },
              { n: 3, href: '/docs/production-debugging',label: 'Debugging Guide', hint: '— the 4-command workflow' },
              { n: 4, href: '/docs/concepts',            label: 'Core Concepts',   hint: '— understand why it works' },
              { n: 5, href: '/cli',                      label: 'CLI Reference',   hint: '— every command, with examples' },
            ].map(({ n, href, label, hint }, i, arr) => (
              <div key={n}>
                <Link href={href} style={{ display: 'flex', alignItems: 'center', gap: 10, textDecoration: 'none', color: 'inherit' }}>
                  <span style={{ width: 22, height: 22, borderRadius: '50%', background: 'rgba(255,255,255,.12)', color: '#fff', fontSize: '.68rem', fontWeight: 800, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>{n}</span>
                  <span style={{ fontWeight: 600, fontSize: '.9rem' }}>{label}</span>
                  <span style={{ fontSize: '.8rem', color: 'var(--mg-muted)' }}>{hint}</span>
                </Link>
                {i < arr.length - 1 && <div style={{ paddingLeft: 11, height: 16, borderLeft: '1px dashed rgba(255,255,255,.2)' }} />}
              </div>
            ))}
          </div>
        </div>
      </section>
    </MarketingLayout>
  )
}
