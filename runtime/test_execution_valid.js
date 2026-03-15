const __secrets = {};
const __payload = { name: "Alice" };
const __ctx = { payload: __payload, env: __secrets, secrets: { get: () => null }, log: () => {} };

// Mock bundled code
var __flux_fn = {
    __flux: true,
    execute: async (payload, ctx) => {
        return { message: `Hello ${payload.name}` };
    }
};

(async () => {
    let result = await __flux_fn.execute(__payload, __ctx);
    console.log("SUCCESS:", result);
})();
