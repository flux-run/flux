import type { Metadata } from 'next'
import Link from 'next/link'
import { MarketingLayout } from '@/components/marketing/MarketingLayout'
import { CodeWindow } from '@/components/marketing/CodeWindow'

export const metadata: Metadata = {
  title: 'Product — Fluxbase',
  description: 'Time-travel debugging, mutation history, incident replay, regression detection. Every tool a developer needs to understand and fix production systems fast.',
}

const inner: React.CSSProperties = { maxWidth: 1040, margin: '0 auto', padding: '0 24px' }
const muted: React.CSSProperties = { color: 'var(--mg-muted)' }
const section = (bg?: string): React.CSSProperties => ({
  borderTop: '1px solid var(--mg-border)', padding: '80px 0',
  ...(bg ? { background: bg } : {}),
})
const grid2: React.CSSProperties = { display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 48, alignItems: 'center' }

export default function ProductPage() {
  return (
    <MarketingLayout>
      {/* ── Hero ─────────────────────────────────────────────── */}
      <section className="hero" style={{ paddingBottom: 48 }}>
        <span className="eyebrow">Product</span>
        <h1 style={{ fontSize: 'clamp(2rem,5vw,3rem)' }}>
          Every production question,<br />
          <span className="gradient-text">answered in one command.</span>
        </h1>
        <p style={{ maxWidth: 580, margin: '0 auto 24px', color: 'var(--mg-muted)' }}>
          Fluxbase captures a deterministic record of every request and every database mutation. Then gives you tools to query that record from the terminal.
        </p>
        <div style={{ display: 'flex', gap: 12, justifyContent: 'center', flexWrap: 'wrap' }}>
          <Link href="/docs/quickstart" className="btn-primary">Get Started →</Link>
          <Link href="/cli" className="btn-secondary">CLI Reference</Link>
        </div>
      </section>

      {/* ── Task-Oriented Design ─────────────────────────────── */}
      <section style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <span className="section-label">Task-Oriented Design</span>
          <h2 className="section-h2">Start with the question, not the tool.</h2>
          <p style={{ ...muted, fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            Fluxbase CLI commands map directly to the questions developers ask when something breaks in production.
          </p>
          <div style={{ overflowX: 'auto' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '.9rem' }}>
              <thead>
                <tr>
                  {['Developer Question', 'Command', 'What it does'].map(h => (
                    <th key={h} style={{ textAlign: 'left', padding: '8px 16px', borderBottom: '1px solid var(--mg-border)', color: 'var(--mg-muted)', fontSize: '.75rem', textTransform: 'uppercase', letterSpacing: '.05em' }}>{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {[
                  ['Why did my request fail?',        'flux why <id>',           'Root cause, span tree, suggestions'],
                  ['Which commit introduced this bug?','flux bug bisect',         'Binary-searches git history'],
                  ['What changed in the database?',   'flux state history',      'Every row mutation, linked to request'],
                  ['Who set this field to this value?','flux state blame',        'Per-column last-write attribution'],
                  ['What happens if I replay this?',  'flux incident replay',    'Safe re-run, side-effects off'],
                  ['How do two requests differ?',     'flux trace diff',         'Span-by-span comparison'],
                  ['How does my query get compiled?', 'flux explain',            'Dry-run with policy + SQL preview'],
                ].map(([q, cmd, desc]) => (
                  <tr key={cmd}>
                    <td style={{ padding: '12px 16px', borderBottom: '1px solid var(--mg-border)', color: 'var(--mg-text)' }}>{q}</td>
                    <td style={{ padding: '12px 16px', borderBottom: '1px solid var(--mg-border)', whiteSpace: 'nowrap' }}><code>{cmd}</code></td>
                    <td style={{ padding: '12px 16px', borderBottom: '1px solid var(--mg-border)', color: 'var(--mg-muted)', fontSize: '.87rem' }}>{desc}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </section>

      {/* ── Deterministic Execution ──────────────────────────── */}
      <section id="deterministic-execution" style={section()}>
        <div style={inner}>
          <div style={grid2}>
            <div>
              <span className="section-label">Deterministic Execution</span>
              <h3 style={{ fontSize: '1.4rem', fontWeight: 800, letterSpacing: '-.02em', marginBottom: 12, color: 'var(--mg-text)' }}>Every request is recorded automatically.</h3>
              <p style={{ ...muted, fontSize: '.95rem', lineHeight: 1.7 }}>
                The Fluxbase runtime captures a complete record of every request as it happens — gateway auth, function spans, every database query, tool latencies, async job hand-offs. No instrumentation, no SDK, no config. If the request ran, it&apos;s recorded.
              </p>
            </div>
            <CodeWindow label="automatic recording">{`<span style="color:var(--mg-muted);"># Every request produces:</span>\n\n  trace_requests      <span style="color:var(--mg-green);">→</span> span tree (gateway to db)\n  state_mutations     <span style="color:var(--mg-green);">→</span> every row change + request link\n  execution_spans     <span style="color:var(--mg-green);">→</span> timing, errors, tool calls\n\n<span style="color:var(--mg-muted);"># Nothing to configure. Zero SDK changes.</span>\n<span style="color:var(--mg-muted);"># The runtime records it all.</span>`}</CodeWindow>
          </div>
        </div>
      </section>

      {/* ── Time-Travel Debugging ────────────────────────────── */}
      <section id="time-travel-debugging" style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <div style={grid2}>
            <CodeWindow label="flux trace debug 550e8400">{`<span style="color:var(--mg-green);">$</span> flux trace debug <span style="color:var(--mg-accent);">550e8400</span>\n\n  <span style="color:var(--mg-muted);">Step 1/4  gateway</span>\n  <span style="color:var(--mg-muted);">─────────────────────────────────────</span>\n  Input:   POST /signup  <span style="color:var(--mg-green);">{ email: "a@b.com" }</span>\n  Output:  <span style="color:var(--mg-green);">{ tenant_id: "t_123", passed: true }</span>\n  Time:    4ms\n\n  <span style="color:var(--mg-muted);">Step 2/4  create_user</span>\n  <span style="color:var(--mg-muted);">─────────────────────────────────────</span>\n  Input:   <span style="color:var(--mg-green);">{ email: "a@b.com" }</span>\n  Output:  <span style="color:var(--mg-green);">{ userId: "u_42" }</span>\n  Time:    81ms\n\n  <span style="color:var(--mg-muted);">↓ next  ↑ prev  e expand  q quit</span>`}</CodeWindow>
            <div>
              <span className="section-label">Time-Travel Debugging</span>
              <h3 style={{ fontSize: '1.4rem', fontWeight: 800, letterSpacing: '-.02em', marginBottom: 12, color: 'var(--mg-text)' }}>Step through any production request.</h3>
              <p style={{ ...muted, fontSize: '.95rem', lineHeight: 1.7 }}>
                <code>flux trace debug &lt;id&gt;</code> opens an interactive terminal UI where you can navigate each span of a production request. See the exact input and output at every step — what the gateway received, what the function returned, what the database wrote. All from the actual production execution.
              </p>
            </div>
          </div>
        </div>
      </section>

      {/* ── Mutation History ─────────────────────────────────── */}
      <section id="mutation-history" style={section()}>
        <div style={inner}>
          <div style={grid2}>
            <div>
              <span className="section-label">Data Mutation History</span>
              <h3 style={{ fontSize: '1.4rem', fontWeight: 800, letterSpacing: '-.02em', marginBottom: 12, color: 'var(--mg-text)' }}>See every change ever made to a row.</h3>
              <p style={{ ...muted, fontSize: '.95rem', lineHeight: 1.7 }}>
                <code>flux state history</code> shows every INSERT, UPDATE, and DELETE on any row, linked back to the request that caused it. <code>flux state blame</code> shows which request owns each column&apos;s current value. Instantly answer &quot;who or what set this field to this value?&quot;
              </p>
            </div>
            <CodeWindow label="flux state history users --id 42">{`<span style="color:var(--mg-green);">$</span> flux state history users --id 42\n\n  <span style="color:#f8f8f2;">users id=42</span>  (7 mutations)\n\n  <span style="color:var(--mg-muted);">2026-03-10 12:00:00</span>  INSERT  <span style="color:var(--mg-green);">email=a@b.com, plan=free</span>\n  <span style="color:var(--mg-muted);">2026-03-10 14:21:59</span>  UPDATE  name: null → Alice Smith  <span style="color:var(--mg-accent);">req:a3c91ef0</span>\n  <span style="color:var(--mg-muted);">2026-03-10 14:22:01</span>  UPDATE  plan: free → pro           <span style="color:var(--mg-accent);">req:4f9a3b2c</span>\n  <span style="color:var(--mg-muted);">2026-03-10 14:22:01</span>  UPDATE  plan: pro → null  <span style="color:var(--mg-muted);">(rolled back)</span>  <span style="color:var(--mg-red);">req:550e8400</span>\n\n<span style="color:var(--mg-muted);">$</span> flux state blame users --id 42\n\n  email    a@b.com     <span style="color:var(--mg-accent);">req:4f9a3b2c</span>  12:00:00\n  plan     free        <span style="color:var(--mg-red);">req:550e8400</span>  14:22:01  <span style="color:var(--mg-red);">✗ rolled back</span>`}</CodeWindow>
          </div>
        </div>
      </section>

      {/* ── Incident Replay ──────────────────────────────────── */}
      <section id="incident-replay" style={section('var(--mg-bg-surface)')}>
        <div style={inner}>
          <div style={grid2}>
            <CodeWindow label="flux incident replay 14:00..14:05">{`<span style="color:var(--mg-green);">$</span> flux incident replay 14:00..14:05\n\n  Replaying 23 requests from 14:00–14:05…\n\n  <span style="color:var(--mg-muted);">Side-effects: hooks off · events off · cron off</span>\n  <span style="color:var(--mg-muted);">Database writes: on · mutation log: on</span>\n\n  <span style="color:var(--mg-green);">✔</span>  <span style="color:var(--mg-accent);">req:4f9a3b2c</span>  POST /create_user   200  81ms\n  <span style="color:var(--mg-green);">✔</span>  <span style="color:var(--mg-accent);">req:a3c91ef0</span>  GET  /list_users    200  12ms\n  <span style="color:var(--mg-red);">✗</span>  <span style="color:var(--mg-accent);">req:550e8400</span>  POST /signup        500  44ms\n     <span style="color:var(--mg-red);">└─ Still failing: Stripe timeout</span>\n\n  23 replayed · 22 passing · 1 still failing`}</CodeWindow>
            <div>
              <span className="section-label">Incident Replay</span>
              <h3 style={{ fontSize: '1.4rem', fontWeight: 800, letterSpacing: '-.02em', marginBottom: 12, color: 'var(--mg-text)' }}>Test your fix against the exact incident.</h3>
              <p style={{ ...muted, fontSize: '.95rem', lineHeight: 1.7 }}>
                <code>flux incident replay</code> re-executes all requests from a time window against your current code. Outbound side-effects are disabled — no emails, no webhooks, no Slack. Database writes and mutation logs run normally. After your commit, replay the incident to confirm the fix before deploying.
              </p>
            </div>
          </div>
        </div>
      </section>

      {/* ── Regression Detection ─────────────────────────────── */}
      <section id="regression-detection" style={section()}>
        <div style={inner}>
          <div style={grid2}>
            <div>
              <span className="section-label">Regression Detection</span>
              <h3 style={{ fontSize: '1.4rem', fontWeight: 800, letterSpacing: '-.02em', marginBottom: 12, color: 'var(--mg-text)' }}>Find the commit that introduced the bug.</h3>
              <p style={{ ...muted, fontSize: '.95rem', lineHeight: 1.7 }}>
                <code>flux bug bisect</code> binary-searches your git history comparing trace behaviour before and after each commit. It automatically identifies the first commit where a given request started failing. Like <code>git bisect</code>, but for production behaviour rather than a test suite.
              </p>
            </div>
            <CodeWindow label="flux bug bisect">{`<span style="color:var(--mg-green);">$</span> flux bug bisect --request <span style="color:var(--mg-accent);">550e8400</span>\n\n  Bisecting 42 commits (2026-03-01..2026-03-10)…\n\n  Testing <span style="color:var(--mg-muted);">abc123</span>…  <span style="color:var(--mg-green);">✔ passes</span>\n  Testing <span style="color:var(--mg-muted);">fde789</span>…  <span style="color:var(--mg-green);">✔ passes</span>\n  Testing <span style="color:var(--mg-muted);">def456</span>…  <span style="color:var(--mg-red);">✗ fails</span>\n\n  <span style="color:#f8f8f2;">FIRST BAD COMMIT</span>\n  <span style="color:var(--mg-accent);">def456</span>  "feat: add retry logic to stripe.charge"\n  <span style="color:var(--mg-muted);">2026-03-08 by alice@example.com</span>\n\n  <span style="color:var(--mg-green);">→</span> Compare before/after:\n     flux trace diff <span style="color:var(--mg-muted);">abc123:550e8400 def456:550e8400</span>`}</CodeWindow>
          </div>
        </div>
      </section>

      {/* ── CTA ─────────────────────────────────────────────── */}
      <section className="cta-strip">
        <h2>Ready to debug production like it&apos;s local?</h2>
        <p style={{ maxWidth: 480, margin: '0 auto 32px', color: 'var(--mg-muted)' }}>
          Everything on this page is available immediately after <code>flux deploy</code>. No configuration, no setup, no SDK changes.
        </p>
        <div style={{ display: 'flex', gap: 12, justifyContent: 'center', flexWrap: 'wrap' }}>
          <Link href="/docs/quickstart" className="btn-primary">Start Building →</Link>
          <Link href="/how-it-works" className="btn-secondary">How It Works</Link>
        </div>
      </section>
    </MarketingLayout>
  )
}
