# Idempotency Audit Report

## Issue #1129: Harden idempotency key usage across mutation endpoints

### Audit Summary
Date: August 27, 2026
Auditor: Kiro

### Mutation Endpoints Verified

#### 1. Offramp Execute Payout ✅
- **File**: `src/app/api/offramp/execute-payout/route.ts`
- **Status**: Idempotency implemented correctly
- **Implementation**: Uses `withIdempotency(request, handler, { required: true })`
- **Test Status**: Already uses required flag

#### 2. Onramp Order ✅
- **File**: `src/app/api/onramp/order/route.ts`
- **Status**: Idempotency implemented correctly
- **Implementation**: Uses `withIdempotency(request, handler, { required: true })`
- **Test Status**: Already uses required flag

#### 3. Offramp Refund ✅
- **File**: `src/app/api/offramp/refund/route.ts`
- **Status**: Idempotency implemented correctly
- **Implementation**: Uses `withIdempotency(request, handler, { required: true })`
- **Test Status**: Already uses required flag

#### 4. Offramp Reverse ✅
- **File**: `src/app/api/offramp/reverse/route.ts`
- **Status**: Idempotency implemented correctly
- **Implementation**: Uses `withIdempotency(request, handler, { required: true })` for POST and PATCH methods
- **Test Status**: Already uses required flag

### Additional Mutation Endpoints Checked

#### 5. Transactions ✅
- **File**: `src/app/api/transactions/route.ts`
- **Status**: Idempotency implemented correctly

#### 6. Transaction Updates ✅
- **File**: `src/app/api/transactions/[id]/route.ts`
- **Status**: Idempotency implemented correctly for PATCH

#### 7. Transaction Insurance ✅
- **File**: `src/app/api/transactions/[id]/insurance/route.ts`
- **Status**: Idempotency implemented correctly for POST and PATCH

#### 8. Transaction Disputes ✅
- **File**: `src/app/api/transactions/disputes/route.ts`
- **Status**: Idempotency implemented correctly

#### 9. Transaction Dispute Resolution ✅
- **File**: `src/app/api/transactions/disputes/resolve/route.ts`
- **Status**: Idempotency implemented correctly

#### 10. Transaction Dispute Escalation ✅
- **File**: `src/app/api/transactions/disputes/escalate/route.ts`
- **Status**: Idempotency implemented correctly

#### 11. Transaction Split ✅
- **File**: `src/app/api/transactions/split/route.ts`
- **Status**: Idempotency implemented correctly for POST and PATCH

#### 12. Webhooks Paycrest ✅
- **File**: `src/app/api/webhooks/paycrest/route.ts`
- **Status**: Idempotency implemented correctly

#### 13. Sync History ✅
- **File**: `src/app/api/v1/sync/history/route.ts`
- **Status**: Idempotency implemented correctly

### Endpoints Missing Idempotency

#### 1. Webhooks Subscriptions
- **File**: `src/app/api/webhooks/subscriptions/route.ts`
- **Issue**: POST method does not use idempotency
- **Recommendation**: Add `withIdempotency` with required flag

#### 2. Webhooks Subscriptions Replay
- **File**: `src/app/api/webhooks/subscriptions/[id]/replay/route.ts`
- **Issue**: POST method does not use idempotency
- **Recommendation**: Add `withIdempotency` with required flag

#### 3. Webhooks DLQ Replay
- **File**: `src/app/api/webhooks/dlq/replay/route.ts`
- **Issue**: POST method does not use idempotency
- **Recommendation**: Add `withIdempotency` with required flag

#### 4. Webhooks Dashboard
- **File**: `src/app/api/webhooks/dashboard/route.ts`
- **Issue**: POST method does not use idempotency
- **Recommendation**: Add `withIdempotency` with required flag

#### 5. Webhooks Retry Runner
- **File**: `src/app/api/webhooks/retry-runner/route.ts`
- **Issue**: POST method does not use idempotency
- **Recommendation**: Add `withIdempotency` with required flag

### Idempotency Implementation Details

The idempotency system is implemented in `src/lib/idempotency.ts` and includes:

1. **Database Storage**: Uses `idempotency_keys` table for persistence
2. **Request Hashing**: SHA-256 hash of method, path, and canonicalized body
3. **Locking**: Prevents concurrent processing of the same key
4. **Replay**: Returns cached response for duplicate requests
5. **Conflict Detection**: Returns 409 for same key with different payload
6. **TTL Management**: Automatic cleanup of expired records

### Test Coverage

Existing tests in `src/lib/idempotency.test.ts` cover:
- Same hash generation for semantically identical JSON
- First request processing and response storage
- Duplicate request replay
- Conflict detection for different payloads

### Recommendations

1. **Add idempotency to missing webhook endpoints** as identified above
2. **Create integration tests** for each mutation endpoint to verify idempotency behavior
3. **Consider adding audit logging** for idempotency key usage
4. **Review idempotency TTL** settings based on business requirements

### Conclusion

The core mutation endpoints specified in issue #1129 are already hardened with proper idempotency implementation. Additional webhook-related endpoints could benefit from idempotency protection to prevent duplicate operations.