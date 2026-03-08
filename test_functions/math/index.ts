import { defineFunction } from "@fluxbase/functions";
import { z } from "zod";

const Input = z.object({
  operation: z.enum(["add", "sub", "mul", "div"]),
  a: z.number(),
  b: z.number(),
});

const Output = z.object({
  result: z.number(),
  operation: z.string(),
  a: z.number(),
  b: z.number(),
});

export default defineFunction({
  name: "math",
  description: "Performs arithmetic: add, sub, mul, div on two numbers",

  input: Input,
  output: Output,

  handler: async ({ input, ctx }) => {
    ctx.log(`Computing ${input.a} ${input.operation} ${input.b}`);

    let result: number;
    switch (input.operation) {
      case "add":
        result = input.a + input.b;
        break;
      case "sub":
        result = input.a - input.b;
        break;
      case "mul":
        result = input.a * input.b;
        break;
      case "div":
        if (input.b === 0) throw new Error("Division by zero");
        result = input.a / input.b;
        break;
    }

    return { result, operation: input.operation, a: input.a, b: input.b };
  },
});
