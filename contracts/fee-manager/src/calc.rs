//! Pure fee arithmetic for the fee-manager contract (issue #984).

use soroban_sdk::Env;
use stellar_spend_shared::{
    errors::ContractError,
    validation::{basis_points_of, require_basis_points, require_positive_amount, MAX_BASIS_POINTS},
};

use crate::{DataKey, SCHEMA_VERSION};

/// Fee calculation handler
pub struct Calculator;

impl Calculator {
    /// Fee for `amount` at an explicit rate.
    pub fn calculate_fee(env: Env, amount: i128, fee_rate: u32) -> Result<i128, ContractError> {
        Self::require_current_schema(&env)?;
        if Self::paused_flag(&env) {
            return Err(ContractError::Paused);
        }
        require_positive_amount(amount)?;
        require_basis_points(fee_rate, MAX_BASIS_POINTS)?;

        basis_points_of(amount, fee_rate)
    }

    /// Fee for `amount` at the configured default rate.
    pub fn calculate_default_fee(env: Env, amount: i128) -> Result<i128, ContractError> {
        let rate = Self::get_default_rate(env.clone())?;
        Self::calculate_fee(env, amount, rate)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn require_current_schema(env: &Env) -> Result<(), ContractError> {
        use stellar_spend_shared::validation::check_schema_version;

        check_schema_version(
            env.storage().instance().get(&DataKey::Schema),
            SCHEMA_VERSION,
        )
    }

    fn paused_flag(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    fn get_default_rate(env: Env) -> Result<u32, ContractError> {
        Self::require_current_schema(&env)?;
        env.storage()
            .instance()
            .get(&DataKey::DefaultRate)
            .ok_or(ContractError::NotInitialized)
    }
}