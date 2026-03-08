import { defineFunction } from "@fluxbase/functions";
import { z } from "zod";

const Input = z.object({
  name: z.string(),
});

const Output = z.object({
  message: z.string(),
  timestamp: z.string(),
});

export default defineFunction({
  name: "hello",
  description: "Returns a greeting for the provided name",

  input: Input,
  output: Output,

  handler: async ({ input, ctx }) => {
    ctx.log(`Greeting ${input.name}`);

    return {
      message: `Hello ${input.name}!`,
      timestamp: new Date().toISOString(),
    };
  },
});
