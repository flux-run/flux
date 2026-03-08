export default async function (ctx) {
  return {
    message: "Hello from Fluxbase!",
    timestamp: new Date().toISOString(),
  };
}
