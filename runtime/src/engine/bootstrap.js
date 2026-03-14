// Concurrent V8 worker bootstrap.
// Defines __flux_run_task and starts the persistent bootstrap loop.
// Runs in the V8 context: Deno.core.ops.* is available, no imports needed.

async function __flux_run_task(task) {
    var __fluxbase_logs = [];

    // Per-task seeded PRNG (same algorithm as build_wrapper in executor.rs)
    // Defensive BigInt guard: serde_v8 may return i64s > Number.MAX_SAFE_INTEGER as BigInt.
    var __seed = typeof task.execution_seed === 'bigint'
        ? Number(task.execution_seed & BigInt(0xFFFFFFFF))
        : task.execution_seed;
    var __t = (__seed ^ 0xDEADBEEF) >>> 0;
    if (__t === 0) __t = 0x1;
    var __task_rand = function() {
        __t += 0x6D2B79F5;
        var r = Math.imul(__t ^ (__t >>> 15), 1 | __t);
        r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
        return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };

    var __payload = task.payload;
    var __secrets = task.secrets || {};

    // Per-task uuid/nanoid helpers that use the isolated __task_rand closure.
    // We do NOT override global Math.random: in concurrent mode multiple tasks
    // are in-flight simultaneously and overwriting a shared global would cause
    // a race — Task B's seed would clobber Task A's mid-execution.
    // Users who call Math.random() directly get V8's native PRNG (fast, non-seeded).
    // For deterministic replay use ctx.uuid() or ctx.nanoid() which are isolated.
    var __uuid = function() {
        var b = new Uint8Array(16);
        for (var i = 0; i < 16; i++) b[i] = Math.floor(__task_rand() * 256);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        var h = function(x) { return (x + 256).toString(16).slice(1); };
        return h(b[0])+h(b[1])+h(b[2])+h(b[3])+"-"+h(b[4])+h(b[5])+"-"+
               h(b[6])+h(b[7])+"-"+h(b[8])+h(b[9])+"-"+
               h(b[10])+h(b[11])+h(b[12])+h(b[13])+h(b[14])+h(b[15]);
    };
    var __nanoid = function(size) {
        size = size || 21;
        var abc = "useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";
        var id = "";
        for (var ni = 0; ni < size; ni++) id += abc[Math.floor(__task_rand() * abc.length)];
        return id;
    };

    var __ctx = {
        payload: __payload,
        env:     __secrets,

        // Deterministic per-request UUID/nanoid — isolated from other concurrent tasks.
        // Use these (not Math.random) for replay-safe ID generation.
        uuid:   function() { return __uuid(); },
        nanoid: function(size) { return __nanoid(size); },

        secrets: {
            get: function(key) {
                return __secrets[key] !== undefined ? __secrets[key] : null;
            },
        },

        log: function(message, level) {
            __fluxbase_logs.push({
                level:     level || "info",
                message:   String(message),
                span_type: "event",
                source:    "function",
            });
        },

        tools: {
            run: async function() {
                throw new Error("ctx.tools is not available in this runtime");
            },
        },

        workflow: {
            run: async function(steps, options) {
                options = options || {};
                var outputs = {};
                for (var i = 0; i < steps.length; i++) {
                    var step = steps[i];
                    var name = step.name || ("step_" + Object.keys(outputs).length);
                    var _start = Date.now();
                    try {
                        var result = await step.fn(__ctx, outputs);
                        var duration = Date.now() - _start;
                        __fluxbase_logs.push({
                            level:     "info",
                            message:   "workflow:" + name + "  " + duration + "ms",
                            span_type: "workflow_step",
                            source:    "workflow",
                        });
                        outputs[name] = result;
                    } catch (e) {
                        var dur = Date.now() - _start;
                        __fluxbase_logs.push({
                            level:     "error",
                            message:   "workflow:" + name + "  failed (" + dur + "ms): " + (e && e.message),
                            span_type: "workflow_step",
                            source:    "workflow",
                        });
                        if (options.continueOnError) {
                            outputs[name] = { __error: e && e.message };
                        } else {
                            throw e;
                        }
                    }
                }
                return outputs;
            },

            parallel: async function(steps) {
                var settled = await Promise.allSettled(steps.map(function(step) {
                    var name = step.name || "step";
                    var _start = Date.now();
                    return step.fn(__ctx).then(function(result) {
                        var duration = Date.now() - _start;
                        __fluxbase_logs.push({
                            level:     "info",
                            message:   "workflow:" + name + "  " + duration + "ms (parallel)",
                            span_type: "workflow_step",
                            source:    "workflow",
                        });
                        return result;
                    });
                }));
                var outputs = {};
                settled.forEach(function(r, i) {
                    var name = (steps[i] && steps[i].name) ? steps[i].name : ("step_" + i);
                    outputs[name] = r.status === "fulfilled"
                        ? r.value
                        : { __error: r.reason && r.reason.message };
                });
                return outputs;
            },
        },

        queue: {
            push: async function(functionName, payload, opts) {
                opts = opts || {};
                var delay = opts.delay
                    ? (function() {
                        var _d = String(opts.delay);
                        if (_d.endsWith("h")) return parseInt(_d) * 3600;
                        if (_d.endsWith("m")) return parseInt(_d) * 60;
                        if (_d.endsWith("s")) return parseInt(_d);
                        return parseInt(_d);
                    })()
                    : (opts.delay_seconds || null);
                var result = await Deno.core.ops.op_queue_push(
                    functionName,
                    payload !== undefined ? payload : {},
                    {
                        delay_seconds:   delay,
                        idempotency_key: opts.idempotencyKey || opts.idempotency_key || null,
                    },
                    {
                        queue_url:     task.queue_url,
                        api_url:       task.api_url,
                        service_token: task.service_token,
                        project_id:    task.project_id || null,
                    }
                );
                __fluxbase_logs.push({
                    level:     "info",
                    message:   "queue_push:" + functionName + "  job_id=" + (result && result.job_id),
                    span_type: "queue_push",
                    source:    "queue",
                });
                return result;
            },
        },

        db: {
            query: async function(sql, params) {
                var _start = Date.now();
                var result = await Deno.core.ops.op_db_query(
                    sql,
                    Array.isArray(params) ? params : [],
                    {
                        data_engine_url: task.data_engine_url,
                        service_token:   task.service_token,
                        database:        task.database,
                        request_id:      task.request_id,
                    }
                );
                __fluxbase_logs.push({
                    level:       "info",
                    message:     "db:query  " + (Date.now() - _start) + "ms  " + (result && result.meta ? result.meta.rows + " rows" : ""),
                    span_type:   "db_query",
                    source:      "db",
                    duration_ms: Date.now() - _start,
                });
                return result && result.data ? result.data : result;
            },
            execute: async function(sql, params) {
                return __ctx.db.query(sql, params);
            },
        },

        // SSRF-protected HTTP — blocks RFC1918, loopback, link-local, metadata endpoints.
        fetch: async function(url, opts) {
            var _start = Date.now();
            var result = await Deno.core.ops.op_http_fetch(
                url,
                opts || {},
                { request_id: task.request_id }
            );
            __fluxbase_logs.push({
                level:       "info",
                message:     "http:" + (opts && opts.method || "GET") + "  " + url + "  " + result.status + "  " + (Date.now() - _start) + "ms",
                span_type:   "http_fetch",
                source:      "http",
                duration_ms: Date.now() - _start,
            });
            return result;
        },

        // ctx.sleep(ms) — yields the event loop; other concurrent tasks run while sleeping.
        sleep: async function(ms) {
            await Deno.core.ops.op_sleep(ms | 0);
        },

        // ctx.function.invoke(name, payload) — call another Flux function in-process.
        function: {
            invoke: async function(name, payload) {
                var _start = Date.now();
                var result = await Deno.core.ops.op_function_invoke(
                    name,
                    payload !== undefined ? payload : {},
                    {
                        runtime_url:   task.runtime_url || "",
                        service_token: task.service_token,
                        request_id:    task.request_id,
                    }
                );
                __fluxbase_logs.push({
                    level:       "info",
                    message:     "invoke:" + name + "  " + (Date.now() - _start) + "ms",
                    span_type:   "function_invoke",
                    source:      "function",
                    duration_ms: Date.now() - _start,
                });
                return result;
            },
        },
    };

    // Extract the user function in an isolated scope so concurrent tasks don't conflict
    var targetFn = new Function("var __fluxbase_fn;\n" + task.code + "\nreturn __fluxbase_fn;")();

    // esbuild wraps the default export under .default
    if (targetFn && targetFn.default) {
        targetFn = targetFn.default;
    }

    var __result;
    if (typeof targetFn === "object" && targetFn !== null && targetFn.__fluxbase === true) {
        try {
            __result = await targetFn.execute(__payload, __ctx);
        } catch (e) {
            var errCode = e.code || "EXECUTION_ERROR";
            throw new Error(JSON.stringify({ code: errCode, message: e.message }));
        }
    } else if (typeof targetFn === "function") {
        __result = await targetFn(__ctx);
    } else {
        throw new Error(
            "Bundle must export a defineFunction() result or an async function. Got: " + typeof targetFn
        );
    }

    Deno.core.ops.op_task_complete(
        task.request_id,
        JSON.stringify({ result: __result, logs: __fluxbase_logs })
    );
}

(async function __flux_bootstrap() {
    for (;;) {
        var task = await Deno.core.ops.op_next_task();
        __flux_run_task(task).catch(function(e) {
            try {
                Deno.core.ops.op_task_error(task.request_id, (e && e.message) || String(e));
            } catch (_) {}
        });
    }
})();
