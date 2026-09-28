//! Escrow contract for Stellar-Spend off-ramp settlement.
//!
//! Holds a bookkeeping record of a depositor's intent to bridge funds. The
//! settlement authority may `release` a deposit to the bridge, or the depositor may
//! `refund` it once the timeout ledger has passed. A deposit can be released or
//! refunded, never both.
//!
//! ## Security model
//!
//! ### Check-Effects-Interactions (CEI)
//! `release` and `refund` follow strict CEI order: validate inputs and
//! authorisation, mutate storage under an explicit reentrancy lock, then emit
//! events. Because a contract can in principle be invoked from another contract,
//! a boolean lock in instance storage rejects any re-entrant call with
//! `ContractError::Reentrant` rather than letting it observe half-written state.
//!
//! ### Error taxonomy
//! All errors use the canonical [`ContractError`] from `stellar-spend-shared`.
//!
//! # Scope limitation
//!
//! This contract records deposit *state*; it does not itself move tokens. `deposit`
//! does not pull funds via a token client and `release`/`refund` do not pay out.
//! Settlement is performed off-chain against the events emitted here — wiring
//! `stellar_spend_shared::token` into these entrypoints is tracked separately.
//!
//! # Storage
//!
//! All state lives in instance storage, including the deposit map. That bounds the
//! contract to the number of deposits that fit in a single instance entry; it is a
//! deliberate trade so that [`EscrowContract::migrate`] can enumerate every deposit
//! during a schema upgrade (Soroban cannot iterate persistent-storage keys).
//!
//! # Module layout (#812)
//!
//! Business logic is split into focused modules; this file is a thin
//! contract-trait dispatcher:
//!
//! | Module        | Responsibility                                       |
//! |---------------|------------------------------------------------------|
//! | `create`      | [`EscrowContract::deposit`]                          |
//! | `release`     | [`EscrowContract::release`] + shared guards          |
//! | `refund`      | [`EscrowContract::refund`]                           |
//! | `dispute`     | [`EscrowContract::set_timeout`], `can_refund`        |

#![no_std]
mod dispute;
mod create;
mod refund;
mod release;

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Map};
use stellar_spend_shared::{errors::ContractError, validation::check_schema_version};

// ── Sub-modules (issue #812) ──────────────────────────────────────────────────

/// Current storage layout version. Bump whenever a stored type changes shape, and
/// extend [`EscrowContract::migrate`] to convert the previous layout.
pub const SCHEMA_VERSION: u32 = 2;

/// Default refund timeout, in ledgers (~7 days at 1s close time).
pub const DEFAULT_TIMEOUT_LEDGERS: u32 = 604_800;
/// Smallest configurable timeout. Zero would make deposits refundable immediately.
pub const MIN_TIMEOUT_LEDGERS: u32 = 1;
/// Largest configurable timeout, guarding against `timeout_ledger` overflow.
///
/// Note this exceeds the network's maximum entry TTL, so a deposit configured with
/// a timeout near this ceiling needs its instance TTL bumped by an outside caller
/// before the deadline arrives; [`INSTANCE_TTL_EXTEND_TO`] only covers ~30 days.
pub const MAX_TIMEOUT_LEDGERS: u32 = 10_000_000;

/// Ledgers of instance TTL requested on every state-changing call (~30 days).
///
/// Without this the instance entry — which holds the deposit map — archives while
/// deposits are still open, making them permanently unreleasable and unrefundable.
pub const INSTANCE_TTL_EXTEND_TO: u32 = 518_400;
/// Only pay to extend when the remaining TTL has fallen below this (~6 days).
pub const INSTANCE_TTL_THRESHOLD: u32 = 103_680;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Settlement authority; the only address permitted to release or reconfigure.
    Admin,
    /// Refund timeout in ledgers, applied to deposits at creation time.
    Timeout,
    /// `Map<u64, EscrowDeposit>` of every deposit ever created.
    Deposits,
    /// Monotonic counter backing deposit ids.
    NextId,
    /// Storage layout version, see [`SCHEMA_VERSION`].
    Schema,
    /// Reentrancy guard: `true` while a release/refund is executing.
    Lock,
    /// `Map<u64, Dispute>` of all disputes, keyed by escrow id.
    Disputes,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    Pending,
    Active,
    Disputed,
    Resolved,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowDeposit {
    pub depositor: Address,
    pub amount: i128,
    pub bridge_address: Address,
    pub timestamp: u64,
    pub timeout_ledger: u32,
    pub status: EscrowStatus,
    pub fee_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowDepositV1 {
    pub depositor: Address,
    pub amount: i128,
    pub bridge_address: Address,
    pub timestamp: u64,
    pub timeout_ledger: u32,
    pub released: bool,
    pub refunded: bool,
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Initialise the escrow with its settlement authority.
    ///
    /// Rejects a second call rather than silently reassigning the authority, which
    /// would let anyone who front-ran the deploy take over the contract.
    pub fn init(env: Env, settlement_authority: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Schema) {
            return Err(ContractError::AlreadyInitialized);
        }
        settlement_authority.require_auth();

        let storage = env.storage().instance();
        storage.set(&DataKey::Admin, &settlement_authority);
        storage.set(&DataKey::Timeout, &DEFAULT_TIMEOUT_LEDGERS);
        storage.set(&DataKey::Deposits, &Map::<u64, EscrowDeposit>::new(&env));
        storage.set(&DataKey::NextId, &0u64);
        storage.set(&DataKey::Schema, &SCHEMA_VERSION);
        storage.set(&DataKey::Lock, &false);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("init"),), settlement_authority);
        Ok(())
    }

    /// Record a deposit and return its id.
    ///
    /// Delegates to [`create::deposit`].
    pub fn deposit(
        env: Env,
        depositor: Address,
        amount: i128,
        bridge_address: Address,
        fee_bps: u32,
    ) -> Result<u64, ContractError> {
        Self::require_current_schema(&env)?;
        create::deposit(&env, depositor, amount, bridge_address, fee_bps)
    }

    /// Release a deposit to `recipient`. Settlement authority only.
    ///
    /// Delegates to [`release::release`].
    pub fn release(env: Env, deposit_id: u64, recipient: Address) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        release::release(&env, deposit_id, recipient)
    }

    /// Refund a deposit to its depositor once the timeout ledger has passed.
    ///
    /// Delegates to [`refund::refund`].
    pub fn refund(env: Env, deposit_id: u64) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        refund::refund(&env, deposit_id)
    }

    /// Fetch a deposit record.
    pub fn get_deposit(env: Env, deposit_id: u64) -> Result<EscrowDeposit, ContractError> {
        Self::require_current_schema(&env)?;
        release::load_deposits(&env)?
            .get(deposit_id)
            .ok_or(ContractError::NotFound)
    }

    /// The stored schema version.
    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Schema)
            .ok_or(ContractError::NotInitialized)
    }

    /// Update the refund timeout applied to *future* deposits. Authority only.
    ///
    /// Delegates to [`dispute::set_timeout`].
    pub fn set_timeout(env: Env, timeout_ledgers: u32) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        dispute::set_timeout(&env, timeout_ledgers)
    }

    /// Whether `refund` would currently succeed for this deposit.
    ///
    /// Delegates to [`dispute::can_refund`].
    pub fn can_refund(env: Env, deposit_id: u64) -> Result<bool, ContractError> {
        Self::require_current_schema(&env)?;
        dispute::can_refund(&env, deposit_id)
    }

    // ── Upgrade surface (issue #817) ──────────────────────────────────────────

    /// Replace the contract WASM. Authority only.
    ///
    /// Storage is untouched by the swap, so the new build must be able to read the
    /// existing layout — run `migrate` immediately afterwards, in the same
    /// transaction where possible.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        release::require_admin(&env)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.events().publish((symbol_short!("upgrade"),), ());
        Ok(())
    }

    /// Convert persisted state from an older schema to [`SCHEMA_VERSION`].
    ///
    /// Deliberately skips the `require_current_schema` guard every other entrypoint
    /// applies — it is the one call that is *supposed* to run against stale state.
    /// Returns the version migrated from.
    pub fn migrate(env: Env) -> Result<u32, ContractError> {
        release::require_admin(&env)?;

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
            let old: Map<u64, EscrowDepositV1> = env
                .storage()
                .instance()
                .get(&DataKey::Deposits)
                .unwrap_or_else(|| Map::new(&env));

            let mut migrated: Map<u64, EscrowDeposit> = Map::new(&env);
            for (id, v1) in old.iter() {
                let status = if v1.released {
                    EscrowStatus::Resolved
                } else if v1.refunded {
                    EscrowStatus::Cancelled
                } else {
                    EscrowStatus::Pending
                };
                migrated.set(
                    id,
                    EscrowDeposit {
                        depositor: v1.depositor,
                        amount: v1.amount,
                        bridge_address: v1.bridge_address,
                        timestamp: v1.timestamp,
                        timeout_ledger: v1.timeout_ledger,
                        status,
                        fee_bps: 0,
                    },
                );
            }
            env.storage().instance().set(&DataKey::Deposits, &migrated);
        }

        env.storage()
            .instance()
            .set(&DataKey::Schema, &SCHEMA_VERSION);
        Self::bump_instance_ttl(&env);
        env.events()
            .publish((symbol_short!("migrate"),), (stored, SCHEMA_VERSION));
        Ok(stored)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

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

#[cfg(feature = "testutils")]
pub mod test_utils;

#[cfg(test)]
mod test;
mod tests;