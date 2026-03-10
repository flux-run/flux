/**
 * Fluxbase Demo: create_user
 *
 * Real function behind demo.fluxbase.co — triggered by the landing-page form.
 * Shows: gateway → function → workflow → outlook email, all traced.
 */

export default {
  __fluxbase: true,

  async execute(payload: any, ctx: any) {
    const name  = (payload && payload.name)  || "Developer";
    const email = (payload && payload.email) || "";

    if (!email || !email.includes("@")) {
      throw new Error("invalid_email");
    }

    ctx.log(`demo signup: ${name} <${email}>`, "info");

    await ctx.workflow.run([
      {
        name: "db.insert(users)",
        fn: async () => {
          // Simulate a DB write (no actual DB in demo, real write would use ctx.tools or data-engine)
          ctx.log(`db.insert(demo_users): ok`, "info");
          return { inserted: true };
        },
      },
      {
        name: "workflow.send_welcome",
        fn: async () => {
          const requestId = (ctx.env && ctx.env.REQUEST_ID) || (payload && payload.REQUEST_ID) || "unknown";
          await ctx.tools.run("outlook.send_email", {
            to:      email,
            subject: "You just ran a real backend trace on Fluxbase",
            body:    "Hello " + name + ",\n\nYou triggered a real backend on Fluxbase. Trace: " + requestId + "\n\n- The Fluxbase Team",
          });
          return { sent: true, to: email };
        },
      },
    ]);

    return { ok: true, email };
  },
};

