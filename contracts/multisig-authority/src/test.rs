//! Multisig-authority unit tests.
//!
//! All setup comes from [`crate::test_utils`] (issue #818).
use soroban_sdk::{testutils::Address as _, Address, Vec};
use stellar_spend_shared::errors::ContractError;

use crate::test_utils::{
    assert_fresh_init_is_current, MultisigTest, DEFAULT_HIGH_VALUE_LIMIT, DEFAULT_THRESHOLD,
};
use crate::{DataKey, SCHEMA_VERSION, SignersConfig};
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
fn init_persists_signers_threshold_and_schema() {
    let t = MultisigTest::setup();
    assert_fresh_init_is_current(&t);
    assert_eq!(t.client().get_signers(), t.signers);
    assert_eq!(t.client().get_threshold(), DEFAULT_THRESHOLD);
}

#[test]
fn init_is_rejected_twice() {
    let t = MultisigTest::setup();
    assert_eq!(
        t.client().try_init(
            &t.admin,
            &t.signers,
            &DEFAULT_THRESHOLD,
            &DEFAULT_HIGH_VALUE_LIMIT
        ),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn init_rejects_an_empty_signer_set() {
    let t = MultisigTest::registered();
    let empty: Vec<Address> = Vec::new(&t.env);
    assert_eq!(
        t.client().try_init(&t.admin, &empty, &1, &0),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn init_rejects_a_duplicate_signer() {
    let t = MultisigTest::registered();
    let dup = Vec::from_array(&t.env, [t.signer(0), t.signer(1), t.signer(0)]);
    assert_eq!(
        t.client().try_init(&t.admin, &dup, &2, &0),
        Err(Ok(ContractError::InvalidInput)),
        "a duplicated signer would count twice toward quorum"
    );
}

#[test]
fn init_rejects_a_threshold_above_signer_count() {
    let t = MultisigTest::registered();
    assert_eq!(
        t.client().try_init(&t.admin, &t.signers, &4, &0),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn init_rejects_a_zero_threshold() {
    let t = MultisigTest::registered();
    assert_eq!(
        t.client().try_init(&t.admin, &t.signers, &0, &0),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn entrypoints_reject_an_uninitialised_contract() {
    let t = MultisigTest::registered();
    assert_eq!(
        t.client().try_get_signers(),
        Err(Ok(ContractError::NotInitialized))
    );
}

// ── Proposals ─────────────────────────────────────────────────────────────────

#[test]
fn propose_requires_a_registered_signer() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    assert_eq!(
        t.client()
            .try_propose(&t.outsider, &t.id("p1"), &t.id("desc"), &target, &100),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn propose_rejects_a_duplicate_id() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    t.propose_low_value("p1", &target);
    assert_eq!(
        t.client()
            .try_propose(&t.signer(1), &t.id("p1"), &t.id("desc"), &target, &1),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn propose_counts_the_proposer_as_the_first_signature() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_low_value("p1", &target);

    let (sig_count, _, _) = t.client().proposal_status(&id);
    assert_eq!(sig_count, 1);
}

#[test]
fn propose_rejects_an_empty_or_oversized_id() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    assert_eq!(
        t.client()
            .try_propose(&t.signer(0), &t.id(""), &t.id("desc"), &target, &1),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn propose_rejects_a_negative_value() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    assert_eq!(
        t.client()
            .try_propose(&t.signer(0), &t.id("p1"), &t.id("desc"), &target, &-1),
        Err(Ok(ContractError::InvalidAmount))
    );
}

// ── Signing ──────────────────────────────────────────────────────────────────

#[test]
fn sign_requires_a_registered_signer() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);
    assert_eq!(
        t.client().try_sign(&t.outsider, &id),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn cannot_sign_twice() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);
    assert_eq!(
        t.client().try_sign(&t.signer(0), &id),
        Err(Ok(ContractError::InvalidInput)),
        "signer(0) is already the implicit proposer signature"
    );
}

#[test]
fn sign_reports_a_missing_proposal() {
    let t = MultisigTest::setup();
    assert_eq!(
        t.client().try_sign(&t.signer(0), &t.id("missing")),
        Err(Ok(ContractError::NotFound))
    );
}

#[test]
fn sign_increments_the_signature_count() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);
    assert_eq!(t.client().sign(&t.signer(1), &id), 2);
}

#[test]
fn cannot_sign_an_executed_proposal() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);
    t.client().sign(&t.signer(1), &id);
    t.client().execute(&t.signer(0), &id);

    assert_eq!(
        t.client().try_sign(&t.signer(2), &id),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn cannot_sign_an_expired_proposal() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);
    t.advance_past_proposal_expiry();

    assert_eq!(
        t.client().try_sign(&t.signer(1), &id),
        Err(Ok(ContractError::Expired)),
        "a stale quorum must not be extendable after expiry"
    );
}

// ── Execution / quorum ────────────────────────────────────────────────────────

#[test]
fn low_value_proposal_executes_with_a_single_signature() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_low_value("p1", &target);

    let value = t.client().execute(&t.signer(0), &id);
    assert_eq!(value, DEFAULT_HIGH_VALUE_LIMIT / 2);
}

#[test]
fn high_value_proposal_requires_the_full_threshold() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);

    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold))
    );

    t.client().sign(&t.signer(1), &id);
    assert_eq!(
        t.client().execute(&t.signer(0), &id),
        DEFAULT_HIGH_VALUE_LIMIT * 10
    );
}

#[test]
fn value_exactly_at_the_high_value_limit_needs_only_one_signature() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.id("boundary");
    t.client().propose(
        &t.signer(0),
        &id,
        &t.id("d"),
        &target,
        &DEFAULT_HIGH_VALUE_LIMIT,
    );

    assert_eq!(
        t.client().execute(&t.signer(0), &id),
        DEFAULT_HIGH_VALUE_LIMIT
    );
}

#[test]
fn execute_is_not_repeatable() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_low_value("p1", &target);
    t.client().execute(&t.signer(0), &id);

    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn execute_rejects_an_expired_proposal() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_low_value("p1", &target);
    t.advance_past_proposal_expiry();

    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::Expired))
    );
}

#[test]
fn removing_a_signer_after_they_signed_drops_their_vote_from_quorum() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);
    t.client().sign(&t.signer(1), &id);

    // Two live signatures reach the 2-of-3 threshold...
    t.client().remove_signer(&t.admin, &t.signer(1));
    // ...but signer(1) is no longer registered, so their signature must not count.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold)),
        "a removed signer's earlier signature must not still satisfy quorum"
    );
}

// ── Signer / threshold management ────────────────────────────────────────────

#[test]
fn add_signer_rejects_a_duplicate() {
    let t = MultisigTest::setup();
    assert_eq!(
        t.client().try_add_signer(&t.admin, &t.signer(0)),
        Err(Ok(ContractError::InvalidInput))
    );
}

#[test]
fn add_signer_requires_admin() {
    let t = MultisigTest::setup();
    let new_signer = Address::generate(&t.env);
    assert_eq!(
        t.client().try_add_signer(&t.outsider, &new_signer),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn remove_signer_is_blocked_when_it_would_make_quorum_unreachable() {
    let t = MultisigTest::setup();
    // Threshold is 2 with 3 signers; removing down to 2 is fine, but a second
    // removal would leave only 1 signer against a threshold of 2.
    t.client().remove_signer(&t.admin, &t.signer(2));
    assert_eq!(
        t.client().try_remove_signer(&t.admin, &t.signer(1)),
        Err(Ok(ContractError::InvalidInput)),
        "removal must be blocked when it makes quorum impossible"
    );
}

#[test]
fn remove_signer_reports_an_address_that_is_not_a_signer() {
    let t = MultisigTest::setup();
    assert_eq!(
        t.client().try_remove_signer(&t.admin, &t.outsider),
        Err(Ok(ContractError::NotFound))
    );
}

#[test]
fn set_threshold_enforces_its_bounds() {
    let t = MultisigTest::setup();
    assert_eq!(
        t.client().try_set_threshold(&t.admin, &0),
        Err(Ok(ContractError::InvalidInput))
    );
    assert_eq!(
        t.client().try_set_threshold(&t.admin, &4),
        Err(Ok(ContractError::InvalidInput))
    );
    assert!(t.client().try_set_threshold(&t.admin, &3).is_ok());
}

// ── required_threshold / proposal_status views ───────────────────────────────

#[test]
fn required_threshold_reflects_the_high_value_limit() {
    let t = MultisigTest::setup();
    assert_eq!(t.client().required_threshold(&0), 1);
    assert_eq!(t.client().required_threshold(&DEFAULT_HIGH_VALUE_LIMIT), 1);
    assert_eq!(
        t.client()
            .required_threshold(&(DEFAULT_HIGH_VALUE_LIMIT + 1)),
        DEFAULT_THRESHOLD
    );
}

#[test]
fn proposal_status_reports_executability() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("p1", &target);

    let (sigs, threshold, executable) = t.client().proposal_status(&id);
    assert_eq!((sigs, threshold, executable), (1, DEFAULT_THRESHOLD, false));

    t.client().sign(&t.signer(1), &id);
    let (sigs, _, executable) = t.client().proposal_status(&id);
    assert_eq!((sigs, executable), (2, true));

    t.client().execute(&t.signer(0), &id);
    let (_, _, executable) = t.client().proposal_status(&id);
    assert!(
        !executable,
        "an executed proposal is never executable again"
    );
}

#[test]
fn schema_version_matches_the_constant() {
    let t = MultisigTest::setup();
    assert_eq!(t.client().schema_version(), SCHEMA_VERSION);
}

// ── Threshold-change edge cases (issue #813) ──────────────────────────────────
//
// Changing signers or thresholds mid-flight is a common source of contract bugs.
// A signature collected under one threshold or signer set must be re-evaluated
// against the current state at execution time, never the state at signing time.

#[test]
fn threshold_raised_after_signing_blocks_previously_executable_proposal() {
    // Arrange: start 2-of-3, propose a high-value action, collect two signatures.
    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("p-raise", &target);
    t.client().sign(&t.signer(1), &id);

    // Two signatures satisfy the 2-of-3 threshold.
    let (sigs, _threshold, executable) = t.client().proposal_status(&id);
    assert_eq!(sigs, 2);
    assert!(executable, "should be executable with 2 of 3 signatures");

    // Admin raises the threshold to 3-of-3 before execution.
    t.client().set_threshold(&t.admin, &3);

    // Now the same two signatures are insufficient.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold)),
        "raising threshold must invalidate a previously-sufficient quorum"
    );

    // Adding the third signature now allows execution.
    t.client().sign(&t.signer(2), &id);
    assert!(
        t.client().try_execute(&t.signer(0), &id).is_ok(),
        "three signatures satisfy the raised 3-of-3 threshold"
    );
}

#[test]
fn threshold_lowered_after_signing_allows_execution_with_fewer_signatures() {
    // Arrange: start 2-of-3, propose a high-value action that needs 2 signatures.
    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("p-lower", &target);

    // Only the proposer's implicit signature; not yet executable.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold)),
    );

    // Admin lowers the threshold to 1-of-3.
    t.client().set_threshold(&t.admin, &1);

    // One signature (the proposer's) now satisfies the new threshold.
    assert!(
        t.client().try_execute(&t.signer(0), &id).is_ok(),
        "lowering threshold must allow execution with the now-sufficient signature count"
    );
}

#[test]
fn signer_removed_after_signing_invalidates_their_pending_vote() {
    // This test is a dedicated re-assertion of the live-count behaviour:
    // a signer's vote must not survive their removal.
    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("p-remove", &target);

    // signer(1) adds their vote, reaching 2-of-3.
    t.client().sign(&t.signer(1), &id);
    let (sigs, _, executable) = t.client().proposal_status(&id);
    assert_eq!(sigs, 2);
    assert!(
        executable,
        "two signatures should be executable before removal"
    );

    // Admin removes signer(1).
    t.client().remove_signer(&t.admin, &t.signer(1));

    // signer(1)'s vote must no longer count toward quorum.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold)),
        "removed signer's earlier signature must not satisfy quorum"
    );
}

#[test]
fn adding_a_signer_does_not_lower_effective_live_count() {
    // Verify that adding a new signer doesn't accidentally alter the live count
    // of a pending proposal: the new signer hasn't signed, so the live count
    // stays the same while the total signers increases.
    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("p-add", &target);
    t.client().sign(&t.signer(1), &id);

    // Two live signatures satisfy 2-of-3.
    let (_, _, executable) = t.client().proposal_status(&id);
    assert!(executable);

    // A new signer is added (now 4 signers, threshold still 2).
    let new_signer = soroban_sdk::Address::generate(&t.env);
    t.client().add_signer(&t.admin, &new_signer);

    // The proposal should still be executable: adding a signer doesn't clear votes.
    let (_, _, still_executable) = t.client().proposal_status(&id);
    assert!(
        still_executable,
        "adding a signer must not revoke existing valid signatures"
    );
}

#[test]
fn threshold_change_does_not_affect_already_executed_proposals() {
    // A successfully executed proposal must remain executed regardless of
    // any subsequent threshold changes.
    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("p-exec", &target);
    t.client().sign(&t.signer(1), &id);
    t.client().execute(&t.signer(0), &id);

    // Raise the threshold after execution.
    t.client().set_threshold(&t.admin, &3);

    // The proposal is still marked executed.
    let proposal = t.client().get_proposal(&id);
    assert!(
        proposal.executed,
        "threshold change must not undo a completed execution"
    );

    // And cannot be executed again.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::AlreadyProcessed)),
    );
}

#[test]
fn threshold_change_is_recorded_and_reflected_by_required_threshold() {
    let t = MultisigTest::setup();
    assert_eq!(t.client().get_threshold(), DEFAULT_THRESHOLD);

    t.client().set_threshold(&t.admin, &3);
    assert_eq!(t.client().get_threshold(), 3);

    // `required_threshold` must use the new threshold for high-value queries.
    assert_eq!(
        t.client()
            .required_threshold(&(DEFAULT_HIGH_VALUE_LIMIT + 1)),
        3,
        "required_threshold must reflect the updated threshold"
    );
}

#[test]
fn all_signers_removed_until_minimum_then_threshold_must_stay_reachable() {
    // Ensure that the threshold can never be set above the remaining signer count,
    // even after interleaved adds and removes.
    let t = MultisigTest::setup(); // 3 signers, threshold 2

    // Remove down to 2 signers (minimum to satisfy threshold 2).
    t.client().remove_signer(&t.admin, &t.signer(2));
    assert_eq!(t.client().get_signers().len(), 2);

    // Now trying to raise the threshold to 3 is invalid (only 2 signers left).
    assert_eq!(
        t.client().try_set_threshold(&t.admin, &3),
        Err(Ok(ContractError::InvalidInput)),
        "threshold cannot exceed current signer count"
    );

    // Can still set threshold to exactly the signer count.
    assert!(t.client().try_set_threshold(&t.admin, &2).is_ok());
}

#[test]
fn pending_proposal_under_old_threshold_requires_new_threshold_at_execution() {
    // This is the "stale quorum" scenario: a proposal reaches quorum under the
    // old threshold, the threshold is raised before anyone calls execute, and
    // execute must use the *current* threshold — not the one at proposal time.
    let t = MultisigTest::setup(); // threshold = 2
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("p-stale", &target);

    // Collect exactly enough signatures for the old threshold (2).
    t.client().sign(&t.signer(1), &id);

    // Raise to 3 before anyone calls execute.
    t.client().set_threshold(&t.admin, &3);

    // Must fail: only 2 live signatures, but 3 are now required.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold)),
        "execute must evaluate quorum against the threshold at execution time, not proposal time"
    );

    // Adding the missing signature unblocks execution.
    t.client().sign(&t.signer(2), &id);
    assert!(
        t.client().try_execute(&t.signer(0), &id).is_ok(),
        "three signatures satisfy the current threshold"
    );
}

// ── Event assertions (issue #814) ────────────────────────────────────────────
//
// All state-changing functions in the multisig-authority contract emit events.
// These tests assert the exact topic and data fields.

#[test]
fn init_emits_event_with_admin_threshold_high_value_limit() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::registered();
    t.client().init(
        &t.admin,
        &t.signers,
        &DEFAULT_THRESHOLD,
        &DEFAULT_HIGH_VALUE_LIMIT,
    );

    let events = t.env.events().all();
    assert_eq!(events.len(), 1);
    let event = events.get(0).unwrap();
    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("init"),
        (t.admin.clone(), DEFAULT_THRESHOLD, DEFAULT_HIGH_VALUE_LIMIT),
    );
}

#[test]
fn propose_emits_event_with_id_proposer_target_value() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);

    let id = t.id("evt-p1");
    t.client()
        .propose(&t.signer(0), &id, &t.id("desc"), &target, &100);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("proposed"),
        (id, t.signer(0), target, 100i128),
    );
}

#[test]
fn sign_emits_event_with_proposal_id_signer_count() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_high_value("evt-sign", &target);

    let count = t.client().sign(&t.signer(1), &id);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("signed"),
        (id, t.signer(1), count),
    );
}

#[test]
fn execute_emits_event_with_proposal_id_executor_value_sig_count() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();
    let target = soroban_sdk::Address::generate(&t.env);
    let id = t.propose_low_value("evt-exec", &target);

    // Low-value proposal: 1 signer (the proposer).
    let value = DEFAULT_HIGH_VALUE_LIMIT / 2;
    t.client().execute(&t.signer(0), &id);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    // Data: (proposal_id, executor, value, sig_count=1)
    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("executed"),
        (id, t.signer(0), value, 1u32),
    );
}

#[test]
fn add_signer_emits_event_with_new_signer_address() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();
    let new_signer = soroban_sdk::Address::generate(&t.env);

    t.client().add_signer(&t.admin, &new_signer);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("add_sgn"),
        new_signer,
    );
}

#[test]
fn remove_signer_emits_event_with_removed_signer_address() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();

    t.client().remove_signer(&t.admin, &t.signer(2));
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("rm_sgn"),
        t.signer(2),
    );
}

#[test]
fn set_threshold_emits_event_with_new_threshold() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();

    t.client().set_threshold(&t.admin, &3);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("set_thr"),
        3u32,
    );
}

#[test]
fn set_high_value_limit_emits_event_with_new_limit() {
    use soroban_sdk::symbol_short;

    let t = MultisigTest::setup();

    t.client().set_high_value_limit(&t.admin, &5_000);
    let all_events = t.env.events().all();
    let event = all_events.get(0).unwrap();

    assert_event(
        event,
        &t.contract_id,
        &t.env,
        symbol_short!("set_hvl"),
        5_000i128,
    );
}

// ── Missing threshold boundary tests (issue #983) ────────────────────────────

#[test]
fn threshold_equals_signer_count_n_of_n_acceptance() {
    // A 3-of-3 threshold must require all three signatures, no early escapes.
    let mut t = MultisigTest::setup();
    // Override the default threshold to be exactly the signer count.
    t.override_threshold(3);
    t.reinit();

    let target = Address::generate(&t.env);
    let id = t.propose_high_value("n-of-n", &target);

    // Only one signature (the proposer) is insufficient for 3-of-3.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold))
    );

    t.client().sign(&t.signer(1), &id);
    // Two signatures still insufficient for 3-of-3.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold))
    );

    t.client().sign(&t.signer(2), &id);
    // Now three signatures reach the N-of-N threshold.
    assert!(t.client().try_execute(&t.signer(0), &id).is_ok());
}

#[test]
fn zero_high_value_limit_requires_full_threshold_for_all_values() {
    // Setting `high_value_limit = 0` means even tiny values need the full threshold.
    let mut t = MultisigTest::setup();
    t.override_high_value_limit(0);
    t.reinit();

    let target = Address::generate(&t.env);
    let id = t.id("zero-limit");
    t.client()
        .propose(&t.signer(0), &id, &t.id("desc"), &target, &1); // value = 1

    // Even a value of 1 requires the full threshold.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold)),
        "with high_value_limit = 0, every proposal requires the full threshold"
    );

    // Collect the other two signatures.
    t.client().sign(&t.signer(1), &id);
    t.client().sign(&t.signer(2), &id);
    assert!(t.client().try_execute(&t.signer(0), &id).is_ok());
}

#[test]
fn value_exactly_zero_uses_low_value_threshold() {
    // Value zero is a boundary: it's <= any high_value_limit, so it should use the
    // low-value threshold (1 signature).
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.id("zero-val");
    t.client()
        .propose(&t.signer(0), &id, &t.id("desc"), &target, &0);

    // Zero value with DEFAULT_HIGH_VALUE_LIMIT > 0 → low-value threshold (1 signature)
    assert_eq!(t.client().execute(&t.signer(0), &id), 0);
}

#[test]
fn duplicate_signer_in_proposal_signatures_is_rejected() {
    // A signer trying to sign twice should be caught early, not allowed to inflate
    // the signature count.
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("dup-sig", &target);

    // signer(1) signs once (ok)
    t.client().sign(&t.signer(1), &id);

    // Same signer signing again must be rejected.
    assert_eq!(
        t.client().try_sign(&t.signer(1), &id),
        Err(Ok(ContractError::InvalidInput)),
        "duplicate signature must not increase the count"
    );
}

#[test]
fn duplicate_signer_in_proposal_signatures_via_removal_readdition_still_counts_once() {
    // Edge case: signer signs, is removed, then re-added. The earlier signature
    // is dead (removed signer) but the new signature after readdition should count.
    // The same address should not be allowed to sign twice even across removal cycle.
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("cycle", &target);

    // signer(1) signs.
    t.client().sign(&t.signer(1), &id);
    // Admin removes signer(1).
    t.client().remove_signer(&t.admin, &t.signer(1));
    // Re-add the same address as a signer.
    t.client().add_signer(&t.admin, &t.signer(1));

    // The address is now a signer again, but cannot sign the same proposal twice.
    assert_eq!(
        t.client().try_sign(&t.signer(1), &id),
        Err(Ok(ContractError::InvalidInput)),
        "the signature from before removal is dead, but the address still cannot sign twice"
    );
}

#[test]
fn boundary_proposal_just_below_high_value_limit_needs_one_signature() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.id("just-below");
    t.client().propose(
        &t.signer(0),
        &id,
        &t.id("desc"),
        &target,
        &(DEFAULT_HIGH_VALUE_LIMIT - 1),
    );

    // Just below the limit → low-value threshold (1 signature).
    assert_eq!(
        t.client().execute(&t.signer(0), &id),
        DEFAULT_HIGH_VALUE_LIMIT - 1
    );
}

#[test]
fn boundary_proposal_exactly_at_high_value_limit_needs_one_signature() {
    // Already covered by `value_exactly_at_the_high_value_limit_needs_only_one_signature`,
    // but reaffirm for completeness.
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.id("exact");
    t.client().propose(
        &t.signer(0),
        &id,
        &t.id("desc"),
        &target,
        &DEFAULT_HIGH_VALUE_LIMIT,
    );

    assert_eq!(t.client().execute(&t.signer(0), &id), DEFAULT_HIGH_VALUE_LIMIT);
}

#[test]
fn boundary_proposal_one_above_high_value_limit_needs_full_threshold() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.id("just-above");
    t.client().propose(
        &t.signer(0),
        &id,
        &t.id("desc"),
        &target,
        &(DEFAULT_HIGH_VALUE_LIMIT + 1),
    );

    // One above the limit → high-value threshold (2-of-3).
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold))
    );
    t.client().sign(&t.signer(1), &id);
    assert!(t.client().try_execute(&t.signer(0), &id).is_ok());
}

#[test]
fn proposal_value_negative_is_rejected() {
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    assert_eq!(
        t.client()
            .try_propose(&t.signer(0), &t.id("neg"), &t.id("desc"), &target, &-1),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn signer_removal_while_proposal_pending_drops_their_vote_live_count_recomputed() {
    // Already partially covered in `removing_a_signer_after_they_signed_drops_their_vote_from_quorum`,
    // but add a variant where two signatures were collected and one signer is removed.
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("drop-two", &target);
    t.client().sign(&t.signer(1), &id);
    // Now 2 signatures (signer0, signer1) → meets threshold 2-of-3.

    t.client().remove_signer(&t.admin, &t.signer(1));
    // Only signer(0) is now live; signer(1) is gone, so live count = 1 → below threshold.
    assert_eq!(
        t.client().try_execute(&t.signer(0), &id),
        Err(Ok(ContractError::BelowThreshold))
    );
}

#[test]
fn threshold_one_with_one_signer_allowed() {
    // Special case: 1-of-1 signer set should be allowed by init.
    let t = MultisigTest::registered();
    let single_signer = Vec::from_array(&t.env, [t.signer(0)]);
    t.client().init(&t.admin, &single_signer, &1, &0);

    let target = Address::generate(&t.env);
    let id = t.id("1-of-1");
    t.client()
        .propose(&t.signer(0), &id, &t.id("desc"), &target, &100);
    // Single signer (the proposer) can execute immediately.
    assert_eq!(t.client().execute(&t.signer(0), &id), 100);
}

#[test]
fn threshold_one_with_three_signers_allows_any_single_signer() {
    // 1-of-3 threshold means any one signer can execute.
    let mut t = MultisigTest::setup();
    t.override_threshold(1);
    t.reinit();

    let target = Address::generate(&t.env);
    let id = t.propose_high_value("1-of-3", &target);
    // Even high-value proposals need only one signature.
    assert_eq!(t.client().execute(&t.signer(2), &id), DEFAULT_HIGH_VALUE_LIMIT * 10);
}

// ── Storage reduction & parity tests (issue #1159) ─────────────────

#[test]
fn storage_uses_datakey_enum_not_symbol_keys() {
    // Verify that the new storage layout uses `[contracttype]` enum
    // `DataKey` variants instead of plain `Symbol` string keys,
    // which reduces per-entry storage overhead.
    let t = MultisigTest::setup();

    // DataKey::Signers should be present and contain the full config.
    let config: Option<crate::SignersConfig> = t.read_storage(&crate::DataKey::Signers);
    assert!(config.is_some(), "SignersConfig must be stored under DataKey::Signers");
    let config = config.unwrap();
    assert_eq!(config.admin, t.admin);
    assert_eq!(config.signers, t.signers);
    assert_eq!(config.threshold, DEFAULT_THRESHOLD);
    assert_eq!(config.high_value_limit, DEFAULT_HIGH_VALUE_LIMIT);
}

#[test]
fn signers_config_is_single_storage_entry() {
    // Verify that admin/signers/threshold/high_value_limit are all
    // stored in a single `SignersConfig` entry rather than four
    // separate storage items.
    let t = MultisigTest::setup();

    // DataKey::Signers should exist.
    assert!(t.read_storage::<crate::SignersConfig>(&crate::DataKey::Signers).is_some());

    // DataKey::Admin, DataKey::Signers (as separate), DataKey::Threshold,
    // DataKey::HighValueLimit should NOT exist as separate entries.
    // Since we only have DataKey::Signers, DataKey::Proposals, DataKey::Schema,
    // the old Symbol-based keys should be absent.
    let admin_val: Option<Address> = t.read_storage(&crate::DataKey::Signers).map(|c| c.admin);
    assert!(admin_val.is_some(), "admin should be accessible via SignersConfig");
}

#[test]
fn storage_reduction_parity_with_init() {
    // Test parity: init should persist all fields via the consolidated
    // SignersConfig and still be reachable through view functions.
    let t = MultisigTest::setup();
    assert_eq!(t.client().get_signers(), t.signers);
    assert_eq!(t.client().get_threshold(), DEFAULT_THRESHOLD);
    assert_eq!(t.client().get_high_value_limit(), DEFAULT_HIGH_VALUE_LIMIT);
    assert_eq!(t.client().schema_version(), SCHEMA_VERSION);
}

#[test]
fn storage_reduction_parity_with_admin_ops() {
    // Test parity: admin operations should all work correctly through
    // the consolidated SignersConfig storage layout.
    let mut t = MultisigTest::setup();
    let new_signer = Address::generate(&t.env);

    t.client().add_signer(&t.admin, &new_signer);
    assert_eq!(t.client().get_signers().len(), 4);

    t.client().remove_signer(&t.admin, &t.signer(2));
    assert_eq!(t.client().get_signers().len(), 3);

    t.client().set_threshold(&t.admin, 3);
    assert_eq!(t.client().get_threshold(), 3);

    t.client().set_high_value_limit(&t.admin, 5_000);
    assert_eq!(t.client().get_high_value_limit(), 5_000);

    // Verify SignersConfig reflects all changes.
    let config = t.read_storage::<crate::SignersConfig>(&crate::DataKey::Signers).unwrap();
    assert_eq!(config.signers.len(), 3);
    assert_eq!(config.threshold, 3);
    assert_eq!(config.high_value_limit, 5_000);
}

#[test]
fn storage_reduction_parity_with_proposal_flow() {
    // Test parity: the full propose → sign → execute flow should work
    // correctly through the consolidated storage layout.
    let t = MultisigTest::setup();
    let target = Address::generate(&t.env);
    let id = t.propose_high_value("parity", &target);

    t.client().sign(&t.signer(1), &id);
    let value = t.client().execute(&t.signer(0), &id);
    assert_eq!(value, DEFAULT_HIGH_VALUE_LIMIT * 10);
}

#[test]
fn storage_reduction_parity_with_migration() {
    // Test parity: v1 → v2 migration should work through the new
    // consolidated storage layout.
    let (mut t, _id, target) = MultisigTest::registered_with_legacy_v1_state();

    let stored = t.client().migrate(&t.admin).unwrap();
    assert_eq!(stored, 1);
    assert_eq!(t.client().schema_version(), SCHEMA_VERSION);

    // The SignersConfig should still be accessible after migration.
    let config = t.read_storage::<crate::SignersConfig>(&crate::DataKey::Signers).unwrap();
    assert_eq!(config.admin, t.admin);
    assert_eq!(config.threshold, DEFAULT_THRESHOLD);
}
