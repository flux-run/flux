# Gateway Production Checklist

**Target audience:** DevOps, SRE, production engineers.
Verify before exposing the gateway to real traffic.

---

## Security

- [ ] TLS terminated at load balancer (gateway serves HTTP only)
- [ ] `INTERNAL_SERVICE_TOKEN` set and different from any user-facing credential
- [ ] `/internal/*` routes not exposed through public load balancer
- [ ] Sensitive headers (`authorization`, `cookie`, `x-api-key`) redacted in logs
- [ ] CORS configured per route (no wildcard `*` in production)
- [ ] JWKS cache enabled (avoid network call per request)
- [ ] API key validation returns 401 (not 500) on cache miss

---

## Reliability

- [ ] `MAX_REQUEST_SIZE_BYTES` configured (default 10MB)
- [ ] Runtime timeout configured (`RUNTIME_TIMEOUT_SECS`, default 30s)
- [ ] Snapshot readiness: 503 during cold start (not 404)
- [ ] Snapshot refresh: every 60s, keeps old snapshot on refresh failure
- [ ] Circuit breaker for Runtime: open on sustained 5xx, return 503
- [ ] Database connection pool: min 5, max 20, 5s acquire timeout

---

## Observability

- [ ] `x-request-id` generated for every request (UUID v7)
- [ ] `x-request-id` forwarded to Runtime and echoed in response
- [ ] Platform logs insert is async (non-blocking)
- [ ] Health check at `GET /health` wired to load balancer
- [ ] Monitoring: `p99 latency`, `5xx rate`, `active connections`

---

## Performance

- [ ] Route snapshot in memory (O(1) lookup, <10MB total)
- [ ] Single-flight query deduplication enabled
- [ ] Response caching: role-aware, 30s default TTL
- [ ] Shared `reqwest::Client` (TCP connection reuse)
- [ ] No `.collect().await` on response bodies (stream through)

---

## Load testing

- [ ] 1,000 rps for 5 minutes: all requests complete, p99 < 500ms
- [ ] Spike 100 → 5,000 rps: graceful degradation (503, not crash)
- [ ] Overload 10,000 rps for 2 minutes: gateway survives, memory < 2GB
- [ ] Memory stable over 24h (no leaks)

---

## Deployment

### Pre-deploy
- [ ] All tests passing
- [ ] Load test passed
- [ ] Security audit completed (no TODOs in critical paths)

### Deploy sequence
1. Deploy to staging → smoke test → monitor 12h
2. Canary (10% traffic) → monitor 30 min → check error rate
3. Full rollout → monitor 2h → keep old version for rollback

### Rollback
- Detection: auto-rollback if 5xx rate > 5%
- Manual: revert to previous container image

---

*For Gateway architecture, see [gateway.md](gateway.md).
For the full framework, see [framework.md](framework.md).*
