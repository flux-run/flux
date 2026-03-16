export default async function handler(req) {
  return { message: "Hello, " + (req.name || "world") + "!" }
}
