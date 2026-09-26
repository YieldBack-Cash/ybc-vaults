//! The conformance suite every vault adapter runs.
//!
//! Each property here is a SEP-56 rule (rounding directions, previews that
//! never overstate, limits, the two events), one YBC depends on that the
//! standard does not guarantee, or one drawn straight from a finding in
//! `ybc-contracts/docs/THREAT_MODEL.md`.
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
//! YBC itself uses, so it cannot pass on an adapter-private function YBC would
//! never call.

use soroban_sdk::{
    testutils::Address as _, token::TokenClient, xdr::ScErrorCode, Address, Env,
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

    /// Mints `amount` of the underlying to `to`.
    fn mint(&self, to: &Address, amount: i128);

    /// Makes the protocol accrue `bps` basis points of yield, funded so that
    /// redeeming the gain actually pays.
    fn accrue(&self, bps: i128);

    /// Makes the protocol lose `bps` of value. Returns `false` if the protocol
    /// cannot lose value, in which case the write-down property is skipped.
    fn write_down(&self, bps: i128) -> bool;
}

/// The conformance client: SEP-56 exactly as `vault_common::sep56` declares
/// it, which is also what every adapter is compile-checked against.
pub use vault_common::sep56::{Sep56Client as VaultClient, Sep56Vault};

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
pub mod ledger;
pub mod protocols;

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
            conversions_round_down_and_never_gain_on_a_round_trip,
            preview_deposit_never_overstates_the_deposit,
            preview_mint_never_understates_the_mint,
            preview_withdraw_never_understates_the_withdraw,
            preview_redeem_never_overstates_the_redeem,
            mint_mints_exactly_the_shares_requested,
            mint_rejects_zero_and_negative_amounts,
            mint_then_withdraw_never_profits,
            withdraw_pays_exactly_the_assets_requested,
            withdraw_rejects_zero_and_negative_amounts,
            withdraw_beyond_the_balance_fails_rather_than_clamping,
            operator_without_an_allowance_cannot_withdraw,
            operator_allowance_is_consumed_by_withdraw,
            max_redeem_is_the_balance_and_max_withdraw_its_value,
            max_mint_is_max_deposit_in_shares,
            deposit_and_mint_publish_the_standard_deposit_event,
            redeem_and_withdraw_publish_the_standard_withdraw_event,
            outstanding_shares_are_never_worth_more_than_total_assets,
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
