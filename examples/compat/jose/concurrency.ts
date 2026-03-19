import { SignJWT, generateKeyPair } from "jose";

async function run() {
  console.log("Generating key pair...");
  const { publicKey, privateKey } = await generateKeyPair('ES256', { extractable: true });

  console.log("Executing concurrent signatures...");
  // Sign 5 times concurrently to guarantee promise pool order resolution holds determinism
  const tokens = await Promise.all([
    new SignJWT({ id: 1 }).setProtectedHeader({ alg: 'ES256' }).sign(privateKey),
    new SignJWT({ id: 2 }).setProtectedHeader({ alg: 'ES256' }).sign(privateKey),
    new SignJWT({ id: 3 }).setProtectedHeader({ alg: 'ES256' }).sign(privateKey),
    new SignJWT({ id: 4 }).setProtectedHeader({ alg: 'ES256' }).sign(privateKey),
    new SignJWT({ id: 5 }).setProtectedHeader({ alg: 'ES256' }).sign(privateKey)
  ]);

  return { tokens };
}
await run();
