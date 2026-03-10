/**
 * Demo: Phase 2 — Workflow Engine
 *
 * This function runs a multi-step user onboarding workflow.
 * Each step is named and traced independently:
 *
 *   ▸ workflow:validate          2ms
 *   ▸ workflow:notify_slack     45ms
 *   ▸ workflow:send_welcome     38ms
 *   ▸ workflow:create_ticket    61ms
 *
 * Connect apps in dashboard → Integrations before running.
 * Set secrets: FLUXBASE_COMPOSIO_KEY
 */

export default async function(ctx) {
  const result = await ctx.workflow.run([
    {
      name: "validate",
      fn: async (ctx, _prev) => {
        const { email, name } = ctx.payload;
        if (!email || !name) throw new Error("email and name are required");
        ctx.log(`Onboarding user: ${name} <${email}>`);
        return { email, name, validated: true };
      },
    },
    {
      name: "notify_slack",
      fn: async (ctx, prev) => {
        return await ctx.tools.run("slack.send_message", {
          channel: "#signups",
          text: `New user: ${prev.validate.name} (${prev.validate.email})`,
        });
      },
    },
    {
      name: "send_welcome",
      fn: async (ctx, prev) => {
        return await ctx.tools.run("gmail.send_email", {
          to:      prev.validate.email,
          subject: "Welcome to Fluxbase",
          body:    `Hi ${prev.validate.name}, your account is ready. Let's build something great.`,
        });
      },
    },
    {
      name: "create_ticket",
      fn: async (ctx, prev) => {
        return await ctx.tools.run("linear.create_issue", {
          title:       `Onboard: ${prev.validate.name}`,
          description: `New signup from ${prev.validate.email}. Trigger onboarding sequence.`,
          team:        "OPS",
        });
      },
    },
  ]);

  return {
    success: true,
    steps:   Object.keys(result),
    outputs: result,
  };
}
