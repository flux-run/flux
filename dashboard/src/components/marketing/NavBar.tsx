'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { FluxIcon, FluxWordmark } from '@/components/FluxLogo'

const links = [
  { href: '/product', label: 'Product' },
  { href: '/how-it-works', label: 'How It Works' },
  { href: '/cli', label: 'CLI' },
  { href: '/docs', label: 'Docs' },
]

export function NavBar() {
  const pathname = usePathname()

  return (
    <nav style={{
      position: 'fixed', top: 0, left: 0, right: 0, zIndex: 100,
      height: 56,
      display: 'flex', alignItems: 'center', gap: 32,
      padding: '0 24px',
      background: 'rgba(14,14,16,.92)',
      backdropFilter: 'blur(10px)',
      borderBottom: '1px solid var(--mg-border)',
    }}>
      <Link href="/" style={{ display: 'flex', alignItems: 'center', gap: 8, textDecoration: 'none' }}>
        <FluxIcon size={24} />
        <FluxWordmark fontSize={15} baseColor="rgba(255,255,255,0.9)" />
      </Link>

      <div style={{ display: 'flex', gap: 24 }}>
        {links.map(({ href, label }) => (
          <Link key={href} href={href} style={{
            color: pathname === href || (href !== '/' && pathname?.startsWith(href))
              ? 'var(--mg-text)'
              : 'var(--mg-muted)',
            fontSize: '.875rem',
            textDecoration: 'none',
            transition: 'color .15s',
          }}>
            {label}
          </Link>
        ))}
      </div>

      <Link href="/dashboard" style={{
        marginLeft: 'auto',
        background: 'var(--mg-accent)',
        color: '#fff', fontSize: '.8rem', fontWeight: 600,
        padding: '6px 14px', borderRadius: 6,
        textDecoration: 'none',
        transition: 'opacity .15s',
      }}>
        Dashboard →
      </Link>
    </nav>
  )
}
