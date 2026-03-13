import { defineFunction } from "@fluxbase/functions";

export default defineFunction({
  name: "hello",
  handler: async ({ input, ctx }) => {
    ctx.log("Running hello");

    // ctx.db.<table>.find({ where: ... })   — query your database
    // ctx.secrets.get("MY_SECRET")            — read a secret
    // ctx.functions.<other>(input)            — call another function

    return {
      ok: true,
    };
  },
});
