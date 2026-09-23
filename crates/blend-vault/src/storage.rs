use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, Address, Env, Symbol};
use vault_common::VaultError;

use crate::vault::VaultData;

// Share balances and allowances are OpenZeppelin's (`stellar_tokens::fungible::Base`)
// and never appear here.

//---------- Storage Keys ----------//

const POOL_KEY: &str = "Pool";
const ADMIN_KEY: &str = "Admin";
const ASSET_KEY: &str = "Asset";
const VAULT_DATA_KEY: &str = "Vault";
const ROUTER_KEY: &str = "Router";
const BLND_TOKEN_KEY: &str = "BlndToken";

//---------- TTL ----------//

pub use vault_common::ttl::DAY_IN_LEDGERS as ONE_DAY_LEDGERS;

const LEDGER_BUMP_VAULT: u32 = 120 * ONE_DAY_LEDGERS;
const LEDGER_THRESHOLD_VAULT: u32 = LEDGER_BUMP_VAULT - 20 * ONE_DAY_LEDGERS;

//---------- Instance ----------//

pub fn get_pool(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, POOL_KEY))
        .unwrap_optimized()
}

pub fn set_pool(e: &Env, pool: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, POOL_KEY), &pool);
}

pub fn get_admin(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, ADMIN_KEY))
        .unwrap_optimized()
}

pub fn set_admin(e: &Env, admin: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, ADMIN_KEY), &admin);
}

pub fn get_asset(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, ASSET_KEY))
        .unwrap_optimized()
}

pub fn set_asset(e: &Env, asset: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, ASSET_KEY), &asset);
}

/// The Soroswap router used to harvest BLND, or `None` until `set_router`.
pub fn get_router(e: &Env) -> Option<Address> {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, ROUTER_KEY))
}

pub fn set_router(e: &Env, router: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, ROUTER_KEY), &router);
}

pub fn get_blnd_token(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, BLND_TOKEN_KEY))
        .unwrap_optimized()
}

pub fn set_blnd_token(e: &Env, blnd_token: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, BLND_TOKEN_KEY), &blnd_token);
}

//---------- Persistent ----------//
// Persistent data is not bumped on read: the entry is almost always written
// when accessed, so bumping on write is sufficient.

pub fn set_vault_data(e: &Env, vault: &VaultData) {
    let key = Symbol::new(e, VAULT_DATA_KEY);
    e.storage()
        .persistent()
        .set::<Symbol, VaultData>(&key, vault);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_VAULT, LEDGER_BUMP_VAULT);
}

pub fn get_vault_data(e: &Env) -> VaultData {
    let key = Symbol::new(e, VAULT_DATA_KEY);
    e.storage()
        .persistent()
        .get::<Symbol, VaultData>(&key)
        .unwrap_or_else(|| panic_with_error!(e, VaultError::NotInitialized))
}
