# Gateway Production-Readiness Checklist

**Service:** Fluxbase Gateway  
**Target Audience:** DevOps, SRE, Production Engineers  
**Purpose:** Verify gateway is production-grade before handling real traffic

---

## 🔐 Security

### Authentication & Authorization

- [ ] **Host Header Validation**
  - [ ] Gateway validates incoming `Host:` header matches expected domain
  - [ ] Rejects requests with mismatched Host (prevents DNS rebinding)
  - [ ] Configurable allowed hosts via `ALLOWED_HOSTS` env

- [ ] **TLS Termination**
  - [ ] TLS terminated at load balancer (before gateway)
  - [ ] Gateway serves over HTTP only (no TLS management burden)
  - [ ] Load balancer enforces TLS 1.2+
  - [ ] HSTS header set by load balancer

- [ ] **Internal Endpoints Protection**
  - [ ] `/internal/*` routes require `X-Service-Token` header
  - [ ] Service token validated against `INTERNAL_SERVICE_TOKEN` env
  - [ ] Internal endpoints NOT exposed through public CORS layer
  - [ ] Example: `/internal/cache/invalidate`, `/internal/cache/stats`

- [ ] **Secrets Redaction**
  - [ ] Authorization headers marked `[REDACTED]` in logs
  - [ ] API keys marked `[REDACTED]` in traces
  - [ ] JWT claims NOT stored in plaintext (store UUID only)
  - [ ] Cookies marked `[REDACTED]`

---

### API Key & JWT Validation

- [ ] **API Key Validation**
  - [ ] Query database for each request (no caching)
  - [ ] Cache miss → 401 Unauthorized (not 500)
  - [ ] Invalid key → 401 with clear error message
  - [ ] Rate limit per API key (not just IP)

- [ ] **JWT Verification**
  - [ ] JWKS cache implemented (avoid network call per request)
  - [ ] JWKS cache invalidates on 404 (key rotation)
  - [ ] Expired tokens → 401 Unauthorized
  - [ ] Invalid signature → 401 Unauthorized
  - [ ] Missing required claims (aud, iss) → 401 Unauthorized

---

### CORS Configuration

- [ ] **CORS Enforcement**
  - [ ] `Access-Control-Allow-Origin` respects route config
  - [ ] Wildcard `*` allowed only for dev (not production)
  - [ ] `Access-Control-Allow-Methods` restricted per route
  - [ ] Preflight OPTIONS requests return 204 (not 200)

---

## ✅ Reliability

### Circuit Breaker & Failure Handling

- [ ] **Circuit Breaker for Runtime**
  - [ ] If runtime consistently fails (5xx), circuit opens
  - [ ] Openning: return 503 (not proxy errors to user)
  - [ ] Timeout: 30 seconds (configurable via `RUNTIME_TIMEOUT_SECS`)
  - [ ] Half-open: attempt recovery with single request
  - [ ] Metrics: `circuit_breaker_state` (open/half-open/closed)

- [ ] **Failed Request Handling**
  - [ ] Runtime timeout → 504 Gateway Timeout
  - [ ] Runtime 5xx → 502 Bad Gateway (with body from runtime)
  - [ ] Network error → 502 Bad Gateway (with generic error)
  - [ ] Connection refused → 502 (not 500)

---

### Timeout Policy

- [ ] **Request Timeouts**
  - [ ] Gateway → Runtime: 30 seconds (configurable)
  - [ ] Database queries: 5 seconds (configurable)
  - [ ] JWKS fetch: 5 seconds (with exponential backoff)
  - [ ] Rate limit check: <100ms (in-memory)

- [ ] **Cold Start Handling**
  - [ ] Snapshot refresh: waits up to 30 seconds at startup
  - [ ] Until snapshot ready: return 503 (not 404)
  - [ ] Snapshot refresh fails: log error but continue (graceful degrade)
  - [ ] Periodic refresh: every 60 seconds (configurable)

---

### Retry Policy

- [ ] **Idempotent Retry**
  - [ ] GET, HEAD, DELETE: safe to retry on 5xx
  - [ ] POST with `Idempotency-Key`: safe to retry
  - [ ] POST without key: never retry (data corruption risk)
  - [ ] Max retries: 2 (default, configurable)

- [ ] **Exponential Backoff**
  - [ ] Wait: 100ms + random jitter
  - [ ] Multiply: 2x per retry
  - [ ] Cap: 5 seconds max wait

---

### Snapshot Ready Gate

- [ ] **Snapshot Initialization**
  - [ ] At startup: block server until snapshot loads
  - [ ] If snapshot fails: log error and continue (empty snapshot)
  - [ ] Empty snapshot: return 503 for all requests
  - [ ] Recovery: restart gateway to reload snapshot

- [ ] **Periodic Refresh**
  - [ ] Background task: refresh every 60 seconds
  - [ ] If refresh fails: log error, keep old snapshot
  - [ ] Refresh duration: <5 seconds (timeout if longer)
  - [ ] Concurrent requests: don't block during refresh

---

## 📊 Observability

### Request Logging

- [ ] **Tracing Headers**
  - [ ] Generate `x-request-id` (UUID v7) if not present
  - [ ] Forward `x-request-id` to runtime
  - [ ] Echo `x-request-id` in response headers
  - [ ] Forward `x-parent-span-id` (enable trace hierarchy)

- [ ] **Platform Logs**
  - [ ] Log every request: gateway → routing → runtime
  - [ ] Include: request_id, tenant_id, project_id, status_code, latency_ms
  - [ ] Redact sensitive headers
  - [ ] Async insert (non-blocking)

---

### Metrics

- [ ] **Request Metrics**
  - [ ] Counter: `http_requests_total` (by method, path, status)
  - [ ] Histogram: `http_request_duration_seconds` (by method, path)
  - [ ] Gauge: `gateway_connections_active`

- [ ] **Cache Metrics**
  - [ ] Counter: `cache_hits_total`, `cache_misses_total`
  - [ ] Gauge: `cache_size_bytes`, `cache_entries_count`
  - [ ] Hit rate: `(hits / (hits + misses)) * 100`

- [ ] **Rate Limit Metrics**
  - [ ] Counter: `rate_limit_exceeded_total` (by route, client_ip)
  - [ ] Gauge: `rate_limit_remaining` (per key)

- [ ] **Circuit Breaker Metrics**
  - [ ] Gauge: `circuit_breaker_state` (0=closed, 1=open, 2=half-open)
  - [ ] Counter: `circuit_breaker_trips_total`

---

### Health Checks

- [ ] **Liveness Probe**
  - [ ] `GET /health` → 200 OK (always)
  - [ ] Used by: load balancer (restart if 5xx)

- [ ] **Readiness Probe**
  - [ ] `GET /health` checks: routes snapshot loaded
  - [ ] If snapshot empty: 503 (not ready)
  - [ ] Used by: Kubernetes (delay traffic until ready)

- [ ] **Version Endpoint**
  - [ ] `GET /version` → JSON with commit SHA, build time
  - [ ] Used by: deployment verification

---

### Error Tracking

- [ ] **Error Rate Monitoring**
  - [ ] Track 5xx errors (gateway is failing)
  - [ ] Track 4xx errors (client is wrong)
  - [ ] Alert if error rate > 1% (immediate)
  - [ ] Alert if error rate > 0.1% (warning)

- [ ] **Latency Monitoring**
  - [ ] Track p50, p95, p99 latencies
  - [ ] Alert if p99 > 5 seconds
  - [ ] Alert if p95 > 2 seconds

---

## ⚡ Performance

### Request Body Limits

- [ ] **Content-Length Validation**
  - [ ] Check `Content-Length` header before reading body
  - [ ] Return 413 Payload Too Large if > MAX_REQUEST_SIZE
  - [ ] Default: 10MB (configurable via `MAX_REQUEST_SIZE_BYTES`)
  - [ ] Config allows per-route overrides (optional future)

- [ ] **Body Buffering**
  - [ ] Gateway uses `to_bytes()` (buffers entire body in memory)
  - [ ] For large responses (streaming): forward directly
  - [ ] For large requests: consider external storage (artifact_uri)

---

### Connection Pooling

- [ ] **Database Pool**
  - [ ] Min connections: 5
  - [ ] Max connections: 20 (configurable)
  - [ ] Timeout: 5 seconds to acquire from pool
  - [ ] Idle timeout: 30 seconds (close unused connections)

- [ ] **HTTP Client Pool**
  - [ ] Shared `reqwest::Client` (reuse TCP connections)
  - [ ] Max redirects: 0 (no redirects)
  - [ ] Timeout: 30 seconds per request

---

### Caching Strategy

- [ ] **Route Snapshot Caching**
  - [ ] Memory-resident hash map: (tenant_id, method, path) → RouteRecord
  - [ ] Size limit: none (routes table is small <10MB)
  - [ ] Refresh: every 60 seconds
  - [ ] Hit rate: >99.9% (should rarely miss)

- [ ] **JWKS Caching**
  - [ ] Memory-resident: issuer → JWKS keys
  - [ ] TTL: 24 hours (or until 404)
  - [ ] Invalidation: on 404 response (key rotation)

- [ ] **Query Result Caching**
  - [ ] Optional: cache API key lookups → 5 minute TTL
  - [ ] Trade-off: invalidation complexity vs latency gain

---

### Response Streaming

- [ ] **Stream Responses (Not Buffer)**
  - [ ] For large responses, use `hyper::Body` streaming
  - [ ] Don't buffer entire response in memory
  - [ ] Forward response body chunk-by-chunk
  - [ ] Verify: no `.collect().await` in response handling

---

## 🧪 Testing & Validation

### Load Testing

- [ ] **Functional Load Test**
  - [ ] 1,000 requests/second for 5 minutes
  - [ ] Verify: all requests complete successfully
  - [ ] Memory: stable (no leaks)
  - [ ] Latency: p99 < 500ms

- [ ] **Spike Test**
  - [ ] 100 → 5,000 requests/second (spike)
  - [ ] Verify: circuit breaker engages gracefully
  - [ ] Verify: 503 returned (not 500)
  - [ ] Recovery: back to normal within 30 seconds

- [ ] **Overload Test**
  - [ ] 10,000 requests/second for 2 minutes
  - [ ] Verify: gateway doesn't crash
  - [ ] Verify: graceful degradation (503, not 500)
  - [ ] Verify: memory stays under 2GB

---

### Security Testing

- [ ] **DNS Rebinding**
  - [ ] Send request with `Host: attacker.com`
  - [ ] Verify: 400 Bad Request (not proxied)

- [ ] **Authorization Bypass**
  - [ ] Request without API key (api_key route)
  - [ ] Verify: 401 Unauthorized
  - [ ] Request with invalid key
  - [ ] Verify: 401 Unauthorized

- [ ] **JWT Tampering**
  - [ ] Modify JWT payload
  - [ ] Verify: 401 Unauthorized
  - [ ] Get expired token
  - [ ] Verify: 401 Unauthorized

---

### Snapshot Readiness

- [ ] **Cold Start**
  - [ ] Kill gateway container
  - [ ] Boot new container
  - [ ] Verify: initial requests return 503 (not 404)
  - [ ] Wait 5 seconds
  - [ ] Verify: requests return 200 (snapshot loaded)

---

## 📋 Deployment

### Pre-Deploy Checklist

- [ ] Code review completed (no TODOs in critical paths)
- [ ] All tests passing locally and in CI
- [ ] Load test passed (1000 req/s, p99 < 500ms)
- [ ] Security audit completed (no obvious vulnerabilities)

### Deployment Steps

1. **Deploy to Staging**
   - [ ] Run smoke tests (verify basic functionality)
   - [ ] Run security tests (verify auth works)
   - [ ] Run performance tests (verify latency)
   - [ ] Monitor for 12 hours (check for crashes, memory leaks)

2. **Deploy to Production (Canary)**
   - [ ] Route 10% of traffic to new gateway
   - [ ] Monitor for 30 minutes (error rate, latency, memory)
   - [ ] Check: no 5xx errors (only expected 4xx)
   - [ ] If good: proceed; if bad: rollback

3. **Deploy to Production (Full)**
   - [ ] Route 100% of traffic to new gateway
   - [ ] Monitor for 2 hours (all metrics stable)
   - [ ] Keep old version running for quick rollback

### Rollback Procedure

- [ ] Detection: if error rate > 5%, auto-rollback
- [ ] Manual: `kubectl set image gateway=<previous-sha>`
- [ ] Verify: traffic back to working version within 2 min

---

## 🎯 Production Success Criteria

✅ Gateway is **production-ready** when:

1. **Reliability**
   - [ ] 99.9% uptime (< 8 minutes downtime per month)
   - [ ] p99 latency < 500ms
   - [ ] Error rate < 0.1% (excludes client 4xx errors)

2. **Security**
   - [ ] Zero auth bypass exploits (in bug bounty)
   - [ ] Zero data leaks (secrets not in logs)
   - [ ] All internal endpoints protected

3. **Capacity**
   - [ ] Handles 100M requests/day
   - [ ] Memory stable at < 2GB
   - [ ] Database pool never exhausted

4. **Observability**
   - [ ] All requests traced (x-request-id in logs)
   - [ ] Metrics integrated (Prometheus scrapes `/metrics`)
   - [ ] Alerts set up (error rate, latency, memory)

---

## 🚀 Launch Readiness Summary

| Component | Status | Owner | ETA |
|-----------|--------|-------|-----|
| Gateway code | ✅ Implemented | Platform | Now |
| Database schema | ✅ Implemented | Platform | Now |
| Load testing | ⏳ Pending | DevOps | This week |
| Security audit | ⏳ Pending | Security | This week |
| Runtime integration | ⏳ Pending | Runtime team | Next week |
| Staging validation | ⏳ Pending | QA | Week after |
| Production canary | ⏳ Pending | DevOps | 2 weeks |
| Production GA | ⏳ Pending | DevOps | 3 weeks |

---

## Contact

**Platform Team Lead:** [name]  
**On-Call:** [schedule]  
**Emergency Escalation:** [phone]

