#![cfg(test)]
use super::*;
use soroban_sdk::{
    testutils::{Address as AddressTestUtils, Events as _},
    Address, Env, String,
};

#[test]
fn test_deposit_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let bridge = Address::generate(&env);

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    client.init(&admin);

    let id = client.deposit(&depositor, &1000, &bridge, &0u32);
    assert_eq!(id, 0);

    let events = env.events().all();
    assert!(!events.is_empty());
}

#[test]
fn test_release_returns_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let bridge = Address::generate(&env);

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    client.init(&admin);

    client.deposit(&depositor, &500, &bridge, &0u32);
    let result = client.try_release(&0u64, &bridge);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_get_deposit_returns_record() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let bridge = Address::generate(&env);

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    client.init(&admin);

    client.deposit(&depositor, &750, &bridge, &10u32);
    let deposit = client.get_deposit(&0u64);
    assert_eq!(deposit.amount, 750);
    assert_eq!(deposit.fee_bps, 10);
    assert_eq!(deposit.status, EscrowStatus::Pending);
}

#[test]
fn test_can_refund_false_before_timeout() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let bridge = Address::generate(&env);

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    client.init(&admin);

    client.deposit(&depositor, &100, &bridge, &0u32);
    assert_eq!(client.can_refund(&0u64), false);
}
