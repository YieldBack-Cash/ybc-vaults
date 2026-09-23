//! The properties. See the crate docs for how an adapter runs them.
//!
//! Register entries in brackets refer to `ybc-contracts/docs/THREAT_MODEL.md`.

use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address};

use crate::{
    actor, assert_contract_error, assert_failed, deposit_for, expiry, shares, underlying, vault,
    ConformanceFixture, VaultError, DEPOSIT, ONE,
};

// ── surface ─────────────────────────────────────────────────────────────────

pub fn query_asset_matches_the_fixture(f: &impl ConformanceFixture) {
    assert_eq!(vault(f).query_asset(), f.asset());
}

/// Every YBC consumer assumes 7 decimals (the 1e7 fixed-point scale).
pub fn share_token_has_seven_decimals_and_metadata(f: &impl ConformanceFixture) {
    let t = shares(f);
    assert_eq!(t.decimals(), 7);
    assert!(t.name().len() > 0, "share token has no name");
    assert!(t.symbol().len() > 0, "share token has no symbol");
}

// ── deposit ─────────────────────────────────────────────────────────────────

pub fn deposit_mints_shares_to_the_receiver_and_debits_from(f: &impl ConformanceFixture) {
    let user = actor(f);
    let receiver = Address::generate(f.env());
    let funds_before = underlying(f).balance(&user);

    let minted = vault(f).deposit(&DEPOSIT, &receiver, &user, &user);

    assert!(minted > 0);
    assert_eq!(shares(f).balance(&receiver), minted);
    assert_eq!(shares(f).balance(&user), 0);
    assert_eq!(underlying(f).balance(&user), funds_before - DEPOSIT);
    assert_eq!(vault(f).total_supply(), minted);
}

/// [F-1, F-3] A negative amount must never reach the ledger.
pub fn deposit_rejects_zero_and_negative_amounts(f: &impl ConformanceFixture) {
    let user = actor(f);
    assert_contract_error(
        vault(f).try_deposit(&0, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
    assert_contract_error(
        vault(f).try_deposit(&-1, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
}

// ── redeem ──────────────────────────────────────────────────────────────────

pub fn redeem_rejects_zero_and_negative_amounts(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    assert_contract_error(
        vault(f).try_redeem(&0, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
    assert_contract_error(
        vault(f).try_redeem(&-1, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
}

pub fn redeem_burns_exactly_the_shares_requested(f: &impl ConformanceFixture) {
    let user = actor(f);
    let minted = deposit_for(f, &user);
    let supply = vault(f).total_supply();
    let burn = minted / 3;

    vault(f).redeem(&burn, &user, &user, &user);

    assert_eq!(shares(f).balance(&user), minted - burn);
    assert_eq!(vault(f).total_supply(), supply - burn);
}

/// `convert_to_assets` is what the yield manager high-water-marks into the
/// protocol's exchange rate, so a redeem paying *more* than it would mean the
/// rate under-reports value the vault actually holds.
pub fn redeem_pays_the_receiver_what_it_returns_and_no_more_than_quoted(
    f: &impl ConformanceFixture,
) {
    let user = actor(f);
    let receiver = Address::generate(f.env());
    let minted = deposit_for(f, &user);
    let burn = minted / 2;

    let quoted = vault(f).convert_to_assets(&burn);
    let before = underlying(f).balance(&receiver);
    let paid = vault(f).redeem(&burn, &receiver, &user, &user);

    assert!(paid > 0);
    assert_eq!(underlying(f).balance(&receiver) - before, paid);
    assert!(paid <= quoted, "paid {paid} > quoted {quoted}");
}

/// A vault that clamped to the balance would return a short payout, which a
/// consumer only notices through its own slippage bound.
pub fn redeem_beyond_the_balance_fails_rather_than_clamping(f: &impl ConformanceFixture) {
    let user = actor(f);
    let minted = deposit_for(f, &user);

    assert_failed(vault(f).try_redeem(&(minted + 1), &user, &user, &user));
    assert_eq!(
        shares(f).balance(&user),
        minted,
        "nothing burned on failure"
    );
}

/// With every auth mocked, the only thing standing between an arbitrary
/// operator and someone else's shares is the allowance check.
pub fn operator_without_an_allowance_cannot_redeem(f: &impl ConformanceFixture) {
    let owner = actor(f);
    let operator = Address::generate(f.env());
    let minted = deposit_for(f, &owner);

    assert_failed(vault(f).try_redeem(&minted, &operator, &owner, &operator));
    assert_eq!(shares(f).balance(&owner), minted);
}

pub fn operator_allowance_is_consumed_by_redeem(f: &impl ConformanceFixture) {
    let owner = actor(f);
    let operator = Address::generate(f.env());
    let minted = deposit_for(f, &owner);
    let burn = minted / 4;
    shares(f).approve(&owner, &operator, &burn, &expiry(f));

    vault(f).redeem(&burn, &operator, &owner, &operator);

    assert_eq!(shares(f).balance(&owner), minted - burn);
    assert_eq!(
        shares(f).allowance(&owner, &operator),
        0,
        "allowance consumed"
    );
    assert!(
        underlying(f).balance(&operator) > 0,
        "operator received the assets"
    );
}

// ── share token ─────────────────────────────────────────────────────────────

/// [F-2] `blend-vault-v2` read the credit side before writing the debit, so a
/// self-transfer of the whole balance doubled it.
pub fn self_transfer_leaves_the_balance_unchanged(f: &impl ConformanceFixture) {
    let user = actor(f);
    let minted = deposit_for(f, &user);

    shares(f).transfer(&user, &user, &minted);

    assert_eq!(shares(f).balance(&user), minted);
    assert_eq!(vault(f).total_supply(), minted);
}

pub fn self_transfer_from_leaves_the_balance_unchanged(f: &impl ConformanceFixture) {
    let user = actor(f);
    let spender = Address::generate(f.env());
    let minted = deposit_for(f, &user);
    shares(f).approve(&user, &spender, &minted, &expiry(f));

    shares(f).transfer_from(&spender, &user, &user, &minted);

    assert_eq!(shares(f).balance(&user), minted);
    assert_eq!(vault(f).total_supply(), minted);
}

pub fn transfer_conserves_total_supply(f: &impl ConformanceFixture) {
    let user = actor(f);
    let other = Address::generate(f.env());
    let minted = deposit_for(f, &user);
    let moved = minted / 5;

    shares(f).transfer(&user, &other, &moved);

    assert_eq!(shares(f).balance(&user), minted - moved);
    assert_eq!(shares(f).balance(&other), moved);
    assert_eq!(vault(f).total_supply(), minted);
}

/// [F-1] A negative transfer credits the sender and debits the recipient.
pub fn transfer_rejects_a_negative_amount(f: &impl ConformanceFixture) {
    let user = actor(f);
    let other = Address::generate(f.env());
    let minted = deposit_for(f, &user);

    assert_failed(shares(f).try_transfer(&user, &other, &-1));
    assert_eq!(shares(f).balance(&user), minted);
    assert_eq!(shares(f).balance(&other), 0);
}

pub fn transfer_from_rejects_a_negative_amount(f: &impl ConformanceFixture) {
    let user = actor(f);
    let spender = Address::generate(f.env());
    let minted = deposit_for(f, &user);
    shares(f).approve(&user, &spender, &minted, &expiry(f));

    assert_failed(shares(f).try_transfer_from(&spender, &user, &spender, &-1));
    assert_eq!(shares(f).balance(&user), minted);
}

pub fn approve_rejects_a_negative_amount(f: &impl ConformanceFixture) {
    let user = actor(f);
    let spender = Address::generate(f.env());

    assert_failed(shares(f).try_approve(&user, &spender, &-1, &expiry(f)));
    assert_eq!(shares(f).allowance(&user, &spender), 0);
}

// ── rate ────────────────────────────────────────────────────────────────────

/// The yield manager assumes the rate never falls; it must at least rise when
/// the protocol accrues.
pub fn rate_is_non_decreasing_under_accrual(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    let before = vault(f).convert_to_assets(&ONE);

    f.accrue(1_000); // +10%

    let after = vault(f).convert_to_assets(&ONE);
    assert!(after > before, "rate did not rise: {before} -> {after}");
}

/// A loss must be reported, not hidden. The consumer's high-water mark decides
/// how to treat it; the vault's job is to tell the truth.
pub fn a_write_down_is_reported_honestly(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    let before = vault(f).convert_to_assets(&ONE);

    if !f.write_down(2_000) {
        return; // this protocol cannot lose value
    }

    let after = vault(f).convert_to_assets(&ONE);
    assert!(after < before, "write-down hidden: {before} -> {after}");
}

// ── sweep ───────────────────────────────────────────────────────────────────

fn stray_token(f: &impl ConformanceFixture) -> Address {
    let issuer = Address::generate(f.env());
    f.env().register_stellar_asset_contract_v2(issuer).address()
}

pub fn sweep_moves_a_stray_token(f: &impl ConformanceFixture) {
    let airdrop = stray_token(f);
    StellarAssetClient::new(f.env(), &airdrop).mint(&f.vault(), &(500 * ONE));

    vault(f).sweep(&airdrop, &f.admin(), &(500 * ONE));

    let t = soroban_sdk::token::TokenClient::new(f.env(), &airdrop);
    assert_eq!(t.balance(&f.admin()), 500 * ONE);
    assert_eq!(t.balance(&f.vault()), 0);
}

pub fn sweep_refuses_the_underlying_and_the_share_token(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);

    assert_contract_error(
        vault(f).try_sweep(&f.asset(), &f.admin(), &1),
        VaultError::SweepForbidden as u32,
    );
    assert_contract_error(
        vault(f).try_sweep(&f.vault(), &f.admin(), &1),
        VaultError::SweepForbidden as u32,
    );
}

pub fn sweep_rejects_a_non_positive_amount(f: &impl ConformanceFixture) {
    let airdrop = stray_token(f);
    assert_contract_error(
        vault(f).try_sweep(&airdrop, &f.admin(), &0),
        VaultError::AmountNotPositive as u32,
    );
    assert_contract_error(
        vault(f).try_sweep(&airdrop, &f.admin(), &-1),
        VaultError::AmountNotPositive as u32,
    );
}

pub fn sweep_cannot_touch_depositor_funds(f: &impl ConformanceFixture) {
    let user = actor(f);
    let minted = deposit_for(f, &user);
    let airdrop = stray_token(f);
    StellarAssetClient::new(f.env(), &airdrop).mint(&f.vault(), &ONE);

    vault(f).sweep(&airdrop, &f.admin(), &ONE);

    assert_eq!(shares(f).balance(&user), minted);
    let paid = vault(f).redeem(&minted, &user, &user, &user);
    assert!(paid > 0 && paid <= DEPOSIT, "full exit still pays: {paid}");
    assert_eq!(shares(f).balance(&user), 0);
}
