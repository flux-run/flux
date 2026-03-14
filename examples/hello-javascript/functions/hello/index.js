// @ts-check
import { defineFunction } from "@fluxbase/functions"

export default defineFunction({
  name: "hello",
  description: "TODO: describe what hello does",

  /** @param {{ input: unknown, ctx: import("@fluxbase/functions").FluxContext }} args */
  handler: async ({ input, ctx }) => {
    ctx.log("hello invoked")

    // ctx.db.<table>.find / findOne / insert / update / delete
    // ctx.secrets.get("MY_SECRET")
    // ctx.queue.push("email", { ... })
    // ctx.tools.run("slack.send_message", { ... })

    return { ok: true }
  },
})
