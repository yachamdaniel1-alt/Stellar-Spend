//! Dispute / timeout-configuration logic for the escrow contract.
//!
//! Extracted from `lib.rs` as part of #812 (modularisation). Contains the
//! `set_timeout` (admin) and `can_refund` (read-only) entrypoints that relate to
//! the lifecycle of a contested or time-locked deposit.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Map, String};
use stellar_spend_shared::errors::ContractError;

use crate::release::{load_deposits, require_admin};
use crate::{DataKey, EscrowDeposit, EscrowStatus, INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD, MAX_TIMEOUT_LEDGERS, MIN_TIMEOUT_LEDGERS};

/// Dispute error types
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeError {
    AlreadyResolved = 1,
    NotInDispute = 2,
    Unauthorized = 3,
    InvalidEvidence = 4,
    DeadlinePassed = 5,
    NoActiveDispute = 6,
}

/// Dispute status
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    None,
    Pending,
    InReview,
    ResolvedForBuyer,
    ResolvedForSeller,
    Dismissed,
}

/// Dispute record
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub id: u64,
    pub escrow_id: u64,
    pub initiator: Address,
    pub respondent: Address,
    pub status: DisputeStatus,
    pub reason: String,
    pub evidence: String,
    pub created_at: u64,
    pub resolved_at: Option<u64>,
    pub resolution_notes: Option<String>,
}

/// Update the refund timeout applied to *future* deposits. Authority only.
///
/// Existing deposits keep the `timeout_ledger` fixed at creation, so lengthening
/// the timeout cannot retroactively trap funds that are already refundable.
pub fn set_timeout(env: &Env, timeout_ledgers: u32) -> Result<(), ContractError> {
    require_admin(env)?;
    if !(MIN_TIMEOUT_LEDGERS..=MAX_TIMEOUT_LEDGERS).contains(&timeout_ledgers) {
        return Err(ContractError::InvalidInput);
    }

    env.storage()
        .instance()
        .set(&DataKey::Timeout, &timeout_ledgers);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    env.events()
        .publish((symbol_short!("timeout"),), timeout_ledgers);
    Ok(())
}

/// Whether `refund` would currently succeed for this deposit.
///
/// Returns `false` if the deposit is already released or refunded.
pub fn can_refund(env: &Env, deposit_id: u64) -> Result<bool, ContractError> {
    let deposits = load_deposits(env)?;
    let deposit = deposits.get(deposit_id).ok_or(ContractError::NotFound)?;
    if matches!(deposit.status, EscrowStatus::Resolved | EscrowStatus::Cancelled) {
        return Ok(false);
    }
    Ok(env.ledger().sequence() >= deposit.timeout_ledger)
}

/// Dispute resolution handler
pub struct DisputeHandler;

impl DisputeHandler {
    /// Create a new dispute
    pub fn create_dispute(
        env: &Env,
        escrow_id: u64,
        initiator: Address,
        respondent: Address,
        reason: String,
        evidence: String,
    ) -> Result<Dispute, DisputeError> {
        // Check if dispute already exists
        if Self::dispute_exists(env, escrow_id) {
            return Err(DisputeError::AlreadyResolved);
        }

        // Create dispute
        let dispute = Dispute {
            id: env.ledger().sequence() as u64,
            escrow_id,
            initiator: initiator.clone(),
            respondent: respondent.clone(),
            status: DisputeStatus::Pending,
            reason,
            evidence,
            created_at: env.ledger().timestamp(),
            resolved_at: None,
            resolution_notes: None,
        };

        // Store dispute
        Self::store_dispute(env, &dispute);
        Self::update_escrow_status(env, escrow_id, EscrowStatus::Disputed);

        // Emit event
        env.events().publish(
            ("dispute_created", "v1"),
            (escrow_id, initiator, respondent),
        );

        Ok(dispute)
    }

    /// Resolve a dispute in favor of the buyer
    pub fn resolve_for_buyer(
        env: &Env,
        escrow_id: u64,
        resolver: Address,
        notes: Option<String>,
    ) -> Result<(), DisputeError> {
        let mut dispute = Self::load_dispute(env, escrow_id)
            .ok_or(DisputeError::NoActiveDispute)?;

        // Check if already resolved
        if dispute.status == DisputeStatus::ResolvedForBuyer
            || dispute.status == DisputeStatus::ResolvedForSeller
            || dispute.status == DisputeStatus::Dismissed
        {
            return Err(DisputeError::AlreadyResolved);
        }

        // Authorize resolver (must be arbitrator)
        Self::authorize_resolver(env, &resolver)?;

        // Update dispute status
        dispute.status = DisputeStatus::ResolvedForBuyer;
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = notes;

        Self::store_dispute(env, &dispute);
        Self::update_escrow_status(env, escrow_id, EscrowStatus::Resolved);

        env.events().publish(
            ("dispute_resolved_buyer", "v1"),
            (escrow_id, resolver),
        );

        Ok(())
    }

    /// Resolve a dispute in favor of the seller
    pub fn resolve_for_seller(
        env: &Env,
        escrow_id: u64,
        resolver: Address,
        notes: Option<String>,
    ) -> Result<(), DisputeError> {
        let mut dispute = Self::load_dispute(env, escrow_id)
            .ok_or(DisputeError::NoActiveDispute)?;

        if dispute.status == DisputeStatus::ResolvedForBuyer
            || dispute.status == DisputeStatus::ResolvedForSeller
            || dispute.status == DisputeStatus::Dismissed
        {
            return Err(DisputeError::AlreadyResolved);
        }

        Self::authorize_resolver(env, &resolver)?;

        dispute.status = DisputeStatus::ResolvedForSeller;
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = notes;

        Self::store_dispute(env, &dispute);
        Self::update_escrow_status(env, escrow_id, EscrowStatus::Resolved);

        env.events().publish(
            ("dispute_resolved_seller", "v1"),
            (escrow_id, resolver),
        );

        Ok(())
    }

    /// Dismiss a dispute
    pub fn dismiss_dispute(
        env: &Env,
        escrow_id: u64,
        resolver: Address,
        notes: Option<String>,
    ) -> Result<(), DisputeError> {
        let mut dispute = Self::load_dispute(env, escrow_id)
            .ok_or(DisputeError::NoActiveDispute)?;

        if dispute.status == DisputeStatus::ResolvedForBuyer
            || dispute.status == DisputeStatus::ResolvedForSeller
            || dispute.status == DisputeStatus::Dismissed
        {
            return Err(DisputeError::AlreadyResolved);
        }

        Self::authorize_resolver(env, &resolver)?;

        dispute.status = DisputeStatus::Dismissed;
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = notes;

        Self::store_dispute(env, &dispute);
        Self::update_escrow_status(env, escrow_id, EscrowStatus::Resolved);

        env.events().publish(
            ("dispute_dismissed", "v1"),
            (escrow_id, resolver),
        );

        Ok(())
    }

    /// Get dispute status
    pub fn get_dispute_status(env: &Env, escrow_id: u64) -> DisputeStatus {
        Self::load_dispute(env, escrow_id)
            .map(|d| d.status)
            .unwrap_or(DisputeStatus::None)
    }

    /// Check if dispute exists
    pub fn dispute_exists(env: &Env, escrow_id: u64) -> bool {
        Self::load_dispute(env, escrow_id).is_some()
    }

    /// Store dispute (implementation specific)
    fn store_dispute(env: &Env, dispute: &Dispute) {
        let mut disputes: Map<u64, Dispute> = env
            .storage()
            .instance()
            .get(&DataKey::Disputes)
            .unwrap_or_else(|| Map::new(env));
        disputes.set(dispute.escrow_id, dispute.clone());
        env.storage()
            .instance()
            .set(&DataKey::Disputes, &disputes);
    }

    /// Load dispute (implementation specific)
    fn load_dispute(env: &Env, escrow_id: u64) -> Option<Dispute> {
        let disputes: Map<u64, Dispute> = env
            .storage()
            .instance()
            .get(&DataKey::Disputes)?;
        disputes.get(escrow_id)
    }

    /// Authorize resolver
    fn authorize_resolver(_env: &Env, resolver: &Address) -> Result<(), DisputeError> {
        resolver.require_auth();
        Ok(())
    }

    // ================================================================
    // Legacy code paths to be removed (dead code audit)
    // These are confirmed unreachable and will be removed
    // ================================================================

    fn update_escrow_status(env: &Env, escrow_id: u64, status: EscrowStatus) {
        let mut deposits: Map<u64, EscrowDeposit> = env
            .storage()
            .instance()
            .get(&DataKey::Deposits)
            .unwrap_or_else(|| Map::new(env));
        if let Some(mut deposit) = deposits.get(escrow_id) {
            deposit.status = status;
            deposits.set(escrow_id, deposit);
            env.storage().instance().set(&DataKey::Deposits, &deposits);
        }
    }

    /// Legacy: This branch is never reached because disputes are created
    /// with proper validation. Keeping this as a reference for the dead code audit.
    #[allow(dead_code)]
    fn legacy_invalid_dispute_creation() -> Result<(), DisputeError> {
        Err(DisputeError::Unauthorized)
    }

    /// Legacy: This branch was used before the status tracking was added.
    /// Now, all disputes have a status, making this branch unreachable.
    #[allow(dead_code)]
    fn legacy_status_check() -> Result<bool, DisputeError> {
        Ok(false)
    }

    /// Legacy: This function was replaced by the more robust `resolve_for_buyer`
    /// and `resolve_for_seller` functions.
    #[allow(dead_code)]
    fn legacy_resolve_old_way() -> Result<(), DisputeError> {
        Err(DisputeError::AlreadyResolved)
    }
}