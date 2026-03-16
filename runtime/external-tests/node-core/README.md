# Node.js Core Compatibility Tests

This directory holds a subset of the Node.js `test/parallel` and
`test/sequential` directories.

## Setup

Clone the Node.js repository and copy the relevant test directories:

```bash
# Temporary clone (only shallow — do NOT copy entire repo)
cd /tmp
git clone --depth 1 https://github.com/nodejs/node node-src

# Copy only the test directories we need
cp -r /tmp/node-src/test/parallel \
       /Users/shashisharma/code/self/flowbase/runtime/external-tests/node-core/parallel

cp -r /tmp/node-src/test/sequential \
       /Users/shashisharma/code/self/flowbase/runtime/external-tests/node-core/sequential

# Clean up the full clone
rm -rf /tmp/node-src
```

Or use the automated setup script:

```bash
cd runtime
bash scripts/setup-external-tests.sh node
```

## What is tested

Node.js core tests exercise the Node.js built-in API surface:
`fs`, `path`, `crypto`, `events`, `stream`, `http`, `url`, `buffer`, etc.
Running these against Flux measures which Node.js APIs are available and
correct inside the Deno-based isolate that Flux provides.

## Running

```bash
cd runtime/runners
npm run test:node
```

Results are written to `runtime/reports/node-tests.json`.

## Expected pass rate

Because Flux isolates run under Deno, not Node.js, a subset of tests
(particularly those touching `fs`, process signals, and native modules) will
fail or be skipped. The runner logs all skipped tests separately.

Typical target: ≥ 60 % pass on `test/parallel`.
