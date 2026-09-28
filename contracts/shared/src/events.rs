use soroban_sdk::{Address, Env, Vec, String, symbol_short};

/// Emit an admin initialized event
pub fn emit_admin_initialized(env: &Env, admin: Address) {
    env.events()
        .publish((symbol_short!("admin_ini"), "v1"), (admin, env.ledger().timestamp()));
}

pub fn emit_escrow_created(env: &Env, escrow_id: u64, buyer: Address, seller: Address, amount: i128) {
    env.events()
        .publish((symbol_short!("escr_crt"), "v1"), (escrow_id, buyer, seller, amount, env.ledger().timestamp()));
}

pub fn emit_escrow_funded(env: &Env, escrow_id: u64, funder: Address, amount: i128) {
    env.events()
        .publish((symbol_short!("escr_fnd"), "v1"), (escrow_id, funder, amount, env.ledger().timestamp()));
}

pub fn emit_escrow_released(env: &Env, escrow_id: u64, recipient: Address, amount: i128) {
    env.events()
        .publish((symbol_short!("escr_rel"), "v1"), (escrow_id, recipient, amount, env.ledger().timestamp()));
}

pub fn emit_escrow_refunded(env: &Env, escrow_id: u64, recipient: Address, amount: i128) {
    env.events()
        .publish((symbol_short!("escr_rfd"), "v1"), (escrow_id, recipient, amount, env.ledger().timestamp()));
}

pub fn emit_dispute_created(env: &Env, escrow_id: u64, initiator: Address, respondent: Address, reason: String) {
    env.events()
        .publish((symbol_short!("dispute"), "v1"), (escrow_id, initiator, respondent, reason, env.ledger().timestamp()));
}

pub fn emit_dispute_resolved(env: &Env, escrow_id: u64, resolver: Address, outcome: String) {
    env.events()
        .publish((symbol_short!("dspute_rs"), "v1"), (escrow_id, resolver, outcome, env.ledger().timestamp()));
}

pub fn emit_fee_set(env: &Env, fee_type: String, fee_rate: i128) {
    env.events()
        .publish((symbol_short!("fee_set"), "v1"), (fee_type, fee_rate, env.ledger().timestamp()));
}

pub fn emit_fee_collected(env: &Env, fee_type: String, amount: i128, recipient: Address) {
    env.events()
        .publish((symbol_short!("fee_col"), "v1"), (fee_type, amount, recipient, env.ledger().timestamp()));
}

pub fn emit_multisig_submitted(env: &Env, proposal_id: u64, proposer: Address, description: String) {
    env.events()
        .publish((symbol_short!("msig_sub"), "v1"), (proposal_id, proposer, description, env.ledger().timestamp()));
}

pub fn emit_multisig_approved(env: &Env, proposal_id: u64, approver: Address) {
    env.events()
        .publish((symbol_short!("msig_app"), "v1"), (proposal_id, approver, env.ledger().timestamp()));
}

pub fn emit_multisig_executed(env: &Env, proposal_id: u64, executor: Address) {
    env.events()
        .publish((symbol_short!("msig_exec"), "v1"), (proposal_id, executor, env.ledger().timestamp()));
}

pub fn emit_treasury_deposit(env: &Env, depositor: Address, amount: i128, asset: String) {
    env.events()
        .publish((symbol_short!("trsy_dep"), "v1"), (depositor, amount, asset, env.ledger().timestamp()));
}

pub fn emit_treasury_withdrawal(env: &Env, recipient: Address, amount: i128, asset: String) {
    env.events()
        .publish((symbol_short!("trsy_wit"), "v1"), (recipient, amount, asset, env.ledger().timestamp()));
}

pub fn emit_contract_paused(env: &Env, caller: Address) {
    env.events()
        .publish((symbol_short!("cntr_pau"), "v1"), (caller, env.ledger().timestamp()));
}

pub fn emit_contract_unpaused(env: &Env, caller: Address) {
    env.events()
        .publish((symbol_short!("cntr_unp"), "v1"), (caller, env.ledger().timestamp()));
}

pub fn emit_contract_upgraded(env: &Env, new_wasm_hash: Vec<u8>) {
    env.events()
        .publish((symbol_short!("cntr_upg"), "v1"), (new_wasm_hash, env.ledger().timestamp()));
}

pub fn emit_error(env: &Env, error_code: u32, error_message: String, context: String) {
    env.events()
        .publish((symbol_short!("error_occ"), "v1"), (error_code, error_message, context, env.ledger().timestamp()));
}
