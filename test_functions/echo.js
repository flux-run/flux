export default async function (ctx) {
  return {
    echo: ctx.payload,
    timestamp: new Date().toISOString(),
  };
}
