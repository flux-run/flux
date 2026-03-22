Deno.serve(async (req) => {
  console.log("Fetching example.com...");
  const res = await fetch("https://example.com");
  console.log("Fetched status:", res.status);
  return new Response(`Status: ${res.status}`);
});
