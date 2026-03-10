/**
 * Demo: New user signup → multi-tool flow
 *
 * When a user signs up:
 *   1. Save to database (ctx.db — Phase 2 feature, mocked here)
 *   2. Send Slack notification to #signups
 *   3. Send welcome email via Gmail
 *   4. Create a Linear issue for onboarding follow-up
 *
 * This is the canonical Fluxbase demo — one function, multiple tools,
 * everything traced automatically.
 *
 * Trace output:
 *   ▸ gateway:route                  3ms
 *   ▸ function:user_signup          210ms
 *     ▸ tool:slack.send_message      47ms
 *     ▸ tool:gmail.send_email        89ms
 *     ▸ tool:linear.create_issue     71ms
 *   total: 213ms
 */

import { defineFunction } from "@fluxbase/functions";
import { z } from "zod";

export default defineFunction({
  name: "user_signup",
  description: "Handle new user signup — notify team + send welcome email",

  input: z.object({
    email:    z.string().email(),
    name:     z.string(),
    plan:     z.enum(["free", "pro", "team"]).default("free"),
    referrer: z.string().optional(),
  }),

  handler: async ({ input, ctx }) => {
    const { email, name, plan, referrer } = input;

    ctx.log(`New signup: ${name} (${email}) on ${plan} plan`);

    // Run all notifications in parallel — each is traced individually
    const [slackResult, emailResult, linearResult] = await Promise.all([

      // 1. Notify the team on Slack
      ctx.tools.run("slack.send_message", {
        channel: "#signups",
        text: `👋 *New signup!*\n*Name:* ${name}\n*Email:* ${email}\n*Plan:* ${plan}${referrer ? `\n*Referrer:* ${referrer}` : ""}`,
      }),

      // 2. Send the user a welcome email
      ctx.tools.run("gmail.send_email", {
        to:      email,
        subject: `Welcome to Fluxbase, ${name}!`,
        body:    `Hi ${name},\n\nYour account is ready. Deploy your first function in 30 seconds:\n\n  npm install -g @fluxbase/cli\n  flux login\n  flux init my-api && cd my-api\n  flux deploy\n\nQuestions? Reply to this email.\n\n— The Fluxbase Team`,
      }),

      // 3. Create a Linear issue to track onboarding
      ctx.tools.run("linear.create_issue", {
        title:       `Onboarding: ${name} (${email})`,
        description: `New ${plan} user signed up.\n\nEmail: ${email}\nPlan: ${plan}${referrer ? `\nReferrer: ${referrer}` : ""}`,
        teamId:      ctx.secrets.get("LINEAR_TEAM_ID") ?? "",
        labelIds:    ["onboarding"],
      }),

    ]);

    return {
      success: true,
      user:    { email, name, plan },
      actions: {
        slack_notified:       !!slackResult,
        welcome_email_sent:   !!emailResult,
        linear_issue_created: !!linearResult,
      },
    };
  },
});
