//! Multisig-authority storage schema upgrade safety tests (issue #817).
//!
//! v1 → v2 adds `expires_at` to every proposal. The harness seeds a genuine
//! `Map<String, ProposalV1>` (no `expires_at` field at all) and verifies `migrate`
//! derives a sane expiry from each proposal's existing `created_at` rather than
//! leaving in-flight proposals either immediately expired or permanently signable.

use multisig_authority::test_utils::MultisigTest;
use multisig_authority::{DataKey, DEFAULT_PROPOSAL_TTL_LEDGERS, SCHEMA_VERSION};
use stellar_spend_shared::errors::ContractError;

// ── Pre-conditions ───────────────────────────────────────────────────────────

#[test]
fn seeded_state_reports_the_old_schema_version() {
    let (t, _, _) = MultisigTest::registered_with_legacy_v1_state();
    assert_eq!(t.stored_schema(), Some(1));
    assert_ne!(
        SCHEMA_VERSION, 1,
        "this test is meaningless once v1 is current"
    );
}

#[test]
fn entrypoints_refuse_to_run_against_un_migrated_state() {
    let (t, id, _) = MultisigTest::registered_with_legacy_v1_state();

    assert_eq!(
        t.client().try_get_proposal(&id),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_sign(&t.signer(1), &id),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::MigrationRequired))
    );
    assert_eq!(
        t.client().try_get_signers(),
        Err(Ok(ContractError::MigrationRequired))
    );
}

// ── The migration ────────────────────────────────────────────────────────────

#[test]
fn migrate_derives_expiry_from_the_existing_created_at() {
    let (t, id, _) = MultisigTest::registered_with_legacy_v1_state();

    assert_eq!(t.client().migrate(&t.admin), 1);
    assert_eq!(t.stored_schema(), Some(SCHEMA_VERSION));

    let migrated = t.client().get_proposal(&id);
    assert_eq!(
        migrated.expires_at,
        migrated.created_at + DEFAULT_PROPOSAL_TTL_LEDGERS,
        "expiry must be derived from the pre-existing created_at, not from the \
         migration's own ledger"
    );
}

#[test]
fn migration_preserves_every_existing_signature() {
    let (t, id, _) = MultisigTest::registered_with_legacy_v1_state();
    let before = t.client().migrate(&t.admin);
    assert_eq!(before, 1);

    let migrated = t.client().get_proposal(&id);
    assert_eq!(migrated.signatures.len(), 1);
    assert_eq!(migrated.signatures.get(0).unwrap(), t.signer(0));
    assert!(!migrated.executed);
}

#[test]
fn contract_is_fully_operational_after_migration() {
    let (t, id, target) = MultisigTest::registered_with_legacy_v1_state();
    t.client().migrate(&t.admin);

    // The migrated proposal can still collect signatures and execute.
    assert_eq!(t.client().sign(&t.signer(1), &id), 2);
    let value = t.client().execute(&t.signer(0), &id);
    assert_eq!(
        value,
        multisig_authority::test_utils::DEFAULT_HIGH_VALUE_LIMIT * 10
    );

    // And brand-new proposals work normally.
    let new_id = t.id("post-migration");
    t.client()
        .propose(&t.signer(0), &new_id, &t.id("d"), &target, &10);
    assert_eq!(t.client().execute(&t.signer(0), &new_id), 10);
}

#[test]
fn a_migrated_proposal_still_expires_on_schedule() {
    let (t, id, _) = MultisigTest::registered_with_legacy_v1_state();
    t.client().migrate(&t.admin);

    // seed_v1_state recorded created_at = START_LEDGER; migrate must not reset the
    // clock to "now", or every migrated proposal would get a fresh TTL for free.
    t.advance_past_proposal_expiry();
    assert_eq!(
        t.client().try_sign(&t.signer(1), &id),
        Err(Ok(ContractError::Expired))
    );
}

// ── Guards ───────────────────────────────────────────────────────────────────

#[test]
fn migrate_is_rejected_when_already_current() {
    let t = MultisigTest::setup();
    assert_eq!(
        t.client().try_migrate(&t.admin),
        Err(Ok(ContractError::SchemaAlreadyCurrent))
    );
}

#[test]
fn migrate_is_not_repeatable() {
    let (t, _, _) = MultisigTest::registered_with_legacy_v1_state();
    t.client().migrate(&t.admin);
    assert_eq!(
        t.client().try_migrate(&t.admin),
        Err(Ok(ContractError::SchemaAlreadyCurrent))
    );
}

#[test]
fn migrate_requires_admin() {
    let (t, _, _) = MultisigTest::registered_with_legacy_v1_state();
    assert_eq!(
        t.client().try_migrate(&t.outsider),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn migrate_rejects_state_from_a_future_build() {
    let t = MultisigTest::setup();
    t.env.as_contract(&t.contract_id, || {
        t.env.storage().instance().set(
            &DataKey::Schema,
            &(SCHEMA_VERSION + 1),
        );
    });

    assert_eq!(
        t.client().try_migrate(&t.admin),
        Err(Ok(ContractError::SchemaVersionUnsupported))
    );
    assert_eq!(
        t.client().try_get_signers(),
        Err(Ok(ContractError::SchemaVersionUnsupported))
    );
}

#[test]
fn migrate_on_an_uninitialised_contract_reports_not_found() {
    // `migrate` checks admin via local `assert_is_admin` which reads
    // `DataKey::Signers`; since `registered()` never called `init`, the
    // config is absent and the helper reports `NotFound`.
    let t = MultisigTest::registered();
    assert_eq!(
        t.client().try_migrate(&t.admin),
        Err(Ok(ContractError::NotFound))
    );
}
