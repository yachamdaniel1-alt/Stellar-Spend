#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn with_client(f: impl FnOnce(&TreasuryContractClient)) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(TreasuryContract, ());
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    f(&client);
}

#[test]
fn test_initialize_treasury() {
    with_client(|client| {
        let state = client.get_state();
        assert_eq!(state.total_balance, 0);
        assert_eq!(state.reserved, 0);
        assert_eq!(state.available, 0);
    });
}

#[test]
fn test_deposit_overflow_protection() {
    with_client(|client| {
        let result = client.try_deposit(&i128::MAX);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), i128::MAX);

        let result = client.try_deposit(&1);
        assert!(result.is_err());
    });
}

#[test]
fn test_withdraw_overflow_protection() {
    with_client(|client| {
        client.deposit(&1000);
        let result = client.try_withdraw(&1000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), 0);
    });
}

#[test]
fn test_reserve_overflow_protection() {
    with_client(|client| {
        client.deposit(&100);
        let result = client.try_reserve(&i128::MAX);
        assert!(result.is_err());
    });
}

#[test]
fn test_release_reserved_overflow_protection() {
    with_client(|client| {
        let result = client.try_release_reserved(&i128::MAX);
        assert!(result.is_err());
    });
}

#[test]
fn test_negative_amount_rejection() {
    with_client(|client| {
        let result = client.try_deposit(&(-1));
        assert!(result.is_err());
    });
}

#[test]
fn test_zero_amount_operations() {
    with_client(|client| {
        let result = client.try_deposit(&0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), 0);
    });
}

#[test]
fn test_near_max_balance_operations() {
    with_client(|client| {
        client.deposit(&(i128::MAX - 1));
        let state = client.get_state();
        assert_eq!(state.total_balance, i128::MAX - 1);
    });
}

#[test]
fn test_state_consistency_after_operations() {
    with_client(|client| {
        client.deposit(&100);
        client.reserve(&30);
        let state = client.get_state();
        assert_eq!(state.total_balance, 100);
        assert_eq!(state.reserved, 30);
        assert_eq!(state.available, 70);
    });
}
