export default async function (req) {
  console.log("Hello from executor MVP!");
  return new Response("Order Created", { status: 201 });
}
