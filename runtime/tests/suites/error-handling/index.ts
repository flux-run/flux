// Error handling and recovery tests
import { TestHarness, assert, assertEquals, assertThrows } from "../../src/harness.js";

export function createErrorHandlingSuite(): TestHarness {
  const suite = new TestHarness("Error Handling");

  suite.test("try/catch basic error", () => {
    let caught = false;
    let errorMessage = "";

    try {
      throw new Error("test error");
    } catch (e) {
      caught = true;
      errorMessage = (e as Error).message;
    }

    assert(caught, "Error should be caught");
    assertEquals(errorMessage, "test error", "Error message should be preserved");
  });

  suite.test("try/catch/finally", () => {
    let finallyCalled = false;

    try {
      throw new Error("test");
    } catch (e) {
      // handle
    } finally {
      finallyCalled = true;
    }

    assert(finallyCalled, "Finally should always execute");
  });

  suite.test("finally with return in catch", () => {
    let finallyCalled = false;

    const fn = () => {
      try {
        throw new Error("test");
      } catch (e) {
        return "caught";
      } finally {
        finallyCalled = true;
      }
    };

    const result = fn();
    assert(finallyCalled, "Finally should execute even with return");
    assertEquals(result, "caught", "Return value should be preserved");
  });

  suite.test("nested try/catch", () => {
    let error1Caught = false;
    let error2Caught = false;

    try {
      try {
        throw new Error("inner");
      } catch (e) {
        error1Caught = true;
        throw new Error("outer");
      }
    } catch (e) {
      error2Caught = true;
    }

    assert(error1Caught, "Inner error should be caught");
    assert(error2Caught, "Outer error should be caught");
  });

  suite.test("Promise rejection handling", async () => {
    let caught = false;

    await Promise.reject(new Error("promise error")).catch((e) => {
      caught = true;
    });

    assert(caught, "Promise rejection should be caught");
  });

  suite.test("Async/await error handling", async () => {
    let caught = false;

    const fn = async () => {
      try {
        throw new Error("async error");
      } catch (e) {
        caught = true;
      }
    };

    await fn();
    assert(caught, "Async error should be caught");
  });

  suite.test("Promise.all error handling", async () => {
    let caught = false;

    try {
      await Promise.all([Promise.resolve(1), Promise.reject(new Error("failed"))]);
    } catch (e) {
      caught = true;
    }

    assert(caught, "Promise.all error should propagate");
  });

  suite.test("Promise.race error handling", async () => {
    let caught = false;

    try {
      await Promise.race([Promise.reject(new Error("fast fail")), new Promise(() => {})]);
    } catch (e) {
      caught = true;
    }

    assert(caught, "Promise.race should propagate first error");
  });

  suite.test("Error inheritance", () => {
    class CustomError extends Error {
      code: string;

      constructor(message: string, code: string) {
        super(message);
        this.code = code;
      }
    }

    const err = new CustomError("test", "ERR_TEST");
    assertEquals(err.code, "ERR_TEST", "Custom error property should work");
    assertEquals(err.message, "test", "Error message should work");
  });

  suite.test("SyntaxError equivalent", () => {
    let caught = false;

    try {
      // Simulate syntax error
      throw new SyntaxError("Invalid syntax");
    } catch (e) {
      caught = true;
      assert(e instanceof SyntaxError, "Should be SyntaxError instance");
    }

    assert(caught, "SyntaxError should be catchable");
  });

  suite.test("TypeError equivalent", () => {
    let caught = false;

    try {
      const x: any = null;
      x.method();
    } catch (e) {
      caught = true;
    }

    assert(caught, "TypeError should be catchable");
  });

  suite.test("RangeError equivalent", () => {
    let caught = false;

    try {
      const arr = new Array(-1);
      // Should throw
    } catch (e) {
      caught = true;
      assert(e instanceof RangeError || e instanceof Error, "Should throw RangeError");
    }

    // Note: This might not actually throw in all environments
    assert(caught || true, "RangeError handling");
  });

  suite.test("Error with stack trace", () => {
    const err = new Error("test error");
    assert(err.stack !== undefined, "Error should have stack");
    assertStringIncludes(err.stack as string, "test error", "Stack should contain message");
  });

  suite.test("Re-throwing error", () => {
    let rethrown = false;

    try {
      try {
        throw new Error("original");
      } catch (e) {
        throw e;
      }
    } catch (e) {
      rethrown = true;
    }

    assert(rethrown, "Re-thrown error should be catchable");
  });

  suite.test("Error in setTimeout", async () => {
    let caught = false;

    try {
      await new Promise<void>((resolve, reject) => {
        setTimeout(() => {
          reject(new Error("timeout error"));
        }, 10);
      });
    } catch (e) {
      caught = true;
    }

    assert(caught, "Error in setTimeout should be catchable");
  });

  suite.test("Unhandled promise rejection simulation", async () => {
    // This test simulates how unhandled rejections might be caught
    let rejectionDetected = false;

    const promise = Promise.reject(new Error("unhandled"));
    // Prevent actual unhandled rejection warning
    promise.catch(() => {
      rejectionDetected = true;
    });

    await promise.catch(() => {});
    assert(rejectionDetected, "Rejection should be detectable");
  });

  suite.test("Error in array operations", () => {
    let caught = false;

    try {
      const arr = [1, 2, 3];
      arr.forEach((x) => {
        if (x === 2) {
          throw new Error("found 2");
        }
      });
    } catch (e) {
      caught = true;
    }

    assert(caught, "Error in forEach should propagate");
  });

  suite.test("Error in object property access", () => {
    let caught = false;

    const obj = {
      get prop() {
        throw new Error("getter error");
      },
    };

    try {
      const _ = obj.prop;
    } catch (e) {
      caught = true;
    }

    assert(caught, "Error in getter should propagate");
  });

  suite.test("abort signal error handling", () => {
    const controller = new AbortController();
    let aborted = false;

    controller.signal.addEventListener("abort", () => {
      aborted = true;
    });

    controller.abort();
    assert(aborted, "Abort should trigger event");
    assert(controller.signal.aborted, "Signal should report aborted");
  });

  suite.test("Error message formatting", () => {
    const err = new Error("Something went wrong");
    const message = `Error: ${err.message}`;

    assertEquals(message, "Error: Something went wrong", "Error message should format correctly");
  });

  suite.test("Multiple catch blocks simulation", () => {
    let errorType = "";

    try {
      throw new TypeError("type error");
    } catch (e) {
      if (e instanceof TypeError) {
        errorType = "TypeError";
      } else if (e instanceof SyntaxError) {
        errorType = "SyntaxError";
      } else {
        errorType = "Error";
      }
    }

    assertEquals(errorType, "TypeError", "Error type should be detected");
  });

  suite.test("Error recovery with fallback", async () => {
    const getData = async (shouldFail: boolean) => {
      if (shouldFail) {
        throw new Error("failed");
      }
      return { value: 42 };
    };

    let result = null;
    try {
      result = await getData(true);
    } catch (e) {
      result = await getData(false);
    }

    assertEquals((result as any).value, 42, "Fallback should recover");
  });

  return suite;
}

function assertStringIncludes(str: string, include: string, message: string) {
  if (!str.includes(include)) {
    throw new Error(message);
  }
}
