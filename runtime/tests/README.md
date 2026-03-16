# Flux Runtime Test Suite

A comprehensive runtime compatibility test suite (~200-300 tests) that validates the Flux runtime across multiple dimensions.

## Architecture

The test suite is organized into 8 layers, each validating different aspects:

### 1. **ECMAScript** (~60 tests)
Tests core JavaScript language features covered by Test262:
- Arrow functions, destructuring, template literals
- Promises, async/await
- Classes and inheritance
- Generators and iterators
- Map/Set/Symbol
- Built-in methods (Array, String, Object, Math, JSON, RegExp)

**Why it matters**: Ensures your runtime handles the JavaScript language correctly.

### 2. **Node.js APIs** (~50 tests)
Tests critical Node.js compatibility:
- **fs**: file read/write, mkdir, stat, rename
- **events**: EventEmitter with on/once/off
- **timers**: setTimeout, setInterval, setImmediate
- **buffer**: Buffer creation, concat, slicing
- **path**: join, basename, dirname, extname
- **process**: env, cwd, pid
- **crypto**: randomBytes, hash creation
- **console**: log, error methods

**Why it matters**: These APIs power the entire npm ecosystem. If Node APIs work, 80% of npm packages will run.

### 3. **Web APIs** (~30 tests)
Tests browser-compatible standards:
- URL/URLSearchParams parsing
- Headers/Request/Response
- Blob/ArrayBuffer
- FormData
- TextEncoder/TextDecoder
- AbortController
- base64 encoding (atob/btoa)

**Why it matters**: Ensures compatibility with fetch-based libraries and browser APIs used in isomorphic code.

### 4. **Frameworks** (~20 tests)
Tests basic framework patterns:
- Express-like routing
- Middleware chains (Koa-like)
- Request/response context
- Route parameters and query strings
- Error handling
- Async handlers
- Request validation
- Static content serving
- Rate limiting, CORS, cookies, dependency injection

**Why it matters**: If Express/Koa patterns work, developers will trust your runtime immediately.

### 5. **Runtime Stress Tests** (~25 tests)
Torture tests covering edge cases:
- Large object/array creation (1000s of items)
- Deep nesting (100+ levels)
- Complex transformations (filter+map+reduce chains)
- Heavy event listener counts
- Closure capture correctness
- Recursive functions
- Generator chains
- WeakMap/Symbol uniqueness

**Why it matters**: Catches performance regressions and ensures the runtime doesn't leak or crash under load.

### 6. **Deterministic Replay** (~20 tests)
Validates determinism for your debugging features:
- Math.random consistency
- Date.now ordering
- setTimeout execution order
- Promise resolution order
- Event emission order
- Object/Map/Set iteration order
- JSON parsing consistency
- Error stack trace format

**Why it matters**: This is unique to Flux. Proves that executions can be replayed deterministically for root cause analysis.

### 7. **Error Handling** (~25 tests)
Tests error recovery and propagation:
- try/catch/finally
- Promise rejection handling
- Async/await errors
- Error inheritance
- Custom error types (SyntaxError, TypeError, RangeError)
- Stack traces
- Error re-throwing
- Errors in setTimeout/array operations/getters
- Abort signals
- Error recovery with fallbacks

**Why it matters**: Production systems crash on errors. Proving error handling works builds confidence.

### 8. **Concurrency** (~25 tests)
Tests isolation and concurrent execution:
- Parallel Promise execution
- Mixed timing Promise.all
- Concurrent setTimeout
- Promise error isolation
- Race conditions (demonstrates limitations)
- Microtask vs macrotask ordering
- Data isolation between executions
- Async context preservation

**Why it matters**: Shows your runtime can safely handle multiple concurrent requests.

## Quick Start

### Install Dependencies
```bash
cd runtime/tests
npm install
```

### Run All Tests
```bash
npm run test:all
```

Output:
```
🚀 Flux Runtime Test Suite

Running ecmascript...
  45/45 passed
Running node...
  50/50 passed
Running web...
  30/30 passed
Running frameworks...
  20/20 passed
Running runtime...
  25/25 passed
Running determinism...
  20/20 passed
Running error-handling...
  25/25 passed
Running concurrency...
  25/25 passed

════════════════════════════════════════════════════════════════
TEST RESULTS
════════════════════════════════════════════════════════════════

✓ ECMAScript (45/45) 12.34ms
✓ Node.js APIs (50/50) 45.67ms
✓ Web APIs (30/30) 8.90ms
✓ Frameworks (20/20) 5.43ms
✓ Runtime (25/25) 89.21ms
✓ Determinism (20/20) 34.56ms
✓ Error Handling (25/25) 23.45ms
✓ Concurrency (25/25) 156.78ms

────────────────────────────────────────────────────────────────
Total: 260/260 passed (0 failed) in 376.34ms
════════════════════════════════════════════════════════════════

✅ All 260 tests passed!
```

### Run Specific Suite
```bash
npm run test:ecmascript    # ECMAScript tests only
npm run test:node          # Node.js API tests only
npm run test:web           # Web API tests only
npm run test:frameworks    # Framework pattern tests
npm run test:runtime       # Runtime stress tests
npm run test:determinism   # Deterministic replay tests
npm run test:errors        # Error handling tests
npm run test:concurrency   # Concurrency tests

# Or use the CLI with filters:
npm run test -- ecmascript
npm run test -- node
```

### Build
```bash
npm run build              # Compile TypeScript
npm run test:watch        # Watch mode
npm run clean             # Clean build output
```

## Adding Custom Tests

### Adding to an Existing Suite

Edit any file in `suites/<layer>/index.ts`:

```typescript
suite.test("your test name", async () => {
  // Arrange
  const input = 42;
  
  // Act
  const result = input * 2;
  
  // Assert
  assertEquals(result, 84, "Should double the input");
});
```

### Creating a New Suite

1. Create a new directory: `suites/my-feature/`
2. Create `index.ts` with a function `createMySuite():`

```typescript
import { TestHarness, assertEquals } from "../../harness.js";

export function createMySuite(): TestHarness {
  const suite = new TestHarness("My Feature");
  
  suite.test("test 1", () => {
    assertEquals(1 + 1, 2);
  });
  
  return suite;
}
```

3. Import and add to `src/cli.ts`:

```typescript
import { createMySuite } from "./suites/my-feature/index.js";

const suites = {
  // ... existing
  "my-feature": createMySuite(),
};
```

## Assertion API

All assertion functions throw on failure and are exported from `src/harness.ts`:

```typescript
assert(condition, message)              // Basic assertion
assertEquals(actual, expected, message) // Equality check
assertThrows(fn, message)               // Function should throw
assertArrayIncludes(arr, item)          // Array contains item
assertStringIncludes(str, substring)    // String contains substring
```

## Test Harness API

Create a test suite and add tests:

```typescript
const suite = new TestHarness("My Suite");

suite.test("sync test", () => {
  // sync test body
});

suite.test("async test", async () => {
  // async test body
  await something();
});

const result = await suite.run();  // Run all tests
// result.passed, result.failed, result.total, result.duration
```

## Integration with CI/CD

### GitHub Actions

Create `.github/workflows/runtime-tests.yml`:

```yaml
name: Runtime Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-node@v3
        with:
          node-version: '18'
      - run: cd runtime/tests && npm install
      - run: npm run test:all
```

## Test Coverage Goals

- **Target**: > 95% of runtime execution paths
- **Critical paths**: 100% (error handling, concurrency, IO)
- **Edge cases**: Comprehensive (deep recursion, large data structures)

## Known Limitations

1. **Race Conditions**: The concurrency tests demonstrate that without proper synchronization primitives (mutexes, atomics), race conditions are possible. This is expected in JavaScript.

2. **Async Microtask Ordering**: Exact Promise ordering can vary depending on implementation. Tests validate that microtasks run before macrotasks, but relative ordering among same-priority tasks may vary.

3. **Network Tests**: The test suite doesn't test actual network operations. Fetch is exercised with Response objects, but real network requests would require mocking infrastructure.

4. **File System Limits**: Tests create temp files. Ensure your test environment has at least 100MB free disk space.

## Performance Baselines

Target execution times (on modern hardware):

| Layer | Tests | Target Time |
|-------|-------|------------|
| ECMAScript | 45 | < 50ms |
| Node.js APIs | 50 | < 100ms |
| Web APIs | 30 | < 50ms |
| Frameworks | 20 | < 30ms |
| Runtime | 25 | < 200ms |
| Determinism | 20 | < 100ms |
| Error Handling | 25 | < 50ms |
| Concurrency | 25 | < 300ms |
| **Total** | **260** | **< 900ms** |

If tests exceed these baselines, profile and optimize.

## Troubleshooting

### Tests timing out
- Increase timeout if running on slow hardware
- Check for infinite loops in new tests
- Verify setTimeout mock is working correctly

### "ReferenceError: module not found"
- Run `npm install` in `runtime/tests/`
- Ensure tsconfig.json has correct paths

### "Port already in use"
- Framework tests use localhost:3000 for simulation. Kill any lingering processes.

### Memory issues
- Reduce "Heavy object creation" test count if running on memory-constrained environment
- Run specific suites instead of all

## Contributing

1. Add test cases to relevant suite files
2. Ensure tests pass: `npm run test:all`
3. Keep individual tests < 100ms (add :stress:` prefix for longer tests)
4. Add descriptive assertions with clear messages
5. Update this README if adding new suites

## License

MIT - Same as Flux project
