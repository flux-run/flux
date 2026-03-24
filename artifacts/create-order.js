export default async function (req) {
  console.log("Hello from executor MVP!");
  throw new Error("payment failed");
}
