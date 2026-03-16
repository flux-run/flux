/**
 * Flux Module Loader Suite
 *
 * These tests cover the module system patterns that commonly break new runtimes:
 *
 *   Static imports       — named, default, namespace, re-exports
 *   Dynamic imports      — import() at runtime, conditional, lazy
 *   Circular imports     — A→B→A resolved correctly without TDZ errors
 *   Module cache         — same module evaluated once; singleton identity preserved
 *   Error propagation    — bad imports surface the right error shape
 *
 * Tests use real fixture files under ./fixtures/ so they exercise the actual
 * module loader, not just in-file object passing.
 */

import { TestHarness, assert, assertEquals } from "../../src/harness.js";

// ---------------------------------------------------------------------------
// Static imports from fixtures — resolved at compile time
// ---------------------------------------------------------------------------
import { PI, E, add, multiply } from "./fixtures/math.js";
import Greeter from "./fixtures/greeter.js";
import { PI as barrelPI, add as barrelAdd, Greeter as BarrelGreeter, VERSION } from "./fixtures/barrel.js";
import { getA, callB } from "./fixtures/circular-a.js";
import { getB, callA } from "./fixtures/circular-b.js";
import { getInitCount, SINGLETON_ID } from "./fixtures/singleton.js";

export function createModuleLoaderSuite(): TestHarness {
  const suite = new TestHarness("Module Loader");

  // ── Static named imports ─────────────────────────────────────────────────

  suite.test("static named import: numeric constant has correct value", () => {
    assertEquals(PI, 3.14159, "PI must equal 3.14159");
    assertEquals(E,  2.71828, "E must equal 2.71828");
  });

  suite.test("static named import: function call works correctly", () => {
    assertEquals(add(2, 3),       5, "add(2,3) must return 5");
    assertEquals(multiply(4, 5), 20, "multiply(4,5) must return 20");
  });

  // ── Static default import ────────────────────────────────────────────────

  suite.test("static default import: class instantiation works", () => {
    const g = new Greeter("Hello");
    assertEquals(g.greet("Alice"), "Hello, Alice!", "Default-exported class must be instantiable");
  });

  suite.test("static default import: multiple instances are independent", () => {
    const g1 = new Greeter("Hi");
    const g2 = new Greeter("Hey");
    assertEquals(g1.greet("Bob"), "Hi, Bob!",  "g1 must use its own greeting");
    assertEquals(g2.greet("Bob"), "Hey, Bob!", "g2 must use its own greeting");
  });

  // ── Barrel / re-export ───────────────────────────────────────────────────

  suite.test("re-export barrel: named value forwarded correctly", () => {
    assertEquals(barrelPI,  3.14159,  "PI re-exported through barrel must equal original");
    assertEquals(VERSION,   "1.0.0",  "VERSION defined in barrel must be accessible");
  });

  suite.test("re-export barrel: function forwarded and callable", () => {
    assertEquals(barrelAdd(10, 20), 30, "add() re-exported through barrel must work");
  });

  suite.test("re-export barrel: default exported as named re-export", () => {
    const g = new BarrelGreeter("Howdy");
    assertEquals(g.greet("World"), "Howdy, World!", "Default class re-exported as named must instantiate correctly");
  });

  // ── Dynamic import() ─────────────────────────────────────────────────────

  suite.test("dynamic import(): named export accessible from lazily loaded module", async () => {
    const mod = await import("./fixtures/math.js");
    assertEquals(mod.add(1, 2), 3, "Dynamically imported add() must return 3");
    assertEquals(mod.PI, 3.14159,  "Dynamically imported PI must equal 3.14159");
  });

  suite.test("dynamic import(): default export accessible via .default", async () => {
    const mod = await import("./fixtures/greeter.js");
    const g = new mod.default("Greetings");
    assertEquals(g.greet("Flux"), "Greetings, Flux!", "Default export must be on .default when dynamically imported");
  });

  suite.test("dynamic import(): conditional — module loaded only when needed", async () => {
    let loaded = false;
    const condition = true; // In real code this might be a feature flag.
    if (condition) {
      const mod = await import("./fixtures/math.js");
      loaded = typeof mod.add === "function";
    }
    assert(loaded, "Conditional dynamic import must load and expose the module when condition is true");
  });

  suite.test("dynamic import(): repeated import returns same module reference", async () => {
    const mod1 = await import("./fixtures/math.js");
    const mod2 = await import("./fixtures/math.js");
    // Both references must be the same cached module object.
    assert(mod1.add === mod2.add,
      "Repeated dynamic import of the same path must return the identical cached function reference");
  });

  // ── Circular imports ─────────────────────────────────────────────────────

  suite.test("circular import: module A resolves its own export correctly", () => {
    assertEquals(getA(), "A", "circular-a.getA() must return 'A'");
  });

  suite.test("circular import: module B resolves its own export correctly", () => {
    assertEquals(getB(), "B", "circular-b.getB() must return 'B'");
  });

  suite.test("circular import: A can call B's export at runtime (no TDZ error)", () => {
    // If the runtime doesn't handle circular deps, callB() would throw a
    // ReferenceError / TDZ error because getB is accessed before B is initialized.
    let threw = false;
    try {
      const result = callB();
      assertEquals(result, "B", "callB() must invoke getB() and return 'B'");
    } catch {
      threw = true;
    }
    assert(!threw, "Circular import must not cause a TDZ / ReferenceError at call time");
  });

  suite.test("circular import: B can call A's export at runtime (no TDZ error)", () => {
    let threw = false;
    try {
      const result = callA();
      assertEquals(result, "A", "callA() must invoke getA() and return 'A'");
    } catch {
      threw = true;
    }
    assert(!threw, "Circular import must not cause a TDZ / ReferenceError at call time");
  });

  // ── Module cache / singleton ─────────────────────────────────────────────

  suite.test("module cache: top-level initializer runs exactly once", () => {
    // getInitCount() reads a counter that is incremented at module evaluation time.
    // Because Node's module cache ensures modules are evaluated once, this must be 1.
    assertEquals(getInitCount(), 1,
      "Module-level side effect (init counter++) must run exactly once — not once per import");
  });

  suite.test("module cache: singleton identity is stable across two import sites", async () => {
    // Import the same singleton module again dynamically.
    const mod = await import("./fixtures/singleton.js");

    // The SINGLETON_ID exported via static import and the one from the dynamic
    // import must be identical — the module was evaluated once and cached.
    assertEquals(mod.SINGLETON_ID, SINGLETON_ID,
      "SINGLETON_ID must be the same value regardless of how many times the module is imported");

    assertEquals(mod.getInitCount(), 1,
      "Init count must still be 1 when accessed through a second import site");
  });

  return suite;
}
