import { defineFunction } from "@fluxbase/functions";

export default defineFunction({
  name: "echo",
  description: "Echoes the incoming payload back with a timestamp",

  // No input schema — accepts any payload
  // No output schema — mirrors input

  handler: async ({ ctx }) => {
    ctx.log("Echoing payload");

    return {
      echo: ctx.payload,
      timestamp: new Date().toISOString(),
    };
  },
});
