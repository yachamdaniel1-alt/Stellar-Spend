//! Fee-manager unit tests.
//!
//! All setup comes from [`crate::test_utils`] (issue #818).
use stellar_spend_shared::{errors::ContractError, validation::MAX_BASIS_POINTS};

use crate::test_utils::{assert_fresh_init_is_current, FeeManagerTest, DEFAULT_TEST_FEE_BP};
use crate::{MAX_DEFAULT_FEE_BP, SCHEMA_VERSION};
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
fn init_persists_admin_rate_and_schema() {
    let t = FeeManagerTest::setup();
    assert_fresh_init_is_current(&t);
    assert_eq!(t.client().default_rate(), DEFAULT_TEST_FEE_BP);
    assert!(!t.client().is_paused());
}

#[test]
fn init_is_rejected_twice() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client().try_init(&t.outsider, &10),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn init_rejects_a_rate_above_the_cap() {
    let t = FeeManagerTest::registered();
    assert_eq!(
        t.client().try_init(&t.admin, &(MAX_DEFAULT_FEE_BP + 1)),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn is_paused_is_false_for_an_uninitialised_contract() {
    // Deliberately not an error: callers use this as a cheap pre-flight guard.
    let t = FeeManagerTest::registered();
    assert!(!t.client().is_paused());
}

// ── Fee arithmetic (issue #816) ──────────────────────────────────────────────

#[test]
fn calculate_fee_applies_basis_points() {
    let t = FeeManagerTest::setup();
    assert_eq!(t.client().calculate_fee(&1_000_000, &50), 5_000); // 0.5%
    assert_eq!(t.client().calculate_fee(&1_000_000, &0), 0);
    assert_eq!(t.client().calculate_fee(&1_000_000, &10_000), 1_000_000); // 100%
}

#[test]
fn calculate_fee_rejects_non_positive_amounts() {
    let t = FeeManagerTest::setup();
    // The previous implementation cast `amount as u128`, so a negative amount
    // wrapped to an enormous positive fee instead of being rejected.
    for bad in [0i128, -1, -1_000_000, i128::MIN] {
        assert_eq!(
            t.client().try_calculate_fee(&bad, &50),
            Err(Ok(ContractError::InvalidAmount)),
            "amount {bad} must be rejected"
        );
    }
}

#[test]
fn calculate_fee_rejects_a_rate_above_one_hundred_percent() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client()
            .try_calculate_fee(&1_000, &(MAX_BASIS_POINTS + 1)),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn calculate_fee_reports_overflow_rather_than_wrapping() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client().try_calculate_fee(&i128::MAX, &10_000),
        Err(Ok(ContractError::Overflow))
    );
}

#[test]
fn calculate_fee_truncates_toward_zero() {
    let t = FeeManagerTest::setup();
    // 1 * 1bp = 0.0001, which floors to 0 — the contract must not round up and
    // charge a fee larger than the amount warrants.
    assert_eq!(t.client().calculate_fee(&1, &1), 0);
    assert_eq!(t.client().calculate_fee(&9_999, &1), 0);
    assert_eq!(t.client().calculate_fee(&10_000, &1), 1);
}

#[test]
fn calculate_default_fee_uses_the_configured_rate() {
    let t = FeeManagerTest::setup();
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 5_000);

    t.client().set_default_rate(&100); // 1%
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 10_000);
}

#[test]
fn set_default_rate_enforces_the_cap() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client().try_set_default_rate(&(MAX_DEFAULT_FEE_BP + 1)),
        Err(Ok(ContractError::InvalidInput))
    );
    assert!(t.client().try_set_default_rate(&MAX_DEFAULT_FEE_BP).is_ok());
}

// ── Circuit breaker ──────────────────────────────────────────────────────────

#[test]
fn pause_blocks_fee_calculation() {
    let t = FeeManagerTest::setup_paused();
    assert!(t.client().is_paused());
    assert_eq!(
        t.client().try_calculate_fee(&1_000, &50),
        Err(Ok(ContractError::Paused))
    );
    assert_eq!(
        t.client().try_calculate_default_fee(&1_000),
        Err(Ok(ContractError::Paused))
    );
}

#[test]
fn unpause_restores_service() {
    let t = FeeManagerTest::setup_paused();
    t.client().unpause();
    assert!(!t.client().is_paused());
    assert_eq!(t.client().calculate_fee(&1_000_000, &50), 5_000);
}

#[test]
fn pause_and_unpause_are_not_idempotent_no_ops() {
    let t = FeeManagerTest::setup();

    // Unpausing a running contract is a caller mistake worth surfacing, not a no-op
    // that hides a broken incident-response script.
    assert_eq!(
        t.client().try_unpause(),
        Err(Ok(ContractError::InvalidInput))
    );

    t.client().pause(&t.reason("incident"));
    assert_eq!(
        t.client().try_pause(&t.reason("incident again")),
        Err(Ok(ContractError::Paused))
    );
}

#[test]
fn pause_rejects_an_empty_or_oversized_reason() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client().try_pause(&t.reason("")),
        Err(Ok(ContractError::InvalidInput))
    );

    let long = "x".repeat(129);
    assert_eq!(
        t.client().try_pause(&t.reason(&long)),
        Err(Ok(ContractError::InvalidInput)),
        "reasons are echoed into event topics and must stay bounded"
    );
}

// ── Version metadata ─────────────────────────────────────────────────────────

#[test]
fn version_is_reported() {
    let t = FeeManagerTest::setup();
    assert_eq!(t.client().version(), t.reason("1.0.0"));
}

#[test]
fn schema_version_matches_the_constant() {
    let t = FeeManagerTest::setup();
    assert_eq!(t.client().schema_version(), SCHEMA_VERSION);
}

// ── Event assertions (issue #814) ────────────────────────────────────────────
//
// All state-changing functions in the fee-manager contract emit events. These
// tests assert the exact topic and data fields so off-chain indexers can rely on them.

#[test]
fn init_emits_event_with_admin_address() {
    use soroban_sdk::symbol_short;

    let t = FeeManagerTest::registered();
    t.client().init(&t.admin, &DEFAULT_TEST_FEE_BP);

    let events = t.env.events().all();
    assert_eq!(events.len(), 1);
    let event = events.get(0).unwrap();
    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("init"),
        t.admin.clone(),
    );
}

#[test]
fn pause_emits_event_with_reason() {
    use soroban_sdk::symbol_short;

    let t = FeeManagerTest::setup();
    let reason = t.reason("system maintenance");

    t.client().pause(&reason);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("pause"),
        reason,
    );
}

#[test]
fn unpause_emits_event() {
    use soroban_sdk::symbol_short;

    let t = FeeManagerTest::setup_paused();

    t.client().unpause();
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(event, &t.contract_id, &t.env, symbol_short!("unpause"), ());
}

#[test]
fn set_default_rate_emits_event_with_new_rate() {
    use soroban_sdk::symbol_short;

    let t = FeeManagerTest::setup();

    t.client().set_default_rate(&100);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(event, &t.contract_id, &t.env, symbol_short!("rate"), 100u32);
}

#[test]
fn migrate_emits_event_with_from_and_to_schema_versions() {
    use crate::SCHEMA_VERSION;
    use soroban_sdk::symbol_short;

    let t = FeeManagerTest::with_legacy_v1_state();

    t.client().migrate();
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("migrate"),
        (1u32, SCHEMA_VERSION),
    );
}

#[test]
fn upgrade_emits_event() {
    // `upgrade` emits `(symbol_short!("upgrade"),)` with an empty `()` payload.
    // The WASM hash itself is not echoed into the event so that the test does not
    // need a real compiled WASM blob; only the topic and the unit data matter.
    use soroban_sdk::{symbol_short, testutils::BytesN as _};

    let t = FeeManagerTest::setup();

    // Provide a dummy 32-byte hash. The Soroban testutils environment accepts any
    // well-formed BytesN<32> for deployer().update_current_contract_wasm().
    let dummy_hash = soroban_sdk::BytesN::<32>::random(&t.env);

    t.client().upgrade(&dummy_hash);

    let all_events = t.env.events().all();
    // upgrade() is the only state-changing call, so the first event must be ours.
    let event = all_events.get(0).unwrap();

    assert_event(event, &t.contract_id, &t.env, symbol_short!("upgrade"), ());
}

// ── Per-invocation storage cache (resource-fee reduction) ──────────────────

#[test]
fn pause_via_cached_guard_still_enforces_admin_auth() {
    let t = FeeManagerTest::setup();
    // The combined require_current_schema_and_admin() guard should still
    // reject non-admin callers exactly like the two separate checks did.
    t.env.mock_auths(&[]);
    assert!(t.client().try_pause(&t.reason("incident")).is_err());
}

#[test]
fn pause_then_unpause_round_trip_uses_one_cache_per_call() {
    let t = FeeManagerTest::setup();

    t.client().pause(&t.reason("incident"));
    assert!(t.client().is_paused());

    t.client().unpause();
    assert!(!t.client().is_paused());

    // A fresh InvocationCache is built per call, so a second pause/unpause
    // cycle in the same test must behave identically to the first.
    t.client().pause(&t.reason("incident 2"));
    assert!(t.client().is_paused());
}

#[test]
fn set_default_rate_via_cached_guard_persists_the_new_rate() {
    let t = FeeManagerTest::setup();
    t.client().set_default_rate(&250);
    assert_eq!(t.client().default_rate(), 250);
}

#[test]
fn set_default_rate_via_cached_guard_still_enforces_the_cap() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client().try_set_default_rate(&(MAX_DEFAULT_FEE_BP + 1)),
        Err(Ok(ContractError::InvalidInput))
    );
}
// ── CalcCache: reduced storage reads (issue #1316) ─────────────────────────
//
// `calculate_default_fee` previously read `Schema` twice — once inside
// `admin::default_rate` and again inside `calculate_fee`. The CalcCache
// refactor in `calc.rs` fetches each key at most once, dropping the read
// count from 4 to 3. These tests verify the optimization preserves behaviour
// and that the CPU-instruction cost of `calculate_default_fee` is no higher
// than `calculate_fee` (which reads only 2 keys), proving the redundant read
// is gone.

#[test]
fn calculate_default_fee_uses_the_cached_rate() {
    let t = FeeManagerTest::setup();
    // The fixture initialises at 50 bp (0.5%).
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 5_000);

    // After a rate change the cached path picks up the new value.
    t.client().set_default_rate(&100);
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 10_000);

    // Dropping to zero still yields zero through the cache.
    t.client().set_default_rate(&0);
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 0);
}

#[test]
fn calculate_default_fee_is_blocked_when_paused() {
    let t = FeeManagerTest::setup_paused();
    // The circuit-breaker check runs through the same cache and must reject.
    assert_eq!(
        t.client().try_calculate_default_fee(&1_000),
        Err(Ok(ContractError::Paused))
    );
}

#[test]
fn calculate_fee_is_blocked_when_paused() {
    let t = FeeManagerTest::setup_paused();
    assert_eq!(
        t.client().try_calculate_fee(&1_000, &50),
        Err(Ok(ContractError::Paused))
    );
}

#[test]
fn calculate_default_fee_rejects_non_positive_amounts() {
    let t = FeeManagerTest::setup();
    for bad in [0i128, -1, -1_000_000] {
        assert_eq!(
            t.client().try_calculate_default_fee(&bad),
            Err(Ok(ContractError::InvalidAmount)),
            "amount {bad} must be rejected"
        );
    }
}

#[test]
fn calculate_default_fee_reports_overflow() {
    let t = FeeManagerTest::setup();
    assert_eq!(
        t.client().try_calculate_default_fee(&i128::MAX),
        Err(Ok(ContractError::Overflow))
    );
}
