use soroban_sdk::{Address, Env, Symbol, Vec};

use crate::errors::ContractError;

pub use crate::policy::{required_threshold, verify_threshold};

/// Authorization error types
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthError {
    Unauthorized = 1,
    NotAdmin = 2,
    InvalidSigner = 3,
    InsufficientPermissions = 4,
}

pub struct AdminAuth;

impl AdminAuth {
    pub fn require_admin(env: &Env, admin: &Address, caller: &Address) -> Result<(), AuthError> {
        if caller != admin {
            return Err(AuthError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    pub fn require_admin_or_role(
        env: &Env,
        admin: &Address,
        caller: &Address,
        role_check: fn(&Address) -> bool,
    ) -> Result<(), AuthError> {
        if caller == admin {
            caller.require_auth();
            return Ok(());
        }
        if role_check(caller) {
            caller.require_auth();
            return Ok(());
        }
        Err(AuthError::Unauthorized)
    }

    pub fn require_role(
        env: &Env,
        caller: &Address,
        role_check: fn(&Address) -> bool,
    ) -> Result<(), AuthError> {
        if !role_check(caller) {
            return Err(AuthError::InsufficientPermissions);
        }
        caller.require_auth();
        Ok(())
    }

    pub fn is_admin(env: &Env, admin: &Address, caller: &Address) -> bool {
        caller == admin
    }
}

pub fn assert_is_admin(env: &Env, admin: &Address, admin_key: &str) -> Result<(), ContractError> {
    let stored_admin: Address = env.storage().instance().get(&Symbol::new(env, admin_key))
        .ok_or(ContractError::NotFound)?;
    if admin != &stored_admin {
        return Err(ContractError::Unauthorized);
    }
    admin.require_auth();
    Ok(())
}

pub fn assert_is_signer(env: &Env, signer: &Address, signers_key: &str) -> Result<(), ContractError> {
    let signers: Vec<Address> = env.storage().instance().get(&Symbol::new(env, signers_key))
        .ok_or(ContractError::NotInitialized)?;
    if !signers.contains(signer) {
        return Err(ContractError::Unauthorized);
    }
    signer.require_auth();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{required_threshold, verify_threshold};
    use proptest::prelude::*;

    #[test]
    fn threshold_zero_signers_is_full() {
        assert_eq!(required_threshold(3, 0, 1_000), 3);
    }

    #[test]
    fn threshold_value_below_limit_returns_one() {
        assert_eq!(required_threshold(3, 1_000, 500), 1);
    }

    proptest! {
        #[test]
        fn required_threshold_policy_invariant_holds(
            full_threshold in 1u32..=32u32,
            high_value_limit in 0i128..=1_000_000_000i128,
            value in 0i128..=1_000_000_000i128,
        ) {
            let required = required_threshold(full_threshold, high_value_limit, value);
            if high_value_limit > 0 && value <= high_value_limit {
                prop_assert_eq!(required, 1);
            } else {
                prop_assert_eq!(required, full_threshold);
            }
        }

        #[test]
        fn verify_threshold_matches_the_policy_invariant(
            sig_count in 0u32..=32u32,
            full_threshold in 1u32..=32u32,
            high_value_limit in 0i128..=1_000_000_000i128,
            value in 0i128..=1_000_000_000i128,
        ) {
            let required = required_threshold(full_threshold, high_value_limit, value);
            let ok = verify_threshold(sig_count, full_threshold, high_value_limit, value);
            if sig_count >= required {
                prop_assert!(ok.is_ok());
            } else {
                prop_assert_eq!(ok.unwrap_err(), ContractError::BelowThreshold);
            }
        }
    }
}
