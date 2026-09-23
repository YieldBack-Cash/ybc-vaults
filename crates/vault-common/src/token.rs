/// Implements SEP-41 for an adapter contract by delegating every function to
/// OpenZeppelin's `Base`.
///
/// Required on the same address as the SEP-56 surface: YBC custodies these
/// shares in its yield manager and holds them as an AMM reserve.
///
/// `Base`, not OpenZeppelin's `Vault`: `Vault` derives `total_assets` from the
/// contract's own token balance, which is always zero for an adapter whose
/// assets live in a lending position, and it exposes no hook to override that.
/// The adapter supplies the rate; `Base` supplies the ledger.
///
/// ```ignore
/// #[contract]
/// pub struct MyVault;
///
/// vault_common::impl_share_token!(MyVault);
/// ```
///
/// Set metadata in the constructor with
/// `stellar_tokens::fungible::Base::set_metadata(e, 7, name, symbol)`; every
/// YBC consumer assumes 7 decimals.
///
/// The expansion is wrapped in a private module so the trait can be named by
/// its bare identifier: `#[contractimpl]` derives export symbols from the
/// trait's path, and a `$crate::...` path is not a valid identifier.
#[macro_export]
macro_rules! impl_share_token {
    ($contract:ident) => {
        mod __vault_common_share_token {
            use super::*;
            use $crate::soroban_sdk::{contractimpl, Address, Env, MuxedAddress, String};
            use $crate::stellar_tokens::fungible::{Base, FungibleToken};

            #[contractimpl]
            impl FungibleToken for $contract {
                type ContractType = Base;

                fn total_supply(e: &Env) -> i128 {
                    Base::total_supply(e)
                }

                fn balance(e: &Env, account: Address) -> i128 {
                    Base::balance(e, &account)
                }

                fn allowance(e: &Env, owner: Address, spender: Address) -> i128 {
                    Base::allowance(e, &owner, &spender)
                }

                fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
                    Base::transfer(e, &from, &to, amount)
                }

                fn transfer_from(
                    e: &Env,
                    spender: Address,
                    from: Address,
                    to: Address,
                    amount: i128,
                ) {
                    Base::transfer_from(e, &spender, &from, &to, amount)
                }

                fn approve(
                    e: &Env,
                    owner: Address,
                    spender: Address,
                    amount: i128,
                    live_until_ledger: u32,
                ) {
                    Base::approve(e, &owner, &spender, amount, live_until_ledger)
                }

                fn decimals(e: &Env) -> u32 {
                    Base::decimals(e)
                }

                fn name(e: &Env) -> String {
                    Base::name(e)
                }

                fn symbol(e: &Env) -> String {
                    Base::symbol(e)
                }
            }
        }
    };
}
