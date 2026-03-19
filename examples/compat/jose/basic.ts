import { SignJWT, jwtVerify } from "jose";

export default async function (req: any) {
  const secret = new TextEncoder().encode("cc7e0d44fd473002f1c42167459001140ec6389b7353f8088f4d9a95f2f596f2");
  const rand = crypto.randomUUID();

  console.log("Generating simple JWT...");
  const alg = 'HS256';

  const jwt = await new SignJWT({ random: rand })
    .setProtectedHeader({ alg })
    .setIssuedAt()
    .setIssuer('urn:flux:issuer')
    .setAudience('urn:flux:audience')
    .setExpirationTime('2h')
    .sign(secret);

  console.log("Validating generated JWT...");
  const { payload, protectedHeader } = await jwtVerify(jwt, secret, {
    issuer: 'urn:flux:issuer',
    audience: 'urn:flux:audience',
  });

  return new Response(JSON.stringify({ payload, protectedHeader }), {
    headers: { "Content-Type": "application/json" }
  });
}
