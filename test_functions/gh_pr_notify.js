/**
 * Demo: GitHub webhook → Slack notification
 *
 * This flow triggers when a GitHub PR is opened and notifies the team on Slack.
 *
 * Setup:
 *   flux secrets set FLUXBASE_COMPOSIO_KEY <your-key>
 *   # Then connect GitHub + Slack in your Fluxbase dashboard → Integrations
 *
 * Deploy:
 *   flux deploy
 *
 * Test (simulate a GitHub PR webhook):
 *   curl -X POST https://gw.fluxbase.co/gh_pr_notify \
 *     -H "Authorization: Bearer <api-key>" \
 *     -d '{"action":"opened","pull_request":{"title":"Add dark mode","user":{"login":"shashi"},"html_url":"https://github.com/..."},"repository":{"full_name":"my-org/my-repo"}}'
 *
 * Trace:
 *   flux trace <request-id>
 *
 *   ▸ gateway:route              2ms
 *   ▸ function:gh_pr_notify     52ms
 *     ▸ tool:slack.send_message 48ms
 *   total: 54ms
 */

import { defineFunction } from "@fluxbase/functions";
import { z } from "zod";

export default defineFunction({
  name: "gh_pr_notify",
  description: "Notify Slack when a GitHub PR is opened",

  input: z.object({
    action:       z.string(),
    pull_request: z.object({
      title:    z.string(),
      html_url: z.string(),
      user:     z.object({ login: z.string() }),
    }),
    repository: z.object({ full_name: z.string() }),
  }),

  handler: async ({ input, ctx }) => {
    // Only notify on new PRs
    if (input.action !== "opened") {
      return { ignored: true, reason: `action=${input.action}` };
    }

    const { pull_request: pr, repository: repo } = input;

    ctx.log(`PR opened: ${pr.title} by ${pr.user.login}`);

    // Send Slack notification
    await ctx.tools.run("slack.send_message", {
      channel: "#dev",
      text: `🚀 *New PR* by *${pr.user.login}* in \`${repo.full_name}\`\n> *${pr.title}*\n${pr.html_url}`,
    });

    return {
      notified: true,
      pr_title: pr.title,
      author:   pr.user.login,
    };
  },
});
