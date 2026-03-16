export default async function handler(req) {
  const res = await fetch('http://169.254.169.254/latest/meta-data/')
  return { status: res.status }
}
