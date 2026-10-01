//! The SEP-56 interface, declared once.
//!
//! Every adapter exposes the same seventeen functions. Declaring them here
//! does three things: the compiler refuses an adapter that is missing one or
//! has drifted a signature ([`impl_sep56!`]), the five views the standard
//! fully determines for a fee-less vault are written once instead of per
//! adapter, and the conformance suite's client is generated from this trait
//! rather than from a third copy.

use soroban_sdk::{contractclient, Address, Env};

/// The full SEP-56 interface, exactly as the standard declares it.
///
/// Adapters do not implement this trait by hand: [`impl_sep56!`] implements
/// it by forwarding to the adapter's inherent functions, so a missing or
/// misshaped function is a compile error at the adapter, and the client the
/// attribute generates (`Sep56Client`) is what the conformance suite calls.
#[contractclient(name = "Sep56Client")]
pub trait Sep56Vault {
    fn total_supply(e: &Env) -> i128;
    fn query_asset(e: &Env) -> Address;
    fn total_assets(e: &Env) -> i128;
    fn convert_to_shares(e: &Env, assets: i128) -> i128;
    fn convert_to_assets(e: &Env, shares: i128) -> i128;
    fn max_deposit(e: &Env, receiver: Address) -> i128;
    fn preview_deposit(e: &Env, assets: i128) -> i128;
    fn deposit(e: &Env, assets: i128, receiver: Address, from: Address, operator: Address) -> i128;
    fn max_mint(e: &Env, receiver: Address) -> i128;
    fn preview_mint(e: &Env, shares: i128) -> i128;
    fn mint(e: &Env, shares: i128, receiver: Address, from: Address, operator: Address) -> i128;
    fn max_withdraw(e: &Env, owner: Address) -> i128;
    fn preview_withdraw(e: &Env, assets: i128) -> i128;
    fn withdraw(
        e: &Env,
        assets: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128;
    fn max_redeem(e: &Env, owner: Address) -> i128;
    fn preview_redeem(e: &Env, shares: i128) -> i128;
    fn redeem(e: &Env, shares: i128, receiver: Address, owner: Address, operator: Address) -> i128;
}

/// Completes and checks an adapter's SEP-56 surface.
///
/// The adapter writes the eleven SEP-56 functions that depend on its
/// protocol (`query_asset`, `total_assets`, the two conversions,
/// `max_deposit`, `preview_mint`, `preview_withdraw`, and the four flows)
/// plus `withdraw_limit` (below). This macro then:
///
/// 1. **emits the five views the standard fully determines** for a vault
///    with no fees, in a second `#[contractimpl]` block on the adapter:
///    `preview_deposit` is `convert_to_shares`, `preview_redeem` is
///    `convert_to_assets`, `max_redeem` is the owner's share balance and
///    `max_withdraw` its value, both capped by the adapter's
///    `withdraw_limit`, and `max_mint` is `max_deposit` converted. An
///    adapter that ever charged a fee would write these itself instead of
///    invoking the macro;
/// 2. **implements [`Sep56Vault`]** for the adapter by forwarding every
///    function to the inherent one, so the build fails at the adapter the
///    moment a function is missing or its signature drifts from the standard.
///
/// Requires [`impl_share_token!`](crate::impl_share_token) on the same
/// contract, for `total_supply` and the balance reads.
///
/// ```ignore
/// #[contract]
/// pub struct MyVault;
///
/// vault_common::impl_share_token!(MyVault);
/// vault_common::impl_sep56!(MyVault);
/// ```
///
/// Besides the eleven SEP-56 functions it forwards to, the adapter must
/// define `withdraw_limit(e) -> i128`: the most underlying the protocol
/// could pay out to anyone right now (its free liquidity, or zero while it
/// is halted). `max_withdraw` and `max_redeem` are capped by it. It is a
/// snapshot of liquidity shared with every other supplier, not a
/// reservation: another exit can consume it first, so it sizes a request
/// and guarantees nothing.
#[macro_export]
macro_rules! impl_sep56 {
    ($contract:ident) => {
        mod __vault_common_sep56 {
            use super::*;
            use $crate::soroban_sdk::{contractimpl, Address, Env};
            use $crate::stellar_tokens::fungible::{Base, FungibleToken};

            // ── the views SEP-56 determines for a fee-less vault ────────────
            #[contractimpl]
            impl $contract {
                /// Shares a deposit of `assets` would mint, floored. No fees, so
                /// this is `convert_to_shares`; `deposit` mints this or more.
                pub fn preview_deposit(e: &Env, assets: i128) -> i128 {
                    Self::convert_to_shares(e, assets)
                }

                /// Assets a redeem of `shares` would pay, floored. No fees, so
                /// this is `convert_to_assets`, and `redeem` pays exactly this.
                pub fn preview_redeem(e: &Env, shares: i128) -> i128 {
                    Self::convert_to_assets(e, shares)
                }

                /// The owner's shares, capped at the shares whose value the
                /// protocol can pay out right now (`withdraw_limit`). Under the
                /// cap, redeeming this many pays no more than the limit.
                pub fn max_redeem(e: &Env, owner: Address) -> i128 {
                    let balance = Base::balance(e, &owner);
                    let limit = Self::withdraw_limit(e);
                    if Self::convert_to_assets(e, balance) <= limit {
                        balance
                    } else {
                        Self::convert_to_shares(e, limit).min(balance)
                    }
                }

                /// What `owner` could redeem right now, in assets: the value of
                /// their shares, capped by what the protocol can pay out.
                pub fn max_withdraw(e: &Env, owner: Address) -> i128 {
                    Self::convert_to_assets(e, Base::balance(e, &owner))
                        .min(Self::withdraw_limit(e))
                }

                /// `max_deposit` in shares, floored.
                pub fn max_mint(e: &Env, receiver: Address) -> i128 {
                    Self::convert_to_shares(e, Self::max_deposit(e, receiver))
                }
            }

            // ── the compile-time conformance check ──────────────────────────
            //
            // The forwarding functions live at module level, where `Sep56Vault`
            // is not in scope, so each `<$contract>::name` path can only resolve
            // to an inherent function: a missing or misshaped one is a compile
            // error here. (Inside the trait impl itself the trait *is* in
            // scope, and the same path would silently resolve to the trait
            // method being defined.)
            fn fwd_query_asset(e: &Env) -> Address {
                <$contract>::query_asset(e)
            }
            fn fwd_total_assets(e: &Env) -> i128 {
                <$contract>::total_assets(e)
            }
            fn fwd_convert_to_shares(e: &Env, assets: i128) -> i128 {
                <$contract>::convert_to_shares(e, assets)
            }
            fn fwd_convert_to_assets(e: &Env, shares: i128) -> i128 {
                <$contract>::convert_to_assets(e, shares)
            }
            fn fwd_max_deposit(e: &Env, receiver: Address) -> i128 {
                <$contract>::max_deposit(e, receiver)
            }
            fn fwd_preview_deposit(e: &Env, assets: i128) -> i128 {
                <$contract>::preview_deposit(e, assets)
            }
            fn fwd_deposit(
                e: &Env,
                assets: i128,
                receiver: Address,
                from: Address,
                operator: Address,
            ) -> i128 {
                <$contract>::deposit(e, assets, receiver, from, operator)
            }
            fn fwd_max_mint(e: &Env, receiver: Address) -> i128 {
                <$contract>::max_mint(e, receiver)
            }
            fn fwd_preview_mint(e: &Env, shares: i128) -> i128 {
                <$contract>::preview_mint(e, shares)
            }
            fn fwd_mint(
                e: &Env,
                shares: i128,
                receiver: Address,
                from: Address,
                operator: Address,
            ) -> i128 {
                <$contract>::mint(e, shares, receiver, from, operator)
            }
            fn fwd_max_withdraw(e: &Env, owner: Address) -> i128 {
                <$contract>::max_withdraw(e, owner)
            }
            fn fwd_preview_withdraw(e: &Env, assets: i128) -> i128 {
                <$contract>::preview_withdraw(e, assets)
            }
            fn fwd_withdraw(
                e: &Env,
                assets: i128,
                receiver: Address,
                owner: Address,
                operator: Address,
            ) -> i128 {
                <$contract>::withdraw(e, assets, receiver, owner, operator)
            }
            fn fwd_max_redeem(e: &Env, owner: Address) -> i128 {
                <$contract>::max_redeem(e, owner)
            }
            fn fwd_preview_redeem(e: &Env, shares: i128) -> i128 {
                <$contract>::preview_redeem(e, shares)
            }
            fn fwd_redeem(
                e: &Env,
                shares: i128,
                receiver: Address,
                owner: Address,
                operator: Address,
            ) -> i128 {
                <$contract>::redeem(e, shares, receiver, owner, operator)
            }

            impl $crate::sep56::Sep56Vault for $contract {
                fn total_supply(e: &Env) -> i128 {
                    <$contract as FungibleToken>::total_supply(e)
                }
                fn query_asset(e: &Env) -> Address {
                    fwd_query_asset(e)
                }
                fn total_assets(e: &Env) -> i128 {
                    fwd_total_assets(e)
                }
                fn convert_to_shares(e: &Env, assets: i128) -> i128 {
                    fwd_convert_to_shares(e, assets)
                }
                fn convert_to_assets(e: &Env, shares: i128) -> i128 {
                    fwd_convert_to_assets(e, shares)
                }
                fn max_deposit(e: &Env, receiver: Address) -> i128 {
                    fwd_max_deposit(e, receiver)
                }
                fn preview_deposit(e: &Env, assets: i128) -> i128 {
                    fwd_preview_deposit(e, assets)
                }
                fn deposit(
                    e: &Env,
                    assets: i128,
                    receiver: Address,
                    from: Address,
                    operator: Address,
                ) -> i128 {
                    fwd_deposit(e, assets, receiver, from, operator)
                }
                fn max_mint(e: &Env, receiver: Address) -> i128 {
                    fwd_max_mint(e, receiver)
                }
                fn preview_mint(e: &Env, shares: i128) -> i128 {
                    fwd_preview_mint(e, shares)
                }
                fn mint(
                    e: &Env,
                    shares: i128,
                    receiver: Address,
                    from: Address,
                    operator: Address,
                ) -> i128 {
                    fwd_mint(e, shares, receiver, from, operator)
                }
                fn max_withdraw(e: &Env, owner: Address) -> i128 {
                    fwd_max_withdraw(e, owner)
                }
                fn preview_withdraw(e: &Env, assets: i128) -> i128 {
                    fwd_preview_withdraw(e, assets)
                }
                fn withdraw(
                    e: &Env,
                    assets: i128,
                    receiver: Address,
                    owner: Address,
                    operator: Address,
                ) -> i128 {
                    fwd_withdraw(e, assets, receiver, owner, operator)
                }
                fn max_redeem(e: &Env, owner: Address) -> i128 {
                    fwd_max_redeem(e, owner)
                }
                fn preview_redeem(e: &Env, shares: i128) -> i128 {
                    fwd_preview_redeem(e, shares)
                }
                fn redeem(
                    e: &Env,
                    shares: i128,
                    receiver: Address,
                    owner: Address,
                    operator: Address,
                ) -> i128 {
                    fwd_redeem(e, shares, receiver, owner, operator)
                }
            }
        }
    };
}
