use soroban_sdk::{contracttype, panic_with_error, Address, Env, IntoVal, TryFromVal, Val};
use vault_common::VaultError;

use crate::vault::VaultData;

// Share balances and allowances are OpenZeppelin's (`stellar_tokens::fungible::Base`)
// and never appear here.

/// Every key this contract writes. Typed, so a key is a variant rather than a
/// string built at each call, and a missing required key always raises the
/// same typed `NotInitialized` rather than an opaque trap.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The Blend pool supplied into (instance).
    Pool,
    /// The reserve asset (instance).
    Asset,
    /// Authorized for `set_admin` and `set_router` (instance).
    Admin,
    /// The BLND token, for emissions harvesting (instance).
    BlndToken,
    /// The Soroswap router for harvesting; absent until `set_router` (instance).
    Router,
    /// The share/bToken ratio state (persistent).
    Vault,
}

//---------- TTL ----------//

use vault_common::ttl::DAY_IN_LEDGERS;

// The vault's totals are a single persistent entry read on every call, so it
// is bumped on a longer cycle than the instance: 120 days ahead once fewer
// than 100 remain.
const LEDGER_BUMP_VAULT: u32 = 120 * DAY_IN_LEDGERS;
const LEDGER_THRESHOLD_VAULT: u32 = LEDGER_BUMP_VAULT - 20 * DAY_IN_LEDGERS;

//---------- Instance ----------//

/// A required instance value, or `NotInitialized`.
fn required<T: TryFromVal<Env, Val>>(e: &Env, key: DataKey) -> T {
    e.storage()
        .instance()
        .get(&key)
        .unwrap_or_else(|| panic_with_error!(e, VaultError::NotInitialized))
}

fn set_instance<T: IntoVal<Env, Val>>(e: &Env, key: DataKey, value: &T) {
    e.storage().instance().set(&key, value);
}

pub fn get_pool(e: &Env) -> Address {
    required(e, DataKey::Pool)
}

pub fn set_pool(e: &Env, pool: &Address) {
    set_instance(e, DataKey::Pool, pool);
}

pub fn get_asset(e: &Env) -> Address {
    required(e, DataKey::Asset)
}

pub fn set_asset(e: &Env, asset: &Address) {
    set_instance(e, DataKey::Asset, asset);
}

pub fn get_admin(e: &Env) -> Address {
    required(e, DataKey::Admin)
}

pub fn set_admin(e: &Env, admin: &Address) {
    set_instance(e, DataKey::Admin, admin);
}

pub fn get_blnd_token(e: &Env) -> Address {
    required(e, DataKey::BlndToken)
}

pub fn set_blnd_token(e: &Env, blnd_token: &Address) {
    set_instance(e, DataKey::BlndToken, blnd_token);
}

/// The Soroswap router used to harvest BLND, or `None` until `set_router`.
pub fn get_router(e: &Env) -> Option<Address> {
    e.storage().instance().get(&DataKey::Router)
}

pub fn set_router(e: &Env, router: &Address) {
    set_instance(e, DataKey::Router, router);
}

//---------- Persistent ----------//
// Persistent data is not bumped on read: the entry is almost always written
// when accessed, so bumping on write is sufficient.

pub fn set_vault_data(e: &Env, vault: &VaultData) {
    let key = DataKey::Vault;
    e.storage().persistent().set(&key, vault);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_VAULT, LEDGER_BUMP_VAULT);
}

pub fn get_vault_data(e: &Env) -> VaultData {
    e.storage()
        .persistent()
        .get(&DataKey::Vault)
        .unwrap_or_else(|| panic_with_error!(e, VaultError::NotInitialized))
}
