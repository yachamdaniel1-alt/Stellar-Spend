use stellar_spend_shared::errors::ContractError;

/// Treasury balance management with overflow protection
pub struct BalanceManager;

impl BalanceManager {
    /// Add to a balance with checked addition
    pub fn add(
        current: i128,
        amount: i128,
    ) -> Result<i128, ContractError> {
        if amount < 0 {
            return Err(ContractError::InvalidAmount);
        }
        
        current
            .checked_add(amount)
            .ok_or(ContractError::ArithmeticOverflow)
    }

    /// Subtract from a balance with checked subtraction
    pub fn sub(
        current: i128,
        amount: i128,
    ) -> Result<i128, ContractError> {
        if amount < 0 {
            return Err(ContractError::InvalidAmount);
        }
        
        if amount > current {
            return Err(ContractError::InsufficientBalance);
        }
        
        current
            .checked_sub(amount)
            .ok_or(ContractError::ArithmeticOverflow)
    }

    /// Validate that a balance is within safe bounds
    pub fn validate_balance(balance: i128) -> Result<(), ContractError> {
        if balance < 0 {
            return Err(ContractError::InvalidAmount);
        }
        Ok(())
    }

    /// Safe multiplication for fee calculation
    pub fn safe_mul(
        value: i128,
        multiplier: i128,
    ) -> Result<i128, ContractError> {
        if value < 0 || multiplier < 0 {
            return Err(ContractError::InvalidAmount);
        }
        
        value
            .checked_mul(multiplier)
            .ok_or(ContractError::ArithmeticOverflow)?
            .checked_div(10000)
            .ok_or(ContractError::ArithmeticOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ================================================================
    // Near-u128::MAX Balance Tests
    // ================================================================

    #[test]
    fn test_add_near_max_balance() {
        // Test adding to a near-max balance
        let current = i128::MAX - 100;
        let amount = 50;
        
        let result = BalanceManager::add(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), i128::MAX - 50);
    }

    #[test]
    fn test_add_overflow_at_max() {
        // Test that adding beyond MAX causes overflow
        let current = i128::MAX;
        let amount = 1;
        
        let result = BalanceManager::add(current, amount);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::ArithmeticOverflow);
    }

    #[test]
    fn test_add_overflow_near_max() {
        // Test overflow when amount pushes past MAX
        let current = i128::MAX - 50;
        let amount = 100;
        
        let result = BalanceManager::add(current, amount);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::ArithmeticOverflow);
    }

    #[test]
    fn test_add_to_zero() {
        let current = 0;
        let amount = 1000;
        
        let result = BalanceManager::add(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000);
    }

    #[test]
    fn test_add_large_but_safe() {
        let current = i128::MAX / 2;
        let amount = i128::MAX / 4;
        
        let result = BalanceManager::add(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (i128::MAX / 2) + (i128::MAX / 4));
    }

    // ================================================================
    // Zero/Near-Zero Withdrawal Tests
    // ================================================================

    #[test]
    fn test_sub_zero_balance() {
        let current = 0;
        let amount = 0;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_sub_zero_from_positive() {
        let current = 1000;
        let amount = 0;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000);
    }

    #[test]
    fn test_sub_to_zero() {
        let current = 1000;
        let amount = 1000;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_sub_below_zero() {
        let current = 100;
        let amount = 200;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::InsufficientBalance);
    }

    #[test]
    fn test_sub_underflow_at_min() {
        // Subtracting from a near-min balance
        let current = 1;
        let amount = 2;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::InsufficientBalance);
    }

    #[test]
    fn test_sub_large_amount() {
        let current = i128::MAX;
        let amount = i128::MAX - 100;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 100);
    }

    #[test]
    fn test_sub_invalid_amount() {
        let current = 1000;
        let amount = -50;
        
        let result = BalanceManager::sub(current, amount);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::InvalidAmount);
    }

    // ================================================================
    // Edge Case Tests
    // ================================================================

    #[test]
    fn test_add_invalid_amount() {
        let current = 1000;
        let amount = -50;
        
        let result = BalanceManager::add(current, amount);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::InvalidAmount);
    }

    #[test]
    fn test_safe_mul_valid() {
        let value = 1000;
        let multiplier = 100; // 1% (100 bps)
        
        let result = BalanceManager::safe_mul(value, multiplier);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10); // 1000 * 100 / 10000 = 10
    }

    #[test]
    fn test_safe_mul_zero() {
        let value = 1000;
        let multiplier = 0;
        
        let result = BalanceManager::safe_mul(value, multiplier);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_safe_mul_overflow() {
        let value = i128::MAX;
        let multiplier = 10000;
        
        let result = BalanceManager::safe_mul(value, multiplier);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::ArithmeticOverflow);
    }

    #[test]
    fn test_validate_balance_valid() {
        let balance = 1000;
        let result = BalanceManager::validate_balance(balance);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_balance_zero() {
        let balance = 0;
        let result = BalanceManager::validate_balance(balance);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_balance_negative() {
        let balance = -1000;
        let result = BalanceManager::validate_balance(balance);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContractError::InvalidAmount);
    }

    // ================================================================
    // Property-Style Tests (simulating fuzzing)
    // ================================================================

    #[test]
    fn test_add_commutative() {
        // Addition should be commutative
        let a = 1000;
        let b = 500;
        
        let result1 = BalanceManager::add(a, b);
        let result2 = BalanceManager::add(b, a);
        
        assert_eq!(result1.unwrap(), result2.unwrap());
    }

    #[test]
    fn test_add_associative() {
        // Addition should be associative
        let a = 1000;
        let b = 500;
        let c = 250;
        
        let result1 = BalanceManager::add(BalanceManager::add(a, b).unwrap(), c);
        let result2 = BalanceManager::add(a, BalanceManager::add(b, c).unwrap());
        
        assert_eq!(result1.unwrap(), result2.unwrap());
    }

    #[test]
    fn test_sub_add_inverse() {
        // Subtraction should be the inverse of addition
        let current = 1000;
        let amount = 300;
        
        let after_add = BalanceManager::add(current, amount).unwrap();
        let after_sub = BalanceManager::sub(after_add, amount).unwrap();
        
        assert_eq!(after_sub, current);
    }
}
