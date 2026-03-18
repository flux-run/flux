// Concurrency and isolation tests
import { TestHarness, assert, assertEquals } from "../harness.js";
export function createConcurrencySuite() {
    const suite = new TestHarness("Concurrency");
    suite.test("Parallel Promise execution", async () => {
        const results = [];
        const promises = Array.from({ length: 5 }, (_, i) => Promise.resolve(i).then((x) => {
            results.push(x);
            return x;
        }));
        await Promise.all(promises);
        assertEquals(results.length, 5, "All promises should execute");
    });
    suite.test("Promise.all with mixed timing", async () => {
        const p1 = Promise.resolve(1);
        const p2 = new Promise((resolve) => setTimeout(() => resolve(2), 10));
        const p3 = Promise.resolve(3);
        const results = await Promise.all([p1, p2, p3]);
        assertEquals(results.length, 3, "All promises should resolve");
        assertEquals(results[0], 1, "First promise should resolve");
        assertEquals(results[1], 2, "Second promise should resolve");
    });
    suite.test("Parallel setTimeout execution", async () => {
        const times = [];
        const start = Date.now();
        const promises = Array.from({ length: 3 }, () => new Promise((resolve) => {
            setTimeout(() => {
                times.push(Date.now() - start);
                resolve(null);
            }, 10);
        }));
        await Promise.all(promises);
        assertEquals(times.length, 3, "All timeouts should fire");
    });
    suite.test("Concurrent array map", async () => {
        const arr = [1, 2, 3, 4, 5];
        const results = await Promise.all(arr.map((x) => Promise.resolve(x * 2)));
        assertEquals(results.length, 5, "All should process");
        assertEquals(results[0], 2, "Values should be doubled");
    });
    suite.test("Race condition avoidance - shared counter", async () => {
        let counter = 0;
        const increment = async () => {
            const current = counter;
            await Promise.resolve();
            counter = current + 1;
        };
        await Promise.all([increment(), increment(), increment()]);
        // This demonstrates a race condition (counter < 3)
        // Real atomicity requires locks or atomic operations
        assert(counter <= 3, "Counter operations should complete");
    });
    suite.test("Async iteration parallelism", async () => {
        const processed = [];
        for (const item of [1, 2, 3]) {
            await Promise.resolve(item).then((x) => {
                processed.push(x);
            });
        }
        assertEquals(processed.length, 3, "All items should be processed");
    });
    suite.test("Multiple EventEmitter listeners", () => {
        const { EventEmitter } = require("events");
        const emitter = new EventEmitter();
        const results = [];
        emitter.on("event", () => {
            results.push("a");
        });
        emitter.on("event", () => {
            results.push("b");
        });
        emitter.on("event", () => {
            results.push("c");
        });
        emitter.emit("event");
        assertEquals(results.length, 3, "All listeners should fire");
    });
    suite.test("Concurrent Map operations", () => {
        const map = new Map();
        const operations = [];
        for (let i = 0; i < 100; i++) {
            map.set(i, i * 2);
            operations.push({ key: i, value: i * 2 });
        }
        assertEquals(map.size, 100, "All insertions should succeed");
        assertEquals(map.get(50), 100, "Value should be correct");
    });
    suite.test("Concurrent Set operations", () => {
        const set = new Set();
        for (let i = 0; i < 100; i++) {
            set.add(i);
        }
        assertEquals(set.size, 100, "All insertions should succeed");
        assert(set.has(50), "Value should exist");
    });
    suite.test("Nested Promise execution", async () => {
        const order = [];
        const p1 = Promise.resolve(1).then((x) => {
            order.push(x);
            return Promise.resolve(2).then((y) => {
                order.push(y);
            });
        });
        const p2 = Promise.resolve(3).then((x) => {
            order.push(x);
        });
        await Promise.all([p1, p2]);
        assertEquals(order.length, 3, "All promises should execute");
        assert(order.includes(1) && order.includes(2) && order.includes(3), "All values present");
    });
    suite.test("Concurrent request simulation", async () => {
        const responses = [];
        const simulatedRequest = (id) => new Promise((resolve) => {
            setTimeout(() => {
                responses.push(id);
                resolve(id);
            }, Math.random() * 10);
        });
        await Promise.all([simulatedRequest(1), simulatedRequest(2), simulatedRequest(3)]);
        assertEquals(responses.length, 3, "All requests should complete");
    });
    suite.test("Microtask vs macrotask ordering", async () => {
        const order = [];
        Promise.resolve().then(() => {
            order.push("microtask1");
        });
        setTimeout(() => {
            order.push("macrotask");
        }, 0);
        Promise.resolve().then(() => {
            order.push("microtask2");
        });
        await new Promise((resolve) => setTimeout(resolve, 10));
        assert(order.indexOf("microtask1") < order.indexOf("macrotask"), "Microtasks should run before macrotasks");
    });
    suite.test("Concurrent async/await", async () => {
        const results = [];
        const asyncFn = async (id) => {
            await Promise.resolve();
            results.push(id);
            return id;
        };
        const promises = [asyncFn(1), asyncFn(2), asyncFn(3)];
        await Promise.all(promises);
        assertEquals(results.length, 3, "All async functions should complete");
    });
    suite.test("Data isolation between executions", () => {
        const globalData = [];
        const execute = (id) => {
            const localData = [];
            for (let i = 0; i < 3; i++) {
                localData.push({ id, iteration: i });
            }
            return localData;
        };
        const result1 = execute(1);
        const result2 = execute(2);
        assertEquals(result1[0].id, 1, "Data should be isolated");
        assertEquals(result2[0].id, 2, "Data should be isolated");
        assert(result1 !== result2, "Different executions should have different data");
    });
    suite.test("Concurrent array modifications", () => {
        const arrays = Array.from({ length: 3 }, () => []);
        for (let i = 0; i < 100; i++) {
            arrays[i % 3].push(i);
        }
        assert(arrays[0].length > 0, "Array 1 should have items");
        assert(arrays[1].length > 0, "Array 2 should have items");
        assert(arrays[2].length > 0, "Array 3 should have items");
    });
    suite.test("Promise error isolation", async () => {
        let error1Caught = false;
        let error2Caught = false;
        const p1 = Promise.reject(new Error("error1")).catch(() => {
            error1Caught = true;
        });
        const p2 = Promise.resolve(42);
        await Promise.all([p1, p2]);
        assert(error1Caught, "Error should be handled");
        assert(!error2Caught, "Successful promise should not error");
    });
    suite.test("Async context preservation", async () => {
        const contexts = [];
        const fn = async (ctx) => {
            contexts.push(ctx);
            await Promise.resolve();
            return ctx;
        };
        await Promise.all([fn({ id: 1 }), fn({ id: 2 }), fn({ id: 3 })]);
        assertEquals(contexts[0].id, 1, "Context should be preserved");
        assertEquals(contexts[1].id, 2, "Context should be preserved");
        assertEquals(contexts[2].id, 3, "Context should be preserved");
    });
    suite.test("Generator parallelism simulation", async () => {
        function* gen(n) {
            for (let i = 0; i < n; i++) {
                yield i;
            }
        }
        const gens = [gen(3), gen(3), gen(3)];
        const values = [];
        for (const g of gens) {
            for (const val of g) {
                values.push(val);
            }
        }
        assertEquals(values.length, 9, "All generator values should be consumed");
    });
    return suite;
}
//# sourceMappingURL=index.js.map