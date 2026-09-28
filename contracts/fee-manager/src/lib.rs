//! Fee calculation and emergency circuit breaker for Stellar-Spend.
//!
//! Owns the flat fee rate applied to bridge transfers and the pause switch that
//! halts fee-bearing operations during an incident. Tiered, amount-dependent fee
//! schedules live in the `treasury` contract; this one applies a single rate.
//!
//! All errors use the canonical [`ContractError`] from `stellar-spend-shared`.
//!
//! # Dead code removed (issue #815)
//!
//! The previous revision carried three constructs that could never do anything:
//!
//! * `VERSION` / `version()` built a `String` via `String::from_slice`, which is not
//!   a `soroban_sdk::String` constructor. Replaced with `contractmeta!` — version
//!   metadata belongs in the WASM custom section, not in a runtime entrypoint that
//!   costs a host call to read.
//! * `migrate(new_version)` took a version argument, emitted an event, and returned.
//!   It touched no storage, so calling it did nothing an off-chain event could not do
//!   for free. Replaced with a real schema migration (issue #817).
//! * `calculate_fee` cast a possibly-negative `i128` through `u128` before
//!   multiplying, so a negative amount wrapped to an enormous positive fee instead of
//!   being rejected. The branch handling that case did not exist at all.
//!
//! # Module layout (issue #984)
//!
//! Business logic is split into focused modules; this file is a thin
//! contract-trait dispatcher:
//!
//! | Module | Responsibility                                       |
//! |--------|------------------------------------------------------|
//! | `admin` | [`FeeManagerContract::init`], `pause`, `unpause`, `set_default_rate`, upgrade surface |
//! | `calc`  | [`FeeManagerContract::calculate_fee`], `calculate_default_fee`, pure arithmetic |

#![no_std]

use soroban_sdk::{contract, contractimpl, contractmeta, contracttype, Address, BytesN, Env, String};
use stellar_spend_shared::errors::ContractError;

contractmeta!(key = "version", val = "1.0.0");
contractmeta!(key = "contract", val = "stellar-spend-fee-manager");

/// Current storage layout version.
pub const SCHEMA_VERSION: u32 = 2;

/// Ceiling on the configurable default rate (5%), matching the treasury's per-tier cap.
pub const MAX_DEFAULT_FEE_BP: u32 = 500;

/// Rate applied when a contract initialised under schema v1 is migrated forward.
pub const MIGRATED_DEFAULT_FEE_BP: u32 = 50;

/// Instance TTL extension (~30 days) applied on state-changing calls.
pub const INSTANCE_TTL_EXTEND_TO: u32 = 518_400;
/// Only pay to extend when remaining TTL drops below ~6 days.
pub const INSTANCE_TTL_THRESHOLD: u32 = 103_680;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    /// Default fee rate in basis points. Added in schema v2.
    DefaultRate,
    Schema,
}

// ── Sub-modules (issue #984) ──────────────────────────────────────────────────

pub mod admin;
pub mod calc;

#[contract]
pub struct FeeManagerContract;

#[contractimpl]
impl FeeManagerContract {
    /// Initialise with an admin and a starting fee rate.
    pub fn init(env: Env, admin: Address, default_fee_bp: u32) -> Result<(), ContractError> {
        admin::Admin::init(env, admin, default_fee_bp)
    }

    /// Human-readable contract version, sourced from the same string as `contractmeta!`.
    pub fn version(env: Env) -> String {
        String::from_str(&env, "1.0.0")
    }

    /// Trip the circuit breaker. Admin only.
    pub fn pause(env: Env, reason: String) -> Result<(), ContractError> {
        admin::Admin::pause(env, reason)
    }

    /// Reset the circuit breaker. Admin only.
    pub fn unpause(env: Env) -> Result<(), ContractError> {
        admin::Admin::unpause(env)
    }

    /// Whether the circuit breaker is currently tripped.
    pub fn is_paused(env: Env) -> bool {
        admin::Admin::is_paused(env)
    }

    /// Fee for `amount` at an explicit rate.
    pub fn calculate_fee(env: Env, amount: i128, fee_rate: u32) -> Result<i128, ContractError> {
        calc::Calculator::calculate_fee(env, amount, fee_rate)
    }

    /// Fee for `amount` at the configured default rate.
    pub fn calculate_default_fee(env: Env, amount: i128) -> Result<i128, ContractError> {
        calc::Calculator::calculate_default_fee(env, amount)
    }

    /// The configured default fee rate, in basis points.
    pub fn default_rate(env: Env) -> Result<u32, ContractError> {
        admin::Admin::default_rate(env)
    }

    /// Update the default fee rate. Admin only.
    pub fn set_default_rate(env: Env, fee_bp: u32) -> Result<(), ContractError> {
        admin::Admin::set_default_rate(env, fee_bp)
    }

    // ── Upgrade surface ────────────────────────────────────────────────────────

    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        admin::Admin::schema_version(env)
    }

    /// Replace the contract WASM. Admin only. Run `migrate` immediately after.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        admin::Admin::upgrade(env, new_wasm_hash)
    }

    /// Convert persisted state to [`SCHEMA_VERSION`]. Returns the version migrated from.
    pub fn migrate(env: Env) -> Result<u32, ContractError> {
        admin::Admin::migrate(env)
    }
}

#[cfg(feature = "testutils")]
pub mod test_utils;

#[cfg(test)]
mod test;
