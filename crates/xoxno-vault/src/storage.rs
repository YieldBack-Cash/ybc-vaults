use soroban_sdk::{contracttype, panic_with_error, Address, Env};
use vault_common::VaultError;

/// Set once at construction and never mutated.
///
/// `hub_id` and `spoke_id` are constructor parameters rather than constants on
/// purpose: an account's spoke binding is permanent, so baking a choice into the
/// WASM would make the deployment decision unrecoverable without a new binary.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub controller: Address,
    /// Resolved once from `get_pool_address()`. Exists only so the vault can
    /// pre-authorize the pool pulling tokens out of it during a supply.
    pub pool: Address,
    pub asset: Address,
    /// Authorized for `sweep` and nothing else. It cannot touch the underlying
    /// asset, the share token, or the lending position.
    pub admin: Address,
    pub hub_id: u32,
    pub spoke_id: u32,
}

#[contracttype]
pub enum DataKey {
    Config,
    AccountId,
}

pub fn has_config(e: &Env) -> bool {
    e.storage().instance().has(&DataKey::Config)
}

pub fn set_config(e: &Env, config: &Config) {
    e.storage().instance().set(&DataKey::Config, config);
}

pub fn get_config(e: &Env) -> Config {
    e.storage()
        .instance()
        .get(&DataKey::Config)
        .unwrap_or_else(|| panic_with_error!(e, VaultError::NotInitialized))
}

/// The vault's XOXNO account, or `0` if it has not opened one yet.
///
/// `0` is also XOXNO's "create me an account" sentinel on `supply`, so the same
/// value means "none yet" in both directions and needs no separate flag.
pub fn get_account_id(e: &Env) -> u64 {
    e.storage().instance().get(&DataKey::AccountId).unwrap_or(0)
}

/// Written once, on the first successful supply, and never cleared.
///
/// XOXNO's own reference adapter clears this mapping when the controller reports
/// the account as gone. That is deliberately not ported: the id is the only
/// route back to the collateral, nothing can re-point it, and the vault has no
/// reason to ever abandon its account.
pub fn set_account_id(e: &Env, account_id: u64) {
    e.storage().instance().set(&DataKey::AccountId, &account_id);
}
