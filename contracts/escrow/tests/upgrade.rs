//! Storage schema upgrade safety tests (issue #817).
//!
//! A Soroban upgrade swaps the contract WASM but leaves storage untouched. If the
//! new build reads an existing entry with a differently-shaped type, the decode
//! either fails outright or — worse — succeeds against the wrong field layout and
//! silently corrupts live balances. These tests deploy a **v1** storage layout,
//! apply the upgrade path, and assert every pre-existing entry is still readable
//! and semantically unchanged.
//!
//! The seeding lives in `escrow::test_utils::seed_v1_state`, which writes a genuine
//! `Map<u64, EscrowDepositV1>` through `env.as_contract`, so these tests exercise
//! real old-shaped bytes rather than a current record with a field zeroed out.

use escrow::test_utils::{v1_deposit, EscrowTest, START_LEDGER};
use escrow::{DEFAULT_TIMEOUT_LEDGERS, EscrowStatus, SCHEMA_VERSION};
use stellar_spend_shared::errors::ContractError;

// ── Pre-conditions: the seeded state really is the old layout ────────────────

#[test]
fn seeded_state_reports_the_old_schema_version() {
    let t = EscrowTest::with_legacy_v1_state();
    assert_eq!(t.stored_schema(), Some(1));
    assert_ne!(
        SCHEMA_VERSION, 1,
        "this test is meaningless once v1 is current"
    );
}

#[test]
fn seeded_state_decodes_as_v1_and_not_as_v2() {
    let t = EscrowTest::with_legacy_v1_state();

    // Decoding as v1 succeeds: the bytes are genuinely the old shape.
    let v1 = t.deposits_v1();
    assert_eq!(v1.len(), 2);
    assert_eq!(v1.get(0).unwrap().amount, 5_000);

    // Decoding the same bytes as v2 must NOT quietly succeed — that is exactly the
    // silent-corruption failure mode this issue is about.
    //
    // The read has to reach an *element*: `Map` is a lazily-decoded handle to a host
    // object, so materialising `Map<u64, EscrowDeposit>` from v1 bytes succeeds on
    // its own and only fails once a record is actually converted.
    let as_v2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        t.deposits_v2().get(0).map(|record| record.amount)
    }));
    assert!(
        as_v2.is_err(),
        "v1 bytes decoded cleanly as a v2 record; the layouts are not actually \
         distinguishable and the migration guard cannot protect anything"
    );
}

// ── Guard: stale state is refused before migration ──────────────────────────

#[test]
fn entrypoints_refuse_to_run_against_un_migrated_state() {
    let t = EscrowTest::with_legacy_v1_state();

    // Every normal entrypoint must fail closed rather than decode v1 bytes as v2.
    assert_eq!(
        t.client().try_get_deposit(&0),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_deposit(&t.depositor, &100, &t.bridge, &0),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_release(&0, &t.bridge),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_refund(&0),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_set_timeout(&100),
        Err(Ok(ContractError::MigrationRequired))
    );
}

// ── The migration itself ─────────────────────────────────────────────────────

#[test]
fn migrate_preserves_every_existing_deposit_field() {
    let t = EscrowTest::with_legacy_v1_state();
    let before = t.deposits_v1();

    assert_eq!(t.client().migrate(), 1, "should report migrating from v1");
    assert_eq!(t.stored_schema(), Some(SCHEMA_VERSION));

    let after = t.deposits_v2();
    assert_eq!(after.len(), before.len(), "no deposit may be dropped");

    for (id, old) in before.iter() {
        let new = after.get(id).expect("deposit vanished during migration");
        assert_eq!(new.depositor, old.depositor, "deposit {id}: depositor");
        assert_eq!(new.amount, old.amount, "deposit {id}: amount");
        assert_eq!(
            new.bridge_address, old.bridge_address,
            "deposit {id}: bridge"
        );
        assert_eq!(new.timestamp, old.timestamp, "deposit {id}: timestamp");
        assert_eq!(
            new.timeout_ledger, old.timeout_ledger,
            "deposit {id}: timeout ledger"
        );
        let expected_status = if old.released {
            EscrowStatus::Resolved
        } else if old.refunded {
            EscrowStatus::Cancelled
        } else {
            EscrowStatus::Pending
        };
        assert_eq!(new.status, expected_status, "deposit {id}: status");
        assert_eq!(new.fee_bps, 0, "deposit {id}: new field defaults to zero");
    }
}

#[test]
fn migrated_deposits_are_readable_through_the_public_api() {
    let t = EscrowTest::with_legacy_v1_state();
    t.client().migrate();

    // The whole point: state written by the old build is usable by the new one.
    let open = t.client().get_deposit(&0);
    assert_eq!(open.amount, 5_000);
    assert_eq!(open.status, EscrowStatus::Pending);

    let already_released = t.client().get_deposit(&1);
    assert_eq!(already_released.amount, 9_100);
    assert_eq!(already_released.status, EscrowStatus::Resolved);
}

#[test]
fn migration_does_not_reopen_a_settled_deposit() {
    let t = EscrowTest::with_legacy_v1_state();
    t.client().migrate();

    // Deposit 1 was released under v1; the upgrade must not have cleared that flag
    // and made the funds claimable a second time.
    assert_eq!(
        t.client().try_release(&1, &t.bridge),
        Err(Ok(ContractError::AlreadyProcessed))
    );
    t.advance_past_timeout();
    assert_eq!(
        t.client().try_refund(&1),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn contract_is_fully_operational_after_migration() {
    let t = EscrowTest::with_legacy_v1_state();
    t.client().migrate();

    // New deposits continue the old id sequence rather than colliding with it.
    let id = t.deposit_with_fee(2_500, 30);
    assert_eq!(id, 2, "next id must continue past the migrated records");
    assert_eq!(t.client().get_deposit(&id).fee_bps, 30);

    // And the migrated open deposit still follows the normal lifecycle.
    assert_eq!(t.client().release(&0, &t.bridge), 5_000);
}

// ── Migration is guarded and non-repeatable ─────────────────────────────────

#[test]
fn migrate_is_rejected_when_already_current() {
    let t = EscrowTest::setup();
    assert_eq!(
        t.client().try_migrate(),
        Err(Ok(ContractError::SchemaAlreadyCurrent)),
        "a freshly initialised contract has nothing to migrate"
    );
}

#[test]
fn migrate_is_not_repeatable() {
    let t = EscrowTest::with_legacy_v1_state();
    t.client().migrate();
    assert_eq!(
        t.client().try_migrate(),
        Err(Ok(ContractError::SchemaAlreadyCurrent)),
        "running migrate twice must not re-default fee_bps on live records"
    );
}

#[test]
fn migrate_rejects_state_from_a_future_build() {
    let t = EscrowTest::setup();
    t.env.as_contract(&t.contract_id, || {
        t.env
            .storage()
            .instance()
            .set(&escrow::DataKey::Schema, &(SCHEMA_VERSION + 1));
    });

    // Downgrading is not something this code can do safely, so it must refuse.
    assert_eq!(
        t.client().try_migrate(),
        Err(Ok(ContractError::SchemaVersionUnsupported))
    );
    assert_eq!(
        t.client().try_get_deposit(&0),
        Err(Ok(ContractError::SchemaVersionUnsupported)),
        "normal entrypoints must also refuse state they are too old to understand"
    );
}

#[test]
fn migrate_on_an_uninitialised_contract_reports_not_initialised() {
    let t = EscrowTest::registered();
    assert_eq!(
        t.client().try_migrate(),
        Err(Ok(ContractError::NotInitialized))
    );
}

#[test]
fn migration_preserves_a_refunded_deposit() {
    // The shared fixture covers open and released records; seed a custom v1 state to
    // cover the third terminal state, which must also survive the upgrade.
    let t = EscrowTest::registered();
    let deadline = START_LEDGER + DEFAULT_TIMEOUT_LEDGERS;
    t.seed_v1_state(&[
        (
            0,
            v1_deposit(&t.depositor, &t.bridge, 1_200, deadline, false, true),
        ),
        (
            1,
            v1_deposit(&t.other, &t.bridge, 800, deadline, false, false),
        ),
    ]);

    t.client().migrate();

    let refunded = t.client().get_deposit(&0);
    assert_eq!(refunded.status, EscrowStatus::Cancelled, "a refunded deposit must stay refunded");
    assert_eq!(refunded.amount, 1_200);
    assert_eq!(
        t.client().try_release(&0, &t.bridge),
        Err(Ok(ContractError::AlreadyProcessed)),
        "the upgrade must not make already-refunded funds releasable"
    );

    // The untouched neighbour is unaffected.
    assert_eq!(t.client().get_deposit(&1).status, EscrowStatus::Pending);
}

// ── Sanity: the fixture's own assumptions ───────────────────────────────────

#[test]
fn legacy_fixture_matches_the_timeout_arithmetic_of_a_v1_build() {
    let t = EscrowTest::with_legacy_v1_state();
    t.client().migrate();
    assert_eq!(
        t.client().get_deposit(&0).timeout_ledger,
        START_LEDGER + DEFAULT_TIMEOUT_LEDGERS,
        "seeded deadlines must line up with what a v1 deposit would have computed"
    );
}
