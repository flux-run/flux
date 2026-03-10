#!/usr/bin/env node
// dashboard/scripts/copy-static.js
// Copies the repo-level website/ static site into dashboard/public/ so that:
//   - Vite includes it in dist/ (served on Vercel at /docs/*, /examples/*, etc.)
//   - Docker build (context: ./dashboard) can COPY public/ files
//
// Run automatically via the "prebuild" npm script.

import fs   from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const SRC  = path.resolve(__dirname, "../../website");    // repo root website/
const DEST = path.resolve(__dirname, "../public");        // dashboard/public/

function copyDir(src, dest) {
  if (!fs.existsSync(src)) {
    console.warn(`  [copy-static] source not found: ${src} — skipping`);
    return;
  }
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, entry.name);
    const d = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDir(s, d);
    } else {
      fs.copyFileSync(s, d);
    }
  }
}

// Directories to copy from website/ into public/
const copies = [
  ["docs",     "docs"],
  ["examples", "examples"],
  ["assets",   "assets"],
];

// Individual files
const files = [
  ["index.html",  "index.html"],
  ["install.sh",  "install.sh"],
];

let changed = 0;

for (const [srcDir, destDir] of copies) {
  const s = path.join(SRC, srcDir);
  const d = path.join(DEST, destDir);
  if (fs.existsSync(s)) {
    copyDir(s, d);
    console.log(`  [copy-static] ${srcDir}/ → public/${destDir}/`);
    changed++;
  }
}

for (const [srcFile, destFile] of files) {
  const s = path.join(SRC, srcFile);
  const d = path.join(DEST, destFile);
  if (fs.existsSync(s)) {
    fs.mkdirSync(path.dirname(d), { recursive: true });
    fs.copyFileSync(s, d);
    console.log(`  [copy-static] ${srcFile} → public/${destFile}`);
    changed++;
  }
}

if (changed === 0) {
  console.warn("  [copy-static] nothing copied — is website/ present?");
} else {
  console.log(`  [copy-static] done (${changed} items)`);
}
