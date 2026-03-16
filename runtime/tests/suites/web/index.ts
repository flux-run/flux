// Web API tests (fetch, URL, Headers, Response)
import { TestHarness, assert, assertEquals, assertThrows } from "../../src/harness.js";

export function createWebSuite(): TestHarness {
  const suite = new TestHarness("Web APIs");

  suite.test("URL - basic parsing", () => {
    const url = new URL("https://example.com/path?q=1#hash");
    assertEquals(url.protocol, "https:", "URL.protocol should work");
    assertEquals(url.hostname, "example.com", "URL.hostname should work");
    assertEquals(url.pathname, "/path", "URL.pathname should work");
  });

  suite.test("URL.searchParams - get/set", () => {
    const url = new URL("https://example.com");
    url.searchParams.set("key", "value");
    assertEquals(url.searchParams.get("key"), "value", "searchParams.set/get should work");
  });

  suite.test("URL.searchParams - has/delete", () => {
    const url = new URL("https://example.com?key=value");
    assert(url.searchParams.has("key"), "searchParams.has should work");
    url.searchParams.delete("key");
    assert(!url.searchParams.has("key"), "searchParams.delete should work");
  });

  suite.test("TextEncoder/TextDecoder", () => {
    const encoder = new TextEncoder();
    const encoded = encoder.encode("hello");
    assertEquals(encoded.length, 5, "TextEncoder should work");

    const decoder = new TextDecoder();
    const decoded = decoder.decode(encoded);
    assertEquals(decoded, "hello", "TextDecoder should work");
  });

  suite.test("Headers - set/get", () => {
    const headers = new Headers();
    headers.set("content-type", "application/json");
    assertEquals(headers.get("content-type"), "application/json", "Headers.set/get should work");
  });

  suite.test("Headers - has/delete", () => {
    const headers = new Headers({ "x-custom": "value" });
    assert(headers.has("x-custom"), "Headers.has should work");
    headers.delete("x-custom");
    assert(!headers.has("x-custom"), "Headers.delete should work");
  });

  suite.test("Headers - iteration", () => {
    const headers = new Headers({ "x-a": "1", "x-b": "2" });
    let count = 0;
    headers.forEach(() => {
      count++;
    });
    assertEquals(count, 2, "Headers.forEach should work");
  });

  suite.test("Blob - constructor", () => {
    const blob = new Blob(["hello", " ", "world"]);
    assertEquals(blob.size, 11, "Blob size should be correct");
    assertEquals(blob.type, "", "Blob type should default to empty");
  });

  suite.test("Blob - with type", () => {
    const blob = new Blob(["test"], { type: "text/plain" });
    assertEquals(blob.type, "text/plain", "Blob type should be set");
  });

  suite.test("Blob - text method", async () => {
    const blob = new Blob(["hello world"]);
    const text = await blob.text();
    assertEquals(text, "hello world", "Blob.text should work");
  });

  suite.test("Blob - arrayBuffer method", async () => {
    const blob = new Blob(["hello"]);
    const buffer = await blob.arrayBuffer();
    assertEquals(buffer.byteLength, 5, "Blob.arrayBuffer should work");
  });

  suite.test("FormData - basic", () => {
    const fd = new FormData();
    fd.append("field", "value");
    assertEquals(fd.get("field"), "value", "FormData.get should work");
  });

  suite.test("FormData - has/delete", () => {
    const fd = new FormData();
    fd.append("field", "value");
    assert(fd.has("field"), "FormData.has should work");
    fd.delete("field");
    assert(!fd.has("field"), "FormData.delete should work");
  });

  suite.test("URLSearchParams - basic", () => {
    const params = new URLSearchParams("key=value&foo=bar");
    assertEquals(params.get("key"), "value", "URLSearchParams.get should work");
  });

  suite.test("URLSearchParams - set/append", () => {
    const params = new URLSearchParams();
    params.set("a", "1");
    params.append("b", "2");
    assertEquals(params.get("a"), "1", "URLSearchParams.set should work");
    assertEquals(params.get("b"), "2", "URLSearchParams.append should work");
  });

  suite.test("AbortController - signal", () => {
    const controller = new AbortController();
    assert(!controller.signal.aborted, "Signal should not be aborted initially");
    controller.abort();
    assert(controller.signal.aborted, "Signal should be aborted");
  });

  suite.test("AbortController - abort event", () => {
    const controller = new AbortController();
    let aborted = false;
    controller.signal.addEventListener("abort", () => {
      aborted = true;
    });
    controller.abort();
    assert(aborted, "Abort event should fire");
  });

  suite.test("atob/btoa - base64", () => {
    const encoded = btoa("hello world");
    const decoded = atob(encoded);
    assertEquals(decoded, "hello world", "atob/btoa should work");
  });

  suite.test("Request - constructor", () => {
    const req = new Request("https://example.com", { method: "POST" });
    assertEquals(req.url, "https://example.com", "Request.url should work");
    assertEquals(req.method, "POST", "Request.method should work");
  });

  suite.test("Response - constructor", () => {
    const res = new Response("hello");
    assertEquals(res.status, 200, "Response.status should default to 200");
    assertEquals(res.statusText, "OK", "Response.statusText should work");
  });

  suite.test("Response - with status", () => {
    const res = new Response("error", { status: 404 });
    assertEquals(res.status, 404, "Response.status should be set");
  });

  suite.test("Response - text method", async () => {
    const res = new Response("hello world");
    const text = await res.text();
    assertEquals(text, "hello world", "Response.text should work");
  });

  suite.test("Response - json method", async () => {
    const data = { key: "value" };
    const res = new Response(JSON.stringify(data));
    const json = (await res.json()) as any;
    assertEquals(json.key, "value", "Response.json should work");
  });

  suite.test("Response - arrayBuffer method", async () => {
    const res = new Response("test");
    const buffer = await res.arrayBuffer();
    assertEquals(buffer.byteLength, 4, "Response.arrayBuffer should work");
  });

  suite.test("Response - headers", () => {
    const res = new Response("test", {
      headers: { "content-type": "text/plain" },
    });
    assertEquals(res.headers.get("content-type"), "text/plain", "Response headers should work");
  });

  suite.test("Response.ok property", () => {
    const res200 = new Response("ok", { status: 200 });
    assert(res200.ok, "Response with 200 should have ok=true");

    const res404 = new Response("not found", { status: 404 });
    assert(!res404.ok, "Response with 404 should have ok=false");
  });

  suite.test("Response - clone", () => {
    const res = new Response("test");
    const cloned = res.clone();
    assertEquals(res.status, cloned.status, "Cloned response should have same status");
  });

  return suite;
}
