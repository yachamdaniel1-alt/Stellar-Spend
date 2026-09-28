# Stellar-Spend v1 API Reference

> **Base URL:** `https://api.stellar-spend.com` (production) · `http://localhost:3001` (development)
>
> **Interactive explorer:** Visit **[/api/docs](/api/docs)** for the Swagger UI.
> The complete OpenAPI 3.0 specification is available at [`/openapi.yaml`](/openapi.yaml).
>
> **Version:** `v1.0.0` — Published 2026-09-27
>
> **Spec stats:** 70 paths · 84 operations across all versions

---

## Table of Contents

- [Authentication](#authentication)
- [Rate Limits](#rate-limits)
- [Error Handling](#error-handling)
- [Namespaces](#namespaces)
  - [Health](#health)
  - [FX Rates](#fx-rates)
  - [Offramp](#offramp)
    - [Currencies](#currencies)
    - [Institutions](#institutions)
    - [Quote](#quote)
    - [Verify Account](#verify-account)
    - [Rate](#rate)
    - [Bridge](#bridge)
    - [Paycrest](#paycrest)
    - [Status](#status)
    - [Fees](#fees)
    - [Quote Aggregate](#quote-aggregate)
    - [Reconciliation](#reconciliation)
  - [Sync](#sync)
  - [Webhooks](#webhooks)
- [Changelog](#changelog)

---

## Authentication

All `/api/v1/*` endpoints require an API key. Pass your key via either:

| Header | Example |
|--------|---------|
| `X-API-Key: <key>` | `X-API-Key: sk_live_abc123` |
| `Authorization: Bearer <key>` | `Authorization: Bearer sk_live_abc123` |

API keys are issued with one or more [scopes](#api-key-scopes) that restrict which endpoints they can access. Keys can be managed via `GET /api/api-keys` and `POST /api/api-keys`.

### API Key Scopes

| Scope | Endpoints |
|-------|-----------|
| `offramp:read` | GET currencies, institutions, rate, bridge status/tx-status, quote |
| `offramp:write` | POST quote, build-tx, submit-soroban, paycrest order, fees |
| `sync:read` | GET sync/history, sync/settings |
| `sync:write` | POST sync/history, sync/settings |
| `webhook:read` | Receive and replay webhook events |

Scopes are assigned at key creation and can be inspected via `GET /api/api-keys/{id}/scopes`.

---

## Rate Limits

Rate limits are applied per API key and per IP address depending on the endpoint. Response headers are included on rate-limited endpoints:

| Header | Description |
|--------|-------------|
| `X-RateLimit-Limit` | Maximum requests in the current window |
| `X-RateLimit-Remaining` | Requests remaining in the current window |
| `X-RateLimit-Reset` | Unix timestamp when the window resets |
| `Retry-After` | Seconds to wait before retrying (on 429 responses) |

Key endpoints and their limits:

| Endpoint | Limit |
|----------|-------|
| `POST /api/v1/offramp/execute-payout` | 5 req/min per IP |
| `POST /api/v1/offramp/paycrest/order` | Rate-limited per IP |
| `POST /api/v1/offramp/bridge/build-tx` | Rate-limited per IP |

---

## Error Handling

All error responses follow a consistent format:

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable description",
    "details": { }
  }
}
```

| Status Code | Meaning |
|-------------|---------|
| `400` | Bad request — validation failed or missing parameter |
| `401` | Unauthorized — invalid or missing credentials |
| `409` | Conflict — idempotency key conflict or resource already exists |
| `429` | Rate limit exceeded |
| `500` | Internal server error |
| `502` | Upstream service unavailable (Paycrest, Allbridge, etc.) |

---

## Namespaces

### Health

#### `GET /api/v1/health`

Service health check. Returns the current operational status of the service.

**Authentication:** Not required

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Service is healthy | [`HealthResponse`](#schemas) |

**Example response:**

```json
{
  "status": "ok",
  "timestamp": "2026-09-27T00:00:00Z"
}
```

**Route handler:** `src/app/api/v1/health/route.ts` — Proxies to `src/app/api/health/route.ts`

---

### FX Rates

#### `GET /api/v1/fx-rates`

Get current foreign exchange rates for supported currencies.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Parameters:** None

**Responses:**

| Code | Description |
|------|-------------|
| `200` | FX rate data |
| `401` | Invalid or missing API key |

**Route handler:** `src/app/api/v1/fx-rates/route.ts` — Wraps `src/app/api/fx-rates/route.ts` with `withApiKeyAuth`

---

### Offramp

The offramp namespace handles USDC-to-fiat conversion, including currency discovery, quoting, bridge transactions, and payout execution.

#### `GET /api/v1/offramp/currencies`

List supported fiat currencies for off-ramping. Response is cached for 5 minutes.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | List of currencies | Object with `data` array of [`Currency`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Paycrest API unreachable | [`Error`](#schemas) |

**Example response:**

```json
{
  "data": [
    { "code": "NGN", "name": "Nigerian Naira", "symbol": "₦" },
    { "code": "KES", "name": "Kenyan Shilling", "symbol": "KSh" }
  ]
}
```

**Route handler:** `src/app/api/v1/offramp/currencies/route.ts` — Wraps `src/app/api/offramp/currencies/route.ts` with `withApiKeyAuth`

---

#### `GET /api/v1/offramp/institutions/{currency}`

List banks and institutions available for a specific fiat currency.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Path Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `currency` | string | Yes | ISO 4217 currency code | `NGN` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | List of institutions | Array of [`Institution`](#schemas) |
| `400` | Unsupported or unknown currency | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/institutions/[currency]/route.ts` — Wraps `src/app/api/offramp/institutions/[currency]/route.ts` with `withApiKeyAuth`

---

#### `POST /api/v1/offramp/quote`

Get a USDC → fiat conversion quote.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** [`QuoteRequest`](#schemas)

| Field | Type | Required | Description | Example |
|-------|------|----------|-------------|---------|
| `amount` | string | Yes | Amount in USDC (cents) | `"100"` |
| `currency` | string | Yes | Target fiat currency | `NGN` |
| `feeMethod` | string | Yes | Fee payment method | `USDC` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Quote result | [`QuoteResponse`](#schemas) |
| `400` | Invalid parameters | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `502` | Upstream unavailable | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/quote/route.ts` — Wraps `src/app/api/offramp/quote/route.ts` with `withApiKeyAuth`

---

#### `POST /api/v1/offramp/verify-account`

Verify a beneficiary bank account before initiating a payout.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** [`VerifyAccountRequest`](#schemas)

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Verified account name | [`VerifyAccountResponse`](#schemas) |
| `400` | Missing fields or account not found | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/verify-account/route.ts` — Wraps `src/app/api/offramp/verify-account/route.ts` with `withApiKeyAuth`

---

#### `GET /api/v1/offramp/rate`

Get the current live spot FX conversion rate.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Parameters:** None

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Spot rate | Object with `rate` (number) and `pair` (string) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Example response:**

```json
{
  "rate": 1598,
  "pair": "USDC/NGN"
}
```

**Route handler:** `src/app/api/v1/offramp/rate/route.ts` — Wraps `src/app/api/offramp/rate/route.ts` with `withApiKeyAuth`

---

#### Bridge

The bridge sub-namespace handles cross-chain transactions via Allbridge and Soroban.

##### `GET /api/v1/offramp/bridge/gas-fee-options`

Get available Allbridge gas fee options. Cached for 60 seconds.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Available fee options | [`GasFeeOptions`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/bridge/gas-fee-options/route.ts` — Wraps `src/app/api/offramp/bridge/gas-fee-options/route.ts` with `withApiKeyAuth`

---

##### `POST /api/v1/offramp/bridge/build-tx`

Build an unsigned Soroban XDR transaction for a bridge transfer. Rate-limited per IP. Returns an unsigned XDR for the wallet to sign.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** [`BuildTxRequest`](#schemas)

**Response Headers:**

| Header | Description |
|--------|-------------|
| `X-Request-Id` | Unique request identifier |
| `X-RateLimit-Limit` | Max requests in window |
| `X-RateLimit-Remaining` | Requests remaining |
| `X-RateLimit-Reset` | Window reset timestamp |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Unsigned XDR transaction | [`BuildTxResponse`](#schemas) |
| `400` | Invalid parameters | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `429` | Rate limit exceeded | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/bridge/build-tx/route.ts` — Wraps `src/app/api/offramp/bridge/build-tx/route.ts` with `withApiKeyAuth`

---

##### `POST /api/v1/offramp/bridge/submit-soroban`

Submit a signed Soroban XDR transaction for on-chain execution.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** [`SubmitSorobanRequest`](#schemas)

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Transaction accepted | [`TxStatusResponse`](#schemas) |
| `400` | RPC error or transaction rejected | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/bridge/submit-soroban/route.ts` — Wraps `src/app/api/offramp/bridge/submit-soroban/route.ts` with `withApiKeyAuth`

---

##### `GET /api/v1/offramp/bridge/tx-status/{hash}`

Poll Stellar on-chain status for a Soroban transaction by its hash.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Path Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `hash` | string | Yes | Soroban transaction hash | `abc123def456` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Stellar tx status | [`TxStatusResponse`](#schemas) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/bridge/tx-status/[hash]/route.ts`

---

##### `GET /api/v1/offramp/bridge/status/{txHash}`

Poll Allbridge for cross-chain transfer status.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Path Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `txHash` | string | Yes | Allbridge transaction hash | `abc123def456` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Bridge transfer status | [`BridgeStatusResponse`](#schemas) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/bridge/status/[txHash]/route.ts`

---

#### Paycrest

The Paycrest sub-namespace handles fiat payout orders via the Paycrest payment processor.

##### `POST /api/v1/offramp/paycrest/order`

Create a Paycrest fiat payout order. Rate-limited per IP. Supports `Idempotency-Key` header for safe retries.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** [`CreateOrderRequest`](#schemas)

**Idempotency:** Pass an `Idempotency-Key` header to safely retry. First call with a given key returns `Idempotency-Status: created`. Subsequent calls with the same key replay the saved response (`replayed`). A key conflict with a different body returns `409`.

**Response Headers:**

| Header | Description |
|--------|-------------|
| `X-Request-Id` | Unique request identifier |
| `X-RateLimit-Limit` | Max requests in window |
| `X-RateLimit-Remaining` | Requests remaining |
| `X-RateLimit-Reset` | Window reset timestamp |
| `Idempotency-Key` | Echo of the request key |
| `Idempotency-Status` | `created`, `replayed`, or `conflict` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `201` | Order created | [`CreateOrderResponse`](#schemas) |
| `400` | Validation error | [`ValidationError`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `409` | Idempotency key conflict | [`Error`](#schemas) |
| `429` | Rate limit exceeded | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/paycrest/order/route.ts` — Wraps `src/app/api/offramp/paycrest/order/route.ts` with `withApiKeyAuth`

---

##### `GET /api/v1/offramp/paycrest/order/{orderId}`

Get the status of a Paycrest payout order.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Path Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `orderId` | string | Yes | Order identifier | `order-uuid` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Order status | [`OrderStatusResponse`](#schemas) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** Not present in `src/app/api/v1/offramp/paycrest/order/` as a separate GET handler. The POST-only route file handles order creation. The GET endpoint is handled by `src/app/api/offramp/paycrest/order/[orderId]/route.ts`.

---

#### Status

##### `GET /api/v1/offramp/status/{orderId}`

Poll the complete status of an off-ramp order.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Path Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `orderId` | string | Yes | Order identifier | `order-uuid` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Off-ramp status | [`OfframpStatusResponse`](#schemas) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/status/[orderId]/route.ts` — Wraps `src/app/api/offramp/status/[orderId]/route.ts` with `withApiKeyAuth`

---

#### Other Offramp Endpoints

##### `POST /api/v1/offramp/fees`

Calculate off-ramp fee breakdown (network gas, bridge fee, and fiat payout fee).

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** Object with `amount` (string, required), `currency` (string, required), `feeMethod` (string, optional: `USDC`, `XLM`, `stablecoin`, `native`), `receiveAmount` (string, optional).

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Fee breakdown | Object with `bridgeFee`, `payoutFee`, `totalFee` (all strings) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/fees/route.ts`

---

##### `POST /api/v1/offramp/quote-aggregate`

Aggregate conversion quotes across multiple bridge and payment providers for rate comparison.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** [`QuoteRequest`](#schemas)

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Aggregate quotes | Object with `quotes` array of [`QuoteResponse`](#schemas) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/offramp/quote-aggregate/route.ts`

---

##### `POST /api/v1/offramp/reconciliation`

Submit reconciliation data for off-ramp transactions.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Route handler:** `src/app/api/v1/offramp/reconciliation/route.ts`

---

### Sync

The sync namespace provides client-side transaction history synchronization.

#### `GET /api/v1/sync/history`

Fetch transaction history for the authenticated user's wallet.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Query Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `wallet` | string | Yes | Wallet address | `GABC...ABCD` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Synced transaction history | Object with `success`, `transactions`, `timestamp`, `wallet` |
| `400` | Missing wallet address | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/sync/history/route.ts` — Direct implementation using `dal.getByUser()` and `withIdempotency`

---

#### `POST /api/v1/sync/history`

Sync transaction history from the client to the server. Accepts multiple transactions and merges them using a last-write-wins conflict resolution strategy.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** Object with `wallet` (string, required), `transactions` (array of [`Transaction`](#schemas), required), `timestamp` (number, required).

**Idempotency:** Wrapped in `withIdempotency` middleware.

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Sync confirmed | Object with `success`, `synced`, `conflicts`, `conflictDetails`, `timestamp` |
| `400` | Validation error | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |
| `500` | Internal server error | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/sync/history/route.ts` — Direct implementation

---

#### `GET /api/v1/sync/settings`

Fetch sync settings for the authenticated user.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Query Parameters:**

| Name | Type | Required | Description | Example |
|------|------|----------|-------------|---------|
| `wallet` | string | Yes | Wallet address | `GABC...ABCD` |

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Settings configuration | Object with `settings` (wallet, syncEnabled, conflictResolutionStrategy, updatedAt) |
| `400` | Bad request | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/sync/settings/route.ts` — Direct implementation

---

#### `POST /api/v1/sync/settings`

Update sync settings for the authenticated user.

**Authentication:** Required (`ApiKeyHeader` or `BearerAuth`)

**Request Body:** Object with `wallet` (string, required), `syncEnabled` (boolean, required).

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Settings saved | Object with `success`, `settings`, `timestamp` |
| `400` | Validation error | [`Error`](#schemas) |
| `401` | Unauthorized | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/sync/settings/route.ts` — Direct implementation

---

### Webhooks

#### `POST /api/v1/webhooks/paycrest`

Receive Paycrest payment event webhooks. Requests are verified using HMAC-SHA256 via the `X-Paycrest-Signature` header.

**Authentication:** HMAC Signature (`PaycrestSignature`) — No API key required

**Request Body:** Object with `event` (string, e.g., `payment_order.settled`) and `data` (object with `id`, etc.).

**Responses:**

| Code | Description | Body |
|------|-------------|------|
| `200` | Webhook received | Object with `received: true` |
| `401` | Invalid signature | [`Error`](#schemas) |

**Route handler:** `src/app/api/v1/webhooks/paycrest/route.ts` — Re-exports `src/app/api/webhooks/paycrest/route.ts`

---

## Endpoints Summary

### Complete v1 Endpoint List

| Method | Path | Namespace | Auth | Operation ID |
|--------|------|-----------|------|--------------|
| GET | `/api/v1/health` | health | None | `getHealthV1` |
| GET | `/api/v1/fx-rates` | fx-rates | API Key / Bearer | `getFxRatesV1` |
| GET | `/api/v1/offramp/currencies` | offramp | API Key / Bearer | `getCurrenciesV1` |
| GET | `/api/v1/offramp/institutions/{currency}` | offramp | API Key / Bearer | `getInstitutionsV1` |
| POST | `/api/v1/offramp/quote` | offramp | API Key / Bearer | `getQuoteV1` |
| POST | `/api/v1/offramp/verify-account` | offramp | API Key / Bearer | `verifyAccountV1` |
| GET | `/api/v1/offramp/rate` | offramp | API Key / Bearer | `getSpotRateV1` |
| GET | `/api/v1/offramp/bridge/gas-fee-options` | offramp/bridge | API Key / Bearer | `getGasFeeOptionsV1` |
| POST | `/api/v1/offramp/bridge/build-tx` | offramp/bridge | API Key / Bearer | `buildBridgeTxV1` |
| POST | `/api/v1/offramp/bridge/submit-soroban` | offramp/bridge | API Key / Bearer | `submitSorobanV1` |
| GET | `/api/v1/offramp/bridge/tx-status/{hash}` | offramp/bridge | API Key / Bearer | `getStellarTxStatusV1` |
| GET | `/api/v1/offramp/bridge/status/{txHash}` | offramp/bridge | API Key / Bearer | `getBridgeStatusV1` |
| POST | `/api/v1/offramp/paycrest/order` | offramp/paycrest | API Key / Bearer | `createPaycrestOrderV1` |
| GET | `/api/v1/offramp/paycrest/order/{orderId}` | offramp/paycrest | API Key / Bearer | `getPaycrestOrderStatusV1` |
| GET | `/api/v1/offramp/status/{orderId}` | offramp | API Key / Bearer | `getOfframpOrderStatusV1` |
| POST | `/api/v1/offramp/fees` | offramp | API Key / Bearer | `calculateOfframpFeesV1` |
| POST | `/api/v1/offramp/quote-aggregate` | offramp | API Key / Bearer | `getAggregateQuoteV1` |
| POST | `/api/v1/offramp/reconciliation` | offramp | API Key / Bearer | — |
| GET | `/api/v1/sync/history` | sync | API Key / Bearer | `getSyncHistory` |
| POST | `/api/v1/sync/history` | sync | API Key / Bearer | `pushSyncHistory` |
| GET | `/api/v1/sync/settings` | sync | API Key / Bearer | `getSyncSettings` |
| POST | `/api/v1/sync/settings` | sync | API Key / Bearer | `saveSyncSettings` |
| POST | `/api/v1/webhooks/paycrest` | webhooks | HMAC Signature | `receivePaycrestWebhookV1` |

### Notable Non-V1 Endpoints

The following endpoints exist in the non-versioned API but do **not** have corresponding `/api/v1/` routes in the current deployment:

| Method | Path | Notes |
|--------|------|-------|
| POST | `/api/offramp/execute-payout` | Base API endpoint exists at `src/app/api/offramp/execute-payout/route.ts`. The `/api/v1/offramp/execute-payout` route is referenced in scope definitions (`src/lib/api-keys/scopes.ts`) but does not yet have a dedicated route handler file. |

---

## Changelog

| Version | Date | Changes |
|---------|------|---------|
| **1.0.0** | 2026-01-01 | Initial public release; versioned `/api/v1/` routes; API key auth; idempotency; webhook HMAC verification |
| **1.1.0** | 2026-06-29 | Added `X-RateLimit-*` response headers; expanded auth/scope/idempotency docs; interactive Swagger UI at `/api/docs` |

---

## Schema Reference

### Common Schemas

<details>
<summary><code>HealthResponse</code></summary>

```json
{
  "status": "ok",
  "timestamp": "2026-09-27T00:00:00Z"
}
```
</details>

<details>
<summary><code>Error</code></summary>

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable description"
  }
}
```
</details>

<details>
<summary><code>Currency</code></summary>

```json
{
  "code": "NGN",
  "name": "Nigerian Naira",
  "symbol": "₦"
}
```
</details>

<details>
<summary><code>Institution</code></summary>

```json
{
  "id": "string",
  "name": "string",
  "currency": "string"
}
```
</details>

<details>
<summary><code>QuoteRequest</code></summary>

```json
{
  "amount": "100",
  "currency": "NGN",
  "feeMethod": "USDC"
}
```
</details>

<details>
<summary><code>QuoteResponse</code></summary>

```json
{
  "amount": "100",
  "currency": "NGN",
  "rate": 1598,
  "fees": { "network": "0.50", "service": "1.00" }
}
```
</details>

<details>
<summary><code>VerifyAccountRequest</code></summary>

```json
{
  "accountNumber": "1234567890",
  "bankCode": "string"
}
```
</details>

<details>
<summary><code>VerifyAccountResponse</code></summary>

```json
{
  "accountName": "John Doe",
  "verified": true
}
```
</details>

<details>
<summary><code>BuildTxRequest</code></summary>

```json
{
  "amount": "100",
  "currency": "NGN",
  "destination": "GABC...ABCD"
}
```
</details>

<details>
<summary><code>BuildTxResponse</code></summary>

```json
{
  "xdr": "base64-encoded-xdr",
  "fee": "0.001"
}
```
</details>

<details>
<summary><code>SubmitSorobanRequest</code></summary>

```json
{
  "xdr": "signed-xdr-string"
}
```
</details>

<details>
<summary><code>TxStatusResponse</code></summary>

```json
{
  "status": "completed",
  "txHash": "0xabc123"
}
```
</details>

<details>
<summary><code>BridgeStatusResponse</code></summary>

```json
{
  "status": "completed",
  "txHash": "0xabc123",
  "amount": "100",
  "currency": "NGN"
}
```
</details>

<details>
<summary><code>CreateOrderRequest</code></summary>

```json
{
  "amount": "100",
  "currency": "NGN",
  "beneficiary": { "accountNumber": "123456", "bankCode": "044" }
}
```
</details>

<details>
<summary><code>CreateOrderResponse</code></summary>

```json
{
  "orderId": "order-uuid",
  "status": "pending"
}
```
</details>

<details>
<summary><code>OrderStatusResponse</code></summary>

```json
{
  "orderId": "order-uuid",
  "status": "settled",
  "amount": "100",
  "currency": "NGN"
}
```
</details>

<details>
<summary><code>OfframpStatusResponse</code></summary>

```json
{
  "status": "completed",
  "id": "order-uuid",
  "amount": "100",
  "currency": "NGN"
}
```
</details>

<details>
<summary><code>GasFeeOptions</code></summary>

```json
{
  "options": [
    { "network": "ethereum", "fee": "0.001" }
  ]
}
```
</details>

<details>
<summary><code>ValidationError</code></summary>

```json
{
  "errors": [
    { "field": "amount", "message": "Invalid amount" }
  ]
}
```
</details>

---

## Implementation Notes

### Route Handler Pattern

All v1 route handlers follow a consistent pattern: they import the base handler from `src/app/api/` and wrap it with authentication middleware:

```typescript
// Example: src/app/api/v1/offramp/quote/route.ts
import { POST as basePOST } from '@/app/api/offramp/quote/route';
import { withApiKeyAuth } from '@/lib/api-keys';

export async function POST(request: NextRequest) {
  return withApiKeyAuth(request, async () => basePOST(request));
}
```

The `withApiKeyAuth` middleware validates the API key from `X-API-Key` or `Authorization: Bearer` headers before forwarding to the base handler.

### Webhooks Exception

The `/api/v1/webhooks/paycrest` endpoint is the only v1 endpoint that does **not** use API key authentication. Instead, it validates incoming requests using HMAC-SHA256 signatures via the `X-Paycrest-Signature` header. It re-exports directly from the base handler.

### Sync Endpoints

The sync endpoints (`/api/v1/sync/history` and `/api/v1/sync/settings`) contain inline implementations rather than proxying to base handlers. They use `withIdempotency` middleware for the POST endpoints and `dal` for database operations. Note that authentication checks are marked as TODO in the current implementation.

### Execute-Payout Gap

The `POST /api/offramp/execute-payout` endpoint exists in the base API (`src/app/api/offramp/execute-payout/route.ts`) with full idempotency, KYC enforcement, and compliance screening, but the `/api/v1/offramp/execute-payout` route has not yet been implemented as a separate route handler file. The API key scopes file (`src/lib/api-keys/scopes.ts`) already defines the `/^\/api\/v1\/offramp\/execute-payout/` path pattern with `write:payouts` scope, indicating planned v1 support.
