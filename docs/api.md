# Stellar-Spend API Documentation

Base URL: `http://localhost:3001` (development)

> **Interactive API explorer:** Visit **[/api/docs](/api/docs)** for the Swagger UI — try any endpoint directly from the browser.
> The complete OpenAPI 3.0 specification is available for external consumers in both formats:
>
> - Raw YAML: **[`/openapi.yaml`](/openapi.yaml)**
> - Raw JSON: **[`/openapi.json`](/openapi.json)**
> - Root Spec File: [`openapi.yaml`](../openapi.yaml)

---

## API Changelog

| Version   | Date       | Summary                                                                                                                                   |
| --------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **1.1.0** | 2026-06-29 | Added `X-RateLimit-*` response headers; expanded auth/scope/idempotency docs; interactive Swagger UI at `/api/docs`; integrator SDK guide |
| **1.0.0** | 2026-01-01 | Initial public release; versioned `/api/v1/` routes; API key auth; idempotency; webhook HMAC verification                                 |

---

## Authentication

All `/api/offramp/*` endpoints are **server-side only** and use environment-configured secrets. Clients call these routes directly — no client-side API key is required or exposed.

The webhook endpoint (`/api/webhooks/paycrest`) verifies requests using HMAC-SHA256 via the `X-Paycrest-Signature` header.

Versioned programmatic endpoints under `/api/v1/*` require an API key via either:

- `X-API-Key: <key>`
- `Authorization: Bearer <key>`

---

## API Key Scopes

API keys are issued with one or more scopes that restrict which endpoints they can access:

| Scope           | Endpoints                                                |
| --------------- | -------------------------------------------------------- |
| `offramp:read`  | GET currencies, institutions, rate, bridge status, quote |
| `offramp:write` | POST quote, build-tx, submit-soroban, paycrest order     |
| `webhook:read`  | Receive and replay webhook events                        |

Scopes are assigned at key creation and shown via `GET /api/api-keys/{id}/scopes`.

---

## Rate Limits

All `/api/v1/*` endpoints enforce per-key rate limits. The following headers are included on every authenticated response:

| Header                  | Description                                    |
| ----------------------- | ---------------------------------------------- |
| `X-RateLimit-Limit`     | Maximum requests allowed in the current window |
| `X-RateLimit-Remaining` | Requests remaining in the current window       |
| `X-RateLimit-Reset`     | Unix timestamp when the window resets          |
| `Retry-After`           | Seconds to wait (only on `429` responses)      |

Default limit: **120 requests per 60 seconds** per key (configurable at key creation).

When the limit is exceeded:

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 12
X-RateLimit-Limit: 120
X-RateLimit-Reset: 1751234567

{ "error": "Rate limit exceeded" }
```

---

## Idempotency

Mutating endpoints support the `Idempotency-Key` request header to allow safe retries without creating duplicate resources.

**How it works:**

1. Send any string as `Idempotency-Key` (a UUID per request is recommended).
2. The server caches the response keyed by `hash(method + path + body + key)`.
3. On retry with the same key and same body: the cached response is returned with `Idempotency-Status: replayed`.
4. Same key but different body: `409 Conflict` with `Idempotency-Status: conflict`.
5. Keys expire after 24 hours (`IDEMPOTENCY_TTL_MS`).

```http
POST /api/v1/offramp/paycrest/order
Idempotency-Key: 550e8400-e29b-41d4-a716-446655440000
```

Response headers:

```
Idempotency-Key: 550e8400-e29b-41d4-a716-446655440000
Idempotency-Status: created    # or: replayed | conflict
```

---

## API Keys

Programmatic access uses managed API keys. Keys are stored hashed in the database, can be rotated or revoked, and track usage plus per-key rate limits.

### Management endpoints

API key management routes require:

```http
Authorization: Bearer <API_KEY_ADMIN_TOKEN>
```

### POST /api/api-keys

Creates a new API key and returns the plaintext secret once.

**Request Body**

```json
{
  "name": "Production Partner",
  "rateLimitMaxRequests": 120,
  "rateLimitWindowMs": 60000
}
```

**Response `201`**

```json
{
  "data": {
    "id": "uuid",
    "name": "Production Partner",
    "keyPrefix": "<public-key-prefix>",
    "status": "active",
    "plaintextKey": "ssp_live_abc123def456.very-secret-value"
  }
}
```

### GET /api/api-keys

Lists all API keys without exposing plaintext secrets.

### POST /api/api-keys/[id]/rotate

Creates a replacement key, marks the prior key as `rotated`, and returns the new plaintext key once.

### POST /api/api-keys/[id]/revoke

Revokes a key immediately.

**Request Body**

```json
{
  "reason": "Key leaked during credential rotation"
}
```

### GET /api/api-keys/[id]/usage

Returns the most recent usage events for the key.

### Programmatic auth behavior

- Only `/api/v1/*` endpoints require API keys.
- `/api/v1/health` remains public.
- Revoked, rotated, invalid, or expired keys return `401`.
- Per-key rate limit violations return `429` with `Retry-After`.
- Successful authenticated requests include `X-API-Key-Id`.

---

## Rate Limiting

Two endpoints enforce per-IP rate limits:

| Endpoint                            | Limit                                        |
| ----------------------------------- | -------------------------------------------- |
| `POST /api/offramp/bridge/build-tx` | See `buildTxLimiter` config                  |
| `POST /api/offramp/paycrest/order`  | See `paycrestOrderLimiter` config            |
| `/api/v1/*` with API keys           | Per-key limit from the stored API key record |

Rate-limited responses return `429` with a `Retry-After` header (seconds) and `X-Request-Id`.

---

## Idempotency

All **mutating** endpoints that change money or state **require** an `Idempotency-Key` header. Read-only endpoints (GET, and POST routes that only fetch data) do not.

### Enforced Endpoints

| Endpoint                                  | Method | Reason                                                           |
| ----------------------------------------- | ------ | ---------------------------------------------------------------- |
| `POST /api/offramp/paycrest/order`        | POST   | Creates Paycrest payout order — replays original response        |
| `POST /api/offramp/execute-payout`        | POST   | Saves transaction record — replays original creation             |
| `POST /api/offramp/bridge/submit-soroban` | POST   | Submits on-chain Soroban transaction — prevents double-submit    |
| `POST /api/offramp/reverse`               | POST   | Initiates a reversal — prevents duplicate reversal requests      |
| `PATCH /api/offramp/reverse`              | PATCH  | Approves/rejects a reversal — prevents duplicate admin actions   |
| `POST /api/offramp/refund`                | POST   | Processes a refund — prevents issuing a refund twice             |
| `POST /api/offramp/insurance`             | POST   | Purchases insurance / files claim — prevents duplicate purchases |
| `POST /api/offramp/recurring`             | POST   | Creates recurring schedule — prevents duplicate schedules        |
| `POST /api/offramp/batch`                 | POST   | Creates/executes batch — prevents duplicate batch creation       |
| `POST /api/transactions`                  | POST   | Saves transaction — replays original write                       |
| `PATCH /api/transactions/[id]`            | PATCH  | Updates transaction — replays original update                    |
| `POST /api/onramp/order`                  | POST   | Creates on-ramp order — prevents duplicate orders                |

### Contract

1. **Requirement:** Clients **must** send an `Idempotency-Key` header (e.g., a UUID v4) on every mutating request.
2. **First request:** Processed normally; response is cached. Response includes `Idempotency-Status: created`.
3. **Replay (same key, same body):** Returns the cached response from the first request. Response includes `Idempotency-Status: replayed`.
4. **Conflict (same key, different body):** Returns `409 Conflict`. Response includes `Idempotency-Status: conflict`.
5. **In-progress (same key, request still processing):** Returns `409 Conflict` with `Idempotency-Status: conflict`.
6. **5xx responses are not cached** — clients can safely retry transient failures with the same key.
7. **TTL:** Completed entries expire after `IDEMPOTENCY_TTL_MS` (default: 24h). In-progress locks expire after `IDEMPOTENCY_LOCK_TTL_MS` (default: 5min).
8. **Expired entries** are cleaned up on each idempotency check via `DELETE FROM idempotency_keys WHERE expires_at <= $1`.
9. **Key scope:** The key is scoped to `(idempotency_key, method, path)` — the same key can be reused across different endpoints.

### Example

```bash
curl -X POST http://localhost:3001/api/offramp/paycrest/order \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: order-create-9d9f2b9d-1" \
  -d '{
    "amount": 100,
    "rate": 1600,
    "token": "USDC",
    "network": "stellar",
    "reference": "ref-001",
    "returnAddress": "0xreturnAddress",
    "recipient": {
      "institution": "ACCESS",
      "accountIdentifier": "1234567890",
      "accountName": "John Doe",
      "currency": "NGN"
    }
  }'
```

Responses include:

- `Idempotency-Key`: the key used for the request
- `Idempotency-Status: created` for the first successful processing
- `Idempotency-Status: replayed` for a cached replay
- `Idempotency-Status: conflict` for mismatched or in-progress reuse

---

## Error Format

All error responses follow this shape:

```json
{ "error": "Human-readable message" }
```

Validation errors from `POST /api/offramp/paycrest/order` include a `details` map:

```json
{
  "error": "Validation failed",
  "details": {
    "amount": "amount must be a positive number",
    "recipient.institution": "recipient.institution is required and must be a string"
  }
}
```

---

## Endpoints

### GET /api/health

Returns service health and version.

**Response `200`**

```json
{
  "status": "ok",
  "timestamp": "2026-04-22T22:30:11.068Z",
  "version": "1.0.0"
}
```

```bash
curl http://localhost:3001/api/health
```

---

### GET /api/notifications/preferences

Returns stored notification preferences for a wallet address.

**Query Parameters**

| Parameter     | Type   | Required | Description                                  |
| ------------- | ------ | -------- | -------------------------------------------- |
| `userAddress` | string | ✅       | Wallet address tied to the transaction owner |

**Response `200`**

```json
{
  "data": {
    "userAddress": "GABC...",
    "email": "user@example.com",
    "phoneNumber": "+2348000000000",
    "emailEnabled": true,
    "smsEnabled": false,
    "notifyOnPending": true,
    "notifyOnCompleted": true,
    "notifyOnFailed": true
  }
}
```

If no preferences exist yet, the route returns `{ "data": null }`.

---

### PUT /api/notifications/preferences

Creates or updates notification preferences for a wallet address.

**Request Body**

```json
{
  "userAddress": "GABC...",
  "email": "user@example.com",
  "phoneNumber": "+2348000000000",
  "emailEnabled": true,
  "smsEnabled": false,
  "notifyOnPending": true,
  "notifyOnCompleted": true,
  "notifyOnFailed": true
}
```

**Response `200`**

```json
{
  "data": {
    "userAddress": "GABC...",
    "email": "user@example.com",
    "phoneNumber": "+2348000000000",
    "emailEnabled": true,
    "smsEnabled": false,
    "notifyOnPending": true,
    "notifyOnCompleted": true,
    "notifyOnFailed": true
  }
}
```

---

### GET /api/transactions/[id]/notifications

Returns notification delivery tracking records for a single transaction.

**Response `200`**

```json
{
  "data": [
    {
      "id": "delivery-uuid",
      "transactionId": "tx_123",
      "channel": "email",
      "status": "sent",
      "templateId": "transaction-completed-v1",
      "destination": "user@example.com"
    }
  ]
}
```

---

### GET /api/offramp/currencies

Returns supported fiat currencies. Cached for 5 minutes.

**Response `200`**

```json
{
  "data": [
    { "code": "NGN", "name": "Nigerian Naira", "symbol": "₦" },
    { "code": "KES", "name": "Kenyan Shilling", "symbol": "KSh" }
  ]
}
```

**Errors**

| Status | Meaning                  |
| ------ | ------------------------ |
| `500`  | Paycrest API unreachable |

```bash
curl http://localhost:3001/api/offramp/currencies
```

---

### GET /api/offramp/institutions/[currency]

Returns supported banks/institutions for a given fiat currency code.

**Path Parameters**

| Parameter  | Type   | Description                    |
| ---------- | ------ | ------------------------------ |
| `currency` | string | Fiat currency code, e.g. `NGN` |

**Response `200`**

```json
[
  { "code": "ACCESS", "name": "Access Bank" },
  { "code": "GTB", "name": "Guaranty Trust Bank" }
]
```

**Errors**

| Status | Meaning                         |
| ------ | ------------------------------- |
| `400`  | Unsupported or unknown currency |
| `500`  | Internal server error           |

```bash
curl http://localhost:3001/api/offramp/institutions/NGN
```

---

### POST /api/offramp/quote

Fetches a conversion quote: Stellar USDC → fiat via Allbridge + Paycrest.

**Request Body**

```json
{
  "amount": "100",
  "currency": "NGN",
  "feeMethod": "USDC"
}
```

| Field       | Type   | Required | Description                                         |
| ----------- | ------ | -------- | --------------------------------------------------- |
| `amount`    | string | ✅       | USDC amount to convert                              |
| `currency`  | string | ✅       | Target fiat currency code                           |
| `feeMethod` | string | ✅       | `"USDC"` \| `"XLM"` \| `"stablecoin"` \| `"native"` |

**Response `200`**

```json
{
  "destinationAmount": "155000.00",
  "rate": 1550.0,
  "currency": "NGN",
  "expiresIn": 300
}
```

**Errors**

| Status | Meaning                                                |
| ------ | ------------------------------------------------------ |
| `400`  | Invalid amount, missing currency, or invalid feeMethod |
| `502`  | Allbridge or Paycrest quote unavailable                |
| `500`  | Unexpected server error                                |

```bash
curl -X POST http://localhost:3001/api/offramp/quote \
  -H "Content-Type: application/json" \
  -d '{"amount":"100","currency":"NGN","feeMethod":"USDC"}'
```

---

### POST /api/offramp/verify-account

Verifies a beneficiary bank account via Paycrest.

**Request Body**

```json
{
  "institution": "ACCESS",
  "accountIdentifier": "0123456789"
}
```

| Field               | Type   | Required | Description                                      |
| ------------------- | ------ | -------- | ------------------------------------------------ |
| `institution`       | string | ✅       | Institution code from `/institutions/[currency]` |
| `accountIdentifier` | string | ✅       | Account number or identifier                     |

**Response `200`**

```json
{ "accountName": "John Doe" }
```

**Errors**

| Status | Meaning                                     |
| ------ | ------------------------------------------- |
| `400`  | Missing fields or account not found         |
| `502`  | Paycrest upstream error (5xx from Paycrest) |
| `500`  | Internal server error                       |

```bash
curl -X POST http://localhost:3001/api/offramp/verify-account \
  -H "Content-Type: application/json" \
  -d '{"institution":"ACCESS","accountIdentifier":"0123456789"}'
```

---

### GET /api/offramp/bridge/gas-fee-options

Returns Allbridge gas fee options for the Stellar → Base USDC bridge. Cached for 60 seconds.

**Response `200`**

```json
{
  "feeOptions": {
    "native": { "int": "1000000", "float": "1.0" },
    "stablecoin": { "int": "500000", "float": "0.5" }
  }
}
```

`native` = XLM fee, `stablecoin` = USDC fee.

**Errors**

| Status | Meaning                              |
| ------ | ------------------------------------ |
| `502`  | Allbridge SDK timeout or unavailable |
| `500`  | Chain details or token not found     |

```bash
curl http://localhost:3001/api/offramp/bridge/gas-fee-options
```

---

### POST /api/offramp/bridge/build-tx

Builds an unsigned Soroban XDR transaction for bridging USDC from Stellar to Base via Allbridge. Rate-limited per IP.

**Request Body**

```json
{
  "amount": "99.5",
  "fromAddress": "GABC...XYZ",
  "toAddress": "0xabc...def",
  "feePaymentMethod": "stablecoin"
}
```

| Field              | Type   | Required | Description                            |
| ------------------ | ------ | -------- | -------------------------------------- |
| `amount`           | string | ✅       | USDC amount to bridge                  |
| `fromAddress`      | string | ✅       | Stellar (G...) sender address          |
| `toAddress`        | string | ✅       | Base (0x...) recipient address         |
| `feePaymentMethod` | string |          | `"stablecoin"` (default) or `"native"` |

**Response `200`**

```json
{
  "xdr": "AAAAAgAAAA...",
  "sourceToken": {
    "symbol": "USDC",
    "decimals": 7,
    "contract": "CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75",
    "chain": "SRB"
  },
  "destinationToken": {
    "symbol": "USDC",
    "decimals": 6,
    "contract": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
    "chain": "ETH"
  }
}
```

**Errors**

| Status | Meaning                                                       |
| ------ | ------------------------------------------------------------- |
| `400`  | Invalid amount, address, or feePaymentMethod                  |
| `429`  | Rate limit exceeded — check `Retry-After` header              |
| `500`  | Simulation failed, insufficient balance, or chain unavailable |

```bash
curl -X POST http://localhost:3001/api/offramp/bridge/build-tx \
  -H "Content-Type: application/json" \
  -d '{
    "amount": "99.5",
    "fromAddress": "GABC...XYZ",
    "toAddress": "0xabc...def",
    "feePaymentMethod": "stablecoin"
  }'
```

---

### POST /api/offramp/bridge/submit-soroban

Submits a signed Stellar XDR transaction to the Soroban RPC.

**Request Body**

```json
{ "signedXdr": "AAAAAgAAAA..." }
```

| Field       | Type   | Required | Description                            |
| ----------- | ------ | -------- | -------------------------------------- |
| `signedXdr` | string | ✅       | Signed transaction XDR from the wallet |

**Response `200`**

```json
{ "status": "PENDING", "hash": "abc123..." }
```

| `status`  | Meaning                                     |
| --------- | ------------------------------------------- |
| `PENDING` | Transaction accepted, awaiting confirmation |
| `SUCCESS` | Transaction confirmed on-chain              |

**Errors**

| Status | Meaning                                                 |
| ------ | ------------------------------------------------------- |
| `400`  | Missing `signedXdr`, RPC error, or transaction rejected |
| `500`  | Soroban RPC not configured or unreachable               |

```bash
curl -X POST http://localhost:3001/api/offramp/bridge/submit-soroban \
  -H "Content-Type: application/json" \
  -d '{"signedXdr":"AAAAAgAAAA..."}'
```

---

### GET /api/offramp/bridge/tx-status/[hash]

Polls the Soroban RPC for a submitted transaction's confirmation status.

**Path Parameters**

| Parameter | Type   | Description                            |
| --------- | ------ | -------------------------------------- |
| `hash`    | string | Transaction hash from `submit-soroban` |

**Response `200`**

```json
{ "status": "SUCCESS", "hash": "abc123..." }
```

| `status`    | Meaning                         |
| ----------- | ------------------------------- |
| `SUCCESS`   | Transaction confirmed           |
| `FAILED`    | Transaction failed on-chain     |
| `NOT_FOUND` | Not yet indexed or invalid hash |

**Errors**

| Status | Meaning                                   |
| ------ | ----------------------------------------- |
| `400`  | RPC returned an error                     |
| `500`  | Soroban RPC not configured or unreachable |

```bash
curl http://localhost:3001/api/offramp/bridge/tx-status/abc123...
```

---

### GET /api/offramp/bridge/status/[txHash]

Polls Allbridge for the cross-chain bridge transfer status.

**Path Parameters**

| Parameter | Type   | Description              |
| --------- | ------ | ------------------------ |
| `txHash`  | string | Stellar transaction hash |

**Response `200`**

```json
{
  "data": {
    "status": "completed",
    "txHash": "abc123...",
    "receiveAmount": "99.0"
  }
}
```

| `status`     | Meaning                               |
| ------------ | ------------------------------------- |
| `pending`    | Bridge not yet picked up the transfer |
| `processing` | Transfer in progress                  |
| `completed`  | USDC arrived on Base                  |
| `failed`     | Bridge transfer failed                |
| `expired`    | Transfer expired                      |

**Errors**

| Status | Meaning             |
| ------ | ------------------- |
| `500`  | Allbridge SDK error |

```bash
curl http://localhost:3001/api/offramp/bridge/status/abc123...
```

---

### POST /api/offramp/paycrest/order

Creates a Paycrest fiat payout order. Rate-limited per IP.

**Request Body**

```json
{
  "amount": 99.0,
  "rate": 1550.0,
  "token": "USDC",
  "network": "base",
  "reference": "ref-unique-123",
  "returnAddress": "0xabc...def",
  "recipient": {
    "institution": "ACCESS",
    "accountIdentifier": "0123456789",
    "accountName": "John Doe",
    "currency": "NGN"
  }
}
```

| Field                         | Type   | Required | Description                      |
| ----------------------------- | ------ | -------- | -------------------------------- |
| `amount`                      | number | ✅       | USDC amount (positive)           |
| `rate`                        | number | ✅       | FX rate from `/quote` (positive) |
| `token`                       | string | ✅       | Token symbol, e.g. `"USDC"`      |
| `network`                     | string | ✅       | Chain name, e.g. `"base"`        |
| `reference`                   | string | ✅       | Unique reference string          |
| `returnAddress`               | string | ✅       | Base address for refunds         |
| `recipient.institution`       | string | ✅       | Bank institution code            |
| `recipient.accountIdentifier` | string | ✅       | Account number                   |
| `recipient.accountName`       | string | ✅       | Verified account name            |
| `recipient.currency`          | string | ✅       | Fiat currency code               |

**Response `200`**

```json
{
  "data": {
    "id": "order-uuid",
    "receiveAddress": "0xpaycrest...address"
  }
}
```

**Errors**

| Status | Meaning                                          |
| ------ | ------------------------------------------------ |
| `400`  | Validation failed — see `details` map            |
| `429`  | Rate limit exceeded — check `Retry-After` header |
| `4xx`  | Paycrest rejected the order                      |
| `500`  | Internal server error                            |

```bash
curl -X POST http://localhost:3001/api/offramp/paycrest/order \
  -H "Content-Type: application/json" \
  -d '{
    "amount": 99.0,
    "rate": 1550.0,
    "token": "USDC",
    "network": "base",
    "reference": "ref-unique-123",
    "returnAddress": "0xabc...def",
    "recipient": {
      "institution": "ACCESS",
      "accountIdentifier": "0123456789",
      "accountName": "John Doe",
      "currency": "NGN"
    }
  }'
```

---

### GET /api/offramp/paycrest/order/[orderId]

Fetches the status of a Paycrest payout order.

**Path Parameters**

| Parameter | Type   | Description                                      |
| --------- | ------ | ------------------------------------------------ |
| `orderId` | string | Order ID from `POST /api/offramp/paycrest/order` |

**Response `200`**

```json
{
  "data": {
    "id": "order-uuid",
    "status": "pending"
  }
}
```

**Errors**

| Status | Meaning               |
| ------ | --------------------- |
| `400`  | Missing orderId       |
| `404`  | Order not found       |
| `500`  | Internal server error |

```bash
curl http://localhost:3001/api/offramp/paycrest/order/order-uuid
```

---

### GET /api/offramp/status/[orderId]

Polls Paycrest order status using Bearer token auth (alternative to the adapter-based route above).

**Path Parameters**

| Parameter | Type   | Description       |
| --------- | ------ | ----------------- |
| `orderId` | string | Paycrest order ID |

**Response `200`**

```json
{ "status": "pending", "id": "order-uuid" }
```

**Errors**

| Status    | Meaning                 |
| --------- | ----------------------- |
| `4xx/5xx` | Forwarded from Paycrest |
| `500`     | Internal server error   |

```bash
curl http://localhost:3001/api/offramp/status/order-uuid
```

---

### POST /api/webhooks/paycrest

Receives Paycrest event webhooks. Verifies the `X-Paycrest-Signature` HMAC-SHA256 header before processing.

**Headers**

| Header                 | Required | Description                                                                          |
| ---------------------- | -------- | ------------------------------------------------------------------------------------ |
| `X-Paycrest-Signature` | ✅       | HMAC-SHA256 hex digest of the raw request body, keyed with `PAYCREST_WEBHOOK_SECRET` |

**Request Body** (example)

```json
{
  "event": "payment_order.settled",
  "data": { "id": "order-uuid" }
}
```

**Response `200`**

```json
{ "received": true }
```

**Errors**

| Status | Meaning                      |
| ------ | ---------------------------- |
| `401`  | Invalid or missing signature |
| `400`  | Malformed JSON payload       |

---

## Typical Off-Ramp Flow

```
1. GET  /api/offramp/currencies              → pick fiat currency
2. GET  /api/offramp/institutions/[currency] → pick bank
3. POST /api/offramp/verify-account          → confirm account name
4. POST /api/offramp/quote                   → get FX rate + destination amount
5. GET  /api/offramp/bridge/gas-fee-options  → pick fee method
6. POST /api/offramp/bridge/build-tx         → get unsigned XDR
   (wallet signs XDR)
7. POST /api/offramp/bridge/submit-soroban   → submit signed XDR → get txHash
8. GET  /api/offramp/bridge/tx-status/[hash] → poll until SUCCESS
9. POST /api/offramp/paycrest/order          → create payout order → get orderId + receiveAddress
   (bridge delivers USDC to receiveAddress)
10. GET /api/offramp/bridge/status/[txHash]  → poll until completed
11. GET /api/offramp/paycrest/order/[id]     → poll until settled
```
