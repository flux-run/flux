# `compat-jose` Golden Benchmark

This test suite represents the formal golden baseline for Flux's WebCrypto and deterministic execution guarantees.

## Run the benchmark

```bash
# from fluxbase/runtime/runners
npm run test:integration -- --suite compat-jose
```

## Maintenance Rule

**Never bypass this suite.**
Any runtime architectural change, Deno upgrade, or serialization format update must perfectly pass the 33/33 integration sequence without modification to `jose-compat.ts`.

## Layer 1 Determinism Extent

This covers raw WebCrypto extraction/serialization via structured JWT signing, JSON Web Key Set export verifications over RSASSA-PKCS1-v1_5, and highly constraint-sensitive symmetric evaluations over PBKDF2/HMAC protocols using `npm:jose`.

See `guarantees.md` for the specific FFI contracts and serialization boundary rules proven by this suite.
