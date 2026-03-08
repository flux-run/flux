import { defineFunction } from '@fluxbase/functions'
import { z } from 'zod'

export default defineFunction({
  name: 'hello',
  input: z.object({ name: z.string() }),
  output: z.object({ message: z.string() }),
  handler: async ({ input }) => ({ message: 'Hello V1' })
})
