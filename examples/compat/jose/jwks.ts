import { generateKeyPair, exportJWK, importJWK, SignJWT, jwtVerify } from "jose";

async function run() {
  console.log("Generating asymmetric RS256 keys...");
  const { publicKey, privateKey } = await generateKeyPair('RS256', { extractable: true });

  console.log("Exporting to JWKS...");
  const publicJwk = await exportJWK(publicKey);
  const privateJwk = await exportJWK(privateKey);

  console.log("Re-importing keys...");
  const importedPrivate = await importJWK(privateJwk, 'RS256');
  const importedPublic = await importJWK(publicJwk, 'RS256');

  console.log("Signing payload...");
  const jwt = await new SignJWT({ data: "flux-determinism-jwk" })
    .setProtectedHeader({ alg: 'RS256' })
    .sign(importedPrivate);

  console.log("Verifying payload...");
  const { payload } = await jwtVerify(jwt, importedPublic);

  return { payload, publicJwk };
}
await run();
