//! Admin operations and circuit breaker for the fee-manager contract (issue #984).

use soroban_sdk::{symbol_short, Address, BytesN, Env, String};
use stellar_spend_shared::{
    errors::ContractError,
    validation::{check_schema_version, require_basis_points, require_string_len, MAX_BASIS_POINTS},
};

use crate::{DataKey, MAX_DEFAULT_FEE_BP, SCHEMA_VERSION};

/// Admin operations handler
pub struct Admin;

/// Per-invocation cache of instance storage reads.
///
/// Soroban meters every `storage().instance().get()` call as a resource cost.
/// Several entrypoints (`pause`, `unpause`, `set_default_rate`) previously
/// re-fetched `Schema` and `Admin` through separate helper calls
/// (`require_current_schema` then `require_admin`), each independently
/// touching the storage instance. This cache fetches a key at most once per
/// invocation and hands back the cached value to any subsequent helper that
/// needs it within the same call, avoiding repeat host-storage round trips.
struct InvocationCache {
    schema: Option<u32>,
    admin: Option<Address>,
    paused: Option<bool>,
}

impl InvocationCache {
    fn new() -> Self {
        Self {
            schema: None,
            admin: None,
            paused: None,
        }
    }

    fn schema(&mut self, env: &Env) -> Result<u32, ContractError> {
        if let Some(v) = self.schema {
            return Ok(v);
        }
        let v: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Schema)
            .ok_or(ContractError::NotInitialized)?;
        self.schema = Some(v);
        Ok(v)
    }

    fn admin(&mut self, env: &Env) -> Result<Address, ContractError> {
        if let Some(v) = self.admin.clone() {
            return Ok(v);
        }
        let v: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        self.admin = Some(v.clone());
        Ok(v)
    }

    fn paused(&mut self, env: &Env) -> bool {
        if let Some(v) = self.paused {
            return v;
        }
        let v = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        self.paused = Some(v);
        v
    }

    /// Validate schema and admin auth in one pass, reusing cached reads.
    fn require_current_schema_and_admin(&mut self, env: &Env) -> Result<(), ContractError> {
        check_schema_version(Some(self.schema(env)?), SCHEMA_VERSION)?;
        let admin = self.admin(env)?;
        admin.require_auth();
        Ok(())
    }
}

impl Admin {
    /// Initialise with an admin and a starting fee rate.
    pub fn init(env: Env, admin: Address, default_fee_bp: u32) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Schema) {
            return Err(ContractError::AlreadyInitialized);
        }
        require_basis_points(default_fee_bp, MAX_DEFAULT_FEE_BP)?;
        admin.require_auth();

        let storage = env.storage().instance();
        storage.set(&DataKey::Admin, &admin);
        storage.set(&DataKey::Paused, &false);
        storage.set(&DataKey::DefaultRate, &default_fee_bp);
        storage.set(&DataKey::Schema, &SCHEMA_VERSION);
        Self::bump_instance_ttl(&env);

        env.events().publish((symbol_short!("init"),), admin);
        Ok(())
    }

    /// Trip the circuit breaker. Admin only.
    ///
    /// `reason` is bounded because it is echoed into an event topic, and unbounded
    /// caller-supplied strings there are a metering hazard.
    pub fn pause(env: Env, reason: String) -> Result<(), ContractError> {
        let mut cache = InvocationCache::new();
        cache.require_current_schema_and_admin(&env)?;
        require_string_len(&reason, 128)?;

        if cache.paused(&env) {
            return Err(ContractError::Paused);
        }

        env.storage().instance().set(&DataKey::Paused, &true);
        Self::bump_instance_ttl(&env);
        env.events().publish((symbol_short!("pause"),), reason);
        Ok(())
    }

    /// Reset the circuit breaker. Admin only.
    pub fn unpause(env: Env) -> Result<(), ContractError> {
        let mut cache = InvocationCache::new();
        cache.require_current_schema_and_admin(&env)?;

        if !cache.paused(&env) {
            return Err(ContractError::InvalidInput);
        }

        env.storage().instance().set(&DataKey::Paused, &false);
        Self::bump_instance_ttl(&env);
        env.events().publish((symbol_short!("unpause"),), ());
        Ok(())
    }

    /// Whether the circuit breaker is currently tripped.
    ///
    /// Returns `false` for an uninitialised contract rather than erroring: callers
    /// use this as a cheap guard and a missing contract is not "paused".
    pub fn is_paused(env: Env) -> bool {
        Self::paused_flag(&env)
    }

    /// The configured default fee rate, in basis points.
    pub fn default_rate(env: Env) -> Result<u32, ContractError> {
        Self::require_current_schema(&env)?;
        env.storage()
            .instance()
            .get(&DataKey::DefaultRate)
            .ok_or(ContractError::NotInitialized)
    }

    /// Update the default fee rate. Admin only.
    pub fn set_default_rate(env: Env, fee_bp: u32) -> Result<(), ContractError> {
        let mut cache = InvocationCache::new();
        cache.require_current_schema_and_admin(&env)?;
        require_basis_points(fee_bp, MAX_DEFAULT_FEE_BP)?;

        env.storage().instance().set(&DataKey::DefaultRate, &fee_bp);
        Self::bump_instance_ttl(&env);
        env.events().publish((symbol_short!("rate"),), fee_bp);
        Ok(())
    }

    // ── Upgrade surface (issue #817) ──────────────────────────────────────────

    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Schema)
            .ok_or(ContractError::NotInitialized)
    }

    /// Replace the contract WASM. Admin only. Run `migrate` immediately after.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        Self::require_admin(&env)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.events().publish((symbol_short!("upgrade"),), ());
        Ok(())
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
            // v1 had no DefaultRate key at all. Backfill it rather than leaving
            // `default_rate` to fail on every call after the WASM swap.
            env.storage()
                .instance()
                .set(&DataKey::DefaultRate, &crate::MIGRATED_DEFAULT_FEE_BP);
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

    fn require_admin(env: &Env) -> Result<Address, ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        admin.require_auth();
        Ok(admin)
    }

    fn require_current_schema(env: &Env) -> Result<(), ContractError> {
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

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(crate::INSTANCE_TTL_THRESHOLD, crate::INSTANCE_TTL_EXTEND_TO);
    }
}