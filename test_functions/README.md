# Fluxbase Test Functions

A collection of simple serverless functions for testing the Fluxbase platform locally.

## Functions

| File       | Name       | Description                                                                  |
| ---------- | ---------- | ---------------------------------------------------------------------------- |
| `hello.js` | hello-test | Returns a simple hello world JSON response                                   |
| `echo.js`  | echo-test  | Mirrors back the incoming request payload with a timestamp                   |
| `math.js`  | math-test  | Performs arithmetic (`add`, `subtract`, `multiply`, `divide`) on `a` and `b` |

## Usage

### Prerequisites

Make sure all three services are running:

```bash
# Terminal 1 — Control Plane API (port 8080)
cd api && cargo run

# Terminal 2 — Runtime Engine (port 8081)
cd runtime && cargo run

# Terminal 3 — Dashboard (port 5173)
cd dashboard && npm run dev
```

### Login

```bash
FLUXBASE_API_URL=http://localhost:8080 flux login
# Enter your API key when prompted
```

### Deploy a Function

```bash
# Deploy from test_functions directory
FLUXBASE_API_URL=http://localhost:8080 flux deploy \
  --name hello-test \
  --runtime nodejs \
  --file test_functions/hello.js
```

### List Functions

```bash
FLUXBASE_API_URL=http://localhost:8080 flux function list
```

### Invoke a Function

```bash
FLUXBASE_API_URL=http://localhost:8080 FLUXBASE_RUNTIME_URL=http://localhost:8081 flux invoke hello-test
FLUXBASE_API_URL=http://localhost:8080 FLUXBASE_RUNTIME_URL=http://localhost:8081 flux invoke echo-test
FLUXBASE_API_URL=http://localhost:8080 FLUXBASE_RUNTIME_URL=http://localhost:8081 flux invoke math-test
```

## Adding New Functions

Create a `.js` file in this directory:

```js
export default async function (req, ctx) {
  return new Response(JSON.stringify({ message: "Hello!" }), {
    headers: { "Content-Type": "application/json" },
  });
}
```

Then deploy it:

```bash
flux deploy --name my-function --runtime nodejs --file test_functions/my-function.js
```
