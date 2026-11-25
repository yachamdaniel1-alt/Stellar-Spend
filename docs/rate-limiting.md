# API Rate Limiting

This document covers the rate limiting policies in Stellar-Spend — tiers, quotas, headers, algorithms, per-endpoint limits, handling 429 errors, and API key-based rate limiting.

---

## Table of Contents

1. [Overview](#overview)
2. [Rate Limit Tiers and Quotas](#rate-limit-tiers-and-quotas)
3. [Rate Limit Headers](#rate-limit-headers)
4. [Rate Limit Algorithms](#rate-limit-algorithms)
5. [Per-Endpoint Rate Limits](#per-endpoint-rate-limits)
6. [API Key-Based Rate Limiting](#api-key-based-rate-limiting)
7. [Handling 429 Errors](#handling-429-errors)
8. [Rate Limit Response Examples](#rate-limit-response-examples)
9. [Requesting a Rate Limit Increase](#requesting-a-rate-limit-increase)

---

## Overview

Stellar-Spend applies rate limiting at two layers:

| Layer             | Scope                               | Implementation                                                |
| ----------------- | ----------------------------------- | ------------------------------------------------------------- |
| **IP-based**      | Per client IP address, per endpoint | `RateLimiter` class (`src/lib/offramp/utils/rate-limiter.ts`) |
| **API key-based** | Per API key, across all endpoints   | `PerKeyRateLimiter` class (`src/lib/api-keys/service.ts`)     |

All rate-limited responses use HTTP status **429 Too Many Requests** and include a `Retry-After` header indicating how many seconds to wait before retrying.

---

## Rate Limit Tiers and Quotas

### IP-based tiers (public endpoints)

These limits apply to all clients regardless of authentication:

| Tier          | Endpoints                                | Limit  | Window |
| ------------- | ---------------------------------------- | ------ | ------ |
| **Sensitive** | Bridge tx build, Paycrest order creation | 10 req | 60 s   |
| **Standard**  | Quote, FX rates, status checks           | 5 req  | 60 s   |

### API key tiers

API keys have individually configurable rate limits stored per key. Default values at key creation:

| Field                  | Default                     |
| ---------------------- | --------------------------- |
| `rateLimitMaxRequests` | Configured at creation time |
| `rateLimitWindowMs`    | Configured at creation time |

API keys are created via the `/api/api-keys` endpoint by an admin. Each key carries its own `rateLimitMaxRequests` and `rateLimitWindowMs` values, allowing fine-grained control per integration partner or use case.

### Rate limit scoping

- IP-based limits are scoped **per IP per endpoint limiter** — hitting the bridge limit does not count against the order limit.
- API key limits are scoped **per key across all authenticated endpoints**.

---

## Rate Limit Headers

Rate-limited responses include the following headers:

| Header         | Description                                                 | Example     |
| -------------- | ----------------------------------------------------------- | ----------- |
| `Retry-After`  | Seconds until the rate limit window resets                  | `42`        |
| `X-API-Key-Id` | ID of the API key that was rate-limited (API key auth only) | `ak_abc123` |

> **Note:** `X-RateLimit-Limit`, `X-RateLimit-Remaining`, and `X-RateLimit-Reset` are not yet included in responses. They are planned for a future release. Track the `Retry-After` header for now.

### Reading `Retry-After`

`Retry-After` is an integer representing seconds. Wait at least this long before retrying.

```
HTTP/1.1 429 Too Many Requests
Retry-After: 42
Content-Type: application/json

{"error":"Rate limit exceeded","retryAfter":42}
```

---

## Rate Limit Algorithms

### Fixed window counter

Both the IP-based `RateLimiter` and the per-key `PerKeyRateLimiter` use a **fixed window counter** algorithm:

1. On the first request in a window, a counter is initialised to `1` and a reset timestamp is set to `now + windowMs`.
2. Each subsequent request in the same window increments the counter.
3. When the counter exceeds `maxRequests`, the request is rejected with 429 and a `retryAfter` value calculated as `ceil((resetTime - now) / 1000)`.
4. When `now >= resetTime`, the counter is reset — the new window begins.

```
Window (60 s)
│◄─────────────────────────────────────────────────────────►│
│ req1  req2  req3  req4  req5 ... req10  req11(BLOCKED)     │
│  ▲                                       ▲                 │
│  │                                       │                 │
│  counter = 1                   counter = 11 > 10 → 429     │
```

### Trade-offs

| Property    | Fixed Window                                   |
| ----------- | ---------------------------------------------- |
| Memory      | Low — one record per key                       |
| Accuracy    | Allows up to 2× the limit at window boundaries |
| Complexity  | Simple to implement and reason about           |
| Persistence | In-memory only — resets on server restart      |

> **Production note:** The current in-memory implementation does not share state across multiple server instances. For multi-instance deployments (Kubernetes, Vercel edge), a distributed store such as Redis is required to enforce limits globally.

### Planned: sliding window

A sliding window log algorithm is planned to address the burst-at-boundary limitation of fixed windows. It records the exact timestamp of each request and counts only those within `[now - windowMs, now]`.

---

## Per-Endpoint Rate Limits

### Bridge transaction build

**Endpoint:** `POST /api/offramp/bridge/build-tx`  
**Limiter:** `buildTxLimiter`  
**Limit:** 10 requests per 60 seconds per IP  
**Rationale:** Building bridge transactions is computationally expensive and interacts with the Allbridge SDK.

### Paycrest order creation

**Endpoint:** `POST /api/offramp/paycrest/order`  
**Limiter:** `paycrestOrderLimiter`  
**Limit:** 5 requests per 60 seconds per IP  
**Rationale:** Order creation initiates real financial transactions; a lower limit prevents accidental or malicious order flooding.

### API key-authenticated endpoints

**Endpoints:** All routes protected by `withApiKeyAuth` middleware  
**Limiter:** `PerKeyRateLimiter` (per API key ID)  
**Limit:** Defined per key (`rateLimitMaxRequests` / `rateLimitWindowMs`)  
**Middleware:** `src/lib/api-keys/auth.ts` → `withApiKeyAuth()`

### Other endpoints

Endpoints not listed above do not currently have a programmatic rate limit applied. Vercel's platform-level protections apply.

---

## API Key-Based Rate Limiting

API keys are the mechanism for authenticating programmatic (server-to-server) access to Stellar-Spend.

### How API key rate limiting works

1. The client sends the API key via the `X-API-Key` header or `Authorization: Bearer <key>`.
2. The `withApiKeyAuth` middleware authenticates the key against the database.
3. `checkApiKeyRateLimit(apiKey)` reads the key's `rateLimitMaxRequests` and `rateLimitWindowMs` fields and calls `PerKeyRateLimiter.check(keyId, maxRequests, windowMs)`.
4. If allowed, the request proceeds; if blocked, a `429` is returned with `Retry-After` and `X-API-Key-Id` headers.
5. All requests — allowed and blocked — are recorded in the usage log via `recordApiKeyUsage()`.

### API key format

All API keys follow this format:

```
ssp_live_<6-byte-hex-public-prefix>.<24-byte-hex-secret>
```

Example: `ssp_live_a1b2c3d4e5f6.0102030405060708090a0b0c0d0e0f101112131415161718`

The public prefix (`keyPrefix`) is stored in plaintext for display; the full key is stored as a SHA-256 hash and never persisted in cleartext after creation.

### Sending an API key

```http
GET /api/v1/fx-rates HTTP/1.1
X-API-Key: <your_api_key>

# Or via Authorization header:
Authorization: Bearer <your_api_key>
```

### API key lifecycle

| Status    | Description                                                 |
| --------- | ----------------------------------------------------------- |
| `active`  | Key is valid and can authenticate requests                  |
| `rotated` | Key has been replaced; the new key is in `rotatedFromKeyId` |
| `revoked` | Key is permanently disabled; all requests will be rejected  |

Rotate a key via `POST /api/api-keys/:id/rotate`. Revoke via the admin API.

---

## Handling 429 Errors

### Detection

A `429` response always includes:

```json
{
  "error": "Rate limit exceeded",
  "retryAfter": 42
}
```

Check both the HTTP status code (`response.status === 429`) and the `Retry-After` header.

### Basic retry with backoff

```ts
async function fetchWithRetry(
  url: string,
  options: RequestInit,
  maxRetries = 3,
): Promise<Response> {
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    const response = await fetch(url, options);

    if (response.status !== 429) return response;

    if (attempt === maxRetries) {
      throw new Error(`Rate limit exceeded after ${maxRetries} retries`);
    }

    const retryAfter = parseInt(response.headers.get('Retry-After') ?? '60', 10);
    const jitter = Math.random() * 1000; // Add up to 1 s of jitter
    const delay = retryAfter * 1000 + jitter;

    console.warn(`Rate limited. Retrying in ${(delay / 1000).toFixed(1)} s...`);
    await new Promise((resolve) => setTimeout(resolve, delay));
  }

  throw new Error('Unreachable');
}
```

### Exponential backoff

For high-frequency clients, use exponential backoff as the base delay:

```ts
const delay = Math.min(
  Math.pow(2, attempt) * 1000 + Math.random() * 500,
  30_000, // cap at 30 s
);
```

### Client-side rate limit budgeting

Proactively track your request rate to avoid hitting limits:

```ts
class RateLimitBudget {
  private requests: number[] = [];
  private readonly limit: number;
  private readonly windowMs: number;

  constructor(limit: number, windowMs: number) {
    this.limit = limit;
    this.windowMs = windowMs;
  }

  canProceed(): boolean {
    const now = Date.now();
    this.requests = this.requests.filter((t) => now - t < this.windowMs);
    if (this.requests.length >= this.limit) return false;
    this.requests.push(now);
    return true;
  }
}

// Usage
const budget = new RateLimitBudget(10, 60_000);

if (!budget.canProceed()) {
  // Back off before making the request
}
```

### React query / SWR integration

When using SWR or React Query, configure retry behaviour to respect `Retry-After`:

```ts
// React Query
queryClient.setDefaultOptions({
  queries: {
    retry: (failureCount, error: any) => {
      if (error?.status === 429) return failureCount < 3;
      return false;
    },
    retryDelay: (attemptIndex, error: any) => {
      if (error?.retryAfter) return error.retryAfter * 1000;
      return Math.min(1000 * 2 ** attemptIndex, 30_000);
    },
  },
});
```

---

## Rate Limit Response Examples

### IP-based 429 (bridge endpoint)

```http
HTTP/1.1 429 Too Many Requests
Content-Type: application/json
Retry-After: 37

{
  "error": "Too many requests. Please try again in 37 seconds."
}
```

### API key 429

```http
HTTP/1.1 429 Too Many Requests
Content-Type: application/json
Retry-After: 55
X-API-Key-Id: ak_7f3c1a9b

{
  "error": "API key rate limit exceeded"
}
```

### Successful response (rate limit not hit)

```http
HTTP/1.1 200 OK
Content-Type: application/json
X-API-Key-Id: ak_7f3c1a9b
X-Request-Id: req_abc123

{
  "rate": 1450.50,
  "currency": "NGN"
}
```

### Invalid API key (401 — not a rate limit)

```http
HTTP/1.1 401 Unauthorized
Content-Type: application/json

{
  "error": "Invalid API key"
}
```

---

## Requesting a Rate Limit Increase

### When to request an increase

- Your integration legitimately needs more than 10 requests per minute on the bridge endpoint
- You are building a high-throughput aggregator or B2B product
- You have already implemented client-side budgeting and still hit limits

### How to request an increase

1. **Open a GitHub issue** using the **Rate Limit Increase Request** template and include:

   - Your API key prefix (the `ssp_live_<6-char>` prefix — never share the full key)
   - Intended use case
   - Required request rate (requests per minute or per second)
   - Expected volume (daily/monthly transaction count)

2. **Admin action:** An admin uses the API key management endpoint to update the key's limits:

```bash
curl -X PATCH https://app.your-domain.com/api/api-keys/<key-id> \
  -H "X-Admin-Token: <API_KEY_ADMIN_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{"rateLimitMaxRequests": 50, "rateLimitWindowMs": 60000}'
```

3. The change takes effect immediately — no redeploy required.

### Self-service alternatives

Before requesting an increase, consider:

- **Caching FX rates** — the `/api/fx-rates` response changes infrequently; cache it for 60 s on your side.
- **Batching** — use `/api/offramp/batch` for bulk operations instead of individual calls.
- **Webhooks** — subscribe to status updates via webhook instead of polling `/api/offramp/status/:id`.
