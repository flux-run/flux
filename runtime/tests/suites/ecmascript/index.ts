// ECMAScript language tests (Test262 subset)
import { TestHarness, assert, assertEquals, assertThrows } from "../../src/harness.js";

export function createECMAScriptSuite(): TestHarness {
  const suite = new TestHarness("ECMAScript");

  // Language features
  suite.test("arrow functions", () => {
    const add = (a: number, b: number) => a + b;
    assertEquals(add(2, 3), 5, "Arrow functions should work");
  });

  suite.test("destructuring - objects", () => {
    const obj = { a: 1, b: 2 };
    const { a, b } = obj;
    assertEquals(a, 1, "Object destructuring should work");
    assertEquals(b, 2, "Object destructuring should work");
  });

  suite.test("destructuring - arrays", () => {
    const arr = [1, 2, 3];
    const [x, y] = arr;
    assertEquals(x, 1, "Array destructuring should work");
    assertEquals(y, 2, "Array destructuring should work");
  });

  suite.test("template literals", () => {
    const name = "World";
    const greeting = `Hello, ${name}!`;
    assertEquals(greeting, "Hello, World!", "Template literals should work");
  });

  suite.test("spread operator", () => {
    const arr1 = [1, 2];
    const arr2 = [...arr1, 3, 4];
    assertEquals(arr2.length, 4, "Spread operator should work");
    assertEquals(arr2[2], 3, "Spread operator should work");
  });

  suite.test("rest parameters", () => {
    const sum = (...numbers: number[]) => numbers.reduce((a, b) => a + b, 0);
    assertEquals(sum(1, 2, 3, 4), 10, "Rest parameters should work");
  });

  suite.test("default parameters", () => {
    const greet = (name = "Guest") => `Hello, ${name}`;
    assertEquals(greet(), "Hello, Guest", "Default parameters should work");
    assertEquals(greet("Alice"), "Hello, Alice", "Default parameters should work");
  });

  // Promises
  suite.test("Promise resolution", async () => {
    const p = Promise.resolve(42);
    const result = await p;
    assertEquals(result, 42, "Promise should resolve");
  });

  suite.test("Promise rejection", async () => {
    const p = Promise.reject(new Error("test error"));
    let caught = false;
    try {
      await p;
    } catch (e) {
      caught = true;
    }
    assert(caught, "Promise rejection should be caught");
  });

  suite.test("Promise.all", async () => {
    const p1 = Promise.resolve(1);
    const p2 = Promise.resolve(2);
    const p3 = Promise.resolve(3);
    const result = await Promise.all([p1, p2, p3]);
    assertEquals(result.length, 3, "Promise.all should work");
    assertEquals(result[0], 1, "Promise.all should work");
  });

  suite.test("Promise.race", async () => {
    const p1 = new Promise((resolve) => setTimeout(() => resolve(1), 100));
    const p2 = Promise.resolve(2);
    const result = await Promise.race([p1, p2]);
    assertEquals(result, 2, "Promise.race should work");
  });

  // Async/Await
  suite.test("async function basics", async () => {
    const asyncFn = async () => {
      return 42;
    };
    const result = await asyncFn();
    assertEquals(result, 42, "Async functions should work");
  });

  suite.test("async/await with Promise", async () => {
    const fn = async () => {
      const result = await Promise.resolve(100);
      return result + 23;
    };
    const result = await fn();
    assertEquals(result, 123, "Async/await should work");
  });

  // Classes
  suite.test("class definition", () => {
    class Person {
      name: string;
      constructor(name: string) {
        this.name = name;
      }
      greet() {
        return `Hello, ${this.name}`;
      }
    }
    const person = new Person("Alice");
    assertEquals(person.greet(), "Hello, Alice", "Classes should work");
  });

  suite.test("class inheritance", () => {
    class Animal {
      name: string;
      constructor(name: string) {
        this.name = name;
      }
    }
    class Dog extends Animal {
      bark() {
        return `${this.name} barks!`;
      }
    }
    const dog = new Dog("Rex");
    assertEquals(dog.name, "Rex", "Class inheritance should work");
    assertEquals(dog.bark(), "Rex barks!", "Class inheritance should work");
  });

  suite.test("static methods", () => {
    class Math2 {
      static add(a: number, b: number) {
        return a + b;
      }
    }
    assertEquals(Math2.add(2, 3), 5, "Static methods should work");
  });

  // Map/Set
  suite.test("Map", () => {
    const map = new Map();
    map.set("a", 1);
    map.set("b", 2);
    assertEquals(map.get("a"), 1, "Map should work");
    assertEquals(map.size, 2, "Map should work");
  });

  suite.test("Set", () => {
    const set = new Set([1, 2, 2, 3]);
    assertEquals(set.size, 3, "Set should deduplicate");
    assert(set.has(1), "Set.has should work");
    assert(!set.has(4), "Set.has should work");
  });

  // Symbols
  suite.test("Symbol basics", () => {
    const sym1 = Symbol("test");
    const sym2 = Symbol("test");
    // Symbols are always unique, even with same description
    const obj: any = { [sym1]: "value1" };
    obj[sym2] = "value2";
    assertEquals(obj[sym1], "value1", "Symbols should be unique keys");
  });

  // Iterators and Generators
  suite.test("for...of loop", () => {
    const arr = [1, 2, 3];
    const results: number[] = [];
    for (const item of arr) {
      results.push(item);
    }
    assertEquals(results.length, 3, "for...of should work");
    assertEquals(results[0], 1, "for...of should work");
  });

  suite.test("generator functions", () => {
    function* gen() {
      yield 1;
      yield 2;
      yield 3;
    }
    const results = [...gen()];
    assertEquals(results.length, 3, "Generators should work");
    assertEquals(results[1], 2, "Generators should work");
  });

  // Error handling
  suite.test("try/catch", () => {
    let caught = false;
    try {
      throw new Error("test");
    } catch (e) {
      caught = true;
    }
    assert(caught, "try/catch should work");
  });

  suite.test("finally block", () => {
    let finallyExecuted = false;
    try {
      throw new Error("test");
    } catch (e) {
      // ignore
    } finally {
      finallyExecuted = true;
    }
    assert(finallyExecuted, "finally should execute");
  });

  // Type coercion
  suite.test("type coercion", () => {
    assertEquals("5" as any as number + 5, 10, "String to number coercion should work");
    assertEquals(true as any + 1, 2, "Boolean to number coercion should work");
  });

  // Object methods
  suite.test("Object.keys", () => {
    const obj = { a: 1, b: 2, c: 3 };
    const keys = Object.keys(obj);
    assertEquals(keys.length, 3, "Object.keys should work");
  });

  suite.test("Object.entries", () => {
    const obj = { a: 1, b: 2 };
    const entries = Object.entries(obj);
    assertEquals(entries.length, 2, "Object.entries should work");
    assertEquals(entries[0][0], "a", "Object.entries should work");
  });

  // Array methods
  suite.test("Array.map", () => {
    const arr = [1, 2, 3];
    const doubled = arr.map((x) => x * 2);
    assertEquals(doubled.length, 3, "Array.map should work");
    assertEquals(doubled[0], 2, "Array.map should work");
  });

  suite.test("Array.filter", () => {
    const arr = [1, 2, 3, 4, 5];
    const evens = arr.filter((x) => x % 2 === 0);
    assertEquals(evens.length, 2, "Array.filter should work");
  });

  suite.test("Array.reduce", () => {
    const arr = [1, 2, 3, 4];
    const sum = arr.reduce((acc, x) => acc + x, 0);
    assertEquals(sum, 10, "Array.reduce should work");
  });

  suite.test("Array.find", () => {
    const arr = [1, 2, 3, 4];
    const found = arr.find((x) => x > 2);
    assertEquals(found, 3, "Array.find should work");
  });

  suite.test("Array.some", () => {
    const arr = [1, 2, 3];
    const hasEven = arr.some((x) => x % 2 === 0);
    assert(hasEven, "Array.some should work");
  });

  suite.test("Array.every", () => {
    const arr = [2, 4, 6];
    const allEven = arr.every((x) => x % 2 === 0);
    assert(allEven, "Array.every should work");
  });

  // String methods
  suite.test("String.includes", () => {
    const str = "hello world";
    assert(str.includes("world"), "String.includes should work");
    assert(!str.includes("xyz"), "String.includes should work");
  });

  suite.test("String.startsWith", () => {
    const str = "hello";
    assert(str.startsWith("he"), "String.startsWith should work");
    assert(!str.startsWith("lo"), "String.startsWith should work");
  });

  suite.test("String.repeat", () => {
    const str = "ab";
    assertEquals(str.repeat(3), "ababab", "String.repeat should work");
  });

  suite.test("String.padStart", () => {
    const str = "5";
    assertEquals(str.padStart(3, "0"), "005", "String.padStart should work");
  });

  suite.test("String.match", () => {
    const str = "abc123def456";
    const matches = str.match(/\d+/g);
    assert(matches !== null, "String.match should work");
    assertEquals(matches?.length, 2, "String.match should work");
  });

  suite.test("String.replace", () => {
    const str = "hello world";
    const replaced = str.replace("world", "there");
    assertEquals(replaced, "hello there", "String.replace should work");
  });

  // Number methods
  suite.test("Number.isInteger", () => {
    assert(Number.isInteger(5), "Number.isInteger should work");
    assert(!Number.isInteger(5.5), "Number.isInteger should work");
  });

  suite.test("Number.isNaN", () => {
    assert(Number.isNaN(NaN), "Number.isNaN should work");
    assert(!Number.isNaN("hello"), "Number.isNaN should work");
  });

  suite.test("Number.parseFloat", () => {
    assertEquals(Number.parseFloat("3.14"), 3.14, "Number.parseFloat should work");
  });

  // Math object
  suite.test("Math.abs", () => {
    assertEquals(Math.abs(-5), 5, "Math.abs should work");
  });

  suite.test("Math.max", () => {
    assertEquals(Math.max(1, 5, 3), 5, "Math.max should work");
  });

  suite.test("Math.min", () => {
    assertEquals(Math.min(1, 5, 3), 1, "Math.min should work");
  });

  suite.test("Math.floor", () => {
    assertEquals(Math.floor(5.7), 5, "Math.floor should work");
  });

  suite.test("Math.ceil", () => {
    assertEquals(Math.ceil(5.1), 6, "Math.ceil should work");
  });

  suite.test("Math.round", () => {
    assertEquals(Math.round(5.5), 6, "Math.round should work");
  });

  suite.test("Math.sqrt", () => {
    assertEquals(Math.sqrt(16), 4, "Math.sqrt should work");
  });

  // JSON
  suite.test("JSON.stringify", () => {
    const obj = { a: 1, b: "test" };
    const json = JSON.stringify(obj);
    assert(json.includes("a"), "JSON.stringify should work");
  });

  suite.test("JSON.parse", () => {
    const json = '{"a":1,"b":"test"}';
    const obj = JSON.parse(json) as any;
    assertEquals(obj.a, 1, "JSON.parse should work");
    assertEquals(obj.b, "test", "JSON.parse should work");
  });

  // Regex
  suite.test("RegExp basics", () => {
    const regex = /hello/i;
    assert(regex.test("HELLO"), "RegExp.test should work");
    assert(!regex.test("goodbye"), "RegExp.test should work");
  });

  suite.test("RegExp.exec", () => {
    const regex = /(\d+)/;
    const result = regex.exec("abc123def");
    assert(result !== null, "RegExp.exec should work");
  });

  return suite;
}
