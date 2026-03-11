/**
 * Fluxbase website build system.
 *
 * Usage:
 *   node build.js          — build all pages once
 *   node build.js --watch  — watch src/ and rebuild on change
 *
 * Each page module in src/pages/**\/index.js (or any .js file) must export:
 *   export const meta = { title, description, path }   // path relative to website/
 *   export function render()                            // returns HTML string
 */

import { writeFileSync, mkdirSync, readdirSync, statSync, watchFile } from 'fs';
import { join, dirname, relative, resolve } from 'path';
import { fileURLToPath } from 'url';

const __dirname  = dirname(fileURLToPath(import.meta.url));
const PAGES_DIR  = join(__dirname, 'src', 'pages');
const OUTPUT_DIR = resolve(__dirname, '../dashboard/public');  // write directly into Vercel-served public/
const VERCEL_JSON = resolve(__dirname, '../dashboard/vercel.json');

// ── Discover all page modules ────────────────────────────────────────────────
function discoverPages(dir, found = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      discoverPages(full, found);
    } else if (entry.endsWith('.js')) {
      found.push(full);
    }
  }
  return found;
}

// ── Build a single page — returns meta.path on success, null on failure ──────
async function buildPage(modulePath) {
  // Break module cache by appending a timestamp (ESM dynamic import caches)
  const url = `file://${modulePath}?t=${Date.now()}`;
  let mod;
  try {
    mod = await import(url);
  } catch (e) {
    console.error(`  ERROR importing ${relative(__dirname, modulePath)}:`, e.message);
    return null;
  }

  if (!mod.meta || !mod.render) {
    console.warn(`  SKIP ${relative(__dirname, modulePath)} — missing meta or render export`);
    return null;
  }

  const html = mod.render();
  const outPath = join(OUTPUT_DIR, mod.meta.path);
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, html, 'utf8');
  console.log(`  ✔  ${mod.meta.path}`);
  return mod.meta.path;
}

// ── Convert a meta.path like "docs/quickstart.html" to a Vercel rewrite ──────
// Rules:
//   home.html          → /  (marketing root)
//   foo/index.html     → /foo
//   foo/bar.html       → /foo/bar
//   anything.html      → /anything
function metaPathToRewrite(metaPath) {
  const dest = '/' + metaPath.replace(/\\/g, '/');
  let source = dest.replace(/\.html$/, '');
  if (source.endsWith('/index')) source = source.slice(0, -6) || '/';
  if (source === '/home') source = '/';
  return { source, destination: dest };
}

// ── Write vercel.json from the collected page paths ───────────────────────────
function writeVercelJson(builtPaths) {
  // Sort: root first, then alphabetically for determinism
  const staticRewrites = builtPaths
    .sort((a, b) => a.localeCompare(b))
    .map(metaPathToRewrite)
    // Root rewrite must come first
    .sort((a, b) => (a.source === '/' ? -1 : b.source === '/' ? 1 : 0));

  // SPA routes — /dashboard and /login serve the React app.
  // All /dashboard sub-paths redirect to /dashboard so the app always
  // loads from root (avoids 404 on hard reload of deep links).
  const spaRewrites = [
    { source: '/login',     destination: '/index.html' },
    { source: '/dashboard', destination: '/index.html' },
  ];

  const spaRedirects = [
    { source: '/dashboard/:path*', destination: '/dashboard', permanent: false },
  ];

  const config = {
    cleanUrls: true,
    redirects: spaRedirects,
    rewrites: [...staticRewrites, ...spaRewrites],
  };

  writeFileSync(VERCEL_JSON, JSON.stringify(config, null, 2) + '\n', 'utf8');
  console.log(`  ✔  vercel.json (${staticRewrites.length} static + ${spaRewrites.length} SPA rewrites + ${spaRedirects.length} SPA redirects)`);
}

// ── Build all ─────────────────────────────────────────────────────────────────
async function buildAll() {
  const pages = discoverPages(PAGES_DIR);
  console.log(`\nBuilding ${pages.length} page(s)…`);
  const builtPaths = [];
  for (const p of pages) {
    const path = await buildPage(p);
    if (path) builtPaths.push(path);
  }
  writeVercelJson(builtPaths);
  console.log('Done.\n');
}

// ── Watch mode ────────────────────────────────────────────────────────────────
function watchSrc() {
  console.log('Watching src/ for changes…');
  const srcDir = join(__dirname, 'src');
  function watchDir(dir) {
    for (const entry of readdirSync(dir)) {
      const full = join(dir, entry);
      if (statSync(full).isDirectory()) {
        watchDir(full);
      } else if (entry.endsWith('.js')) {
        watchFile(full, { interval: 500 }, () => {
          console.log(`\nChanged: ${relative(__dirname, full)}`);
          buildAll();
        });
      }
    }
  }
  watchDir(srcDir);
}

// ── Entry point ───────────────────────────────────────────────────────────────
const isWatch = process.argv.includes('--watch');
await buildAll();
if (isWatch) watchSrc();
