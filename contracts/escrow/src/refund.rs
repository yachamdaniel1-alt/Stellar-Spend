//! Deposit-refund logic for the escrow contract.
//!
//! Extracted from `lib.rs` as part of #812 (modularisation). The depositor may
//! reclaim their funds once the timeout ledger has passed; the function follows
//! strict Check-Effects-Interactions order and holds the reentrancy lock for its
//! duration.

use soroban_sdk::{symbol_short, Env};
use stellar_spend_shared::errors::ContractError;

use crate::release::{acquire_lock, load_deposits, release_lock};
use crate::{DataKey, EscrowStatus, INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD};

/// Refund a deposit to its depositor once the timeout ledger has passed.
///
/// Requires the depositor's authorisation. Marks the deposit as refunded in
/// storage and emits a `refund` event. Returns the original deposit amount.
pub fn refund(env: &Env, deposit_id: u64) -> Result<i128, ContractError> {
    // ── CHECK ──────────────────────────────────────────────────────────
    acquire_lock(env)?;

    let mut deposits = load_deposits(env)?;
    let mut deposit = match deposits.get(deposit_id) {
        Some(d) => d,
        None => {
            release_lock(env);
            return Err(ContractError::NotFound);
        }
    };

    deposit.depositor.require_auth();

    if matches!(deposit.status, EscrowStatus::Resolved | EscrowStatus::Cancelled) {
        release_lock(env);
        return Err(ContractError::AlreadyProcessed);
    }
    if env.ledger().sequence() < deposit.timeout_ledger {
        release_lock(env);
        return Err(ContractError::Expired);
    }

    // ── EFFECT ─────────────────────────────────────────────────────────
    let amount = deposit.amount;
    let depositor = deposit.depositor.clone();
    deposit.status = EscrowStatus::Cancelled;
    deposits.set(deposit_id, deposit);
    env.storage().instance().set(&DataKey::Deposits, &deposits);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    release_lock(env);

    // ── INTERACT ───────────────────────────────────────────────────────
    env.events()
        .publish((symbol_short!("refund"),), (deposit_id, depositor, amount));
    Ok(amount)
}
