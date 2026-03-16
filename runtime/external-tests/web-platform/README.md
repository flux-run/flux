# Web Platform Tests (subset)

This directory holds the `url/`, `fetch/`, and `encoding/` subdirectories of
the W3C Web Platform Tests suite.

## Setup

```bash
# Shallow-clone only the relevant subdirectories via sparse checkout
cd runtime/external-tests
git clone --depth 1 --filter=blob:none --sparse \
         https://github.com/web-platform-tests/wpt web-platform
cd web-platform
git sparse-checkout set url fetch encoding
```

Or use the automated setup script:

```bash
cd runtime
bash scripts/setup-external-tests.sh wpt
```

## What is tested

| Directory  | Tests                                              |
|------------|----------------------------------------------------|
| `url/`     | URL and URLSearchParams parsing & serialization    |
| `fetch/`   | Fetch API — request/response, headers, body types  |
| `encoding/`| TextEncoder / TextDecoder correctness              |

All three of these Web APIs are available in Deno-based runtimes and are
therefore expected to pass at a high rate inside Flux isolates.

## Running

```bash
cd runtime/runners
npm run test:web
```

Results are written to `runtime/reports/web-tests.json`.

## Exclusions

The runner automatically skips:
- Any test that requires a DOM (`document`, `window.location`, …)
- Tests that require a `ServiceWorker` or `Worker` context
- Browser-only security policy tests
