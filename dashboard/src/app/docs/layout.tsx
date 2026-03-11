import Link from 'next/link'

const NAV = [
  {
    title: 'Getting Started',
    links: [
      { href: '/docs',                   label: 'Introduction'          },
      { href: '/docs/install',           label: 'Install CLI'           },
      { href: '/docs/quickstart',        label: 'Quickstart'            },
      { href: '/docs/concepts',          label: 'Core Concepts'         },
    ],
  },
  {
    title: 'Debugging',
    links: [
      { href: '/docs/debugging-production', label: 'Production Debugging' },
      { href: '/docs/observability',        label: 'Observability'         },
      { href: '/cli',                       label: 'CLI Reference'         },
    ],
  },
  {
    title: 'Architecture',
    links: [
      { href: '/docs/architecture',  label: 'Architecture'  },
      { href: '/docs/gateway',       label: 'API Gateway'   },
      { href: '/docs/functions',     label: 'Functions'     },
      { href: '/docs/database',      label: 'Database'      },
      { href: '/docs/secrets',       label: 'Secrets'       },
    ],
  },
]

export default function DocsLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      {/* Bring in the docs-specific stylesheet */}
      {/* eslint-disable-next-line @next/next/no-page-custom-font */}
      <link
        href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=JetBrains+Mono&display=swap"
        rel="stylesheet"
      />
      <link rel="stylesheet" href="/docs-style.css" />

      <nav className="topnav">
        <Link className="logo" href="/">
          <span>flux</span>base
        </Link>
        <div className="nav-links">
          <Link href="/docs" className="active">Docs</Link>
          <Link href="/how-it-works">How It Works</Link>
          <Link href="/cli">CLI</Link>
        </div>
        <Link className="nav-cta" href="https://dashboard.fluxbase.co">
          Dashboard →
        </Link>
      </nav>

      <div className="page-wrap">
        <nav className="sidebar">
          {NAV.map((group) => (
            <div key={group.title} className="sidebar-group">
              <div className="sidebar-group-title">{group.title}</div>
              {group.links.map((l) => (
                <Link key={l.href} href={l.href}>{l.label}</Link>
              ))}
            </div>
          ))}
        </nav>

        <main className="main-content">
          <div className="content">
            {children}
          </div>
        </main>
      </div>
    </>
  )
}
