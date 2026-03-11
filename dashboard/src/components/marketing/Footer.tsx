import Link from 'next/link'
import { FluxIcon, FluxWordmark } from '@/components/FluxLogo'

export function Footer() {
  return (
    <footer style={{
      borderTop: '1px solid var(--mg-border)',
      padding: '24px',
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      flexWrap: 'wrap', gap: 12,
      fontSize: '.82rem',
      color: 'var(--mg-muted)',
    }}>
      <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <FluxIcon size={18} />
        <FluxWordmark fontSize={13} baseColor="var(--mg-muted)" />
        <span style={{ marginLeft: 4 }}>© 2026</span>
      </span>
      <div style={{ display: 'flex', gap: 20, flexWrap: 'wrap' }}>
        {[
          { href: '/product', label: 'Product' },
          { href: '/how-it-works', label: 'How It Works' },
          { href: '/cli', label: 'CLI' },
          { href: '/docs', label: 'Docs' },
          { href: '/docs/quickstart', label: 'Quickstart' },
          { href: '/dashboard', label: 'Dashboard' },
        ].map(({ href, label }) => (
          <Link key={href} href={href} style={{ color: 'var(--mg-muted)', textDecoration: 'none' }}>
            {label}
          </Link>
        ))}
      </div>
    </footer>
  )
}
