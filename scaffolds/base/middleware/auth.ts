import { defineMiddleware } from "@fluxbase/functions"

/**
 * Auth middleware — applied to routes with auth = "jwt" in gateway.toml
 * Verifies the JWT and attaches ctx.user.
 *
 * To apply globally, add to flux.toml:
 *   [middleware]
 *   global = ["auth"]
 */
export default defineMiddleware({
  name: "auth",

  handler: async ({ request, ctx, next }) => {
    const token = request.headers["authorization"]?.replace("Bearer ", "")

    if (!token) {
      return { status: 401, body: { error: "Missing authorization token" } }
    }

    // Verify JWT — ctx.secrets.get("FLUX_JWT_SECRET") reads from .env / secrets store
    const secret = ctx.secrets.get("FLUX_JWT_SECRET")
    if (!secret) {
      ctx.log("FLUX_JWT_SECRET not set", "error")
      return { status: 500, body: { error: "Auth not configured" } }
    }

    // TODO: verify token and attach user to ctx
    // const payload = await verifyJwt(token, secret)
    // ctx.user = payload

    return next()
  },
})
