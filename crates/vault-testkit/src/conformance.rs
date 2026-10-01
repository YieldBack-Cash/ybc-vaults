//! The properties. See the crate docs for how an adapter runs them.
//!
//! Register entries in brackets refer to `ybc-contracts/docs/THREAT_MODEL.md`.

use soroban_sdk::{
    testutils::Address as _, xdr::ContractEventBody, Address, Map, Symbol, TryFromVal, Val,
};

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
    assert!(!t.name().is_empty(), "share token has no name");
    assert!(!t.symbol().is_empty(), "share token has no symbol");
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

/// [F-2] A share ledger that reads the recipient's balance before debiting the
/// sender doubles a whole-balance self-transfer.
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
pub fn rate_rises_under_accrual(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    let before = vault(f).convert_to_assets(&ONE);

    f.accrue(1_000); // target +10%

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

// ── SEP-56: conversions and previews ────────────────────────────────────────

/// Both conversions round down, so a round trip through them never gains.
pub fn conversions_round_down_and_never_gain_on_a_round_trip(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    f.accrue(137); // an awkward index, so the rounding actually bites

    for assets in [1, 7, ONE - 1, ONE, 12_345 * ONE + 3] {
        let shares = vault(f).convert_to_shares(&assets);
        assert!(
            vault(f).convert_to_assets(&shares) <= assets,
            "assets {assets}"
        );
    }
    for shares in [1, 11, ONE + 1, 999 * ONE] {
        let assets = vault(f).convert_to_assets(&shares);
        assert!(
            vault(f).convert_to_shares(&assets) <= shares,
            "shares {shares}"
        );
    }
}

/// [SEP-56] `preview_deposit` MUST NOT exceed what `deposit` mints.
pub fn preview_deposit_never_overstates_the_deposit(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    f.accrue(137);

    let quoted = vault(f).preview_deposit(&(123 * ONE + 7));
    let minted = vault(f).deposit(&(123 * ONE + 7), &user, &user, &user);
    assert!(
        quoted > 0 && quoted <= minted,
        "quoted {quoted} > minted {minted}"
    );
}

/// [SEP-56] `preview_mint` MUST NOT understate what `mint` charges.
pub fn preview_mint_never_understates_the_mint(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    f.accrue(137);

    let quoted = vault(f).preview_mint(&(123 * ONE + 7));
    let charged = vault(f).mint(&(123 * ONE + 7), &user, &user, &user);
    assert!(
        charged > 0 && charged <= quoted,
        "charged {charged} > quoted {quoted}"
    );
}

/// [SEP-56] `preview_withdraw` MUST NOT understate what `withdraw` burns.
pub fn preview_withdraw_never_understates_the_withdraw(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    f.accrue(137);

    let quoted = vault(f).preview_withdraw(&(123 * ONE + 7));
    let burned = vault(f).withdraw(&(123 * ONE + 7), &user, &user, &user);
    assert!(
        burned > 0 && burned <= quoted,
        "burned {burned} > quoted {quoted}"
    );
}

/// [SEP-56] `preview_redeem` MUST NOT overstate what `redeem` pays.
pub fn preview_redeem_never_overstates_the_redeem(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    f.accrue(137);

    let quoted = vault(f).preview_redeem(&(123 * ONE + 7));
    let paid = vault(f).redeem(&(123 * ONE + 7), &user, &user, &user);
    assert!(
        quoted > 0 && quoted <= paid,
        "quoted {quoted} > paid {paid}"
    );
}

// ── SEP-56: mint ────────────────────────────────────────────────────────────

pub fn mint_mints_exactly_the_shares_requested(f: &impl ConformanceFixture) {
    let user = actor(f);
    let receiver = Address::generate(f.env());
    deposit_for(f, &user);
    f.accrue(137);
    let supply = vault(f).total_supply();
    let funds_before = underlying(f).balance(&user);
    let want = 500 * ONE + 1;

    let charged = vault(f).mint(&want, &receiver, &user, &user);

    assert_eq!(shares(f).balance(&receiver), want);
    assert_eq!(vault(f).total_supply(), supply + want);
    assert_eq!(underlying(f).balance(&user), funds_before - charged);
}

/// [F-1, F-3] A negative amount must never reach the ledger.
pub fn mint_rejects_zero_and_negative_amounts(f: &impl ConformanceFixture) {
    let user = actor(f);
    assert_contract_error(
        vault(f).try_mint(&0, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
    assert_contract_error(
        vault(f).try_mint(&-1, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
}

/// Minting then withdrawing what it cost can never leave the user with more
/// than they started with.
pub fn mint_then_withdraw_never_profits(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    f.accrue(137);
    let held = shares(f).balance(&user);
    let want = 333 * ONE + 3;

    let charged = vault(f).mint(&want, &user, &user, &user);
    let burned = vault(f).withdraw(&charged, &user, &user, &user);

    assert!(burned >= want, "burned {burned} < minted {want}");
    assert!(shares(f).balance(&user) <= held);
}

// ── SEP-56: withdraw ────────────────────────────────────────────────────────

pub fn withdraw_pays_exactly_the_assets_requested(f: &impl ConformanceFixture) {
    let user = actor(f);
    let receiver = Address::generate(f.env());
    let minted = deposit_for(f, &user);
    f.accrue(137);
    let want = 250 * ONE + 9;

    let burned = vault(f).withdraw(&want, &receiver, &user, &user);

    assert_eq!(underlying(f).balance(&receiver), want);
    assert_eq!(shares(f).balance(&user), minted - burned);
    assert!(burned > 0 && burned <= minted);
}

pub fn withdraw_rejects_zero_and_negative_amounts(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    assert_contract_error(
        vault(f).try_withdraw(&0, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
    assert_contract_error(
        vault(f).try_withdraw(&-1, &user, &user, &user),
        VaultError::AmountNotPositive as u32,
    );
}

pub fn withdraw_beyond_the_balance_fails_rather_than_clamping(f: &impl ConformanceFixture) {
    let user = actor(f);
    let minted = deposit_for(f, &user);
    let too_much = vault(f).max_withdraw(&user) + ONE;

    assert_failed(vault(f).try_withdraw(&too_much, &user, &user, &user));
    assert_eq!(
        shares(f).balance(&user),
        minted,
        "nothing burned on failure"
    );
}

pub fn operator_without_an_allowance_cannot_withdraw(f: &impl ConformanceFixture) {
    let owner = actor(f);
    let operator = Address::generate(f.env());
    let minted = deposit_for(f, &owner);

    assert_failed(vault(f).try_withdraw(&ONE, &operator, &owner, &operator));
    assert_eq!(shares(f).balance(&owner), minted);
}

pub fn operator_allowance_is_consumed_by_withdraw(f: &impl ConformanceFixture) {
    let owner = actor(f);
    let operator = Address::generate(f.env());
    let minted = deposit_for(f, &owner);
    let want = 100 * ONE;
    let cost = vault(f).preview_withdraw(&want);
    shares(f).approve(&owner, &operator, &cost, &expiry(f));

    let burned = vault(f).withdraw(&want, &operator, &owner, &operator);

    assert_eq!(shares(f).balance(&owner), minted - burned);
    assert_eq!(shares(f).allowance(&owner, &operator), cost - burned);
    assert_eq!(underlying(f).balance(&operator), want);
}

// ── SEP-56: limits ──────────────────────────────────────────────────────────

/// With the protocol liquid. The `withdraw_limit` cap is each adapter's own
/// test.
pub fn max_redeem_is_the_balance_and_max_withdraw_its_value(f: &impl ConformanceFixture) {
    let user = actor(f);
    let minted = deposit_for(f, &user);
    f.accrue(137);

    assert_eq!(vault(f).max_redeem(&user), minted);
    assert_eq!(
        vault(f).max_withdraw(&user),
        vault(f).convert_to_assets(&minted)
    );
    assert_eq!(vault(f).max_redeem(&Address::generate(f.env())), 0);
}

pub fn max_mint_is_max_deposit_in_shares(f: &impl ConformanceFixture) {
    let user = actor(f);
    deposit_for(f, &user);
    let cap = vault(f).max_deposit(&user);
    assert_eq!(vault(f).max_mint(&user), vault(f).convert_to_shares(&cap));
}

// ── SEP-56: events ──────────────────────────────────────────────────────────

/// The last event the vault published, as `(topics, data)`.
fn last_event(f: &impl ConformanceFixture) -> (soroban_sdk::Vec<Val>, Val) {
    use soroban_sdk::testutils::Events as _;
    let all = f.env().events().all().filter_by_contract(&f.vault());
    let ev = all
        .events()
        .last()
        .expect("the vault published no event")
        .clone();
    let ContractEventBody::V0(body) = ev.body;
    let topics = soroban_sdk::Vec::from_iter(
        f.env(),
        body.topics
            .iter()
            .map(|t| Val::try_from_val(f.env(), t).unwrap()),
    );
    (topics, Val::try_from_val(f.env(), &body.data).unwrap())
}

fn assert_flow_event(
    f: &impl ConformanceFixture,
    name: &str,
    addresses: [&Address; 3],
    assets: i128,
    shares: i128,
) {
    let (topics, data) = last_event(f);
    assert_eq!(topics.len(), 4, "topics: {topics:?}");
    assert_eq!(
        Symbol::try_from_val(f.env(), &topics.get(0).unwrap()).unwrap(),
        Symbol::new(f.env(), name)
    );
    for (i, a) in addresses.iter().enumerate() {
        assert_eq!(
            &Address::try_from_val(f.env(), &topics.get(i as u32 + 1).unwrap()).unwrap(),
            *a,
            "topic {}",
            i + 1
        );
    }
    let data: Map<Symbol, i128> = Map::try_from_val(f.env(), &data).unwrap();
    assert_eq!(data.get(Symbol::new(f.env(), "assets")), Some(assets));
    assert_eq!(data.get(Symbol::new(f.env(), "shares")), Some(shares));
}

/// [SEP-56] `deposit` and `mint` publish `Deposit(operator, from, receiver)`
/// with `assets` and `shares` in the data.
pub fn deposit_and_mint_publish_the_standard_deposit_event(f: &impl ConformanceFixture) {
    let user = actor(f);
    let receiver = Address::generate(f.env());

    let minted = vault(f).deposit(&DEPOSIT, &receiver, &user, &user);
    assert_flow_event(f, "deposit", [&user, &user, &receiver], DEPOSIT, minted);

    let charged = vault(f).mint(&ONE, &receiver, &user, &user);
    assert_flow_event(f, "deposit", [&user, &user, &receiver], charged, ONE);
}

/// [SEP-56] `redeem` and `withdraw` publish `Withdraw(operator, receiver, owner)`
/// with `assets` and `shares` in the data.
pub fn redeem_and_withdraw_publish_the_standard_withdraw_event(f: &impl ConformanceFixture) {
    let user = actor(f);
    let receiver = Address::generate(f.env());
    deposit_for(f, &user);

    let paid = vault(f).redeem(&(10 * ONE), &receiver, &user, &user);
    assert_flow_event(f, "withdraw", [&user, &receiver, &user], paid, 10 * ONE);

    let burned = vault(f).withdraw(&(10 * ONE), &receiver, &user, &user);
    assert_flow_event(f, "withdraw", [&user, &receiver, &user], 10 * ONE, burned);
}

// ── solvency ────────────────────────────────────────────────────────────────

/// After any mix of the four entry points, the shares outstanding are never
/// worth more than the position the protocol records for the vault, nor than
/// the vault's own `total_assets`: every rounding fell the vault's way.
pub fn outstanding_shares_are_never_worth_more_than_the_backing(f: &impl ConformanceFixture) {
    let a = actor(f);
    let b = actor(f);
    deposit_for(f, &a);
    f.accrue(137);
    vault(f).mint(&(77 * ONE + 7), &b, &b, &b);
    vault(f).withdraw(&(31 * ONE + 3), &a, &a, &a);
    vault(f).redeem(&(13 * ONE + 1), &b, &b, &b);
    f.accrue(59);
    vault(f).mint(&1, &a, &a, &a);
    vault(f).withdraw(&1, &b, &b, &b);

    let supply = vault(f).total_supply();
    let worth = vault(f).convert_to_assets(&supply);
    let held = vault(f).total_assets();
    assert!(worth <= held, "shares worth {worth} > assets held {held}");

    // The line above can be a tautology: an adapter that derives both figures
    // from one stored total passes it whatever the real position is. This
    // one cannot be, because the right-hand side is the protocol's.
    let backing = f.backing();
    assert!(
        worth <= backing,
        "shares worth {worth} > the position the protocol records {backing}"
    );
    assert!(
        held <= backing,
        "the vault reports {held} but the protocol records {backing}"
    );
}
