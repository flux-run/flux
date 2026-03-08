// math.js - Performs basic arithmetic based on input
export default async function (req, ctx) {
  const { a = 1, b = 1, operation = "add" } = req.body ?? {};
  let result;
  switch (operation) {
    case "add":      result = a + b; break;
    case "subtract": result = a - b; break;
    case "multiply": result = a * b; break;
    case "divide":   result = b !== 0 ? a / b : "division by zero"; break;
    default:         result = "unknown operation";
  }
  return new Response(JSON.stringify({ operation, a, b, result }), {
    headers: { "Content-Type": "application/json" },
  });
}
