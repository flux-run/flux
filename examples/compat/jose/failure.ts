import { generateKeyPair, SignJWT, jwtVerify } from "jose";

async function run() {
  console.log("Generating ES384 pair...");
  const { publicKey, privateKey } = await generateKeyPair('ES384', { extractable: true });

  console.log("Generating invalid signature...");
  const valid_jwt = await new SignJWT({ secure: true })
    .setProtectedHeader({ alg: 'ES384' })
    .sign(privateKey);

  // Mangle the signature
  const mangled = valid_jwt.slice(0, -5) + "aaaaa";

  let errorCaptured = null;
  try {
     console.log("Verifying mangled JWT...");
     await jwtVerify(mangled, publicKey);
  } catch (err) {
     errorCaptured = err.name;
  }

  // Generate an expired token
  console.log("Generating expired JWT...");
  const expJwt = await new SignJWT({ secure: true })
    .setProtectedHeader({ alg: 'ES384' })
    .setExpirationTime(Math.floor(Date.now() / 1000) - 3600) // Expired 1 hr ago
    .sign(privateKey);

  let expCaptured = null;
  try {
     console.log("Verifying expired JWT...");
     await jwtVerify(expJwt, publicKey);
  } catch (err) {
     expCaptured = err.name;
  }

  return { errorCaptured, expCaptured };
}
await run();
