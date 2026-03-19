# WebCrypto Determinism Guarantees

This directory (`compat-jose`) serves as the **Golden Benchmark** for Flux's deterministic execution layer securely interacting with real-world cryptographic libraries. 

This is the canary. **DO NOT BREAK THIS EVER.**
Any underlying runtime change must perfectly execute all integration evaluation suites against this benchmark.

## Core Capabilities
Flux provides deterministic, replayable implementations of:
*   `crypto.subtle.importKey`
*   `crypto.subtle.exportKey`
*   `crypto.subtle.generateKey`
*   `crypto.subtle.deriveKey`
*   `crypto.subtle.deriveBits`
*   `crypto.subtle.sign`
*   `crypto.subtle.verify`
*   `crypto.subtle.digest`

## Production Library Compatibility
*   **Raw WebCrypto API:** Fully supported and deterministic.
*   **`npm:jose`:** Fully supported, including JWT Signing, Verification, and JWKS (JSON Web Key Set) generation.

## Serialization Protocol (V8 ↔ Rust FFI)
Javascript structured data differs fundamentally from strict Rust deterministic serialization requirements. Flux employs a formal `sanitize()` contract layer across the serialization boundary to intercept and correctly marshal raw pointers and objects into purely serializable state, avoiding `serde_v8` panics:
*   `Uint8Array` → `Array` (explicitly mitigates cross-realm `instanceof` failures using `ArrayBuffer.isView`)
*   `CryptoKey` → `JWK` (or unextractable references)
*   `KeyPair` → `{ publicKey: JWK, privateKey: JWK }`

## Spec-Level Constraints
Platform semantics are strictly preserved over hacky globals.
*   **PBKDF2 Constraint:** WebCrypto specifications explicitly forbid `extractable: true` for PBKDF2 keys. The Flux wrapper conditionally enforces `extractable` parameter interceptions natively to abide by platform semantics while maintaining reproducibility.
*   **RS256 Private Key Exports:** RSA key generation correctly evaluates serializability constraints down to nested exponent byte arrays to ensure the JWKS standard behaves identically inside the isolate.
