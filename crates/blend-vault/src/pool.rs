//! The vault's calls into its Blend pool.

use crate::blend::pool::{Client as PoolClient, Request, Reserve};
use crate::constants::SCALAR_12;
use soroban_sdk::{vec, Address, Env, Vec};
use vault_common::math::mul_div_floor;

/// Pool status at or above which supplying is refused (Blend's "frozen").
pub const STATUS_FROZEN: u32 = 4;

/// Underlying the reserve's suppliers have put in, at the current bRate,
/// floored.
pub fn reserve_supplied(e: &Env, reserve: &Reserve) -> i128 {
    mul_div_floor(e, reserve.data.b_supply, reserve.data.b_rate, SCALAR_12)
}

/// Supplies `amount` of `reserve` into the pool on behalf of the vault. The
/// tokens are pulled from `from`, who must have authorized the transfer.
pub fn supply(e: &Env, pool: &Address, reserve: &Address, from: &Address, amount: i128) {
    PoolClient::new(e, pool).submit(
        &e.current_contract_address(),
        from,
        from,
        &vec![
            e,
            Request {
                address: reserve.clone(),
                amount,
                request_type: 0,
            },
        ],
    );
}

/// Withdraws `amount` of `reserve` from the vault's position to `to`.
pub fn withdraw(e: &Env, pool: &Address, reserve: &Address, to: &Address, amount: i128) {
    PoolClient::new(e, pool).submit(
        &e.current_contract_address(),
        &e.current_contract_address(),
        to,
        &vec![
            e,
            Request {
                address: reserve.clone(),
                amount,
                request_type: 1,
            },
        ],
    );
}

/// Claims BLND emissions for `reserve_token_ids` to `to`. Returns the amount.
pub fn claim(e: &Env, pool: &Address, reserve_token_ids: &Vec<u32>, to: &Address) -> i128 {
    PoolClient::new(e, pool).claim(&e.current_contract_address(), reserve_token_ids, to)
}

/// The reserve's full record: config (cap, enabled, index) and data (`b_rate`,
/// `b_supply`).
pub fn reserve(e: &Env, pool: &Address, asset: &Address) -> Reserve {
    PoolClient::new(e, pool).get_reserve(asset)
}

/// The reserve's `b_rate`, 12-decimal fixed point.
pub fn reserve_b_rate(e: &Env, pool: &Address, asset: &Address) -> i128 {
    reserve(e, pool, asset).data.b_rate
}

/// The pool's status word. Blend allows supplies while it is below 4; 4 and 5
/// are frozen, 6 is setup.
pub fn status(e: &Env, pool: &Address) -> u32 {
    PoolClient::new(e, pool).get_config().status
}

/// The emission token id for the reserve's supply (bToken) side:
/// `reserve_index * 2 + 1`.
pub fn reserve_supply_token_id(e: &Env, pool: &Address, reserve: &Address) -> u32 {
    PoolClient::new(e, pool).get_reserve(reserve).config.index * 2 + 1
}

/// The vault's actual bToken balance for a reserve, read from the pool.
pub fn vault_b_token_balance(e: &Env, pool: &Address, reserve: &Address, vault: &Address) -> i128 {
    let reserve_index = PoolClient::new(e, pool).get_reserve(reserve).config.index;
    PoolClient::new(e, pool)
        .get_positions(vault)
        .supply
        .get(reserve_index)
        .unwrap_or(0)
}
