use super::fixture::VaultFixture;

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
#[should_panic(expected = "Error(Contract, #301)")]
fn redeem_before_any_deposit_has_no_account() {
    let f = VaultFixture::new();
    f.vault.redeem(&1, &f.user, &f.user, &f.user);
}
