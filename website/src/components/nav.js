/**
 * Top navigation component.
 * @param {{ active?: string }} props
 *   active — one of 'home' | 'product' | 'how-it-works' | 'cli' | 'docs' | 'pricing'
 */
export function nav({ active = '' } = {}) {
  const links = [
    { href: '/product',        label: 'Product',       key: 'product'       },
    { href: '/how-it-works',   label: 'How It Works',  key: 'how-it-works'  },
    { href: '/cli',            label: 'CLI',            key: 'cli'           },
    { href: '/docs/',          label: 'Docs',           key: 'docs'          },
  ];

  const navLinks = links.map(({ href, label, key }) =>
    `<a href="${href}"${active === key ? ' class="active"' : ''}>${label}</a>`
  ).join('\n    ');

  return `<nav class="topnav">
  <a class="logo" href="/"><span>flux</span>base</a>
  <div class="nav-links">
    ${navLinks}
  </div>
  <a class="nav-cta" href="https://dashboard.fluxbase.co">Dashboard →</a>
</nav>`;
}
