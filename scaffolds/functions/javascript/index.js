// @ts-check
import { defineFunction } from "@flux/functions"

export default defineFunction({
  name: "{name}",
  description: "TODO: describe what {name} does",

  /** @param {{ input: unknown, ctx: import("@flux/functions").FluxContext }} args */
  handler: async ({ input, ctx }) => {
    ctx.log("{name} invoked")

    // ctx.db.<table>.find / findOne / insert / update / delete
    // ctx.secrets.get("MY_SECRET")
    // ctx.queue.push("email", { ... })
    // ctx.tools.run("slack.send_message", { ... })

    return { ok: true }
  },
})
