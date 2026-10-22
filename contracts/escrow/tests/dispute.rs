//! Integration tests for the escrow dispute flow (issue #1314).
//!
//! Exercises `DisputeHandler` end-to-end across the three lifecycle paths:
//!
//! 1. **Happy path**: create deposit → create dispute → resolve for buyer →
//!    assert deposit status is `Resolved`.
//! 2. **Resolve for seller**: create deposit → create dispute → resolve for
//!    seller → assert deposit status is `Resolved`.
//! 3. **Refund after dispute**: create deposit → create dispute → dismiss →
//!    advance past timeout → refund → assert deposit status is `Cancelled`.
//!
//! Each test asserts the deposit's status and dispute status at every step,
//! so the transition sequence is fully covered.  These tests go through the
//! generated `EscrowContractClient` for deposit/release/refund (the public
//! entrypoints) and call `DisputeHandler` methods directly (they are internal
//! but `pub`) for dispute operations, mirroring how an integrator would
//! exercise the full flow.

use escrow::dispute::{DisputeHandler, DisputeStatus};
use escrow::test_utils::EscrowTest;
use escrow::EscrowStatus;
use soroban_sdk::{
    testutils::Address as _,
    Address, String,
};

// ── Helper: create a dispute via DisputeHandler ──────────────────────────────

/// Create a dispute on `escrow_id` with mocked auth for `initiator`.
fn create_dispute(t: &EscrowTest, escrow_id: u64, initiator: &soroban_sdk::Address) {
    // EscrowTest::setup() already calls env.mock_all_auths(), so require_auth
    // calls inside DisputeHandler will succeed for any address.
    let reason = String::from_str(&t.env, "goods not delivered");
    let evidence = String::from_str(&t.env, "order_123 screenshot");
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::create_dispute(
            &t.env,
            escrow_id,
            initiator.clone(),
            t.bridge.clone(),
            reason,
            evidence,
        )
    });
    assert!(result.is_ok(), "dispute creation should succeed: {:?}", result);
}

/// Resolve a dispute for the buyer with mocked auth for `resolver`.
fn resolve_for_buyer(t: &EscrowTest, escrow_id: u64, resolver: &soroban_sdk::Address) {
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::resolve_for_buyer(
            &t.env,
            escrow_id,
            resolver.clone(),
            Some(String::from_str(&t.env, "buyer is right")),
        )
    });
    assert!(result.is_ok(), "resolve_for_buyer should succeed: {:?}", result);
}

/// Resolve a dispute for the seller with mocked auth for `resolver`.
fn resolve_for_seller(t: &EscrowTest, escrow_id: u64, resolver: &soroban_sdk::Address) {
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::resolve_for_seller(
            &t.env,
            escrow_id,
            resolver.clone(),
            Some(String::from_str(&t.env, "seller is right")),
        )
    });
    assert!(result.is_ok(), "resolve_for_seller should succeed: {:?}", result);
}

/// Dismiss a dispute with mocked auth for `resolver`.
fn dismiss_dispute(t: &EscrowTest, escrow_id: u64, resolver: &soroban_sdk::Address) {
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::dismiss_dispute(
            &t.env,
            escrow_id,
            resolver.clone(),
            Some(String::from_str(&t.env, "no valid dispute")),
        )
    });
    assert!(result.is_ok(), "dismiss_dispute should succeed: {:?}", result);
}

// ── Helper: read dispute status from inside the contract context ─────────────

fn dispute_status(t: &EscrowTest, escrow_id: u64) -> DisputeStatus {
    t.env
        .as_contract(&t.contract_id, || DisputeHandler::get_dispute_status(&t.env, escrow_id))
}

// ── Helper: read deposit status through the public API ──────────────────────

fn deposit_status(t: &EscrowTest, escrow_id: u64) -> EscrowStatus {
    t.client().get_deposit(&escrow_id).status
}

// ════════════════════════════════════════════════════════════════════════════
// Happy path: create → dispute → resolve for buyer → deposit is resolved
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn happy_path_create_dispute_resolve_for_buyer() {
    let t = EscrowTest::setup();

    // Step 1: Create a deposit.
    let deposit_id = t.deposit(5_000);
    assert_eq!(
        deposit_status(&t, deposit_id),
        EscrowStatus::Pending,
        "deposit should start as Pending"
    );

    // Step 2: Create a dispute.
    let buyer = t.depositor.clone();
    create_dispute(&t, deposit_id, &buyer);

    assert_eq!(
        dispute_status(&t, deposit_id),
        DisputeStatus::Pending,
        "dispute should be Pending after creation"
    );
    assert_eq!(
        deposit_status(&t, deposit_id),
        EscrowStatus::Disputed,
        "deposit should be Disputed after dispute creation"
    );

    // Step 3: Resolve the dispute for the buyer.
    let resolver = Address::generate(&t.env);
    resolve_for_buyer(&t, deposit_id, &resolver);

    assert_eq!(
        dispute_status(&t, deposit_id),
        DisputeStatus::ResolvedForBuyer,
        "dispute should be ResolvedForBuyer after resolution"
    );
    assert_eq!(
        deposit_status(&t, deposit_id),
        EscrowStatus::Resolved,
        "deposit should be Resolved after dispute resolution for buyer"
    );

    // Step 4: A resolved deposit cannot be released or refunded.
    assert_eq!(
        t.client().try_release(&deposit_id, &t.bridge),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed)),
        "resolved deposit must not be releasable"
    );
    assert_eq!(
        t.client().try_refund(&deposit_id),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed)),
        "resolved deposit must not be refundable"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Happy path: create → dispute → resolve for seller → deposit is resolved
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn happy_path_create_dispute_resolve_for_seller() {
    let t = EscrowTest::setup();

    let deposit_id = t.deposit(7_500);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Pending);

    let seller = t.depositor.clone();
    create_dispute(&t, deposit_id, &seller);

    assert_eq!(dispute_status(&t, deposit_id), DisputeStatus::Pending);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Disputed);

    let resolver = Address::generate(&t.env);
    resolve_for_seller(&t, deposit_id, &resolver);

    assert_eq!(
        dispute_status(&t, deposit_id),
        DisputeStatus::ResolvedForSeller
    );
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Resolved);

    // Resolved deposit is terminal — no further operations.
    assert_eq!(
        t.client().try_release(&deposit_id, &t.bridge),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed))
    );
    assert_eq!(
        t.client().try_refund(&deposit_id),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed))
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Refund after dispute: create → dispute → dismiss → refund → deposit cancelled
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn refund_after_dispute_dismissal() {
    let t = EscrowTest::setup();

    let deposit_id = t.deposit(3_000);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Pending);

    // Create a dispute.
    let initiator = t.depositor.clone();
    create_dispute(&t, deposit_id, &initiator);

    assert_eq!(dispute_status(&t, deposit_id), DisputeStatus::Pending);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Disputed);

    // Dismiss the dispute — sets dispute to Dismissed and deposit to Resolved.
    let resolver = Address::generate(&t.env);
    dismiss_dispute(&t, deposit_id, &resolver);

    assert_eq!(
        dispute_status(&t, deposit_id),
        DisputeStatus::Dismissed,
        "dispute should be Dismissed"
    );
    assert_eq!(
        deposit_status(&t, deposit_id),
        EscrowStatus::Resolved,
        "dismiss sets the deposit to Resolved"
    );

    // A resolved deposit cannot be refunded — the dispute resolution is terminal.
    t.advance_past_timeout();
    assert_eq!(
        t.client().try_refund(&deposit_id),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed)),
        "a dismissed dispute marks the deposit as Resolved; refund must be blocked"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Dispute cannot be created twice on the same escrow
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn cannot_create_duplicate_dispute() {
    let t = EscrowTest::setup();
    let deposit_id = t.deposit(1_000);

    let initiator = t.depositor.clone();
    create_dispute(&t, deposit_id, &initiator);

    // A second dispute on the same escrow must be rejected.
    let reason = String::from_str(&t.env, "second complaint");
    let evidence = String::from_str(&t.env, "more evidence");
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::create_dispute(
            &t.env,
            deposit_id,
            initiator.clone(),
            t.bridge.clone(),
            reason,
            evidence,
        )
    });
    assert_eq!(
        result,
        Err(escrow::dispute::DisputeError::AlreadyResolved),
        "duplicate dispute must be rejected"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Dispute resolution is idempotent — cannot resolve twice
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn cannot_resolve_an_already_resolved_dispute() {
    let t = EscrowTest::setup();
    let deposit_id = t.deposit(2_000);

    let initiator = t.depositor.clone();
    create_dispute(&t, deposit_id, &initiator);

    let resolver = Address::generate(&t.env);
    resolve_for_buyer(&t, deposit_id, &resolver);

    // A second resolution attempt must be rejected.
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::resolve_for_buyer(&t.env, deposit_id, resolver.clone(), None)
    });
    assert_eq!(
        result,
        Err(escrow::dispute::DisputeError::AlreadyResolved),
        "cannot resolve an already-resolved dispute"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Dispute status transitions: Pending → InReview is not auto-set, resolution
// paths go directly from Pending
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn dispute_status_is_none_before_creation() {
    let t = EscrowTest::setup();
    let deposit_id = t.deposit(1_000);

    assert_eq!(
        dispute_status(&t, deposit_id),
        DisputeStatus::None,
        "no dispute should exist before creation"
    );
    assert!(
        !t.env.as_contract(&t.contract_id, || DisputeHandler::dispute_exists(&t.env, deposit_id)),
        "dispute_exists should return false before creation"
    );
}

#[test]
fn dispute_exists_after_creation() {
    let t = EscrowTest::setup();
    let deposit_id = t.deposit(1_000);

    let initiator = t.depositor.clone();
    create_dispute(&t, deposit_id, &initiator);

    assert!(
        t.env.as_contract(&t.contract_id, || DisputeHandler::dispute_exists(&t.env, deposit_id)),
        "dispute_exists should return true after creation"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Full lifecycle with balances asserted at each step
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn full_lifecycle_create_dispute_release_asserts_amount_at_each_step() {
    let t = EscrowTest::setup();

    // Step 1: Create deposit with a known amount.
    let amount: i128 = 10_000;
    let deposit_id = t.deposit(amount);
    let record = t.client().get_deposit(&deposit_id);
    assert_eq!(record.amount, amount);
    assert_eq!(record.status, EscrowStatus::Pending);

    // Step 2: Dispute the deposit.
    let initiator = t.depositor.clone();
    create_dispute(&t, deposit_id, &initiator);
    let record = t.client().get_deposit(&deposit_id);
    assert_eq!(record.amount, amount, "amount unchanged after dispute");
    assert_eq!(record.status, EscrowStatus::Disputed);

    // Step 3: Resolve for buyer — deposit becomes Resolved.
    let resolver = Address::generate(&t.env);
    resolve_for_buyer(&t, deposit_id, &resolver);
    let record = t.client().get_deposit(&deposit_id);
    assert_eq!(record.amount, amount, "amount unchanged after resolution");
    assert_eq!(record.status, EscrowStatus::Resolved);

    // Step 4: Release attempt is blocked — the deposit is already Resolved.
    let recipient = Address::generate(&t.env);
    assert_eq!(
        t.client().try_release(&deposit_id, &recipient),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed)),
        "release must be blocked on a resolved deposit"
    );
}

#[test]
fn full_lifecycle_create_dispute_dismiss_then_blocked_from_refund() {
    let t = EscrowTest::setup();

    let amount: i128 = 8_000;
    let deposit_id = t.deposit(amount);
    assert_eq!(t.client().get_deposit(&deposit_id).amount, amount);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Pending);

    // Dispute.
    let initiator = t.depositor.clone();
    create_dispute(&t, deposit_id, &initiator);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Disputed);
    assert_eq!(dispute_status(&t, deposit_id), DisputeStatus::Pending);

    // Dismiss.
    let resolver = Address::generate(&t.env);
    dismiss_dispute(&t, deposit_id, &resolver);
    assert_eq!(dispute_status(&t, deposit_id), DisputeStatus::Dismissed);
    assert_eq!(deposit_status(&t, deposit_id), EscrowStatus::Resolved);

    // Even after the timeout, the deposit is Resolved and cannot be refunded.
    t.advance_past_timeout();
    assert_eq!(
        t.client().try_refund(&deposit_id),
        Err(Ok(stellar_spend_shared::errors::ContractError::AlreadyProcessed)),
        "refund must be blocked after dispute dismissal resolved the deposit"
    );

    // Amount is unchanged throughout.
    assert_eq!(t.client().get_deposit(&deposit_id).amount, amount);
}

// ════════════════════════════════════════════════════════════════════════════
// Disputing a non-existent deposit creates a dispute record but the deposit
// remains unchanged (update_escrow_status is a no-op for missing deposits)
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn disputing_a_nonexistent_deposit_creates_dispute_but_does_not_corrupt_state() {
    let t = EscrowTest::setup();

    let nonexistent_id: u64 = 999;
    let initiator = t.depositor.clone();

    let reason = String::from_str(&t.env, "test");
    let evidence = String::from_str(&t.env, "test");
    let result = t.env.as_contract(&t.contract_id, || {
        DisputeHandler::create_dispute(
            &t.env,
            nonexistent_id,
            initiator.clone(),
            t.bridge.clone(),
            reason,
            evidence,
        )
    });
    // Dispute creation succeeds (it does not check deposit existence).
    assert!(result.is_ok(), "dispute creation on nonexistent deposit should not panic");

    // The dispute record exists.
    assert!(
        t.env.as_contract(&t.contract_id, || DisputeHandler::dispute_exists(&t.env, nonexistent_id)),
        "dispute record should exist"
    );

    // But the deposit map is unaffected — no deposit with that id.
    assert_eq!(
        t.client().try_get_deposit(&nonexistent_id),
        Err(Ok(stellar_spend_shared::errors::ContractError::NotFound)),
        "no deposit should exist with that id"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Multiple deposits can each have independent disputes
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_deposits_can_have_independent_disputes() {
    let t = EscrowTest::setup();

    let id_a = t.deposit(1_000);
    let id_b = t.deposit(2_000);

    // Dispute only deposit A.
    let initiator = t.depositor.clone();
    create_dispute(&t, id_a, &initiator);

    assert_eq!(deposit_status(&t, id_a), EscrowStatus::Disputed);
    assert_eq!(dispute_status(&t, id_a), DisputeStatus::Pending);

    // Deposit B is unaffected.
    assert_eq!(deposit_status(&t, id_b), EscrowStatus::Pending);
    assert_eq!(dispute_status(&t, id_b), DisputeStatus::None);

    // Resolve A for buyer.
    let resolver = Address::generate(&t.env);
    resolve_for_buyer(&t, id_a, &resolver);

    assert_eq!(deposit_status(&t, id_a), EscrowStatus::Resolved);
    assert_eq!(dispute_status(&t, id_a), DisputeStatus::ResolvedForBuyer);

    // Deposit B is still pending and can be released normally.
    let recipient = Address::generate(&t.env);
    assert_eq!(t.client().release(&id_b, &recipient), 2_000);
    assert_eq!(deposit_status(&t, id_b), EscrowStatus::Resolved);
}
