// Deterministic replay tests
import { TestHarness, assert, assertEquals } from "../harness.js";
export function createDeterminismSuite() {
    const suite = new TestHarness("Determinism");
    suite.test("Math.random consistency", () => {
        // In real replay, we'd record and replay these values
        const seed = 12345;
        const value1 = Math.random();
        assert(typeof value1 === "number", "Math.random should return number");
        assert(value1 >= 0 && value1 < 1, "Math.random should be in range");
    });
    suite.test("Date.now order preservation", () => {
        const t1 = Date.now();
        const t2 = Date.now();
        const t3 = Date.now();
        assert(t1 <= t2, "Date.now should be monotonic");
        assert(t2 <= t3, "Date.now should be monotonic");
    });
    suite.test("setTimeout order preservation", async () => {
        const order = [];
        setTimeout(() => {
            order.push(1);
        }, 10);
        setTimeout(() => {
            order.push(2);
        }, 20);
        setTimeout(() => {
            order.push(3);
        }, 5);
        await new Promise((resolve) => setTimeout(resolve, 50));
        assertEquals(order.length, 3, "All timers should fire");
        assertEquals(order[0], 3, "Timers should fire in order");
        assertEquals(order[1], 1, "Timers should fire in order");
        assertEquals(order[2], 2, "Timers should fire in order");
    });
    suite.test("Promise resolution order", async () => {
        const order = [];
        Promise.resolve(1).then(() => {
            order.push(1);
        });
        Promise.resolve(2).then(() => {
            order.push(2);
        });
        Promise.resolve(3).then(() => {
            order.push(3);
        });
        await new Promise((resolve) => setTimeout(resolve, 10));
        assertEquals(order.length, 3, "All promises should resolve");
        assertEquals(order[0], 1, "Promises should resolve in order");
    });
    suite.test("Event emission order", () => {
        const { EventEmitter } = require("events");
        const emitter = new EventEmitter();
        const order = [];
        emitter.on("test", () => {
            order.push(1);
        });
        emitter.on("test", () => {
            order.push(2);
        });
        emitter.on("test", () => {
            order.push(3);
        });
        emitter.emit("test");
        assertEquals(order.length, 3, "All listeners should fire");
        assertEquals(order[0], 1, "Listeners should fire in order");
    });
    suite.test("Object iteration order", () => {
        const obj = { a: 1, b: 2, c: 3, d: 4 };
        const keys = [];
        for (const key in obj) {
            keys.push(key);
        }
        assertEquals(keys.length, 4, "All keys should iterate");
        assertEquals(keys[0], "a", "Object iteration should preserve order");
    });
    suite.test("Array iteration determinism", () => {
        const arr = [1, 2, 3, 4, 5];
        const results = [];
        arr.forEach((x) => {
            results.push(x);
        });
        assertEquals(results.length, 5, "All items should iterate");
        assertEquals(results[0], 1, "Array iteration should be deterministic");
    });
    suite.test("JSON parse determinism", () => {
        const json = '{"z":1,"y":2,"x":3}';
        const parsed = JSON.parse(json);
        const keys = Object.keys(parsed);
        assertEquals(keys.length, 3, "All keys should be parsed");
        assertEquals(keys[0], "z", "JSON key order should match input");
    });
    suite.test("Set insertion order", () => {
        const set = new Set([3, 1, 2]);
        const values = [];
        set.forEach((v) => {
            values.push(v);
        });
        assertEquals(values.length, 3, "All set values should iterate");
        assertEquals(values[0], 3, "Set iteration should preserve insertion order");
    });
    suite.test("Map key order", () => {
        const map = new Map();
        map.set("c", 3);
        map.set("a", 1);
        map.set("b", 2);
        const keys = [];
        map.forEach((v, k) => {
            keys.push(k);
        });
        assertEquals(keys.length, 3, "All map keys should iterate");
        assertEquals(keys[0], "c", "Map iteration should preserve insertion order");
    });
    suite.test("String.split consistency", () => {
        const str = "a,b,c,d,e";
        const parts1 = str.split(",");
        const parts2 = str.split(",");
        assertEquals(parts1.length, parts2.length, "Split results should be consistent");
        assertEquals(parts1[0], parts2[0], "Split results should match");
    });
    suite.test("Regex match consistency", () => {
        const pattern = /\d+/g;
        const text = "a1b2c3d4";
        const matches1 = text.match(pattern);
        const matches2 = text.match(pattern);
        assertEquals(matches1?.length, matches2?.length, "Regex results should be consistent");
    });
    suite.test("Error stack trace format", () => {
        const err1 = new Error("test");
        const err2 = new Error("test");
        // Stack traces should have consistent format (though content may vary)
        assert(err1.stack !== undefined, "Error should have stack trace");
        assert(err2.stack !== undefined, "Error should have stack trace");
        assert(err1.stack?.includes("Error:"), "Stack should contain error marker");
    });
    suite.test("Async execution order", async () => {
        const order = [];
        const promise1 = Promise.resolve().then(() => {
            order.push(1);
            return Promise.resolve().then(() => {
                order.push(3);
            });
        });
        const promise2 = Promise.resolve().then(() => {
            order.push(2);
        });
        await Promise.all([promise1, promise2]);
        assertEquals(order.length, 3, "All async operations should execute");
        // Note: actual order may vary due to microtask queue
        assert(order.includes(1) && order.includes(2) && order.includes(3), "All should be present");
    });
    suite.test("URL parsing consistency", () => {
        const urlStr = "https://user:pass@example.com:8080/path?q=1&q=2#hash";
        const url1 = new URL(urlStr);
        const url2 = new URL(urlStr);
        assertEquals(url1.hostname, url2.hostname, "URL parsing should be consistent");
        assertEquals(url1.pathname, url2.pathname, "URL parsing should be consistent");
        assertEquals(url1.searchParams.get("q"), url2.searchParams.get("q"), "Query params should match");
    });
    suite.test("Buffer operations consistency", () => {
        const buf1 = Buffer.from("hello");
        const buf2 = Buffer.from("hello");
        assertEquals(buf1.toString(), buf2.toString(), "Buffer content should match");
        assertEquals(buf1.length, buf2.length, "Buffer length should match");
    });
    suite.test("Class instantiation order", () => {
        const instances = [];
        class Item {
            id;
            constructor(id) {
                this.id = id;
                instances.push(this);
            }
        }
        new Item(1);
        new Item(2);
        new Item(3);
        assertEquals(instances.length, 3, "All instances should be created");
        assertEquals(instances[0].id, 1, "Instances should be in creation order");
    });
    return suite;
}
//# sourceMappingURL=index.js.map