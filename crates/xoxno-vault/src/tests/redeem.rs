use super::fixture::VaultFixture;

#[test]
fn redeem_burns_exactly_the_shares_requested() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    let assets = f.vault.redeem(&400_0000000, &f.user, &f.user, &f.user);

    assert_eq!(assets, 400_0000000);
    assert_eq!(f.vault.balance(&f.user), 600_0000000);
    assert_eq!(f.vault.total_supply(), 600_0000000);
}

#[test]
fn redeem_sends_assets_to_the_receiver() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    let before = f.token.balance(&f.other);
    f.vault.redeem(&250_0000000, &f.other, &f.user, &f.user);

    assert_eq!(f.token.balance(&f.other) - before, 250_0000000);
}

#[test]
fn redeem_after_accrual_pays_out_the_yield() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    f.accrue(1_000); // +10%

    let before = f.token.balance(&f.user);
    let assets = f.vault.redeem(&1_000_0000000, &f.user, &f.user, &f.user);

    assert_eq!(assets, 1_100_0000000);
    assert_eq!(f.token.balance(&f.user) - before, 1_100_0000000);
    assert_eq!(f.vault.total_supply(), 0);
}

#[test]
fn full_redeem_leaves_the_position_empty_but_the_account_intact() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    let account = f.vault.account_id();

    f.vault.redeem(&1_000_0000000, &f.user, &f.user, &f.user);

    assert_eq!(f.vault.total_supply(), 0);
    assert_eq!(f.vault.total_assets(), 0);
    // The account id is never cleared — it is the only route back to the
    // position, and nothing can re-point it.
    assert_eq!(f.vault.account_id(), account);
}

#[test]
fn a_later_deposit_reuses_the_account_after_a_full_exit() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    let account = f.vault.account_id();
    f.vault.redeem(&1_000_0000000, &f.user, &f.user, &f.user);

    f.vault.deposit(&500_0000000, &f.user, &f.user, &f.user);

    assert_eq!(f.vault.account_id(), account);
    assert_eq!(f.vault.total_supply(), 500_0000000);
}

#[test]
fn operator_may_redeem_against_a_share_allowance() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    let expiry = f.e.ledger().sequence() + 1_000;
    f.vault.approve(&f.user, &f.other, &300_0000000, &expiry);

    f.vault.redeem(&300_0000000, &f.other, &f.user, &f.other);

    assert_eq!(f.vault.balance(&f.user), 700_0000000);
    assert_eq!(
        f.vault.allowance(&f.user, &f.other),
        0,
        "allowance consumed"
    );
}

#[test]
#[should_panic]
fn operator_without_an_allowance_cannot_redeem() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    // No approve call. mock_all_auths satisfies require_auth, so what is under
    // test here is purely the allowance check.
    f.vault.redeem(&300_0000000, &f.other, &f.user, &f.other);
}

#[test]
#[should_panic]
fn redeem_beyond_the_balance_reverts_rather_than_clamping() {
    let f = VaultFixture::new();
    f.vault.deposit(&100_0000000, &f.user, &f.user, &f.user);

    // Must fail. A vault that clamped to the balance would return a short
    // payout, which a consumer only notices via its own slippage bound.
    f.vault.redeem(&200_0000000, &f.user, &f.user, &f.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn zero_redeem_is_refused() {
    let f = VaultFixture::new();
    f.vault.deposit(&100_0000000, &f.user, &f.user, &f.user);
    f.vault.redeem(&0, &f.user, &f.user, &f.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #301)")]
fn redeem_before_any_deposit_has_no_account() {
    let f = VaultFixture::new();
    f.vault.redeem(&1, &f.user, &f.user, &f.user);
}
