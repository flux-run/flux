Deno.serve((req) => {
  const url = new URL(req.url)

  if (url.pathname === '/hello') {
    return Response.json({
      message: 'hello',
      id: crypto.randomUUID(),
      time: Date.now()
    })
  }

  return new Response('not found', { status: 404 })
})
