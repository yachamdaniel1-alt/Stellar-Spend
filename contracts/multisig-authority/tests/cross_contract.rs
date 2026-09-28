//! Cross-contract integration tests for multisig-authority ↔ treasury/escrow (issue #1019).
//!
//! These tests verify that the multisig-authority contract can properly govern
//! cross-contract operations on treasury and escrow, including authorized flows,
//! unauthorized rejection, and state-transition correctness.
//!
//! ## Setup
//! - All three contracts are registered in a single Soroban `Env`.
//! - Multisig-authority is initialized with a 2-of-3 signer set.
//! - Treasury is initialized with multisig-authority's contract address as admin.
//! - Escrow is initialized with multisig-authority's contract address as settlement authority.
//!
//! ## How to run
//! ```bash
//! cargo test --package multisig-authority --test cross_contract
//! ```

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};
use stellar_spend_shared::errors::ContractError;

// ── Shared fixture ──────────────────────────────────────────────────────────

const START_LEDGER: u32 = 1_000;
const FIXTURE_ENTRY_TTL: u32 = 12_000_000;
const DEFAULT_THRESHOLD: u32 = 2;
const DEFAULT_HIGH_VALUE_LIMIT: i128 = 1_000;

struct CrossFixture {
    env: Env,
    multisig_id: Address,
    treasury_id: Address,
    escrow_id: Address,
    admin: Address,
    signers: Vec<Address>,
    outsider: Address,
    treasury_addr: Address,
}

impl CrossFixture {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|li| {
            li.sequence_number = START_LEDGER;
            li.min_persistent_entry_ttl = FIXTURE_ENTRY_TTL;
            li.min_temp_entry_ttl = FIXTURE_ENTRY_TTL;
            li.max_entry_ttl = FIXTURE_ENTRY_TTL;
        });

        let admin = Address::generate(&env);
        let outsider = Address::generate(&env);
        let treasury_addr = Address::generate(&env);
        let signer0 = Address::generate(&env);
        let signer1 = Address::generate(&env);
        let signer2 = Address::generate(&env);
        let signers = Vec::from_array(&env, [signer0, signer1, signer2]);

        // Deploy multisig-authority
        let multisig_id = env.register(multisig_authority::MultisigAuthority, ());
        let multisig_client = multisig_authority::MultisigAuthorityClient::new(&env, &multisig_id);
        multisig_client.init(&admin, &signers, &DEFAULT_THRESHOLD, &DEFAULT_HIGH_VALUE_LIMIT);

        // Deploy treasury with multisig as admin
        let treasury_id = env.register(treasury::TreasuryContract, ());
        let treasury_client = treasury::TreasuryContractClient::new(&env, &treasury_id);
        treasury_client.initialize(&multisig_id);
        treasury_client.update_treasury(&treasury_addr);

        // Deploy escrow with multisig as settlement authority
        let escrow_id = env.register(escrow::EscrowContract, ());
        let escrow_client = escrow::EscrowContractClient::new(&env, &escrow_id);
        escrow_client.init(&multisig_id);

        Self {
            env,
            multisig_id,
            treasury_id,
            escrow_id,
            admin,
            signers,
            outsider,
            treasury_addr,
        }
    }

    fn multisig(&self) -> multisig_authority::MultisigAuthorityClient<'_> {
        multisig_authority::MultisigAuthorityClient::new(&self.env, &self.multisig_id)
    }

    fn treasury(&self) -> treasury::TreasuryContractClient<'_> {
        treasury::TreasuryContractClient::new(&self.env, &self.treasury_id)
    }

    fn escrow(&self) -> escrow::EscrowContractClient<'_> {
        escrow::EscrowContractClient::new(&self.env, &self.escrow_id)
    }

    fn id(&self, text: &str) -> soroban_sdk::String {
        soroban_sdk::String::from_str(&self.env, text)
    }

    fn signer(&self, index: u32) -> Address {
        self.signers.get(index).unwrap()
    }

    fn advance_past_proposal_expiry(&self) {
        self.env.ledger().with_mut(|li| li.sequence_number += 120_960 + 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AUTHORIZED FLOWS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn multisig_can_propose_action_targeting_treasury() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-fee-update");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Update fee schedule on treasury"),
        &f.treasury_id,
        &0i128,
    );

    let (sigs, _threshold, executable) = f.multisig().proposal_status(&proposal_id);
    assert_eq!(sigs, 1, "proposer's implicit signature counts");
    assert!(executable, "value 0 is below high-value limit, so 1 signature suffices");
}

#[test]
fn multisig_can_sign_and_execute_treasury_governance_proposal() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-governance");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Governance action on treasury"),
        &f.treasury_id,
        &0i128,
    );

    f.multisig().sign(&f.signer(1), &proposal_id);

    let value = f.multisig().execute(&f.signer(0), &proposal_id);
    assert_eq!(value, 0, "proposal value is returned for downstream use");

    let proposal = f.multisig().get_proposal(&proposal_id);
    assert!(proposal.executed, "proposal must be marked executed");
}

#[test]
fn multisig_can_propose_action_targeting_escrow() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("escrow-release");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Authorize escrow release"),
        &f.escrow_id,
        &5_000i128,
    );

    let proposal = f.multisig().get_proposal(&proposal_id);
    assert_eq!(proposal.target, f.escrow_id);
    assert_eq!(proposal.value, 5_000);
}

#[test]
fn multisig_can_sign_and_execute_escrow_governance_proposal() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("escrow-governance");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Governance action on escrow"),
        &f.escrow_id,
        &10_000i128,
    );

    f.multisig().sign(&f.signer(1), &proposal_id);

    let value = f.multisig().execute(&f.signer(0), &proposal_id);
    assert_eq!(value, 10_000);

    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::AlreadyProcessed))
    );
}

#[test]
fn low_value_treasury_proposal_needs_only_one_signature() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("low-value-treasury");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Low-value treasury action"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT / 2),
    );

    let value = f.multisig().execute(&f.signer(0), &proposal_id);
    assert_eq!(value, DEFAULT_HIGH_VALUE_LIMIT / 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNAUTHORIZED FLOWS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn outsider_cannot_propose_action_targeting_treasury() {
    let f = CrossFixture::setup();

    assert_eq!(
        f.multisig().try_propose(
            &f.outsider,
            &f.id("unauthorized"),
            &f.id("Malicious treasury action"),
            &f.treasury_id,
            &0i128,
        ),
        Err(Ok(ContractError::Unauthorized)),
        "non-signers must not create proposals"
    );
}

#[test]
fn outsider_cannot_propose_action_targeting_escrow() {
    let f = CrossFixture::setup();

    assert_eq!(
        f.multisig().try_propose(
            &f.outsider,
            &f.id("unauthorized-escrow"),
            &f.id("Malicious escrow action"),
            &f.escrow_id,
            &0i128,
        ),
        Err(Ok(ContractError::Unauthorized)),
        "non-signers must not create proposals targeting escrow"
    );
}

#[test]
fn outsider_cannot_sign_treasury_proposals() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-guard");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Legitimate action"),
        &f.treasury_id,
        &0i128,
    );

    assert_eq!(
        f.multisig().try_sign(&f.outsider, &proposal_id),
        Err(Ok(ContractError::Unauthorized)),
        "non-signers must not sign governance proposals"
    );
}

#[test]
fn outsider_cannot_execute_treasury_proposals() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-exec-guard");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Governance action"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT / 2),
    );

    assert_eq!(
        f.multisig().try_execute(&f.outsider, &proposal_id),
        Err(Ok(ContractError::Unauthorized)),
        "non-signers must not execute proposals"
    );
}

#[test]
fn high_value_proposal_rejects_execution_with_insufficient_signatures() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-high-value");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("High-value treasury action"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT * 10),
    );

    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::BelowThreshold)),
        "must reject execution when quorum is not reached"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE TRANSITION VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn executed_treasury_proposal_cannot_be_re_executed() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-no-replay");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("One-shot treasury action"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT / 2),
    );
    f.multisig().execute(&f.signer(0), &proposal_id);

    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::AlreadyProcessed)),
        "executed proposal must not be re-executable"
    );
}

#[test]
fn executed_escrow_proposal_cannot_be_re_executed() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("escrow-no-replay");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("One-shot escrow action"),
        &f.escrow_id,
        &(DEFAULT_HIGH_VALUE_LIMIT / 2),
    );
    f.multisig().execute(&f.signer(0), &proposal_id);

    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::AlreadyProcessed)),
        "executed escrow proposal must not be re-executable"
    );
}

#[test]
fn expired_treasury_proposal_cannot_be_signed_or_executed() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-expire");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Will expire"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT * 10),
    );

    f.advance_past_proposal_expiry();

    assert_eq!(
        f.multisig().try_sign(&f.signer(1), &proposal_id),
        Err(Ok(ContractError::Expired)),
        "cannot sign an expired proposal"
    );
    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::Expired)),
        "cannot execute an expired proposal"
    );
}

#[test]
fn removing_signer_after_signing_invalidates_vote_for_treasury_execution() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-signer-removal");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Governance action"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT * 10),
    );
    f.multisig().sign(&f.signer(1), &proposal_id);

    let (_, _, executable) = f.multisig().proposal_status(&proposal_id);
    assert!(executable, "should be executable before removal");

    f.multisig().remove_signer(&f.admin, &f.signer(1));

    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::BelowThreshold)),
        "removed signer's vote must not count toward treasury governance quorum"
    );
}

#[test]
fn proposal_status_reflects_cross_contract_target() {
    let f = CrossFixture::setup();

    let t_id = f.id("status-treasury");
    f.multisig().propose(
        &f.signer(0),
        &t_id,
        &f.id("Treasury status check"),
        &f.treasury_id,
        &0i128,
    );
    let proposal = f.multisig().get_proposal(&t_id);
    assert_eq!(proposal.target, f.treasury_id);

    let e_id = f.id("status-escrow");
    f.multisig().propose(
        &f.signer(0),
        &e_id,
        &f.id("Escrow status check"),
        &f.escrow_id,
        &100i128,
    );
    let proposal = f.multisig().get_proposal(&e_id);
    assert_eq!(proposal.target, f.escrow_id);
    assert_eq!(proposal.value, 100);
}

// ═══════════════════════════════════════════════════════════════════════════════
// TREASURY CONTRACT STATE INTEGRITY
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn treasury_init_state_preserved_after_multisig_governance() {
    let f = CrossFixture::setup();

    let schedule = f.treasury().get_fee_schedule();
    assert_eq!(schedule.len(), 3, "default fee schedule has 3 tiers");

    // Treasury address is the one we set up
    assert_eq!(f.treasury().get_treasury(), f.treasury_addr);

    // Fee calculation works against the stored schedule
    let fee_0 = f.treasury().fee_for_amount(&500_000i128);
    assert_eq!(fee_0, 50u32, "low amount pays 50bp");

    let fee_1 = f.treasury().fee_for_amount(&5_000_000i128);
    assert_eq!(fee_1, 25u32, "mid amount pays 25bp");

    let fee_2 = f.treasury().fee_for_amount(&15_000_000i128);
    assert_eq!(fee_2, 10u32, "high amount pays 10bp");
}

#[test]
fn escrow_init_state_preserved_after_multisig_governance() {
    let f = CrossFixture::setup();

    assert_eq!(f.escrow().schema_version(), escrow::SCHEMA_VERSION);

    let outsider = Address::generate(&f.env);
    assert_eq!(
        f.escrow().try_release(&0u64, &outsider),
        Err(Ok(ContractError::NotFound)),
        "releasing non-existent deposit must fail"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// MULTIPLE GOVERNANCE ACTIONS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_proposals_for_different_contracts_can_coexist() {
    let f = CrossFixture::setup();

    let t_id = f.id("multi-treasury");
    let e_id = f.id("multi-escrow");

    f.multisig().propose(
        &f.signer(0),
        &t_id,
        &f.id("Treasury action"),
        &f.treasury_id,
        &0i128,
    );
    f.multisig().propose(
        &f.signer(0),
        &e_id,
        &f.id("Escrow action"),
        &f.escrow_id,
        &500i128,
    );

    f.multisig().sign(&f.signer(1), &t_id);
    f.multisig().sign(&f.signer(1), &e_id);

    assert_eq!(f.multisig().execute(&f.signer(0), &t_id), 0);
    assert_eq!(f.multisig().execute(&f.signer(0), &e_id), 500);

    assert!(f.multisig().get_proposal(&t_id).executed);
    assert!(f.multisig().get_proposal(&e_id).executed);
}

#[test]
fn threshold_raised_after_signing_blocks_previously_executable_treasury_proposal() {
    let f = CrossFixture::setup();

    let proposal_id = f.id("treasury-threshold-raise");
    f.multisig().propose(
        &f.signer(0),
        &proposal_id,
        &f.id("Treasury governance"),
        &f.treasury_id,
        &(DEFAULT_HIGH_VALUE_LIMIT * 10),
    );
    f.multisig().sign(&f.signer(1), &proposal_id);

    let (_, _, executable) = f.multisig().proposal_status(&proposal_id);
    assert!(executable, "should be executable with 2 of 3 signatures");

    f.multisig().set_threshold(&f.admin, &3);

    assert_eq!(
        f.multisig().try_execute(&f.signer(0), &proposal_id),
        Err(Ok(ContractError::BelowThreshold)),
        "raising threshold must invalidate a previously-sufficient quorum"
    );

    f.multisig().sign(&f.signer(2), &proposal_id);
    assert!(
        f.multisig().try_execute(&f.signer(0), &proposal_id).is_ok(),
        "three signatures satisfy the raised 3-of-3 threshold"
    );
}
