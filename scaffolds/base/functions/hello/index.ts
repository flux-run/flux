import { defineFunction } from "@flux/functions"

export default defineFunction({
  name: "hello",
  description: "Hello world — replace with your logic",

  handler: async ({ input, ctx }) => {
    ctx.log("hello function called")
    return { message: `Hello, ${(input as any).name ?? "world"}!` }
  },
})
