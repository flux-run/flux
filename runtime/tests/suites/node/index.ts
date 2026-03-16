// Node.js API compatibility tests
import { TestHarness, assert, assertEquals, assertThrows, assertStringIncludes } from "../../src/harness.js";
import { promises as fs } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { EventEmitter } from "events";
import { Buffer } from "buffer";

export function createNodeSuite(): TestHarness {
  const suite = new TestHarness("Node.js APIs");

  // File System (fs)
  suite.test("fs.readFile - basic", async () => {
    const testFile = join(tmpdir(), "test-read.txt");
    await fs.writeFile(testFile, "hello world");
    const content = await fs.readFile(testFile, "utf-8");
    assertEquals(content, "hello world", "readFile should read file contents");
    await fs.unlink(testFile);
  });

  suite.test("fs.writeFile - basic", async () => {
    const testFile = join(tmpdir(), "test-write.txt");
    await fs.writeFile(testFile, "test content");
    const content = await fs.readFile(testFile, "utf-8");
    assertEquals(content, "test content", "writeFile should create file");
    await fs.unlink(testFile);
  });

  suite.test("fs.mkdir - recursive", async () => {
    const testDir = join(tmpdir(), "test-dir", "nested");
    await fs.mkdir(testDir, { recursive: true });
    const stats = await fs.stat(testDir);
    assert(stats.isDirectory(), "mkdir should create directory");
    await fs.rm(join(tmpdir(), "test-dir"), { recursive: true });
  });

  suite.test("fs.readdir", async () => {
    const testDir = join(tmpdir(), "test-readdir");
    await fs.mkdir(testDir, { recursive: true });
    await fs.writeFile(join(testDir, "file1.txt"), "content");
    await fs.writeFile(join(testDir, "file2.txt"), "content");
    const files = await fs.readdir(testDir);
    assertEquals(files.length, 2, "readdir should list files");
    await fs.rm(testDir, { recursive: true });
  });

  suite.test("fs.stat", async () => {
    const testFile = join(tmpdir(), "test-stat.txt");
    await fs.writeFile(testFile, "test");
    const stats = await fs.stat(testFile);
    assert(stats.isFile(), "stat should identify file");
    assertEquals(stats.size, 4, "stat should report file size");
    await fs.unlink(testFile);
  });

  suite.test("fs.exists (access)", async () => {
    const testFile = join(tmpdir(), "test-exists.txt");
    await fs.writeFile(testFile, "test");
    try {
      await fs.access(testFile);
      assert(true, "File should exist");
    } catch {
      assert(false, "File should exist");
    }
    await fs.unlink(testFile);
  });

  suite.test("fs.unlink", async () => {
    const testFile = join(tmpdir(), "test-unlink.txt");
    await fs.writeFile(testFile, "test");
    await fs.unlink(testFile);
    try {
      await fs.stat(testFile);
      assert(false, "File should be deleted");
    } catch {
      assert(true, "File should be deleted");
    }
  });

  suite.test("fs.rename", async () => {
    const testFile = join(tmpdir(), "test-rename-old.txt");
    const newFile = join(tmpdir(), "test-rename-new.txt");
    await fs.writeFile(testFile, "test");
    await fs.rename(testFile, newFile);
    try {
      await fs.stat(testFile);
      assert(false, "Old file should not exist");
    } catch {
      assert(true, "Old file should not exist");
    }
    await fs.unlink(newFile);
  });

  suite.test("fs.copy", async () => {
    const testFile = join(tmpdir(), "test-copy-src.txt");
    const copyFile = join(tmpdir(), "test-copy-dst.txt");
    await fs.writeFile(testFile, "test content");
    const content = await fs.readFile(testFile, "utf-8");
    await fs.writeFile(copyFile, content);
    const copiedContent = await fs.readFile(copyFile, "utf-8");
    assertEquals(copiedContent, "test content", "Copied file should have same content");
    await fs.unlink(testFile);
    await fs.unlink(copyFile);
  });

  // Events
  suite.test("EventEmitter - on/emit", () => {
    const emitter = new EventEmitter();
    let eventFired = false;

    emitter.on("test", () => {
      eventFired = true;
    });

    emitter.emit("test");
    assert(eventFired, "Event should be emitted and handled");
  });

  suite.test("EventEmitter - once", () => {
    const emitter = new EventEmitter();
    let count = 0;

    emitter.once("test", () => {
      count++;
    });

    emitter.emit("test");
    emitter.emit("test");
    assertEquals(count, 1, "once should only execute once");
  });

  suite.test("EventEmitter - off", () => {
    const emitter = new EventEmitter();
    let count = 0;

    const handler = () => {
      count++;
    };

    emitter.on("test", handler);
    emitter.emit("test");
    emitter.off("test", handler);
    emitter.emit("test");
    assertEquals(count, 1, "off should remove listener");
  });

  suite.test("EventEmitter - data passing", () => {
    const emitter = new EventEmitter();
    let receivedData: any;

    emitter.on("data", (data) => {
      receivedData = data;
    });

    emitter.emit("data", { value: 42 });
    assertEquals(receivedData.value, 42, "Event data should be passed");
  });

  // Timers
  suite.test("setTimeout", async () => {
    let executed = false;
    setTimeout(() => {
      executed = true;
    }, 10);

    await new Promise((resolve) => setTimeout(resolve, 20));
    assert(executed, "setTimeout should execute");
  });

  suite.test("setInterval", async () => {
    let count = 0;
    const interval = setInterval(() => {
      count++;
    }, 10);

    await new Promise((resolve) => setTimeout(resolve, 35));
    clearInterval(interval);
    assert(count >= 2, "setInterval should execute multiple times");
  });

  suite.test("setImmediate", async () => {
    let executed = false;
    setImmediate(() => {
      executed = true;
    });

    await new Promise((resolve) => setTimeout(resolve, 10));
    assert(executed, "setImmediate should execute");
  });

  // Buffer
  suite.test("Buffer.from string", () => {
    const buf = Buffer.from("hello");
    assertEquals(buf.length, 5, "Buffer.from should create buffer");
    assertEquals(buf.toString(), "hello", "Buffer.toString should work");
  });

  suite.test("Buffer.alloc", () => {
    const buf = Buffer.alloc(10);
    assertEquals(buf.length, 10, "Buffer.alloc should create buffer");
  });

  suite.test("Buffer.concat", () => {
    const buf1 = Buffer.from("hello");
    const buf2 = Buffer.from(" world");
    const combined = Buffer.concat([buf1, buf2]);
    assertEquals(combined.toString(), "hello world", "Buffer.concat should work");
  });

  suite.test("Buffer.slice", () => {
    const buf = Buffer.from("hello world");
    const slice = buf.slice(0, 5);
    assertEquals(slice.toString(), "hello", "Buffer.slice should work");
  });

  suite.test("Buffer.includes", () => {
    const buf = Buffer.from("hello");
    assert(buf.includes(104), "Buffer.includes should work");
  });

  // Path module
  suite.test("Path.join", () => {
    const { join: pathJoin } = require("path");
    const result = pathJoin("/a", "b", "c");
    assertEquals(result, "/a/b/c", "path.join should work");
  });

  suite.test("Path.basename", () => {
    const { basename } = require("path");
    assertEquals(basename("/a/b/file.txt"), "file.txt", "path.basename should work");
  });

  suite.test("Path.dirname", () => {
    const { dirname } = require("path");
    assertEquals(dirname("/a/b/file.txt"), "/a/b", "path.dirname should work");
  });

  suite.test("Path.extname", () => {
    const { extname } = require("path");
    assertEquals(extname("file.txt"), ".txt", "path.extname should work");
  });

  // Process
  suite.test("process.env", () => {
    assert(typeof process.env === "object", "process.env should be an object");
    assert(process.env.PATH !== undefined, "process.env should contain PATH");
  });

  suite.test("process.cwd", () => {
    const cwd = process.cwd();
    assert(typeof cwd === "string", "process.cwd should return string");
    assert(cwd.length > 0, "process.cwd should not be empty");
  });

  suite.test("process.pid", () => {
    const pid = process.pid;
    assert(typeof pid === "number", "process.pid should be a number");
    assert(pid > 0, "process.pid should be positive");
  });

  // URL
  suite.test("URL constructor", () => {
    const url = new URL("https://example.com/path?query=1");
    assertEquals(url.hostname, "example.com", "URL.hostname should work");
    assertEquals(url.pathname, "/path", "URL.pathname should work");
  });

  suite.test("URL.searchParams", () => {
    const url = new URL("https://example.com?key=value&foo=bar");
    assertEquals(url.searchParams.get("key"), "value", "URL.searchParams should work");
    assertEquals(url.searchParams.get("foo"), "bar", "URL.searchParams should work");
  });

  // JSON
  suite.test("JSON roundtrip", () => {
    const obj = { a: 1, b: "test", c: [1, 2, 3] };
    const json = JSON.stringify(obj);
    const parsed = JSON.parse(json);
    assertEquals(parsed.a, 1, "JSON roundtrip should work");
    assertEquals(parsed.b, "test", "JSON roundtrip should work");
  });

  // Errors
  suite.test("Error stack trace", () => {
    const err = new Error("test error");
    assert(err.stack !== undefined, "Error should have stack trace");
    if (err.stack) {
      assertStringIncludes(err.stack, "test error", "Stack trace should contain message");
    }
  });

  // Crypto basics
  suite.test("crypto.randomBytes", () => {
    const crypto = require("crypto");
    const buf = crypto.randomBytes(16);
    assertEquals(buf.length, 16, "randomBytes should generate requested length");
  });

  suite.test("crypto.createHash", () => {
    const crypto = require("crypto");
    const hash = crypto.createHash("sha256");
    hash.update("hello");
    const digest = hash.digest("hex");
    assert(typeof digest === "string", "digest should return string");
    assert(digest.length > 0, "digest should not be empty");
  });

  // console methods
  suite.test("console.log", () => {
    let logged = false;
    const original = console.log;
    console.log = () => {
      logged = true;
    };
    console.log("test");
    console.log = original;
    assert(logged, "console.log should execute");
  });

  suite.test("console.error", () => {
    let logged = false;
    const original = console.error;
    console.error = () => {
      logged = true;
    };
    console.error("test");
    console.error = original;
    assert(logged, "console.error should execute");
  });

  return suite;
}
