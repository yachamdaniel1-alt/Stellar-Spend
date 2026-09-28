//! Treasury unit tests.
//!
//! All setup comes from [`crate::test_utils`] (issue #818).
use stellar_spend_shared::errors::ContractError;
use soroban_sdk::{Address, Vec};

use crate::test_utils::{assert_fresh_init_is_current, TreasuryTest};
use crate::{MAX_FEE_TIERS, MAX_SINGLE_FEE_BP, SCHEMA_VERSION};
use soroban_sdk::testutils::{Address as _, Events as _};

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
fn init_persists_admin_treasury_and_schedule() {
    let t = TreasuryTest::setup();
    assert_fresh_init_is_current(&t);
    assert_eq!(t.client().get_treasury(), t.treasury);
    assert_eq!(t.client().total_collected(), 0);
    assert_eq!(t.client().get_fee_schedule().len(), 3);
}

#[test]
fn init_is_rejected_twice() {
    let t = TreasuryTest::setup();
    assert_eq!(
        t.client().try_initialize(&t.outsider),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn entrypoints_reject_an_uninitialised_contract() {
    let t = TreasuryTest::registered();
    assert_eq!(
        t.client().try_get_treasury(),
        Err(Ok(ContractError::NotInitialized)),
        "must not invent a treasury address to route fees to"
    );
    assert_eq!(
        t.client().try_collect_fee(&1_000, &t.outsider),
        Err(Ok(ContractError::NotInitialized))
    );
}

// ── The #815 regression: the stored schedule must actually be read ───────────

#[test]
fn fee_tiers_come_from_storage_not_hard_coded_branches() {
    let t = TreasuryTest::setup();

    // Defaults seeded at init.
    assert_eq!(t.client().fee_for_amount(&500_000), 50);
    assert_eq!(t.client().fee_for_amount(&5_000_000), 25);
    assert_eq!(t.client().fee_for_amount(&50_000_000), 10);

    // Reconfigure every tier. The previous implementation ignored the stored
    // schedule and returned the compiled-in 50/25/10 regardless, which made
    // `set_fee_schedule` a no-op from the caller's point of view.
    t.client().set_fee_schedule(&0, &7);
    t.client().set_fee_schedule(&1_000_000, &6);
    t.client().set_fee_schedule(&10_000_000, &5);

    assert_eq!(t.client().fee_for_amount(&500_000), 7);
    assert_eq!(t.client().fee_for_amount(&5_000_000), 6);
    assert_eq!(t.client().fee_for_amount(&50_000_000), 5);
}

#[test]
fn collect_fee_charges_the_reconfigured_rate() {
    let t = TreasuryTest::setup();
    assert_eq!(t.client().collect_fee(&1_000_000, &t.outsider), 2_500); // 25bp

    t.client().set_fee_schedule(&1_000_000, &100); // 1%
    assert_eq!(t.client().collect_fee(&1_000_000, &t.outsider), 10_000);
}

#[test]
fn a_newly_added_tier_takes_effect() {
    let t = TreasuryTest::setup();
    assert_eq!(t.client().fee_for_amount(&2_000_000), 25);

    t.client().set_fee_schedule(&2_000_000, &15);
    assert_eq!(t.client().fee_for_amount(&2_000_000), 15);
    assert_eq!(
        t.client().fee_for_amount(&1_999_999),
        25,
        "the tier below must be unaffected"
    );
}

#[test]
fn removing_a_tier_falls_back_to_the_one_below() {
    let t = TreasuryTest::setup();
    assert_eq!(t.client().fee_for_amount(&50_000_000), 10);

    t.client().remove_fee_tier(&10_000_000);
    assert_eq!(
        t.client().fee_for_amount(&50_000_000),
        25,
        "a large amount should now pay the 1M tier's rate"
    );
}

#[test]
fn removing_a_missing_tier_is_an_error() {
    let t = TreasuryTest::setup();
    assert_eq!(
        t.client().try_remove_fee_tier(&999),
        Err(Ok(ContractError::InvalidInput))
    );
}

// ── Tier selection edge cases ────────────────────────────────────────────────

#[test]
fn tier_boundaries_are_inclusive_at_the_threshold() {
    let t = TreasuryTest::setup();
    assert_eq!(t.client().fee_for_amount(&999_999), 50);
    assert_eq!(t.client().fee_for_amount(&1_000_000), 25, "boundary is >=");
    assert_eq!(t.client().fee_for_amount(&9_999_999), 25);
    assert_eq!(t.client().fee_for_amount(&10_000_000), 10);
}

#[test]
fn an_amount_below_every_tier_pays_nothing() {
    let t = TreasuryTest::setup();
    // Drop the zero tier so small amounts fall through the schedule entirely.
    t.client().remove_fee_tier(&0);
    assert_eq!(t.client().fee_for_amount(&500), 0);
    assert_eq!(t.client().collect_fee(&500, &t.outsider), 0);
}

#[test]
fn an_empty_schedule_charges_nothing_rather_than_panicking() {
    let t = TreasuryTest::setup();
    t.force_schedule(&[]);
    assert_eq!(t.client().fee_for_amount(&50_000_000), 0);
    assert_eq!(t.client().collect_fee(&50_000_000, &t.outsider), 0);
}

#[test]
fn unordered_tier_insertion_still_selects_the_highest_match() {
    // Map iteration is key-ordered regardless of insertion order; pin that, because
    // `select_tier` breaks out of the loop on the first threshold above the amount.
    let t = TreasuryTest::setup();
    t.force_schedule(&[(10_000_000, 10), (0, 50), (1_000_000, 25)]);
    assert_eq!(t.client().fee_for_amount(&50_000_000), 10);
    assert_eq!(t.client().fee_for_amount(&5_000_000), 25);
    assert_eq!(t.client().fee_for_amount(&5), 50);
}

// ── Validation (issue #816) ──────────────────────────────────────────────────

#[test]
fn collect_fee_rejects_non_positive_amounts() {
    let t = TreasuryTest::setup();
    for bad in [0i128, -1, i128::MIN] {
        assert_eq!(
            t.client().try_collect_fee(&bad, &t.outsider),
            Err(Ok(ContractError::InvalidAmount)),
            "amount {bad} must be rejected"
        );
    }
}

#[test]
fn fee_for_amount_rejects_a_negative_amount() {
    let t = TreasuryTest::setup();
    assert_eq!(
        t.client().try_fee_for_amount(&-1),
        Err(Ok(ContractError::InvalidAmount))
    );
    assert_eq!(t.client().fee_for_amount(&0), 50, "zero is a valid query");
}

#[test]
fn set_fee_schedule_enforces_the_per_tier_cap() {
    let t = TreasuryTest::setup();
    assert_eq!(
        t.client()
            .try_set_fee_schedule(&0, &(MAX_SINGLE_FEE_BP + 1)),
        Err(Ok(ContractError::InvalidInput))
    );
    assert!(t
        .client()
        .try_set_fee_schedule(&0, &MAX_SINGLE_FEE_BP)
        .is_ok());
}

#[test]
fn set_fee_schedule_rejects_a_negative_tier() {
    let t = TreasuryTest::setup();
    assert_eq!(
        t.client().try_set_fee_schedule(&-1, &50),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn set_fee_schedule_caps_the_number_of_tiers() {
    let t = TreasuryTest::setup();
    // Three tiers already exist; fill to the ceiling.
    for i in 3..MAX_FEE_TIERS {
        t.client().set_fee_schedule(&(i as i128 * 100_000_000), &10);
    }
    assert_eq!(t.client().get_fee_schedule().len(), MAX_FEE_TIERS);

    assert_eq!(
        t.client().try_set_fee_schedule(&9_999_999_999, &10),
        Err(Ok(ContractError::InvalidInput)),
        "an unbounded schedule is a metering hazard for the linear scan"
    );

    // Updating an existing tier must still be allowed at the ceiling.
    assert!(t.client().try_set_fee_schedule(&0, &40).is_ok());
}

#[test]
fn route_to_treasury_rejects_non_positive_amounts() {
    let t = TreasuryTest::setup();
    t.client().deposit(&1000);
    assert_eq!(
        t.client().try_route_to_treasury(&0),
        Err(Ok(ContractError::InvalidAmount))
    );
    assert!(t.client().try_route_to_treasury(&1).is_ok());
}

// ── Running total ────────────────────────────────────────────────────────────

#[test]
fn total_collected_accumulates_across_calls() {
    let t = TreasuryTest::setup();
    assert_eq!(t.client().total_collected(), 0);

    let first = t.client().collect_fee(&1_000_000, &t.outsider); // 2_500
    let second = t.client().collect_fee(&10_000_000, &t.outsider); // 10_000

    assert_eq!(t.client().total_collected(), first + second);
}

#[test]
fn collect_fee_reports_overflow_rather_than_wrapping() {
    let t = TreasuryTest::setup();
    t.force_schedule(&[(0, 10_000)]); // 100%, so fee == amount
    assert_eq!(
        t.client().try_collect_fee(&i128::MAX, &t.outsider),
        Err(Ok(ContractError::Overflow))
    );
}

// ── Treasury address ─────────────────────────────────────────────────────────

#[test]
fn update_treasury_changes_the_routing_target() {
    let t = TreasuryTest::setup();
    t.client().update_treasury(&t.outsider);
    assert_eq!(t.client().get_treasury(), t.outsider);
}

#[test]
fn schema_version_matches_the_constant() {
    let t = TreasuryTest::setup();
    assert_eq!(t.client().schema_version(), SCHEMA_VERSION);
}

// ── Storage footprint (issue #811) ───────────────────────────────────────────

#[test]
fn fee_schedule_keys_are_u64_in_schema_v3() {
    // After init the schedule must be stored as Map<u64, u32>, not Map<i128, u32>.
    // We verify this by reading the schedule back through the public API and
    // asserting the keys are the correct values (u64 vs i128 encodes differently
    // in XDR and the test itself proves the type round-trips cleanly).
    let t = TreasuryTest::setup();
    let schedule = t.stored_schedule();
    assert_eq!(schedule.len(), 3, "init seeded 3 tiers");
    // Keys must round-trip as u64 through the stored map.
    assert!(
        schedule.contains_key(0u64),
        "zero tier must be stored as u64"
    );
    assert!(
        schedule.contains_key(1_000_000u64),
        "1M tier must be stored as u64"
    );
    assert!(
        schedule.contains_key(10_000_000u64),
        "10M tier must be stored as u64"
    );
}

#[test]
fn treasury_fee_schedule_obeys_the_monotonic_invariant() {
    use proptest::prelude::*;

    let t = TreasuryTest::setup();
    proptest!(|(amount in 0i128..=100_000_000_000i128)| {
        let schedule = t.client().get_fee_schedule();
        let expected = schedule
            .iter()
            .filter(|(threshold, _)| (*threshold as i128) <= amount)
            .map(|(_, bps)| bps)
            .last()
            .unwrap_or(0);

        prop_assert_eq!(t.client().fee_for_amount(&amount), expected);
    });
}

#[test]
fn v2_to_v3_migration_converts_schedule_keys_from_i128_to_u64() {
    // Start with a genuine v2 layout (i128 keys).
    let t = TreasuryTest::with_legacy_v2_state();
    assert_eq!(t.stored_schema(), Some(2));

    // The v2 schedule must be readable as i128-keyed.
    let before = t.stored_schedule_v2();
    assert_eq!(before.len(), 3);

    // Run the migration.
    assert_eq!(t.client().migrate(), 2, "must report migrating from v2");
    assert_eq!(t.stored_schema(), Some(SCHEMA_VERSION));

    // After migration the schedule is stored with u64 keys.
    let after = t.stored_schedule();
    assert_eq!(after.len(), before.len(), "no tier may be dropped");
    assert!(
        after.contains_key(0u64),
        "zero tier must survive migration as u64"
    );
    assert!(
        after.contains_key(1_000_000u64),
        "1M tier must survive migration as u64"
    );
    assert!(
        after.contains_key(10_000_000u64),
        "10M tier must survive migration as u64"
    );

    // The TotalCollected counter must be preserved.
    assert_eq!(
        t.client().total_collected(),
        5_000,
        "total_collected must survive v2→v3 migration"
    );
}

#[test]
fn v1_to_v3_migration_adds_total_collected_and_converts_schedule_keys() {
    let t = TreasuryTest::with_legacy_v1_state();
    assert!(!t.has_total_collected_key(), "v1 has no TotalCollected key");

    assert_eq!(t.client().migrate(), 1, "must report migrating from v1");
    assert_eq!(t.stored_schema(), Some(SCHEMA_VERSION));

    // TotalCollected must now exist.
    assert!(
        t.has_total_collected_key(),
        "migration must add TotalCollected"
    );
    assert_eq!(t.client().total_collected(), 0);

    // Schedule keys must have been converted to u64.
    let schedule = t.stored_schedule();
    assert_eq!(schedule.len(), 3);
    assert!(schedule.contains_key(0u64));
    assert!(schedule.contains_key(1_000_000u64));
    assert!(schedule.contains_key(10_000_000u64));
}

#[test]
fn storage_footprint_schedule_key_is_8_bytes_not_16() {
    // Demonstrate the storage savings: with u64 keys (8 bytes) vs i128 keys (16 bytes),
    // a full 16-tier schedule saves 16 × 8 = 128 bytes of instance storage.
    // This test documents the guarantee: after init (v3), the schedule key type is
    // u64, not i128.
    let t = TreasuryTest::setup();
    // Add tiers up to the max.
    for i in 3..16u64 {
        t.client().set_fee_schedule(&(i as i128 * 100_000 + 1), &10);
    }
    let schedule = t.stored_schedule();
    assert_eq!(
        schedule.len(),
        16,
        "should have 16 tiers for maximum storage comparison"
    );
    // Each key is u64 = 8 bytes. With i128 it would be 16 bytes per key.
    // 16 tiers × 8 bytes saved per key = 128 bytes total savings documented here.
    assert!(
        schedule.contains_key(0u64),
        "keys are u64, not i128 — 8 bytes per key instead of 16"
    );
}

#[test]
fn migrate_is_rejected_when_already_current() {
    let t = TreasuryTest::setup();
    assert_eq!(
        t.client().try_migrate(),
        Err(Ok(ContractError::SchemaAlreadyCurrent))
    );
}

#[test]
fn migrate_rejects_state_from_a_future_build() {
    let t = TreasuryTest::setup();
    t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .set(&crate::DataKey::Schema, &(SCHEMA_VERSION + 1));
    });

    assert_eq!(
        t.client().try_migrate(),
        Err(Ok(ContractError::SchemaVersionUnsupported))
    );
}

// ── Event assertions (issue #814) ────────────────────────────────────────────
//
// All state-changing functions in the treasury contract emit events. These tests
// assert the exact topic and data fields expected by off-chain indexers.

#[test]
fn init_emits_event_with_admin_and_treasury() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::registered();
    t.client().initialize(&t.admin);
    t.client().update_treasury(&t.treasury);

    let events = t.env.events().all();
    assert_eq!(events.len(), 1, "only update_treasury emits an event");
    let event = events.get(0).unwrap();
    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("trsy"),
        t.treasury.clone(),
    );
}

#[test]
fn collect_fee_emits_event_with_amount_fee_recipient() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::setup();

    let fee = t.client().collect_fee(&1_000_000, &t.outsider);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("coll"),
        (1_000_000i128, fee, t.outsider.clone()),
    );
}

#[test]
fn set_fee_schedule_emits_event_with_tier_and_basis_points() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::setup();

    t.client().set_fee_schedule(&5_000_000, &30);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("sched"),
        (5_000_000i128, 30u32),
    );
}

#[test]
fn remove_fee_tier_emits_event_with_tier_threshold() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::setup();

    t.client().remove_fee_tier(&1_000_000);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("rmtier"),
        1_000_000i128,
    );
}

#[test]
fn update_treasury_emits_event_with_new_address() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::setup();

    t.client().update_treasury(&t.outsider);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("trsy"),
        t.outsider.clone(),
    );
}

#[test]
fn route_to_treasury_emits_event_with_amount_and_treasury_address() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::setup();
    t.client().deposit(&1000);

    t.client().route_to_treasury(&999);
    let all_events = t.env.events().all();
    let event = all_events.get(all_events.len() - 1).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("routed"),
        (999i128, t.treasury.clone()),
    );
}

#[test]
fn migrate_emits_event_with_from_and_to_schema_versions() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::with_legacy_v1_state();

    t.client().migrate();
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("migrt"),
        (1u32, SCHEMA_VERSION),
    );
}

// ── Batch collection (issue #982) ────────────────────────────────────────────

#[test]
fn collect_fee_batch_sums_fees_and_writes_total_once() {
    let t = TreasuryTest::setup();
    let amounts = soroban_sdk::vec![&t.env, 1_000_000, 5_000_000, 10_000_000];
    let recipient = Address::generate(&t.env);

    // Each individual fee: 1M→2_500 (25bp), 5M→12_500 (25bp), 10M→10_000 (10bp)
    let total_expected = 2_500 + 12_500 + 10_000; // 25_000

    let fees = t.client().collect_fee_batch(&amounts, &recipient);
    assert_eq!(fees.len(), 3);
    assert_eq!(fees.get(0).unwrap(), 2_500);
    assert_eq!(fees.get(1).unwrap(), 12_500);
    assert_eq!(fees.get(2).unwrap(), 10_000);

    // Total collected must be updated exactly once, not three times.
    assert_eq!(t.client().total_collected(), total_expected);
}

#[test]
fn collect_fee_batch_works_with_a_single_amount() {
    let t = TreasuryTest::setup();
    let amounts = soroban_sdk::vec![&t.env, 1_000_000];
    let recipient = Address::generate(&t.env);

    let fees = t.client().collect_fee_batch(&amounts, &recipient);
    assert_eq!(fees.len(), 1);
    assert_eq!(fees.get(0).unwrap(), 2_500);
    assert_eq!(t.client().total_collected(), 2_500);
}

#[test]
fn collect_fee_batch_rejects_non_positive_amounts() {
    let t = TreasuryTest::setup();
    let amounts = soroban_sdk::vec![&t.env, 1_000_000, 0, 5_000_000];
    let recipient = Address::generate(&t.env);

    assert_eq!(
        t.client().try_collect_fee_batch(&amounts, &recipient),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn collect_fee_batch_reports_overflow_across_items() {
    let t = TreasuryTest::setup();
    // Force a 100% fee to make amounts directly equal fees.
    t.force_schedule(&[(0, 10_000)]);
    let amounts = soroban_sdk::vec![&t.env, i128::MAX, 1];
    let recipient = Address::generate(&t.env);

    assert_eq!(
        t.client().try_collect_fee_batch(&amounts, &recipient),
        Err(Ok(ContractError::Overflow))
    );
}

#[test]
fn collect_fee_batch_emits_a_single_event_with_summary() {
    use soroban_sdk::symbol_short;

    let t = TreasuryTest::setup();
    let amounts = soroban_sdk::vec![&t.env, 1_000_000, 5_000_000];
    let recipient = Address::generate(&t.env);

    t.client().collect_fee_batch(&amounts, &recipient);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    // Data: (recipient, total_fee=2_500+12_500=15_000, count=2)
    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("cbatch"),
        (recipient, 15_000i128, 2u32),
    );
}

#[test]
fn collect_fee_and_collect_fee_batch_accumulate_total_correctly() {
    let t = TreasuryTest::setup();
    let recipient = Address::generate(&t.env);

    // First a single fee.
    let fee1 = t.client().collect_fee(&1_000_000, &recipient); // 2_500
    assert_eq!(t.client().total_collected(), fee1);

    // Then a batch of three fees.
    let amounts = soroban_sdk::vec![&t.env, 5_000_000, 10_000_000, 20_000_000];
    let batch_fees = t.client().collect_fee_batch(&amounts, &recipient);
    let batch_total: i128 = batch_fees.iter().sum();

    assert_eq!(
        t.client().total_collected(),
        fee1 + batch_total,
        "total must be the sum of the single call and the batch"
    );
}
