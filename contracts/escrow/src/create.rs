//! Deposit-creation logic for the escrow contract.
//!
//! Extracted from `lib.rs` as part of #812 (modularisation). `lib.rs` delegates
//! straight here; all storage keys, types and constants are re-exported from the
//! parent crate root so callers still have a single import path.

use soroban_sdk::{symbol_short, Address, Env, Map};
use stellar_spend_shared::{
    errors::ContractError,
    validation::{require_basis_points, require_positive_amount, MAX_BASIS_POINTS},
};

use crate::{DataKey, EscrowDeposit, EscrowStatus, DEFAULT_TIMEOUT_LEDGERS, INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD};

/// Record a deposit and return its id.
///
/// The id is a monotonic counter rather than a derived string: `format!` is
/// unavailable in `no_std` without an allocator, and a counter is
/// collision-free without paying to encode addresses into a key.
pub fn deposit(
    env: &Env,
    depositor: Address,
    amount: i128,
    bridge_address: Address,
    fee_bps: u32,
) -> Result<u64, ContractError> {
    // ── CHECK ──────────────────────────────────────────────────────────
    require_positive_amount(amount)?;
    require_basis_points(fee_bps, MAX_BASIS_POINTS)?;
    depositor.require_auth();

    let storage = env.storage().instance();
    let timeout: u32 = storage
        .get(&DataKey::Timeout)
        .unwrap_or(DEFAULT_TIMEOUT_LEDGERS);
    let current_ledger = env.ledger().sequence();

    // Both operands are bounded (`timeout` by MAX_TIMEOUT_LEDGERS at write
    // time), but a near-u32::MAX ledger sequence would still wrap.
    let timeout_ledger = current_ledger
        .checked_add(timeout)
        .ok_or(ContractError::Overflow)?;

    let id: u64 = storage.get(&DataKey::NextId).unwrap_or(0);
    let next_id = id.checked_add(1).ok_or(ContractError::Overflow)?;

    // ── EFFECT ─────────────────────────────────────────────────────────
    let mut deposits: Map<u64, EscrowDeposit> = storage
        .get(&DataKey::Deposits)
        .unwrap_or_else(|| Map::new(env));

    deposits.set(
        id,
        EscrowDeposit {
            depositor: depositor.clone(),
            amount,
            bridge_address: bridge_address.clone(),
            timestamp: env.ledger().timestamp(),
            timeout_ledger,
            status: EscrowStatus::Pending,
            fee_bps,
        },
    );

    storage.set(&DataKey::Deposits, &deposits);
    storage.set(&DataKey::NextId, &next_id);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);

    // ── INTERACT ───────────────────────────────────────────────────────
    env.events().publish(
        (symbol_short!("deposit"),),
        (id, depositor, amount, bridge_address),
    );
    Ok(id)
}
