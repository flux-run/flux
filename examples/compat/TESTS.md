# Flux Compatibility Test Suite

A comprehensive test matrix covering every supported library. Each test file follows the pattern:
- **Smoke** — library loads, no side effects
- **Happy path** — all core features working
- **Failure / edge cases** — errors caught gracefully, constraints handled, timeouts respected
- **Concurrency** — parallel IO through Flux's interception layer

---

## HTTP Clients

### `fetch-compat.ts` — Native fetch (✅ Ready)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/get` | GET | Basic GET, status 200, response body |
| `/post` | POST | JSON body echoed back |
| `/put` | PUT | PUT method support |
| `/delete` | DELETE | DELETE method |
| `/patch` | PATCH | PATCH method |
| `/headers` | GET | Custom request headers forwarded |
| `/response-headers` | GET | Custom response headers read |
| `/text` | GET | text/plain response via `.text()` |
| `/binary` | GET | Binary (PNG) response via `.arrayBuffer()` |
| `/query` | GET | Query string passthrough |
| `/form` | POST | `application/x-www-form-urlencoded` body |
| `/gzip` | GET | Gzip decompression |
| `/deflate` | GET | Deflate decompression |
| `/status-4xx` | GET | 4xx returned, not thrown |
| `/status-5xx` | GET | 5xx returned, not thrown |
| `/timeout` | GET | `AbortSignal.timeout()` cancels request |
| `/abort` | GET | `AbortController` explicit cancel |
| `/unreachable` | GET | Connection refused — network error caught |
| `/invalid-url` | GET | Malformed URL throws caught error |
| `/large-body` | POST | 100KB+ request body |
| `/redirect` | GET | Follows redirects transparently |
| `/no-redirect` | GET | `redirect: "manual"` — not followed |
| `/bearer-auth` | GET | Authorization header forwarded |
| `/concurrent-3` | GET | 3 parallel fetches via `Promise.all` |
| `/concurrent-mixed` | GET | GET + POST in parallel |
| `/sequential` | GET | 3 sequential fetches, results collected |

---

### `axios-compat.ts` — axios (✅ Ready)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/get` | GET | Basic GET |
| `/post` | POST | POST with JSON body |
| `/put` | PUT | PUT method |
| `/patch` | PATCH | PATCH method |
| `/delete` | DELETE | DELETE method |
| `/headers` | GET | Custom headers forwarded |
| `/query-params` | GET | `params` config → query string |
| `/bearer` | GET | Authorization header |
| `/basic-auth` | GET | `auth: { username, password }` config |
| `/instance` | GET | `axios.create()` instance with defaults |
| `/form` | POST | `URLSearchParams` form-encoded body |
| `/gzip` | GET | Gzip handled automatically |
| `/error-4xx` | GET | 4xx throws AxiosError, caught |
| `/error-5xx` | GET | 5xx throws AxiosError, caught |
| `/no-throw` | GET | `validateStatus: () => true` |
| `/timeout` | GET | `timeout` option triggers error |
| `/cancel` | GET | AbortController cancels request |
| `/response-schema` | POST | Response shape validated |
| `/interceptor` | GET | Request + response interceptors applied |
| `/concurrent-3` | GET | 3 parallel requests |
| `/concurrent-mixed` | GET | GET + POST in parallel |

---

### `undici-compat.ts` — undici (✅ Ready)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/get` | GET | `request()` basic GET |
| `/post` | POST | POST with JSON |
| `/put` | PUT | PUT method |
| `/delete` | DELETE | DELETE, `body.dump()` |
| `/patch` | PATCH | PATCH method |
| `/headers` | GET | Custom headers |
| `/response-headers` | GET | Response headers struct |
| `/text` | GET | `body.text()` |
| `/binary` | GET | `body.arrayBuffer()` |
| `/gzip` | GET | Gzip decompression |
| `/fetch-api` | GET | `undici.fetch()` compatibility |
| `/status-404` | GET | 404 — no throw |
| `/status-500` | GET | 500 — no throw |
| `/timeout` | GET | `headersTimeout`/`bodyTimeout` |
| `/unreachable` | GET | Connection refused caught |
| `/large-body` | POST | 100KB+ request |
| `/concurrent-3` | GET | 3 parallel requests |
| `/concurrent-mixed` | GET | GET + POST in parallel |

---

## Databases

### `pg-compat.ts` — node-postgres (✅ Ready)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/setup` | POST | `CREATE TABLE IF NOT EXISTS` |
| `/cleanup` | DELETE | `DROP TABLE` |
| `/select-1` | GET | `SELECT 1` |
| `/now` | GET | `SELECT NOW()` — server timestamp |
| `/version` | GET | `SELECT version()` |
| `/insert` | POST | Parameterized INSERT RETURNING (`$1...$4`) |
| `/select-all` | GET | `SELECT *` |
| `/select-where` | GET | Parameterized WHERE |
| `/update` | PUT | UPDATE by id RETURNING |
| `/delete-row` | DELETE | DELETE RETURNING |
| `/jsonb` | GET | JSONB column insert + retrieval |
| `/arrays` | GET | `TEXT[]` arrays |
| `/null-values` | GET | NULL column insert and read |
| `/boolean` | GET | BOOLEAN column false value |
| `/transaction-commit` | GET | BEGIN → INSERT → COMMIT, verify visible |
| `/transaction-rollback` | GET | BEGIN → INSERT → ROLLBACK → count = 0 |
| `/transaction-savepoint` | GET | SAVEPOINT → partial ROLLBACK |
| `/unique-violation` | POST | Duplicate key → pg error code `23505` |
| `/not-null-violation` | POST | NULL in NOT NULL column → `23502` |
| `/syntax-error` | GET | Bad SQL → pg error code `42601` |
| `/pool-multiple` | GET | Multiple queries through same pool |
| `/client-connect` | GET | Manual `connect()` + `release()` lifecycle |
| `/concurrent` | GET | 5 simultaneous queries via `Promise.all` |

---

## ORMs

### `drizzle-compat.ts` — Drizzle ORM (✅ Ready*)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/setup` | POST | Create `users` + `posts` tables |
| `/cleanup` | DELETE | Drop all test tables |
| `/users` | POST | Insert user via Drizzle `.insert().returning()` |
| `/users` | GET | List all users `.select().from()` |
| `/users/:id` | GET | Select by id `eq(users.id, id)` |
| `/users/:id` | PUT | Update with `.update().set().where()` |
| `/users/:id` | DELETE | Delete with `.delete().where()` |
| `/posts` | POST | Insert post (FK to user) |
| `/posts` | GET | List posts `orderBy(desc(...))` |
| `/filter/active` | GET | `eq(users.active, true)` |
| `/filter/score-gt` | GET | `gt(users.score, threshold)` |
| `/filter/compound` | GET | `and(eq(...), gte(...))` |
| `/filter/name-like` | GET | `like(users.name, ...)` |
| `/filter/in` | GET | `inArray(users.id, ids)` |
| `/order/desc` | GET | `orderBy(desc(users.score))` |
| `/paginate` | GET | `.limit(n).offset(m)` |
| `/aggregate` | GET | `COUNT`, `AVG`, `MAX`, `MIN` via `sql<>` |
| `/jsonb` | GET | JSONB column + `meta->>'role'` operator |
| `/transaction` | POST | Insert user + post in single transaction |
| `/transaction-rollback` | GET | Error in tx → full rollback verified |
| `/unique-email` | POST | Unique constraint → code `23505` |
| `/join` | GET | LEFT JOIN users ↔ posts |
| `/concurrent` | GET | 5 parallel select queries |

*Pending `?bundle` fix.

---

## Validation

### `zod-compat.ts` — Zod (✅ Ready)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/primitives` | POST | string, number, boolean, nullable, optional |
| `/coerce` | POST | `z.coerce.number/boolean/date` |
| `/strings` | POST | email, url, uuid, min, max, regex, startsWith, endsWith, trim, toLowerCase, toUpperCase |
| `/string-bad` | POST | Invalid strings → error messages |
| `/object` | POST | Full user schema |
| `/object-strict` | POST | `.strict()` — unknown keys rejected |
| `/object-passthrough` | POST | `.passthrough()` — unknown keys pass |
| `/nested` | POST | Nested `UserSchema` + `AddressSchema` |
| `/partial` | POST | `.partial()` — all fields optional |
| `/pick` | POST | `.pick({ name, email })` |
| `/omit` | POST | `.omit({ email })` |
| `/array` | POST | `z.array(z.string()).min(1).max(20)` |
| `/array-objects` | POST | Array of validated objects |
| `/tuple` | POST | `z.tuple([string, number, boolean])` |
| `/union` | POST | `z.union([string, number])` |
| `/union-objects` | POST | Email OR phone identifier |
| `/discriminated` | POST | `discriminatedUnion` on `type` field |
| `/transform` | POST | trim + toLowerCase + `parseInt` |
| `/preprocess` | POST | `z.preprocess()` before schema |
| `/refine-password` | POST | `.refine()` — passwords must match |
| `/refine-range` | POST | `.refine()` — min < max |
| `/superrefine` | POST | Multiple custom errors |
| `/optional-nullable` | POST | optional vs nullable vs default vs nullish |
| `/paginate` | GET | Query param coercion with defaults |
| `/error-format` | POST | `flatten()` vs `format()` vs `.issues` |
| `/parse-throws` | POST | `z.parse()` throws `ZodError` |

---

## Auth & Crypto

### `jose-compat.ts` — webcrypto + jose (✅ Ready)
| Route | Method | What it tests |
|---|---|---|
| `/sign` | POST | HMAC-SHA256 JWT sign (custom impl) |
| `/verify` | POST | HMAC-SHA256 JWT verify |
| `/sign-verify-cycle` | POST | Sign → verify round-trip |
| `/verify-bad` | POST | Tampered signature → error caught |
| `/verify-expired` | POST | Expired token → `JWTExpired` caught |
| `/jwks` | GET | RSA-2048 keygen + export JWK |
| `/digest` | GET | SHA-256 digest via `crypto.subtle` |
| `/derive-key` | POST | PBKDF2 key derivation |
| `/jose-sign` | POST | `SignJWT` from `npm:jose` |
| `/jose-verify` | POST | `jwtVerify` from `npm:jose` |

---

## Redis

### `ioredis-compat.ts` — ioredis (🟡 Beta)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/ping` | GET | PING → PONG |
| `/info` | GET | INFO server response |
| `/set-get` | POST | SET (EX) + GET + DEL |
| `/setnx` | POST | SETNX — only sets if not exists |
| `/getset` | POST | GETSET — atomic get-and-replace |
| `/mset-mget` | POST | MSET + MGET multi-key |
| `/append` | POST | APPEND to string |
| `/expiry` | POST | TTL + EXISTS after SET EX |
| `/persist` | POST | PERSIST removes TTL |
| `/pexpiry` | POST | PSETEX + PTTL milliseconds |
| `/incr` | POST | INCR, INCRBY, INCRBYFLOAT, DECR, DECRBY |
| `/hash` | POST | HSET, HGET, HGETALL, HKEYS, HVALS, HLEN, HDEL, HEXISTS, HINCRBY |
| `/hincrby` | POST | HINCRBY + HINCRBYFLOAT |
| `/list` | POST | RPUSH, LPUSH, LLEN, LRANGE, LINDEX, RPOP, LPOP |
| `/list-trim` | POST | LTRIM keep middle elements |
| `/set` | POST | SADD, SCARD, SISMEMBER, SMEMBERS, SREM |
| `/set-ops` | POST | SINTER, SUNION, SDIFF |
| `/zset` | POST | ZADD, ZCARD, ZRANK, ZSCORE, ZRANGE, ZREVRANGE, ZREM |
| `/zrangebyscore` | POST | ZRANGEBYSCORE + ZCOUNT |
| `/pipeline` | POST | Pipelined SET/GET/INCR/DEL |
| `/multi-exec` | POST | MULTI + EXEC atomic transaction |
| `/keys-scan` | GET | SCAN with MATCH pattern |
| `/type-check` | GET | TYPE for string/list/set |
| `/concurrent` | GET | 5 parallel SET/GET operations |

---

### `redis-compat.ts` — node-redis v4 via `flux:redis` (🟡 Beta)
| Route | Method | What it tests |
|---|---|---|
| `/` | GET | Smoke |
| `/ping` | GET | PING |
| `/set-get` | POST | SET (EX) + GET + DEL |
| `/setnx` | POST | SET with NX option |
| `/setex-ttl` | POST | SET EX + TTL + PTTL + EXISTS |
| `/getset` | POST | `getSet()` atomic replace |
| `/mset-mget` | POST | `mSet` / `mGet` |
| `/append` | POST | `append()` |
| `/incr` | POST | incr, incrBy, incrByFloat, decr, decrBy |
| `/hash` | POST | hSet, hGet, hGetAll, hKeys, hVals, hLen, hDel, hExists, hIncrBy |
| `/list` | POST | rPush, lPush, lLen, lRange, lIndex, lSet, rPop, lPop |
| `/list-trim` | POST | `lTrim()` |
| `/set` | POST | sAdd, sCard, sIsMember, sMembers, sRem |
| `/set-ops` | POST | sInter, sUnion, sDiff |
| `/zset` | POST | zAdd, zCard, zRank, zScore, zRange, zRangeWithScores, zRem |
| `/zrangebyscore` | POST | zRangeByScore + zCount |
| `/pipeline` | POST | multi().exec() pipeline |
| `/multi-exec` | POST | Atomic MULTI/EXEC transaction |
| `/scan` | GET | `scanIterator` with MATCH |
| `/expire-delete` | POST | exists + del + exists lifecycle |
| `/concurrent` | GET | 5 parallel SET/GET operations |
