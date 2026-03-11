import Link from 'next/link'
import { MarketingLayout } from '@/components/marketing/MarketingLayout'
import { CodeWindow } from '@/components/marketing/CodeWindow'

const section = (extra?: React.CSSProperties): React.CSSProperties => ({
  borderTop: '1px solid var(--mg-border)',
  padding: '80px 0',
  ...extra,
})

const inner: React.CSSProperties = {
  maxWidth: 1040,
  margin: '0 auto',
  padding: '0 24px',
}

const label = (color: string) => ({
  display: 'inline-block',
  fontSize: '.72rem',
  fontWeight: 700,
  letterSpacing: '.1em',
  textTransform: 'uppercase' as const,
  color,
  background: color === 'var(--mg-accent)' ? 'var(--mg-accent-dim)' : `${color}1a`,
  padding: '4px 12px',
  borderRadius: 20,
  marginBottom: 20,
})

const installCode = `<span style="color:var(--mg-green);">$</span> curl -fsSL https://fluxbase.co/install | bash

  Installing flux CLI…

  <span style="color:var(--mg-green);">✔</span>  Downloaded flux v1.0.0
  <span style="color:var(--mg-green);">✔</span>  Installed to /usr/local/bin/flux

  <span style="color:var(--mg-green);">$</span> flux --version
  flux 1.0.0

  <span style="color:var(--mg-green);">$</span> flux login
  Opening browser… <span style="color:var(--mg-muted);">(or set FLUX_API_KEY env var)</span>
  <span style="color:var(--mg-green);">✔</span>  Logged in as alice@example.com`

const deployCode = `<span style="color:var(--mg-green);">$</span> flux deploy

  Deploying 3 functions…

  <span style="color:var(--mg-green);">✔</span>  create_user   → gw.fluxbase.co/create_user
  <span style="color:var(--mg-green);">✔</span>  list_users    → gw.fluxbase.co/list_users
  <span style="color:var(--mg-green);">✔</span>  send_welcome  (async)

  <span style="color:var(--mg-green);">✔</span>  Deployed in 18s  deploy:d_7f3a9`

const tailCode = `<span style="color:var(--mg-green);">$</span> flux tail

METHOD   ROUTE              FUNCTION        DURATION   STATUS
────────────────────────────────────────────────────────────
POST     /login             auth_user       38ms       <span style="color:var(--mg-green);">✔</span>
   <span style="color:var(--mg-muted);">users.id=7f3a  last_login_at → 2026-03-11T…</span>

POST     /checkout          create_order    121ms      <span style="color:var(--mg-green);">✔</span>
   <span style="color:var(--mg-muted);">orders.id=82b1  insert</span>
   <span style="color:var(--mg-muted);">cart_items.id=a2f1  insert</span>

POST     /signup            create_user     3.2s       <span style="color:var(--mg-red);">✗ 500</span>
   <span style="color:var(--mg-red);">error: Stripe timeout after 10000ms</span>
   <span style="color:var(--mg-accent);">→ flux why 550e8400</span>
   <span style="color:var(--mg-yellow);">⚠ same row as previous request</span>
   <span style="color:var(--mg-muted);">users.id=7f3a  plan free → pro</span>`

const bugCode = `<span style="color:var(--mg-green);">$</span> flux bug bisect --request 550e8400

  Bisecting 42 commits (2026-03-01..2026-03-10)…

  Testing commit abc123…  <span style="color:var(--mg-green);">✔</span> passes
  Testing commit def456…  <span style="color:var(--mg-red);">✗</span> fails

  FIRST BAD COMMIT
  <span style="color:var(--mg-yellow);">def456</span>  "feat: add retry logic to stripe.charge"
  2026-03-08 by alice@example.com`

type CommandCardProps = {
  color: string
  title: string
  links: { href: string; cmd: string; label: string }[]
}

function CommandCard({ color, title, links }: CommandCardProps) {
  return (
    <div>
      <div style={{
        fontSize: '.68rem',
        fontWeight: 700,
        textTransform: 'uppercase',
        letterSpacing: '.12em',
        color,
        marginBottom: 8,
        display: 'flex',
        alignItems: 'center',
        gap: 6,
      }}>
        <span style={{
          display: 'inline-block',
          width: 2,
          height: 12,
          background: color,
          borderRadius: 1,
        }} />
        {title}
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {links.map(({ href, cmd, label }) => (
          <a
            key={href}
            href={href}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              padding: '9px 14px',
              background: 'var(--mg-bg-surface)',
              border: '1px solid var(--mg-border)',
              borderRadius: 8,
              fontFamily: 'var(--font-geist-mono, monospace)',
              fontSize: '.82rem',
              color: 'var(--mg-text)',
              textDecoration: 'none',
            }}
          >
            <span style={{ color, fontWeight: 700 }}>flux</span>
            <span style={{ color: 'var(--mg-muted)' }}>{cmd}</span>
            <span style={{
              fontFamily: 'inherit',
              fontSize: '.72rem',
              color: 'var(--mg-muted)',
              marginLeft: 'auto',
              whiteSpace: 'nowrap',
            }}>{label}</span>
          </a>
        ))}
      </div>
    </div>
  )
}

function SectionHeader({ color, children }: { color: string; children: string }) {
  return (
    <div style={{ maxWidth: 1040, margin: '0 auto', padding: '56px 24px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ display: 'inline-block', width: 3, height: 24, background: color, borderRadius: 2 }} />
        <span style={{ fontSize: '.72rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: '.12em', color }}>
          {children}
        </span>
      </div>
    </div>
  )
}

function CommandSection({
  id,
  cmd,
  title,
  desc,
  codeLabel,
  code,
  reverse = false,
}: {
  id: string
  cmd: string
  title: string
  desc: string
  codeLabel: string
  code: string
  reverse?: boolean
}) {
  const textBlock = (
    <div>
      <div style={{
        fontFamily: 'var(--font-geist-mono, monospace)',
        fontSize: '1rem',
        fontWeight: 700,
        color: 'var(--mg-accent)',
        marginBottom: 10,
      }}>
        {cmd}
      </div>
      <h3 style={{ fontSize: '1.2rem', fontWeight: 700, marginBottom: 12 }}>{title}</h3>
      <p style={{ color: 'var(--mg-muted)', fontSize: '.92rem', lineHeight: 1.7 }}>{desc}</p>
    </div>
  )

  const codeBlock = <CodeWindow label={codeLabel}>{code}</CodeWindow>

  return (
    <div id={id} style={{ ...section(), borderTop: '1px solid var(--mg-border)' }}>
      <div style={inner}>
        <div style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))',
          gap: 48,
          alignItems: 'start',
        }}>
          {reverse ? <>{codeBlock}{textBlock}</> : <>{textBlock}{codeBlock}</>}
        </div>
      </div>
    </div>
  )
}

export default function CLIPage() {
  return (
    <MarketingLayout>
      {/* Hero */}
      <section style={{ padding: '96px 0 56px', textAlign: 'center' }}>
        <div style={inner}>
          <span style={{ ...label('var(--mg-accent)') }}>CLI Reference</span>
          <h1 style={{
            fontSize: 'clamp(2rem,5vw,3rem)',
            fontWeight: 800,
            letterSpacing: '-.04em',
            lineHeight: 1.1,
            marginBottom: 20,
          }}>
            Developers live<br />
            <span className="gradient-text">in the terminal.</span>
          </h1>
          <p style={{
            color: 'var(--mg-muted)',
            fontSize: '1.05rem',
            maxWidth: 560,
            margin: '0 auto 40px',
            lineHeight: 1.7,
          }}>
            Every debugging operation — from deploying a function to bisecting a production regression — is a single <code>flux</code> command.
          </p>

          {/* Command grid */}
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
            gap: 24,
            maxWidth: 1000,
            margin: '0 auto',
            textAlign: 'left',
          }}>
            <CommandCard
              color="var(--mg-green)"
              title="Deploy & Runtime"
              links={[
                { href: '#deploy', cmd: 'deploy', label: 'Deploy functions to production' },
                { href: '#tail', cmd: 'tail', label: 'Stream live request logs' },
              ]}
            />
            <CommandCard
              color="var(--mg-accent)"
              title="Debugging"
              links={[]}
            />
            <CommandCard
              color="#60a5fa"
              title="Data History"
              links={[]}
            />
            <CommandCard
              color="#c084fc"
              title="Incident Analysis"
              links={[
                { href: '#bug', cmd: 'bug bisect', label: 'Find the commit that introduced a bug' },
              ]}
            />
          </div>
        </div>
      </section>

      {/* Installation */}
      <section style={{ ...section(), background: 'var(--mg-bg-surface)' }}>
        <div style={inner}>
          <div style={label('var(--mg-accent)')}>Installation</div>
          <h2 style={{ fontSize: 'clamp(1.4rem,3vw,2rem)', fontWeight: 800, letterSpacing: '-.03em', marginBottom: 10 }}>
            One-line install.
          </h2>
          <p style={{ color: 'var(--mg-muted)', fontSize: '.95rem', maxWidth: 560, margin: '0 0 40px' }}>
            Installs a single static binary. No Node.js, no Python, no dependencies.
          </p>
          <div style={{ maxWidth: 640 }}>
            <CodeWindow label="install">{installCode}</CodeWindow>
          </div>
        </div>
      </section>

      {/* Tutorial */}
      <section style={{ ...section(), background: 'var(--mg-bg-surface)' }}>
        <div style={inner}>
          <div style={{ ...label('var(--mg-green)'), background: 'rgba(61,214,140,.1)' }}>First Tutorial</div>
          <h2 style={{ fontSize: 'clamp(1.4rem,3vw,2rem)', fontWeight: 800, letterSpacing: '-.03em', marginBottom: 10 }}>
            Debug a production bug in 30 seconds.
          </h2>
          <p style={{ color: 'var(--mg-muted)', fontSize: '.95rem', maxWidth: 520, margin: '0 0 40px' }}>
            The fastest way to understand these commands is to see them in sequence against a real failure.
          </p>

          <ol className="steps" style={{ maxWidth: 640 }}>
            <li>
              <div className="step-num">1</div>
              <div className="step-body">
                <h3>Deploy your function</h3>
                <p><code>flux deploy</code> — bundles and deploys your TypeScript functions. Returns a URL per function.</p>
              </div>
            </li>
            <li>
              <div className="step-num">2</div>
              <div className="step-body">
                <h3>Watch for failures</h3>
                <p><code>flux tail</code> — streams live requests. Errors appear in red with their request ID.</p>
              </div>
            </li>
            <li>
              <div className="step-num">3</div>
              <div className="step-body">
                <h3>Root-cause immediately</h3>
                <p><code>flux why &lt;request-id&gt;</code> — takes the ID from <code>flux tail</code> and shows root cause, location, and data changes.</p>
              </div>
            </li>
            <li>
              <div className="step-num">4</div>
              <div className="step-body">
                <h3>Compare before/after your fix</h3>
                <p><code>flux trace diff &lt;id-before&gt; &lt;id-after&gt;</code> — shows which spans changed and by how much after your code change.</p>
              </div>
            </li>
          </ol>

          <Link
            href="/docs/quickstart"
            style={{
              display: 'inline-flex',
              marginTop: 32,
              padding: '12px 24px',
              background: 'var(--mg-accent)',
              color: '#fff',
              borderRadius: 8,
              fontWeight: 600,
              textDecoration: 'none',
              fontSize: '.95rem',
            }}
          >
            Follow the full quickstart →
          </Link>
        </div>
      </section>

      {/* Deploy & Runtime section header */}
      <SectionHeader color="var(--mg-green)">Deploy &amp; Runtime</SectionHeader>

      <CommandSection
        id="deploy"
        cmd="flux deploy"
        title="Deploy functions to production"
        desc="Bundles all functions in the current project, uploads them, and makes them live behind the gateway in ~20 seconds. Returns a deploy ID and the public URL for each function."
        codeLabel="flux deploy"
        code={deployCode}
      />

      <CommandSection
        id="tail"
        cmd="flux tail"
        title="Stream live request logs"
        desc="Real-time request stream with inline data mutations and error messages. Every request shows method, path, duration, and the rows it changed. Errors show the reason immediately."
        codeLabel="flux tail"
        code={tailCode}
        reverse
      />

      {/* Debugging section header (placeholder) */}
      <SectionHeader color="var(--mg-accent)">Debugging</SectionHeader>

      {/* Data History section header (placeholder) */}
      <SectionHeader color="#60a5fa">Data History</SectionHeader>

      {/* Incident Analysis section header */}
      <SectionHeader color="#c084fc">Incident Analysis</SectionHeader>

      <CommandSection
        id="bug"
        cmd="flux bug bisect"
        title="Find the commit that introduced a bug"
        desc="Binary-searches your git history comparing trace behaviour before/after each commit. Automatically identifies the first commit where a specified request started failing."
        codeLabel="flux bug bisect"
        code={bugCode}
      />

      {/* CTA */}
      <section className="cta-strip">
        <h2>Ready to try it?</h2>
        <p style={{ maxWidth: 480, margin: '0 auto 32px' }}>
          Install the CLI, deploy your first function, and trace it end to end in 5 minutes.
        </p>
        <div style={{ display: 'flex', gap: 12, justifyContent: 'center', flexWrap: 'wrap' }}>
          <Link href="/docs/quickstart" className="btn-primary">Quickstart →</Link>
          <Link href="/product" className="btn-secondary">Product overview</Link>
        </div>
      </section>
    </MarketingLayout>
  )
}
