// Runtime stress and torture tests
import { TestHarness, assert, assertEquals } from "../harness.js";
import { EventEmitter } from "events";
export function createRuntimeSuite() {
    const suite = new TestHarness("Runtime");
    suite.test("Heavy object creation", () => {
        const objects = [];
        for (let i = 0; i < 1000; i++) {
            objects.push({ id: i, data: `item-${i}`, nested: { value: i * 2 } });
        }
        assertEquals(objects.length, 1000, "Should create many objects");
        assertEquals(objects[999].id, 999, "Last object should be correct");
    });
    suite.test("Deep object nesting", () => {
        let obj = { value: 0 };
        let current = obj;
        for (let i = 1; i < 100; i++) {
            current.next = { value: i };
            current = current.next;
        }
        assertEquals(obj.next?.next?.next?.value, 3, "Deep nesting should work");
    });
    suite.test("Array manipulation", () => {
        const arr = Array.from({ length: 1000 }, (_, i) => i);
        const filtered = arr.filter((x) => x % 2 === 0);
        const mapped = filtered.map((x) => x * 2);
        const sum = mapped.reduce((a, b) => a + b, 0);
        assertEquals(filtered.length, 500, "Filter should work");
        assert(sum > 0, "Sum should be positive");
    });
    suite.test("String manipulation", () => {
        let str = "a";
        for (let i = 0; i < 10; i++) {
            str += str;
        }
        assert(str.length > 1000, "String concatenation should work");
    });
    suite.test("Regular expressions", () => {
        const pattern = /[0-9]+/g;
        const text = "abc123def456ghi789";
        const matches = text.match(pattern);
        assertEquals(matches?.length, 3, "Regex should find all matches");
    });
    suite.test("Promise chain", async () => {
        let result = 0;
        await Promise.resolve(1)
            .then((x) => {
            result += x;
            return x + 1;
        })
            .then((x) => {
            result += x;
            return x + 1;
        })
            .then((x) => {
            result += x;
        });
        assertEquals(result, 6, "Promise chain should execute");
    });
    suite.test("Multiple event listeners", () => {
        const emitter = new EventEmitter();
        let count = 0;
        for (let i = 0; i < 100; i++) {
            emitter.on("test", () => {
                count++;
            });
        }
        emitter.emit("test");
        assertEquals(count, 100, "All listeners should execute");
    });
    suite.test("Timer precision", async () => {
        const times = [];
        const start = Date.now();
        for (let i = 0; i < 5; i++) {
            await new Promise((resolve) => setTimeout(resolve, 10));
            times.push(Date.now() - start);
        }
        assert(times.length === 5, "Should record 5 timestamps");
        assert(times[4] >= 40, "Total time should be at least 40ms");
    });
    suite.test("Callback nesting", () => {
        let result = 0;
        const callback1 = (fn) => {
            result += 1;
            fn();
        };
        const callback2 = (fn) => {
            result += 2;
            fn();
        };
        const callback3 = (fn) => {
            result += 3;
            fn();
        };
        callback1(() => {
            callback2(() => {
                callback3(() => {
                    result += 4;
                });
            });
        });
        assertEquals(result, 10, "Nested callbacks should work");
    });
    suite.test("Large array operations", () => {
        const size = 10000;
        const arr = Array.from({ length: size }, (_, i) => i);
        const result = arr
            .filter((x) => x % 2 === 0)
            .map((x) => x * 2)
            .slice(0, 100)
            .reduce((a, b) => a + b, 0);
        assert(result > 0, "Large array operations should work");
    });
    suite.test("Object property access", () => {
        const obj = {};
        for (let i = 0; i < 1000; i++) {
            obj[`prop${i}`] = i;
        }
        const sum = Object.keys(obj).reduce((acc, key) => acc + obj[key], 0);
        assertEquals(Object.keys(obj).length, 1000, "Should have 1000 properties");
        assert(sum > 0, "Sum should be positive");
    });
    suite.test("JSON serialization", () => {
        const data = Array.from({ length: 100 }, (_, i) => ({
            id: i,
            name: `item${i}`,
            data: Array.from({ length: 10 }, (_, j) => j),
        }));
        const json = JSON.stringify(data);
        const parsed = JSON.parse(json);
        assertEquals(parsed.length, 100, "Should parse array");
        assertEquals(parsed[0].id, 0, "Should parse nested objects");
    });
    suite.test("Map/Set operations", () => {
        const map = new Map();
        const set = new Set();
        for (let i = 0; i < 1000; i++) {
            map.set(`key${i}`, i);
            set.add(i);
        }
        assertEquals(map.size, 1000, "Map size should be correct");
        assertEquals(set.size, 1000, "Set size should be correct");
    });
    suite.test("Error handling loop", () => {
        let errorCount = 0;
        for (let i = 0; i < 100; i++) {
            try {
                if (i % 10 === 0) {
                    throw new Error(`Error ${i}`);
                }
            }
            catch (e) {
                errorCount++;
            }
        }
        assertEquals(errorCount, 10, "Should catch all errors");
    });
    suite.test("Multiple setTimeout precision", async () => {
        let count = 0;
        const promises = [];
        for (let i = 0; i < 10; i++) {
            promises.push(new Promise((resolve) => {
                setTimeout(() => {
                    count++;
                    resolve(null);
                }, 5);
            }));
        }
        await Promise.all(promises);
        assertEquals(count, 10, "All timeouts should fire");
    });
    suite.test("Async/await loop", async () => {
        let sum = 0;
        for (let i = 0; i < 10; i++) {
            sum += await Promise.resolve(i);
        }
        assertEquals(sum, 45, "Async loop should work");
    });
    suite.test("Closure capture", () => {
        const functions = [];
        for (let i = 0; i < 5; i++) {
            functions.push(() => i);
        }
        const results = functions.map((fn) => fn());
        assertEquals(results[0], 0, "Closures should capture correctly");
        assertEquals(results[4], 4, "Closures should capture correctly");
    });
    suite.test("Recursive function", () => {
        const factorial = (n) => {
            if (n <= 1)
                return 1;
            return n * factorial(n - 1);
        };
        assertEquals(factorial(5), 120, "Recursion should work");
        assertEquals(factorial(10), 3628800, "Recursion should handle larger numbers");
    });
    suite.test("Generator with loop", () => {
        function* gen(max) {
            for (let i = 0; i < max; i++) {
                yield i * 2;
            }
        }
        const values = Array.from(gen(10));
        assertEquals(values.length, 10, "Generator should produce correct length");
        assertEquals(values[5], 10, "Generator should produce correct values");
    });
    suite.test("Symbol uniqueness in loop", () => {
        const symbols = [];
        for (let i = 0; i < 100; i++) {
            symbols.push(Symbol(`sym${i}`));
        }
        const uniqueSymbols = new Set(symbols);
        assertEquals(uniqueSymbols.size, 100, "All symbols should be unique");
    });
    suite.test("WeakMap behavior", () => {
        const wm = new WeakMap();
        const obj = { id: 1 };
        wm.set(obj, "value");
        assertEquals(wm.get(obj), "value", "WeakMap should work");
    });
    return suite;
}
//# sourceMappingURL=index.js.map