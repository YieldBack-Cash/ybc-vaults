use soroban_sdk::{contractclient, panic_with_error, token::TokenClient, vec, Address, Env, Vec};

use crate::errors::BlendVaultError;

#[contractclient(name = "SoroswapRouterClient")]
pub trait SoroswapRouter {
    fn swap_exact_tokens_for_tokens(
        e: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Vec<i128>;
}

/// Swaps `amount_in` of `blnd` for `asset` via the Soroswap router.
/// Returns the amount of `asset` that arrived, which is at least
/// `amount_out_min`.
///
/// Both the amount and the floor are this contract's to establish. The
/// router is an address the admin set and the floor is passed to it, but
/// what it reports back and whether it honoured the floor are its word
/// alone: a router that kept the BLND, paid a few stroops and answered with
/// any figure it liked used to be believed. The asset balance is read either
/// side of the call and the floor is checked against that.
pub fn swap_blnd_for_asset(
    e: &Env,
    router: &Address,
    blnd: &Address,
    asset: &Address,
    amount_in: i128,
    amount_out_min: i128,
) -> i128 {
    let vault = e.current_contract_address();
    TokenClient::new(e, blnd).approve(
        &vault,
        router,
        &amount_in,
        &(e.ledger().sequence() + 1),
    );

    let path = vec![e, blnd.clone(), asset.clone()];
    let deadline = e.ledger().timestamp() + 300;

    let asset_client = TokenClient::new(e, asset);
    let balance_before = asset_client.balance(&vault);

    SoroswapRouterClient::new(e, router).swap_exact_tokens_for_tokens(
        &amount_in,
        &amount_out_min,
        &path,
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
