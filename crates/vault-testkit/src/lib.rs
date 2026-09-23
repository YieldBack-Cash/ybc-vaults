//! The conformance suite every vault adapter runs.
//!
//! Each property here is one YBC depends on and SEP-56 does not guarantee, or
//! one drawn straight from a finding in `ybc-contracts/docs/THREAT_MODEL.md`.
//! The suite is generic over a [`ConformanceFixture`], so a new adapter gets
//! all of it by implementing seven methods and invoking
//! [`conformance_tests!`] once:
//!
//! ```ignore
//! // in crates/<adapter>/src/tests/conformance.rs
//! vault_testkit::conformance_tests!(super::fixture::VaultFixture::new());
//! ```
//!
//! The suite talks to the adapter only through the SEP-56 and SEP-41 surfaces
//! YBC itself uses, plus `sweep`, so it cannot pass on an adapter-private
//! function YBC would never call.

use soroban_sdk::{
    contractclient, testutils::Address as _, token::TokenClient, xdr::ScErrorCode, Address, Env,
    Error, InvokeError,
};

pub use vault_common::VaultError;

/// What the suite needs from an adapter's test fixture.
pub trait ConformanceFixture {
    fn env(&self) -> &Env;

    /// The vault: SEP-56 and SEP-41 on one address.
    fn vault(&self) -> Address;

    /// The underlying asset's contract.
    fn asset(&self) -> Address;

    /// The address authorized for `sweep`.
    fn admin(&self) -> Address;

    /// Mints `amount` of the underlying to `to`.
    fn mint(&self, to: &Address, amount: i128);

    /// Makes the protocol accrue `bps` basis points of yield, funded so that
    /// redeeming the gain actually pays.
    fn accrue(&self, bps: i128);

    /// Makes the protocol lose `bps` of value. Returns `false` if the protocol
    /// cannot lose value, in which case the write-down property is skipped.
    fn write_down(&self, bps: i128) -> bool;
}

/// The SEP-56 subset YBC calls, plus `total_supply` and `sweep`.
#[contractclient(name = "VaultClient")]
pub trait Sep56Vault {
    fn query_asset(e: &Env) -> Address;
    fn convert_to_assets(e: &Env, shares: i128) -> i128;
    fn deposit(e: &Env, assets: i128, receiver: Address, from: Address, operator: Address) -> i128;
    fn redeem(e: &Env, shares: i128, receiver: Address, owner: Address, operator: Address) -> i128;
    fn total_supply(e: &Env) -> i128;
    fn sweep(e: &Env, token: Address, to: Address, amount: i128);
}

pub const ONE: i128 = 1_0000000;
pub const DEPOSIT: i128 = 1_000 * ONE;
pub const FUNDING: i128 = 1_000_000 * ONE;

pub fn vault<'a>(f: &'a impl ConformanceFixture) -> VaultClient<'a> {
    VaultClient::new(f.env(), &f.vault())
}

/// The share token: the same address as the vault.
pub fn shares<'a>(f: &'a impl ConformanceFixture) -> TokenClient<'a> {
    TokenClient::new(f.env(), &f.vault())
}

pub fn underlying<'a>(f: &'a impl ConformanceFixture) -> TokenClient<'a> {
    TokenClient::new(f.env(), &f.asset())
}

/// A fresh address funded with [`FUNDING`] of the underlying.
pub fn actor(f: &impl ConformanceFixture) -> Address {
    let a = Address::generate(f.env());
    f.mint(&a, FUNDING);
    a
}

/// Deposits [`DEPOSIT`] for `who` and returns the shares minted.
pub fn deposit_for(f: &impl ConformanceFixture, who: &Address) -> i128 {
    vault(f).deposit(&DEPOSIT, who, who, who)
}

pub fn expiry(f: &impl ConformanceFixture) -> u32 {
    f.env().ledger().sequence() + 1_000
}

/// Asserts a `try_*` result failed with contract error `code`.
///
/// Generic over the inner error because a `TokenClient` reports a
/// `ConversionError` there while a `contractclient` trait reports `Error`.
pub fn assert_contract_error<T: core::fmt::Debug, E: core::fmt::Debug>(
    res: Result<Result<T, E>, Result<Error, InvokeError>>,
    code: u32,
) {
    match res {
        Err(Ok(err)) => assert_eq!(
            err,
            Error::from_contract_error(code),
            "expected contract error #{code}"
        ),
        other => panic!("expected contract error #{code}, got {other:?}"),
    }
}

/// Asserts a `try_*` result failed for any reason other than a host-side
/// budget or auth mock problem.
pub fn assert_failed<T: core::fmt::Debug, E: core::fmt::Debug>(
    res: Result<Result<T, E>, Result<Error, InvokeError>>,
) {
    match res {
        Err(Ok(err)) => {
            assert!(
                !err.is_code(ScErrorCode::ExceededLimit),
                "failed on budget, not on the property under test"
            );
        }
        Err(Err(_)) => {}
        Ok(v) => panic!("expected failure, got {v:?}"),
    }
}

pub mod conformance;

/// Emits one `#[test]` per conformance property, each constructing a fixture
/// from `$fixture`.
#[macro_export]
macro_rules! conformance_tests {
    ($fixture:expr) => {
        $crate::conformance_tests!(@each $fixture;
            query_asset_matches_the_fixture,
            share_token_has_seven_decimals_and_metadata,
            deposit_mints_shares_to_the_receiver_and_debits_from,
            deposit_rejects_zero_and_negative_amounts,
            redeem_rejects_zero_and_negative_amounts,
            redeem_burns_exactly_the_shares_requested,
            redeem_pays_the_receiver_what_it_returns_and_no_more_than_quoted,
            redeem_beyond_the_balance_fails_rather_than_clamping,
            operator_without_an_allowance_cannot_redeem,
            operator_allowance_is_consumed_by_redeem,
            self_transfer_leaves_the_balance_unchanged,
            self_transfer_from_leaves_the_balance_unchanged,
            transfer_conserves_total_supply,
            transfer_rejects_a_negative_amount,
            transfer_from_rejects_a_negative_amount,
            approve_rejects_a_negative_amount,
            rate_is_non_decreasing_under_accrual,
            a_write_down_is_reported_honestly,
            sweep_moves_a_stray_token,
            sweep_refuses_the_underlying_and_the_share_token,
            sweep_rejects_a_non_positive_amount,
            sweep_cannot_touch_depositor_funds,
        );
    };
    (@each $fixture:expr; $($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                $crate::conformance::$name(&$fixture);
            }
        )*
    };
}
