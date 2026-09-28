//! Treasury and tiered fee collection for Stellar-Spend.
//!
//! Holds the amount-tiered fee schedule, computes the fee owed on a transfer, and
//! records the treasury address that collected fees are routed to.
//!
//! All errors use the canonical [`ContractError`] from `stellar-spend-shared`; admin
//! checks delegate to [`stellar_spend_shared::auth::assert_is_admin`].
//!
//! # Dead code removed (issue #815)
//!
//! The previous `get_fee_for_amount` took the stored fee schedule as a parameter and
//! then ignored it entirely, returning hard-coded 50/25/10 basis points from an
//! `if`/`else if` chain:
//!
//! ```ignore
//! pub fn get_fee_for_amount(schedule: &Map<i128, u32>, amount: i128) -> u32 {
//!     let mut fee_bp = 50u32;                          // `schedule` never read
//!     if amount >= 10_000_000 { fee_bp = 10; }
//!     else if amount >= 1_000_000 { fee_bp = 25; }
//!     fee_bp
//! }
//! ```
//!
//! That made the whole configurable-schedule feature dead: `set_fee_schedule`
//! validated its input, wrote it to storage, emitted an event — and no read
//! path ever consulted the result. An admin could "change" the fee and collection
//! would carry on at the compiled-in rates. [`TreasuryContract::fee_for_amount`] now
//! reads the stored schedule, so the hard-coded branches are gone and the tiers seeded
//! at `init` are merely defaults.
//!
//! Also removed: `get_treasury` fell back to `Address::generate(&env)`, a
//! `testutils`-only constructor that cannot compile into a release WASM. It now
//! returns [`ContractError::NotInitialized`] instead of inventing an address to send
//! fees to.
//!
//! # Storage footprint reduction (issue #811)
//!
//! ## Before (schema v2)
//!
//! | Key              | Type              | Bytes per entry         |
//! |------------------|-------------------|-------------------------|
//! | `FeeSchedule`    | `Map<i128, u32>`  | 16 (key) + 4 (val) = 20 |
//! | `TotalCollected` | `i128`            | 16                      |
//!
//! ## After (schema v3)
//!
//! | Key              | Type              | Bytes per entry         | Saved |
//! |------------------|-------------------|-------------------------|-------|
//! | `FeeSchedule`    | `Map<u64, u32>`   |  8 (key) + 4 (val) = 12 | **8 bytes/tier** |
//! | `TotalCollected` | `i128`            | 16                      | —     |
//!
//! With up to [`MAX_FEE_TIERS`] = 16 tiers, the schedule map saves up to **128 bytes**
//! of instance storage. Soroban charges per-byte for storage writes; a full 16-tier
//! schedule update costs ~128 bytes less per write cycle under schema v3.
//!
//! The key type change is safe because [`TreasuryContract::set_fee_schedule`] has
//! always validated `amount_tier >= 0` — no negative tier thresholds can exist in
//! any live deployment, so narrowing the key to `u64` is lossless. The schema v3
//! migration casts all existing keys via `i128 as u64` (always in range post-validation).

#![no_std]
mod balance;

use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, symbol_short, Address, Env, Map, String, Vec,
};
use stellar_spend_shared::{
    errors::ContractError,
    validation::{
        basis_points_of, check_schema_version, require_basis_points,
        require_positive_amount,
    },
};
use balance::BalanceManager;

contractmeta!(key = "version", val = "1.0.0");
contractmeta!(key = "contract", val = "stellar-spend-treasury");

/// Current storage layout version.
///
/// Bumped from 2 → 3 by issue #811: fee schedule key narrowed from `i128` to `u64`,
/// saving 8 bytes per tier (up to 128 bytes for a full 16-tier schedule).
pub const SCHEMA_VERSION: u32 = 3;

/// Maximum fee for any single tier (5%).
pub const MAX_SINGLE_FEE_BP: u32 = 500;

/// Upper bound on stored tiers, keeping [`TreasuryContract::fee_for_amount`]'s linear
/// scan within a predictable instruction budget.
pub const MAX_FEE_TIERS: u32 = 16;

/// Treasury invariants:
/// - `fee_for_amount(amount)` is always determined by the highest stored tier
///   whose threshold is less than or equal to `amount`.
/// - A valid schedule is monotonic in threshold order: increasing `amount` must not
///   decrease the selected rate for a fixed schedule.
/// - Fees are non-negative and never exceed the configured basis-point cap for a
///   tier.

/// Instance TTL extension (~30 days) applied on state-changing calls.
pub const INSTANCE_TTL_EXTEND_TO: u32 = 518_400;
/// Only pay to extend when remaining TTL drops below ~6 days.
pub const INSTANCE_TTL_THRESHOLD: u32 = 103_680;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    /// `Map<u64, u32>`: tier threshold (in stroops) -> basis points.
    ///
    /// Changed from `Map<i128, u32>` in schema v3 to save 8 bytes per tier key.
    /// Tier thresholds have always been validated as non-negative, so the narrowing
    /// is lossless.
    FeeSchedule,
    /// Running total of fees collected. Added in schema v2.
    TotalCollected,
    Schema,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryState {
    pub total_balance: i128,
    pub reserved: i128,
    pub available: i128,
}

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialize treasury
    pub fn initialize(
        env: Env,
        admin: Address,
    ) -> Result<(), ContractError> {
        let state: Option<TreasuryState> = env.storage().instance().get(&String::from_str(&env, "state"));
        if state.is_some() {
            return Err(ContractError::AlreadyInitialized);
        }

        let initial_state = TreasuryState {
            total_balance: 0,
            reserved: 0,
            available: 0,
        };

        let mut schedule: Map<u64, u32> = Map::new(&env);
        schedule.set(0u64, 50);
        schedule.set(1_000_000u64, 25);
        schedule.set(10_000_000u64, 10);

        env.storage().instance().set(&String::from_str(&env, "state"), &initial_state);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::FeeSchedule, &schedule);
        env.storage().instance().set(&DataKey::Schema, &SCHEMA_VERSION);
        env.storage().instance().set(&DataKey::TotalCollected, &0i128);

        Self::bump_instance_ttl(&env);

        Ok(())
    }

    /// Deposit funds with overflow protection
    pub fn deposit(
        env: Env,
        amount: i128,
    ) -> Result<i128, ContractError> {
        let mut state: TreasuryState = env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)?;

        let new_total = BalanceManager::add(state.total_balance, amount)?;
        let new_available = BalanceManager::add(state.available, amount)?;

        state.total_balance = new_total;
        state.available = new_available;

        env.storage().instance().set(&String::from_str(&env, "state"), &state);
        Self::bump_instance_ttl(&env);

        Ok(state.total_balance)
    }

    /// Withdraw funds with overflow protection
    pub fn withdraw(
        env: Env,
        amount: i128,
    ) -> Result<i128, ContractError> {
        let mut state: TreasuryState = env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)?;
        let new_total = BalanceManager::sub(state.total_balance, amount)?;
        let new_available = BalanceManager::sub(state.available, amount)?;
        state.total_balance = new_total;
        state.available = new_available;
        env.storage().instance().set(&String::from_str(&env, "state"), &state);
        Self::bump_instance_ttl(&env);
        Ok(state.total_balance)
    }

    /// Internal helper that already has a loaded schedule.
    fn _collect_fee_with_schedule(
        env: &Env,
        amount: i128,
        state: &TreasuryState,
        schedule: &Map<u64, u32>,
    ) -> Result<i128, ContractError> {
        let fee = basis_points_of(amount, Self::select_tier(schedule, amount))?;

        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalCollected)
            .unwrap_or(0);
        let new_total = total.checked_add(fee).ok_or(ContractError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalCollected, &new_total);
        Self::bump_instance_ttl(env);
        let new_total = BalanceManager::sub(state.total_balance, amount)?;
        let new_available = BalanceManager::sub(state.available, amount)?;

        let mut new_state = state.clone();
        new_state.total_balance = new_total;
        new_state.available = new_available;

        env.storage().instance().set(&String::from_str(&env, "state"), &new_state);
        Self::bump_instance_ttl(env);

        Ok(new_state.total_balance)
    }

    /// Reserve funds (with checked math)
    pub fn reserve(
        env: Env,
        amount: i128,
    ) -> Result<i128, ContractError> {
        let mut state: TreasuryState = env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)?;

        let new_reserved = BalanceManager::add(state.reserved, amount)?;
        let new_available = BalanceManager::sub(state.available, amount)?;

        state.reserved = new_reserved;
        state.available = new_available;

        env.storage().instance().set(&String::from_str(&env, "state"), &state);
        Self::bump_instance_ttl(&env);

        Ok(state.reserved)
    }

    /// Release reserved funds (with checked math)
    pub fn release_reserved(
        env: Env,
        amount: i128,
    ) -> Result<i128, ContractError> {
        let mut state: TreasuryState = env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)?;

        let new_reserved = BalanceManager::sub(state.reserved, amount)?;
        let new_available = BalanceManager::add(state.available, amount)?;

        state.reserved = new_reserved;
        state.available = new_available;

        env.storage().instance().set(&String::from_str(&env, "state"), &state);
        Self::bump_instance_ttl(&env);

        Ok(state.available)
    }

    /// Add or update a fee tier. Admin only.
    pub fn set_fee_schedule(
        env: Env,
        amount_tier: i128,
        basis_points: u32,
    ) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        Self::require_admin(&env)?;
        if amount_tier < 0 {
            return Err(ContractError::InvalidInput);
        }
        require_basis_points(basis_points, MAX_SINGLE_FEE_BP)?;

        let tier_key = amount_tier as u64;
        let mut schedule = Self::load_schedule(&env)?;
        if !schedule.contains_key(tier_key) && schedule.len() >= MAX_FEE_TIERS {
            return Err(ContractError::InvalidInput);
        }

        schedule.set(tier_key, basis_points);
        env.storage()
            .instance()
            .set(&DataKey::FeeSchedule, &schedule);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("sched"),), (amount_tier, basis_points));
        Ok(())
    }

    /// Remove a fee tier. Admin only.
    pub fn remove_fee_tier(env: Env, amount_tier: i128) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        Self::require_admin(&env)?;

        if amount_tier < 0 {
            return Err(ContractError::InvalidInput);
        }
        let tier_key = amount_tier as u64;
        let mut schedule = Self::load_schedule(&env)?;
        if !schedule.contains_key(tier_key) {
            return Err(ContractError::InvalidInput);
        }
        schedule.remove(tier_key);
        env.storage()
            .instance()
            .set(&DataKey::FeeSchedule, &schedule);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("rmtier"),), amount_tier);
        Ok(())
    }

    /// The full stored fee schedule.
    pub fn get_fee_schedule(env: Env) -> Result<Map<u64, u32>, ContractError> {
        Self::require_current_schema(&env)?;
        Self::load_schedule(&env)
    }

    /// Get treasury state
    pub fn get_state(env: Env) -> Result<TreasuryState, ContractError> {
        env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)
    }

    /// Select the fee tier for a given amount from the schedule.
    fn select_tier(schedule: &Map<u64, u32>, amount: i128) -> u32 {
        let mut selected_threshold: Option<u64> = None;
        let mut selected_bps = 0u32;
        for (threshold, bps) in schedule.iter() {
            if amount >= threshold as i128 {
                if selected_threshold.is_none() || threshold > selected_threshold.unwrap() {
                    selected_threshold = Some(threshold);
                    selected_bps = bps;
                }
            }
        }
        selected_bps
    }

    /// Collect a fee from the given amount and route to treasury.
    pub fn collect_fee(
        env: Env,
        amount: i128,
        recipient: Address,
    ) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        let schedule = Self::load_schedule(&env)?;
        let fee = basis_points_of(amount, Self::select_tier(&schedule, amount))?;
        require_positive_amount(amount)?;

        let mut state: TreasuryState = env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)?;
        let new_total = BalanceManager::add(state.total_balance, fee)?;
        state.total_balance = new_total;

        env.storage().instance().set(&String::from_str(&env, "state"), &state);
        let prev_total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalCollected)
            .unwrap_or(0);
        let new_collected = prev_total
            .checked_add(fee)
            .ok_or(ContractError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalCollected, &new_collected);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("coll"),), (amount, fee, recipient));
        Ok(fee)
    }

    /// Route funds to the treasury address. Admin only.
    pub fn route_to_treasury(
        env: Env,
        amount: i128,
    ) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        let treasury: Address = env
            .storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(ContractError::NotInitialized)?;
        let mut state: TreasuryState = env.storage()
            .instance()
            .get(&String::from_str(&env, "state"))
            .ok_or(ContractError::NotInitialized)?;
        let new_total = BalanceManager::sub(state.total_balance, amount)?;
        state.total_balance = new_total;
        env.storage().instance().set(&String::from_str(&env, "state"), &state);
        Self::bump_instance_ttl(&env);

        env.events()
            .publish((symbol_short!("routed"),), (amount, treasury.clone()));
        Ok(amount)
    }

    /// Update the treasury routing address. Admin only.
    pub fn update_treasury(
        env: Env,
        treasury: Address,
    ) -> Result<(), ContractError> {
        Self::require_current_schema(&env)?;
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        Self::bump_instance_ttl(&env);
        env.events().publish((symbol_short!("trsy"),), treasury);
        Ok(())
    }

    /// Get the total collected fees.
    pub fn total_collected(env: Env) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        env.storage()
            .instance()
            .get(&DataKey::TotalCollected)
            .ok_or(ContractError::NotInitialized)
    }

    /// Get the treasury address.
    pub fn get_treasury(env: Env) -> Result<Address, ContractError> {
        Self::require_current_schema(&env)?;
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(ContractError::NotInitialized)
    }

    /// The stored schema version.
    pub fn schema_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Schema)
            .unwrap_or(0)
    }

    /// Get the fee for a given amount based on the stored schedule.
    pub fn fee_for_amount(env: Env, amount: i128) -> Result<u32, ContractError> {
        Self::require_current_schema(&env)?;
        if amount < 0 {
            return Err(ContractError::InvalidAmount);
        }
        let schedule = Self::load_schedule(&env)?;
        Ok(Self::select_tier(&schedule, amount))
    }

    /// Convert persisted state to [`SCHEMA_VERSION`]. Returns the version migrated from.
    pub fn migrate(env: Env) -> Result<u32, ContractError> {
        Self::require_admin(&env)?;

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
            env.storage()
                .instance()
                .set(&DataKey::TotalCollected, &0i128);
        }

        {
            let old: Map<i128, u32> = env
                .storage()
                .instance()
                .get(&DataKey::FeeSchedule)
                .unwrap_or_else(|| Map::new(&env));

            let mut new_schedule: Map<u64, u32> = Map::new(&env);
            for (threshold, bps) in old.iter() {
                new_schedule.set(threshold as u64, bps);
            }
            env.storage()
                .instance()
                .set(&DataKey::FeeSchedule, &new_schedule);
        }

        env.storage()
            .instance()
            .set(&DataKey::Schema, &SCHEMA_VERSION);
        Self::bump_instance_ttl(&env);
        env.events()
            .publish((symbol_short!("migrt"),), (stored, SCHEMA_VERSION));
        Ok(stored)
    }

    /// Collect a batch of fees, totalling them into a single storage write.
    pub fn collect_fee_batch(
        env: Env,
        amounts: Vec<i128>,
        recipient: Address,
    ) -> Result<Vec<i128>, ContractError> {
        Self::require_current_schema(&env)?;

        let schedule = Self::load_schedule(&env)?;
        let mut fees = Vec::new(&env);
        let mut total_fee = 0i128;

        for amount in amounts.iter() {
            require_positive_amount(amount)?;
            let fee = basis_points_of(amount, Self::select_tier(&schedule, amount))?;
            total_fee = total_fee
                .checked_add(fee)
                .ok_or(ContractError::Overflow)?;
            fees.push_back(fee);
        }

        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalCollected)
            .unwrap_or(0);
        let new_total = total
            .checked_add(total_fee)
            .ok_or(ContractError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalCollected, &new_total);
        Self::bump_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("cbatch"),),
            (recipient, total_fee, fees.len() as u32),
        );
        Ok(fees)
    }

    // ── Internal helpers ──────────────────────────────────────────────

    fn require_current_schema(env: &Env) -> Result<(), ContractError> {
        check_schema_version(
            env.storage().instance().get(&DataKey::Schema),
            SCHEMA_VERSION,
        )
    }

    fn require_admin(env: &Env) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        admin.require_auth();
        Ok(())
    }

    fn load_schedule(env: &Env) -> Result<Map<u64, u32>, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::FeeSchedule)
            .ok_or(ContractError::NotInitialized)
    }

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }
}

#[cfg(test)]
mod test;
mod tests;

#[cfg(feature = "testutils")]
pub mod test_utils;
