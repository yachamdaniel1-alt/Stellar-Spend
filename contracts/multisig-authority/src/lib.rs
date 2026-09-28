//! Multi-signature settlement authority for Stellar-Spend.
//!
//! Implements M-of-N threshold signing for high-value release/upgrade actions.
//! Every collected signature is emitted as an event for off-chain audit logging.
//!
//! Values at or below `high_value_limit` need a single signature; anything above it
//! needs the full threshold. Setting the limit to `0` requires the full threshold for
//! every proposal.
//!
//! Signer/admin/threshold state is consolidated into a single [`SignersConfig`]
//! storage entry keyed by the [`DataKey`] enum, reducing redundant storage
//! writes compared to the previous four separate plain-string-key entries.
//!
//! # Dead code removed (issue #815)
//!
//! * `Proposal::created_at` was documented "for expiry checks" but nothing ever read
//!   it — proposals lived forever, so a signature collected months earlier still
//!   counted toward quorum. Expiry is now enforced against
//!   [`DEFAULT_PROPOSAL_TTL_LEDGERS`] and the field is live.
//! * `required_threshold` was declared `pub fn` inside `#[contractimpl]` while taking
//!   `&Env`, which cannot be exported as an entrypoint. It now takes `Env` and is a
//!   real read-only entrypoint.

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, symbol_short, vec, Address, BytesN, Env,
    Map, String, Vec,
};
use stellar_spend_shared::{
    auth::verify_threshold,
    errors::ContractError,
    validation::{
        check_schema_version, require_non_negative_amount, require_string_len,
        require_unique_addresses, MAX_SIGNERS,
    },
};

contractmeta!(key = "version", val = "1.0.0");
contractmeta!(key = "contract", val = "stellar-spend-multisig-authority");

// ── Storage keys ──────────────────────────────────────────────────────
//
// A `#[contracttype]` enum replaces the previous plain `Symbol::new(&env, "...")`
// keys. Enum variants are stored as compact integer indices in the ledger,
// reducing per-entry storage overhead compared to string symbols.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Signers,
    Proposals,
    Schema,
}

/// Consolidated signer configuration stored under a single [`DataKey::Signers`]
/// entry. Replaces the previous four separate storage entries (admin, signers,
/// threshold, hv_limit), cutting storage writes on init and every admin
/// operation that touches signer state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignersConfig {
    pub admin: Address,
    pub signers: Vec<Address>,
    pub threshold: u32,
    pub high_value_limit: i128,
}

/// Current storage layout version.
pub const SCHEMA_VERSION: u32 = 2;

/// How long a proposal stays signable, in ledgers (~7 days at 5s close time).
///
/// Without an expiry, a signature gathered under one signer set stays valid after
/// that set has changed, which is how stale quorums get replayed.
pub const DEFAULT_PROPOSAL_TTL_LEDGERS: u32 = 120_960;

/// Maximum length of a proposal id, in bytes.
pub const MAX_PROPOSAL_ID_LEN: u32 = 64;
/// Maximum length of a proposal description, in bytes.
pub const MAX_DESCRIPTION_LEN: u32 = 256;

/// Instance TTL extension (~30 days) applied on state-changing calls.
pub const INSTANCE_TTL_EXTEND_TO: u32 = 518_400;
/// Only pay to extend when remaining TTL drops below ~6 days.
pub const INSTANCE_TTL_THRESHOLD: u32 = 103_680;

/// Schema v1 proposal record.
///
/// Retained so [`MultisigAuthority::migrate`] can decode entries written by a v1
/// build. Current code never writes this shape.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalV1 {
    pub id: String,
    pub description: String,
    pub target: Address,
    pub value: i128,
    pub signatures: Vec<Address>,
    pub executed: bool,
    pub created_at: u32,
}

/// Schema v2 proposal record: v1 plus an explicit expiry ledger.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    /// Unique proposal ID (caller-supplied).
    pub id: String,
    /// Human-readable description of the action.
    pub description: String,
    /// Target contract or address the action applies to.
    pub target: Address,
    /// Value involved (in stroops / token base units).
    pub value: i128,
    /// Addresses that have already signed.
    pub signatures: Vec<Address>,
    /// Whether the proposal has been executed.
    pub executed: bool,
    /// Ledger sequence at proposal creation.
    pub created_at: u32,
    /// Ledger after which the proposal can no longer be signed or executed.
    /// Added in schema v2; migrated records get `created_at + DEFAULT_PROPOSAL_TTL_LEDGERS`.
    pub expires_at: u32,
}

#[contract]
pub struct MultisigAuthority;

#[contractimpl]
impl MultisigAuthority {
    /// Initialise the authority with M-of-N signers and a high-value threshold.
    ///
    /// * `admin`            – address allowed to add/remove signers
    /// * `signers`          – initial signer list (must be non-empty, unique)
    /// * `threshold`        – minimum signatures required (1 ≤ threshold ≤ signers.len())
    /// * `high_value_limit` – releases above this amount require the full threshold;
    ///                        set to `0` to always require the full threshold.
    pub fn init(
        env: Env,
        admin: Address,
        signers: Vec<Address>,
        threshold: u32,
        high_value_limit: i128,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Schema) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();

        if signers.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        if signers.len() > MAX_SIGNERS {
            return Err(ContractError::InvalidInput);
        }
        // A duplicated signer would otherwise count twice toward quorum, letting one
        // key satisfy a 2-of-N threshold on its own.
        require_unique_addresses(&signers)?;
        if threshold == 0 || threshold > signers.len() {
            return Err(ContractError::InvalidInput);
        }
        require_non_negative_amount(high_value_limit)?;

        let config = SignersConfig {
            admin,
            signers,
            threshold,
            high_value_limit,
        };
        env.storage().instance().set(&DataKey::Signers, &config);
        env.storage().instance().set(
            &DataKey::Proposals,
            &Map::<String, Proposal>::new(&env),
        );
        env.storage().instance().set(&DataKey::Schema, &SCHEMA_VERSION);
        Self::bump_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("init"),),
            (config.admin, config.threshold, config.high_value_limit),
        );
        Ok(())
    }

    /// Create a new proposal. The proposer must be a registered signer and their
    /// signature counts as the first approval.
    pub fn propose(
        env: Env,
        proposer: Address,
        id: String,
        description: String,
        target: Address,
        value: i128,
    ) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        require_string_len(&id, MAX_PROPOSAL_ID_LEN)?;
        require_string_len(&description, MAX_DESCRIPTION_LEN)?;
        require_non_negative_amount(value)?;

        assert_is_signer(&env, &proposer)?;

        let mut proposals = Self::load_proposals(&env);
        if proposals.contains_key(id.clone()) {
            return Err(ContractError::InvalidInput);
        }

        let created_at = env.ledger().sequence();
        let expires_at = created_at
            .checked_add(DEFAULT_PROPOSAL_TTL_LEDGERS)
            .ok_or(ContractError::Overflow)?;

        proposals.set(
            id.clone(),
            Proposal {
                id: id.clone(),
                description,
                target: target.clone(),
                value,
                signatures: vec![&env, proposer.clone()],
                executed: false,
                created_at,
                expires_at,
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &proposals);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("proposed"),), (id, proposer, target, value));
        Ok(())
    }

    /// Add a signer's approval to an existing proposal.
    ///
    /// Emits a `signed` event for every signature collected (audit trail).
    pub fn sign(env: Env, signer: Address, proposal_id: String) -> Result<u32, ContractError> {
        Self::require_current_schema(&env)?;
        assert_is_signer(&env, &signer)?;

        let mut proposals = Self::load_proposals(&env);
        let mut proposal = proposals
            .get(proposal_id.clone())
            .ok_or(ContractError::NotFound)?;

        if proposal.executed {
            return Err(ContractError::AlreadyProcessed);
        }
        if env.ledger().sequence() > proposal.expires_at {
            return Err(ContractError::Expired);
        }
        if proposal.signatures.contains(signer.clone()) {
            return Err(ContractError::InvalidInput);
        }

        proposal.signatures.push_back(signer.clone());
        let sig_count = proposal.signatures.len();

        proposals.set(proposal_id.clone(), proposal);
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &proposals);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("signed"),), (proposal_id, signer, sig_count));
        Ok(sig_count)
    }

    /// Execute a proposal once the required threshold is met.
    ///
    /// Returns the approved value so the calling contract can act on it.
    pub fn execute(
        env: Env,
        executor: Address,
        proposal_id: String,
    ) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        assert_is_signer(&env, &executor)?;

        let mut proposals = Self::load_proposals(&env);
        let mut proposal = proposals
            .get(proposal_id.clone())
            .ok_or(ContractError::NotFound)?;

        if proposal.executed {
            return Err(ContractError::AlreadyProcessed);
        }
        if env.ledger().sequence() > proposal.expires_at {
            return Err(ContractError::Expired);
        }

        let (full_threshold, high_value_limit) = Self::stored_threshold_and_limit(&env)?;
        // Re-derive the threshold at execution time rather than trusting one from
        // proposal time: the signer set may have shrunk since.
        let live = Self::live_signature_count(&env, &proposal.signatures)?;
        verify_threshold(live, full_threshold, high_value_limit, proposal.value)?;

        let value = proposal.value;
        proposal.executed = true;
        let sig_count = proposal.signatures.len();
        proposals.set(proposal_id.clone(), proposal);
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &proposals);
        Self::bump_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("executed"),),
            (proposal_id, executor, value, sig_count),
        );
        Ok(value)
    }

    // ── Admin operations ─────────────────────────────────────────────────────

    /// Add a new signer. Admin only.
    pub fn add_signer(env: Env, admin: Address, new_signer: Address) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        assert_is_admin(&env, &admin)?;

        let mut config = Self::load_signers_config(&env)?;
        if config.signers.contains(new_signer.clone()) {
            return Err(ContractError::InvalidInput);
        }
        if config.signers.len() >= MAX_SIGNERS {
            return Err(ContractError::InvalidInput);
        }

        config.signers.push_back(new_signer.clone());
        env.storage().instance().set(&DataKey::Signers, &config);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("add_sgn"),), new_signer);
        Ok(())
    }

    /// Remove a signer. Admin only. Fails if removal would make quorum unreachable.
    pub fn remove_signer(env: Env, admin: Address, signer: Address) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        assert_is_admin(&env, &admin)?;

        let mut config = Self::load_signers_config(&env)?;
        let threshold = config.threshold;

        let index = config
            .signers
            .first_index_of(signer.clone())
            .ok_or(ContractError::NotFound)?;

        // Checked subtraction: the original `(signers.len() - 1) < threshold`
        // underflowed to u32::MAX on an empty set and let the check pass.
        let remaining = config
            .signers
            .len()
            .checked_sub(1)
            .ok_or(ContractError::InvalidInput)?;
        if remaining < threshold {
            return Err(ContractError::InvalidInput);
        }

        config.signers.remove(index);
        env.storage().instance().set(&DataKey::Signers, &config);
        Self::bump_instance_ttl(&env);

        env.events().publish((symbol_short!("rm_sgn"),), signer);
        Ok(())
    }

    /// Update the threshold. Admin only.
    pub fn set_threshold(
        env: Env,
        admin: Address,
        new_threshold: u32,
    ) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        assert_is_admin(&env, &admin)?;

        let mut config = Self::load_signers_config(&env)?;
        if new_threshold == 0 || new_threshold > config.signers.len() {
            return Err(ContractError::InvalidInput);
        }

        config.threshold = new_threshold;
        env.storage().instance().set(&DataKey::Signers, &config);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("set_thr"),), new_threshold);
        Ok(())
    }

    /// Update the high-value limit. Admin only.
    pub fn set_high_value_limit(
        env: Env,
        admin: Address,
        limit: i128,
    ) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        assert_is_admin(&env, &admin)?;
        require_non_negative_amount(limit)?;

        let mut config = Self::load_signers_config(&env)?;
        config.high_value_limit = limit;
        env.storage().instance().set(&DataKey::Signers, &config);
        Self::bump_instance_ttl(&env);

        env.events().publish((symbol_short!("set_hvl"),), limit);
        Ok(())
    }

    // ── View functions ────────────────────────────────────────────────────

    /// Signatures required for a proposal of the given value.
    pub fn required_threshold(env: Env, value: i128) -> Result<u32, ContractError> {
        let (full_threshold, high_value_limit) = Self::stored_threshold_and_limit(&env)?;
        Ok(stellar_spend_shared::auth::required_threshold(
            full_threshold,
            high_value_limit,
            value,
        ))
    }

    /// Returns `(signature_count, threshold_required, is_executable)`.
    pub fn proposal_status(
        env: Env,
        proposal_id: String,
    ) -> Result<(u32, u32, bool), ContractError> {
        Self::require_current_schema(&env)?;
        let proposal = Self::load_proposals(&env)
            .get(proposal_id)
            .ok_or(ContractError::NotFound)?;

        let (full_threshold, high_value_limit) = Self::stored_threshold_and_limit(&env)?;
        let threshold = stellar_spend_shared::auth::required_threshold(
            full_threshold,
            high_value_limit,
            proposal.value,
        );
        let live = Self::live_signature_count(&env, &proposal.signatures)?;
        let expired = env.ledger().sequence() > proposal.expires_at;

        Ok((
            proposal.signatures.len(),
            threshold,
            live >= threshold && !proposal.executed && !expired,
        ))
    }

    pub fn get_proposal(env: Env, proposal_id: String) -> Result<Proposal, ContractError> {
        Self::require_current_schema(&env)?;
        Self::load_proposals(&env)
            .get(proposal_id)
            .ok_or(ContractError::NotFound)
    }

    pub fn get_signers(env: Env) -> Result<Vec<Address>, ContractError> {
        Self::require_current_schema(&env)?;
        let config = Self::load_signers_config(&env)?;
        Ok(config.signers)
    }

    pub fn get_threshold(env: Env) -> Result<u32, ContractError> {
        Self::require_current_schema(&env)?;
        let config = Self::load_signers_config(&env)?;
        Ok(config.threshold)
    }

    pub fn get_high_value_limit(env: Env) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        let config = Self::load_signers_config(&env)?;
        Ok(config.high_value_limit)
    }

    // ── Upgrade surface (issue #817) ──────────────────────────────────

    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Schema)
            .ok_or(ContractError::NotInitialized)
    }

    /// Replace the contract WASM. Admin only. Run `migrate` immediately after.
    ///
    /// Note this is an admin-key action, not a threshold-gated one; gating upgrades
    /// behind the multisig itself is tracked separately.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        assert_is_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.events().publish((symbol_short!("upgrade"),), ());
        Ok(())
    }

    /// Convert persisted state to [`SCHEMA_VERSION`]. Returns the version migrated from.
    pub fn migrate(env: Env, admin: Address) -> Result<u32, ContractError> {
        assert_is_admin(&env, &admin)?;

        let stored: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Schema)
            .ok_or(ContractError::NotInitialized)?;

        if stored == SCHEMA_VERSION {
            return Err(ContractError::SchemaAlreadyCurrent);
        }
        if stored > SCHEMA_VERSION {
            return Err(ContractError::SchemaVersionUnsupported);
        }

        if stored == 1 {
            // v1 -> v2: derive `expires_at` from the `created_at` the old build was
            // already recording, so in-flight proposals keep a meaningful deadline
            // instead of all expiring (or never expiring) at once.
            let old: Map<String, ProposalV1> = env
                .storage()
                .instance()
                .get(&DataKey::Proposals)
                .unwrap_or_else(|| Map::new(&env));

            let mut migrated: Map<String, Proposal> = Map::new(&env);
            for (key, v1) in old.iter() {
                let expires_at = v1.created_at.saturating_add(DEFAULT_PROPOSAL_TTL_LEDGERS);
                migrated.set(
                    key,
                    Proposal {
                        id: v1.id,
                        description: v1.description,
                        target: v1.target,
                        value: v1.value,
                        signatures: v1.signatures,
                        executed: v1.executed,
                        created_at: v1.created_at,
                        expires_at,
                    },
                );
            }
            env.storage()
                .instance()
                .set(&DataKey::Proposals, &migrated);
        }

        env.storage()
            .instance()
            .set(&DataKey::Schema, &SCHEMA_VERSION);
        Self::bump_instance_ttl(&env);
        env.events()
            .publish((symbol_short!("migrate"),), (stored, SCHEMA_VERSION));
        Ok(stored)
    }

    // ── Internal helpers ──────────────────────────────────────────────

    fn load_proposals(env: &Env) -> Map<String, Proposal> {
        env.storage()
            .instance()
            .get(&DataKey::Proposals)
            .unwrap_or_else(|| Map::new(env))
    }

    fn load_signers_config(env: &Env) -> Result<SignersConfig, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Signers)
            .ok_or(ContractError::NotInitialized)
    }

    fn stored_threshold_and_limit(env: &Env) -> Result<(u32, i128), ContractError> {
        let config = Self::load_signers_config(env)?;
        Ok((config.threshold, config.high_value_limit))
    }

    /// How many of `signatures` belong to addresses that are still registered signers.
    fn live_signature_count(env: &Env, signatures: &Vec<Address>) -> Result<u32, ContractError> {
        let config = Self::load_signers_config(env)?;
        let mut live = 0u32;
        for candidate in signatures.iter() {
            if config.signers.contains(candidate) {
                live += 1;
            }
        }
        Ok(live)
    }

    fn require_current_schema(env: &Env) -> Result<(), ContractError> {
        check_schema_version(
            env.storage().instance().get(&DataKey::Schema),
            SCHEMA_VERSION,
        )
    }

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }
}

// ── Local admin/signer auth helpers ────────────────────────────────────
//
// These replace `stellar_spend_shared::auth::assert_is_admin` / `assert_is_signer`
// which used plain `Symbol::new(env, key)` lookups. With the consolidated
// [`SignersConfig`] stored under [`DataKey::Signers`], we read the entire
// config in one storage operation instead of two separate lookups.

fn assert_is_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
    let config: SignersConfig = env
        .storage()
        .instance()
        .get(&DataKey::Signers)
        .ok_or(ContractError::NotFound)?;
    if admin != &config.admin {
        return Err(ContractError::Unauthorized);
    }
    admin.require_auth();
    Ok(())
}

fn assert_is_signer(env: &Env, signer: &Address) -> Result<(), ContractError> {
    let config: SignersConfig = env
        .storage()
        .instance()
        .get(&DataKey::Signers)
        .ok_or(ContractError::NotFound)?;
    if !config.signers.contains(signer) {
        return Err(ContractError::Unauthorized);
    }
    signer.require_auth();
    Ok(())
}

#[cfg(feature = "testutils")]
pub mod test_utils;

#[cfg(test)]
mod test;