/**
 * Documentation layout (with sidebar).
 */
import { nav }    from '../components/nav.js';
import { footer } from '../components/footer.js';

const SIDEBAR_NAV = [
  {
    title: 'Getting Started',
    links: [
      { href: '/docs/',             label: 'Introduction'   },
      { href: '/docs/quickstart',   label: 'Quickstart'     },
      { href: '/docs/concepts',     label: 'Core Concepts'  },
    ],
  },
  {
    title: 'Debugging',
    links: [
      { href: '/docs/debugging-production', label: 'Production Debugging' },
      { href: '/cli',               label: 'CLI Reference'  },
      { href: '/docs/observability',label: 'Observability'  },
    ],
  },
  {
    title: 'Architecture',
    links: [
      { href: '/how-it-works',      label: 'How It Works'   },
      { href: '/docs/gateway',      label: 'Gateway'        },
      { href: '/docs/runtime',      label: 'Runtime'        },
      { href: '/docs/data-engine',  label: 'Data Engine'    },
      { href: '/docs/queue',        label: 'Queue'          },
    ],
  },
  {
    title: 'Examples',
    links: [
      { href: '/examples/',         label: 'All Examples'   },
      { href: '/examples/todo-api', label: 'Todo API'       },
      { href: '/examples/ai-backend', label: 'AI Backend'   },
    ],
  },
];

/**
 * @param {{ meta: { title: string, description: string }, activePath?: string, content: string, extraHead?: string }} props
 */
export function docsLayout({ meta, activePath = '', content = '', extraHead = '' } = {}) {
  const FONTS = `<link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=JetBrains+Mono&display=swap" rel="stylesheet">`;

  const sidebarGroups = SIDEBAR_NAV.map(group => {
    const links = group.links.map(l => {
      const isActive = l.href === activePath;
      return `<a href="${l.href}"${isActive ? ' class="active"' : ''}>${l.label}</a>`;
    }).join('\n    ');

    return `<div class="sidebar-group">
    <div class="sidebar-group-title">${group.title}</div>
    ${links}
  </div>`;
  }).join('\n  ');

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>${meta.title}</title>
  <meta name="description" content="${meta.description}">
  ${FONTS}
  <link rel="stylesheet" href="/assets/style.css">
  ${extraHead}
</head>
<body>

${nav({ active: 'docs' })}

<div class="page-wrap">

  <aside class="sidebar">
    ${sidebarGroups}
  </aside>

  <main class="main-content">
    <div class="content">
${content}
    </div>
  </main>

</div>

${footer()}

</body>
</html>`;
}
