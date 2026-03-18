import { createApp } from "./src/app_flux.ts";
import { createDispatchRepository } from "./src/db_flux.ts";

export async function createServerApp() {
  const repository = createDispatchRepository();
  return createApp(repository);
}

if (import.meta.main) {
  const app = await createServerApp();
  Deno.serve(app.fetch);
}