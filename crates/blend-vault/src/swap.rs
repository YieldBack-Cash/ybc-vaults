use soroban_sdk::{contractclient, panic_with_error, token::TokenClient, Address, Env, Vec};
use vault_common::auth::authorize_transfer_as_current;

use crate::errors::BlendVaultError;

/// How long the router may take to execute the swap. Execution is within the
/// same ledger; the slack is for simulation.
const SWAP_DEADLINE_SECS: u64 = 300;

/// The two Soroswap router functions the harvest uses. The router moves the
/// input itself, with `transfer(to, pair, amount)` inside the swap, after
/// `to.require_auth()`; `router_pair_for` names the pair that transfer goes
/// to, which the authorisation below has to spell out.
#[contractclient(name = "SoroswapRouterClient")]
pub trait SoroswapRouter {
    fn router_pair_for(e: Env, token_a: Address, token_b: Address) -> Address;
    fn swap_exact_tokens_for_tokens(
        e: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Vec<i128>;
}

/// Swaps `amount_in` of the first token on `path` for the last, `asset`,
/// via the Soroswap router. Returns the amount of `asset` that arrived,
/// which is at least `amount_out_min`. The caller has checked the path.
///
/// Neither the router's return value nor its own `amount_out_min` check is
/// trusted: the asset balance is read either side of the call and the floor
/// checked against the difference.
///
/// The BLND leaves by a `transfer` the router makes from this contract to the
/// first pair after `to.require_auth()`, which the vault authorises with
/// `authorize_transfer_as_current` immediately before the swap.
pub fn swap_blnd_for_asset(
    e: &Env,
    router: &Address,
    path: &Vec<Address>,
    asset: &Address,
    amount_in: i128,
    amount_out_min: i128,
) -> i128 {
    let vault = e.current_contract_address();
    let router_client = SoroswapRouterClient::new(e, router);
    // The router moves the input into the first pair on the path; later hops
    // move pair to pair and need nothing from this contract.
    let blnd = path.get(0).unwrap();
    let pair = router_client.router_pair_for(&blnd, &path.get(1).unwrap());

    let deadline = e.ledger().timestamp() + SWAP_DEADLINE_SECS;

    let asset_client = TokenClient::new(e, asset);
    let balance_before = asset_client.balance(&vault);

    // Nothing may sit between this and the swap call.
    authorize_transfer_as_current(e, &blnd, &vault, &pair, amount_in);
    router_client.swap_exact_tokens_for_tokens(
        &amount_in,
        &amount_out_min,
        path,
        &vault,
        &deadline,
    );

    let received = asset_client.balance(&vault) - balance_before;
    if received <= 0 {
        panic_with_error!(e, BlendVaultError::SwapNoOutput);
    }
    if received < amount_out_min {
        panic_with_error!(e, BlendVaultError::SwapBelowMinimum);
    }
    received
}
