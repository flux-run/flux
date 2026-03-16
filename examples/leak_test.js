export default async function handler(req) {
  if (globalThis.__count === undefined) globalThis.__count = 0
  globalThis.__count++
  return { count: globalThis.__count }
}
