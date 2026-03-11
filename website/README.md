# Fluxbase Website

Static HTML website built with a component-driven build system. No framework, no dependencies.

## Structure

```
website/
  src/                    ← source (edit these)
    components/           ← reusable HTML-returning JS functions
      nav.js
      footer.js
      code-window.js      ← macOS terminal chrome + color helpers
      section.js          ← eyebrow, sectionHeader, featureCard, comparisonGrid
    layouts/
      landing.js          ← full-width landing pages (no sidebar)
      docs.js             ← documentation layout (with sidebar)
    data/
      cli-commands.js     ← CLI commands reference data
    pages/                ← one file per page; exports meta + render()
      index.js            → index.html
      product.js          → product.html
      how-it-works.js     → how-it-works.html
      cli.js              → cli.html
      docs/
        index.js          → docs/index.html
        quickstart.js     → docs/quickstart.html
  assets/
    style.css             ← shared CSS (edit this for visual changes)
  build.js                ← build orchestrator
  package.json

  *.html                  ← GENERATED — do not edit directly
  docs/*.html             ← generated + legacy manually-written pages
```

## Commands

```bash
# Build all pages once (run before committing)
node build.js

# Watch for changes and rebuild automatically
node build.js --watch
```

## Adding a page

1. Create `src/pages/your-page.js`
2. Export `meta` object: `{ title, description, path }` where `path` is relative to `website/`
3. Export `render()` function that returns a full HTML string
4. Run `node build.js`

```js
import { landingLayout } from '../layouts/landing.js';

export const meta = {
  title:       'My Page — Fluxbase',
  description: 'Description for SEO',
  path:        'my-page.html',
};

export function render() {
  return landingLayout({
    meta,
    active: 'docs',
    content: `<section> ... </section>`,
  });
}
```

## Design system

All design tokens are in `assets/style.css` as CSS variables:

```
--accent      #6c63ff   (purple)
--green       #3dd68c
--yellow      #f5c542
--red         #f87171
--bg          #0e0e10
--bg-surface  #17171a
--border      #2a2a30
--muted       #888890
```

Use `codeWindow({ title, content })` from `src/components/code-window.js` for all terminal/code blocks.
Use colour helpers from the same file: `c.cmd()`, `c.ok()`, `c.err()`, `c.fn()`, `c.db()`, `c.ms()`, `c.dim()`.

## Workflow

Edit source files in `src/` → run `node build.js` → commit the generated HTML files.

The generated HTML files are committed to the repo so the site is deployable without a build step on the server.
