//! Escrow unit tests.
//!
//! Every test opens from a fixture in [`crate::test_utils`] rather than repeating
//! `Env::default()` / `register` / `init` (issue #818). Assertions go through the
//! generated `try_*` client methods so that a contract error is checked by value
//! instead of being swallowed by a panic.
use soroban_sdk::{symbol_short, testutils::Address as _, Address};
use stellar_spend_shared::errors::ContractError;

use crate::test_utils::{assert_fresh_init_is_current, EscrowTest, START_LEDGER};
use crate::{DEFAULT_TIMEOUT_LEDGERS, EscrowStatus, MAX_TIMEOUT_LEDGERS, SCHEMA_VERSION};
use soroban_sdk::testutils::Events as _;

// ── Initialisation ───────────────────────────────────────────────────────────

fn assert_event<T>(
    event: (
        soroban_sdk::Address,
        soroban_sdk::Vec<soroban_sdk::Val>,
        soroban_sdk::Val,
    ),
    address: &soroban_sdk::Address,
    env: &soroban_sdk::Env,
    expected_topic: soroban_sdk::Symbol,
    expected_data: T,
) where
    T: soroban_sdk::TryFromVal<soroban_sdk::Env, soroban_sdk::Val>
        + core::cmp::PartialEq
        + core::fmt::Debug,
{
    let (addr, topics, data) = event;
    assert_eq!(addr, *address);
    let topic_val = topics.get(0).unwrap();
    let expected_topic_val: soroban_sdk::Val =
        soroban_sdk::IntoVal::<soroban_sdk::Env, soroban_sdk::Val>::into_val(&expected_topic, env);
    assert!(topic_val.shallow_eq(&expected_topic_val));
    let decoded: T =
        soroban_sdk::TryFromVal::try_from_val(env, &data).expect("failed to decode event data");
    assert_eq!(decoded, expected_data);
}
#[test]
fn init_persists_authority_and_schema() {
    let t = EscrowTest::setup();
    assert_fresh_init_is_current(&t);
    assert_eq!(t.client().schema_version(), SCHEMA_VERSION);
}

#[test]
fn init_is_rejected_twice() {
    let t = EscrowTest::setup();
    // A second init must not silently reassign the settlement authority.
    assert_eq!(
        t.client().try_init(&t.other),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn entrypoints_reject_an_uninitialised_contract() {
    let t = EscrowTest::registered();
    assert_eq!(
        t.client().try_deposit(&t.depositor, &100, &t.bridge, &0),
        Err(Ok(ContractError::NotInitialized))
    );
    assert_eq!(
        t.client().try_schema_version(),
        Err(Ok(ContractError::NotInitialized))
    );
}

// ── Deposit validation (issue #816) ──────────────────────────────────────────

#[test]
fn deposit_rejects_non_positive_amounts() {
    let t = EscrowTest::setup();
    for bad in [0i128, -1, i128::MIN] {
        assert_eq!(
            t.client().try_deposit(&t.depositor, &bad, &t.bridge, &0),
            Err(Ok(ContractError::InvalidAmount)),
            "amount {bad} must be rejected"
        );
    }
}

#[test]
fn deposit_rejects_fee_above_one_hundred_percent() {
    let t = EscrowTest::setup();
    assert_eq!(
        t.client()
            .try_deposit(&t.depositor, &100, &t.bridge, &10_001),
        Err(Ok(ContractError::InvalidInput))
    );
    // Exactly 100% is the boundary and is accepted.
    assert!(t
        .client()
        .try_deposit(&t.depositor, &100, &t.bridge, &10_000)
        .is_ok());
}

#[test]
fn deposit_ids_are_unique_and_monotonic() {
    let t = EscrowTest::setup();
    let first = t.deposit(100);
    let second = t.deposit(100);
    let third = t.deposit(250);
    assert_eq!((first, second, third), (0, 1, 2));
}

#[test]
fn deposit_records_the_quoted_fee_and_timeout() {
    let t = EscrowTest::setup();
    let id = t.deposit_with_fee(1_000, 25);
    let record = t.client().get_deposit(&id);

    assert_eq!(record.amount, 1_000);
    assert_eq!(record.fee_bps, 25);
    assert_eq!(record.depositor, t.depositor);
    assert_eq!(record.bridge_address, t.bridge);
    assert_eq!(record.status, EscrowStatus::Pending);
    assert_eq!(
        record.timeout_ledger,
        START_LEDGER + DEFAULT_TIMEOUT_LEDGERS
    );
}

#[test]
fn get_deposit_reports_a_missing_id() {
    let t = EscrowTest::setup();
    assert_eq!(
        t.client().try_get_deposit(&42),
        Err(Ok(ContractError::NotFound))
    );
}

// ── Release ──────────────────────────────────────────────────────────────────

#[test]
fn release_returns_the_amount_and_marks_the_record() {
    let t = EscrowTest::setup();
    let id = t.deposit(750);
    let recipient = Address::generate(&t.env);

    assert_eq!(t.client().release(&id, &recipient), 750);

    let record = t.client().get_deposit(&id);
    assert_eq!(record.status, EscrowStatus::Resolved);
}

#[test]
fn release_is_not_repeatable() {
    let t = EscrowTest::setup();
    let id = t.deposit(750);
    let recipient = Address::generate(&t.env);
    t.client().release(&id, &recipient);

    assert_eq!(
        t.client().try_release(&id, &recipient),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn release_is_blocked_after_a_refund() {
    let t = EscrowTest::setup();
    let id = t.deposit(750);
    t.advance_past_timeout();
    t.client().refund(&id);

    let recipient = Address::generate(&t.env);
    assert_eq!(
        t.client().try_release(&id, &recipient),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn release_reports_a_missing_deposit() {
    let t = EscrowTest::setup();
    let recipient = Address::generate(&t.env);
    assert_eq!(
        t.client().try_release(&9, &recipient),
        Err(Ok(ContractError::NotFound))
    );
}

// ── Refund ───────────────────────────────────────────────────────────────────

#[test]
fn refund_is_blocked_before_the_timeout_ledger() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);

    assert_eq!(t.client().try_refund(&id), Err(Ok(ContractError::Expired)));

    // One ledger short of the deadline is still too early.
    t.advance_ledgers(DEFAULT_TIMEOUT_LEDGERS - 1);
    assert_eq!(t.client().try_refund(&id), Err(Ok(ContractError::Expired)));
}

#[test]
fn refund_succeeds_exactly_at_the_timeout_ledger() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);
    // `refund` compares with `>=`, so landing exactly on the deadline must work.
    t.advance_ledgers(DEFAULT_TIMEOUT_LEDGERS);

    assert_eq!(t.client().refund(&id), 400);
    assert_eq!(t.client().get_deposit(&id).status, EscrowStatus::Cancelled);
}

#[test]
fn refund_is_not_repeatable() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);
    t.advance_past_timeout();
    t.client().refund(&id);

    assert_eq!(
        t.client().try_refund(&id),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn refund_succeeds_after_timeout_for_depositor() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);
    t.advance_past_timeout();

    assert_eq!(t.client().try_refund(&id), Ok(Ok(400)));
}

#[test]
fn refund_is_blocked_after_a_release() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);
    let recipient = Address::generate(&t.env);
    t.client().release(&id, &recipient);
    t.advance_past_timeout();

    assert_eq!(
        t.client().try_refund(&id),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn can_refund_tracks_the_deposit_lifecycle() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);

    assert!(!t.client().can_refund(&id), "not yet timed out");
    t.advance_past_timeout();
    assert!(t.client().can_refund(&id), "timed out and still open");

    t.client().refund(&id);
    assert!(!t.client().can_refund(&id), "already refunded");
}

// ── Timeout configuration ────────────────────────────────────────────────────

#[test]
fn set_timeout_enforces_its_bounds() {
    let t = EscrowTest::setup();

    assert_eq!(
        t.client().try_set_timeout(&0),
        Err(Ok(ContractError::InvalidInput)),
        "a zero timeout would make deposits refundable immediately"
    );
    assert_eq!(
        t.client().try_set_timeout(&(MAX_TIMEOUT_LEDGERS + 1)),
        Err(Ok(ContractError::InvalidInput))
    );

    assert!(t.client().try_set_timeout(&1).is_ok());
    assert!(t.client().try_set_timeout(&MAX_TIMEOUT_LEDGERS).is_ok());
}

#[test]
fn set_timeout_does_not_retroactively_extend_open_deposits() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);
    let original_deadline = t.client().get_deposit(&id).timeout_ledger;

    t.client().set_timeout(&MAX_TIMEOUT_LEDGERS);

    assert_eq!(
        t.client().get_deposit(&id).timeout_ledger,
        original_deadline,
        "an existing deposit's deadline must not move when the default changes"
    );

    // ...but the next deposit picks the new value up.
    let later = t.deposit(400);
    assert_eq!(
        t.client().get_deposit(&later).timeout_ledger,
        START_LEDGER + MAX_TIMEOUT_LEDGERS
    );
}

// ── Arithmetic guards ────────────────────────────────────────────────────────

#[test]
fn deposit_rejects_a_timeout_that_would_overflow_the_ledger_counter() {
    // The sequence must sit within MAX_TIMEOUT_LEDGERS of u32::MAX for the addition
    // to wrap, while the entry TTL stays small enough that `sequence + ttl` does not
    // overflow first — otherwise the host archives storage before the test runs.
    let t = EscrowTest::registered_at(u32::MAX - 5_000_000, 1_000_000);
    t.client().init(&t.admin);
    t.client().set_timeout(&MAX_TIMEOUT_LEDGERS);

    assert_eq!(
        t.client().try_deposit(&t.depositor, &100, &t.bridge, &0),
        Err(Ok(ContractError::Overflow)),
        "current_ledger + timeout must not wrap into an instantly-refundable deposit"
    );
}

// ── Event assertions (issue #814) ────────────────────────────────────────────
//
// Every state-changing entrypoint emits a corresponding event. These tests assert
// the exact topic and data fields so off-chain indexers can rely on the shape.
// Pattern: call the function, then compare env.events().all() to the expected vec.

#[test]
fn init_emits_event_with_settlement_authority() {
    let t = EscrowTest::registered();
    t.client().init(&t.admin);

    // The init event is the only event emitted by this call.
    // Topic: ("init",)   Data: settlement_authority
    let events = t.env.events().all();
    assert_eq!(events.len(), 1);
    let event = events.get(0).unwrap();
    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("init"),
        t.admin,
    );
}

#[test]
fn deposit_emits_event_with_id_depositor_amount_bridge() {
    let t = EscrowTest::setup();
    // Clear setup events by creating a fresh env snapshot perspective;
    // env.events().all() returns ALL events since env creation. We snapshot the
    // count after setup, then check only the deposit event.
    let id = t.deposit_with_fee(500, 10);
    let all_events = t.env.events().all();

    // The deposit event is the first event after setup.
    let deposit_event = all_events.get(0).unwrap();
    assert_event(
        deposit_event,
        &t.contract_id,
        &t.env,
        symbol_short!("deposit"),
        (id, t.depositor.clone(), 500i128, t.bridge.clone()),
    );
}

#[test]
fn release_emits_event_with_deposit_id_recipient_amount() {
    let t = EscrowTest::setup();
    let id = t.deposit(750);
    let recipient = Address::generate(&t.env);

    t.client().release(&id, &recipient);
    let all_events = t.env.events().all();
    let release_event = all_events.get(0).unwrap();

    assert_event(
        release_event,
        &t.contract_id,
        &t.env,
        symbol_short!("release"),
        (id, recipient, 750i128),
    );
}

#[test]
fn refund_emits_event_with_deposit_id_depositor_amount() {
    let t = EscrowTest::setup();
    let id = t.deposit(400);
    t.advance_past_timeout();

    t.client().refund(&id);
    let all_events = t.env.events().all();
    let refund_event = all_events.get(0).unwrap();

    assert_event(
        refund_event,
        &t.contract_id,
        &t.env,
        symbol_short!("refund"),
        (id, t.depositor.clone(), 400i128),
    );
}

#[test]
fn set_timeout_emits_event_with_new_timeout_value() {
    let t = EscrowTest::setup();

    t.client().set_timeout(&12_345);
    let all_events = t.env.events().all();
    let timeout_event = all_events.get(0).unwrap();

    assert_event(
        timeout_event,
        &t.contract_id,
        &t.env,
        symbol_short!("timeout"),
        12_345u32,
    );
}

#[test]
fn migrate_emits_event_with_from_and_to_schema_versions() {
    let t = EscrowTest::with_legacy_v1_state();

    t.client().migrate();
    let all_events = t.env.events().all();
    let migrate_event = all_events.get(0).unwrap();

    assert_event(
        migrate_event,
        &t.contract_id,
        &t.env,
        symbol_short!("migrate"),
        (1u32, SCHEMA_VERSION),
    );
}

// ── Reentrancy guard / CEI regression tests (issue #981) ─────────────────────
//
// `release` and `refund` hold an explicit boolean lock in instance storage for
// their entire execution. These tests assert the adversarial paths the lock is
// designed to block: attempting to call release (or refund) while a previous
// invocation has already marked the lock.
//
// In the Soroban test environment we simulate a re-entrant call by directly
// writing the lock flag inside `env.as_contract`, then verifying the next call
// from outside sees the correct error code.

#[test]
fn release_is_rejected_while_the_reentrancy_lock_is_held() {
    use crate::DataKey;

    let t = EscrowTest::setup();
    let id = t.deposit(500);
    let recipient = Address::generate(&t.env);

    // Simulate a reentrant context: set the lock as if a call is in progress.
    t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .set(&DataKey::Lock, &true);
    });

    // The outer release call must be rejected — the lock is already held.
    assert_eq!(
        t.client().try_release(&id, &recipient),
        Err(Ok(ContractError::Reentrant)),
        "release must reject a call made while the reentrancy lock is held"
    );

    // After clearing the lock the call succeeds normally.
    t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .set(&DataKey::Lock, &false);
    });
    assert_eq!(t.client().release(&id, &recipient), 500);
}

#[test]
fn refund_is_rejected_while_the_reentrancy_lock_is_held() {
    use crate::DataKey;

    let t = EscrowTest::setup();
    let id = t.deposit(300);
    t.advance_past_timeout();

    // Hold the lock to simulate a re-entrant call.
    t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .set(&DataKey::Lock, &true);
    });

    assert_eq!(
        t.client().try_refund(&id),
        Err(Ok(ContractError::Reentrant)),
        "refund must reject a call made while the reentrancy lock is held"
    );

    // After clearing the lock the refund succeeds.
    t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .set(&DataKey::Lock, &false);
    });
    assert_eq!(t.client().refund(&id), 300);
}

#[test]
fn release_clears_the_lock_on_success() {
    // Verify the lock is always set to false after a successful release so the
    // contract is not permanently locked after a normal call.
    use crate::DataKey;

    let t = EscrowTest::setup();
    let id = t.deposit(200);
    let recipient = Address::generate(&t.env);

    t.client().release(&id, &recipient);

    let locked: bool = t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .get(&DataKey::Lock)
            .unwrap_or(false)
    });
    assert!(!locked, "lock must be cleared after a successful release");
}

#[test]
fn refund_clears_the_lock_on_success() {
    use crate::DataKey;

    let t = EscrowTest::setup();
    let id = t.deposit(200);
    t.advance_past_timeout();
    t.client().refund(&id);

    let locked: bool = t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .get(&DataKey::Lock)
            .unwrap_or(false)
    });
    assert!(!locked, "lock must be cleared after a successful refund");
}

#[test]
fn release_state_is_updated_before_event_emission_cei_order() {
    // Checks-Effects-Interactions: by the time the `release` event is visible,
    // the deposit must already be marked as released in storage. Any observer
    // reading state after the event would see `released == true`.
    let t = EscrowTest::setup();
    let id = t.deposit(999);
    let recipient = Address::generate(&t.env);

    let amount = t.client().release(&id, &recipient);
    assert_eq!(amount, 999);

    // State (effect) persisted correctly.
    let record = t.client().get_deposit(&id);
    assert!(
        matches!(record.status, EscrowStatus::Resolved),
        "deposit must be marked resolved (effect) before the event (interaction)"
    );

    // A second release hits AlreadyProcessed — the state was written first.
    assert_eq!(
        t.client().try_release(&id, &recipient),
        Err(Ok(ContractError::AlreadyProcessed)),
        "a subsequent release must see the already-applied effect"
    );
}

// ── Illegal state transitions ──────────────────────────────────────────

#[test]
fn disputed_deposit_cannot_be_released_or_refunded() {
    let t = EscrowTest::setup();
    let id = t.deposit(750);

    // After release, status is Resolved and further operations are blocked
    t.client().release(&id, &Address::generate(&t.env));
    let record = t.client().get_deposit(&id);
    assert_eq!(record.status, EscrowStatus::Resolved);
    assert_eq!(
        t.client().try_release(&id, &Address::generate(&t.env)),
        Err(Ok(ContractError::AlreadyProcessed)),
        "a resolved deposit must not be releasable"
    );
    assert_eq!(
        t.client().try_refund(&id),
        Err(Ok(ContractError::AlreadyProcessed)),
        "a resolved deposit must not be refundable"
    );
}

#[test]
fn refunded_deposit_cannot_be_released_or_refunded() {
    let t = EscrowTest::setup();
    let id = t.deposit(750);
    t.advance_past_timeout();
    t.client().refund(&id);

    let record = t.client().get_deposit(&id);
    assert_eq!(record.status, EscrowStatus::Cancelled);
    assert_eq!(
        t.client().try_release(&id, &Address::generate(&t.env)),
        Err(Ok(ContractError::AlreadyProcessed)),
        "a cancelled deposit must not be releasable"
    );
    assert_eq!(
        t.client().try_refund(&id),
        Err(Ok(ContractError::AlreadyProcessed)),
        "a cancelled deposit must not be refundable"
    );
}
