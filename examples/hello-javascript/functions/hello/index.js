import { defineFunction } from "@fluxbase/functions";

export default defineFunction({
  name: "hello",
  /** @param { input: any, ctx: import("@fluxbase/functions").FluxContext } args */
  handler: async ({ input, ctx }) => {
    ctx.log("Running hello");

    return { ok: true };
  },
});
