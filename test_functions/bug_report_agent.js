/**
 * Demo: Phase 3 — Agent Runtime
 *
 * This function gives the agent a goal and lets it autonomously decide
 * which tools to call and in what order.
 *
 * Trace output:
 *   ▸ agent:step=1  312ms  tool=linear.create_issue
 *   ▸ tool:linear.create_issue  289ms
 *   ▸ agent:step=2  298ms  tool=slack.send_message
 *   ▸ tool:slack.send_message  41ms
 *   ▸ agent:step=3  188ms  [done]
 *
 * Required secrets:
 *   FLUXBASE_LLM_KEY      — OpenAI API key
 *   FLUXBASE_COMPOSIO_KEY — Composio API key (for tool execution)
 *
 * Optional:
 *   FLUXBASE_LLM_MODEL — model name (default: gpt-4o-mini)
 *   FLUXBASE_LLM_URL   — endpoint (default: OpenAI)
 */

export default async function(ctx) {
  const { title, description, priority = "medium" } = ctx.payload;

  const result = await ctx.agent.run({
    goal: `
      A new bug has been reported:
      Title: "${title}"
      Description: "${description}"
      Priority: ${priority}

      Please:
      1. Create a Linear issue for this bug
      2. Send a Slack message to #bugs announcing it
    `,
    tools:    ["linear.create_issue", "slack.send_message"],
    maxSteps: 6,
  });

  return {
    success: true,
    answer:  result.answer,
    steps:   result.steps,
    output:  result.output,
  };
}
