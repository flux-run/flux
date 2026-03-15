import { defineFunction } from "@flux/functions"

export default defineFunction({
  name: "hello",
  description: "TODO: describe what hello does",

  handler: async ({ input, ctx }) => {
    ctx.log("hello invoked")

    // ── Database ─────────────────────────────────────────────────────────────
    // Raw SQL (full control):
    //   const rows = await ctx.db.query("SELECT * FROM users WHERE id = $1", [input.userId])
    //
    // ORM-style (generated types from `flux generate`):
    //   const user = await ctx.db.users.findOne({ id: input.userId })
    //   await ctx.db.orders.insert({ userId: user.id, status: "pending" })

    // ── Queue ────────────────────────────────────────────────────────────────
    //   await ctx.queue.push("send_welcome_email", { userId: "123" })
    //   await ctx.queue.push("charge_sub", { planId }, { delay: "24h" })

    // ── Cross-function calls ─────────────────────────────────────────────────
    //   const result = await ctx.function.invoke("validate_user", { id: input.userId })

    // ── HTTP (SSRF-protected) ────────────────────────────────────────────────
    //   const res = await ctx.fetch("https://api.stripe.com/v1/charges", {
    //     method:  "POST",
    //     headers: { Authorization: `Bearer ${ctx.secrets.get("STRIPE_KEY")}` },
    //     body:    JSON.stringify({ amount: 2000, currency: "usd" }),
    //   })
    //   const charge = await res.json()

    // ── Sleep ────────────────────────────────────────────────────────────────
    //   await ctx.sleep(1000) // 1 second — yields event loop, non-blocking

    // ── Replay-safe IDs ──────────────────────────────────────────────────────
    //   const id = ctx.uuid()       // deterministic per-request; safe for flux replay
    //   const slug = ctx.nanoid(10) // short random ID, also replay-safe

    // ── Secrets ──────────────────────────────────────────────────────────────
    //   const key = ctx.secrets.get("MY_SECRET")

    // ── Tools ────────────────────────────────────────────────────────────────
    //   await ctx.tools.run("slack.send_message", { channel: "#ops", text: "hello" })

    return { ok: true }
  },
})
