import { defineFunction } from "@fluxbase/functions";

export default defineFunction({
  name: "hello",
  handler: async ({ ctx, payload }) => {
    ctx.log("Running hello");

    // ctx.db.<table>.find({ where: ... })   — query your database
    // ctx.secrets.MY_SECRET                   — read a secret
    // ctx.functions.<other>()                 — call another function

    return {
      ok: true,
    };
  },
});
