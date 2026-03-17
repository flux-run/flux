import { createApp } from "./src/app.ts";
import { createPostgresTodoRepository, ensureSchema } from "./src/db.ts";

export async function createServerApp() {
  const repository = createPostgresTodoRepository();
  await ensureSchema(repository.sql);
  return createApp(repository);
}

if (import.meta.main) {
  const app = await createServerApp();
  Deno.serve({ port: Number(Deno.env.get("PORT") ?? "8000") }, app.fetch);
}
