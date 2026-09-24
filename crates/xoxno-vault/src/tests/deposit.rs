use super::fixture::VaultFixture;

#[test]
fn first_deposit_opens_an_account_and_mints_shares() {
    let f = VaultFixture::new();
    f.assert_index_starts_at_ray();
    assert_eq!(f.vault.account_id(), 0);

    let minted = f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    // At an index of exactly RAY, one share is one asset unit.
    assert_eq!(minted, 1_000_0000000);
    assert_eq!(f.vault.balance(&f.user), 1_000_0000000);
    assert_eq!(f.vault.total_supply(), 1_000_0000000);
    assert_eq!(f.vault.total_assets(), 1_000_0000000);
    assert_ne!(f.vault.account_id(), 0, "account should have been opened");
}

/// The controller stores positions as 27-decimal Rays; shares are that figure
/// at asset precision. Pins both the vault's conversion and the mock's
/// fidelity to the real protocol, which a testnet simulation found wanting:
/// the first mock stored 7-decimal units and the vault minted 1e20 shares per
/// stroop against the real controller.
#[test]
fn shares_are_the_ray_position_at_asset_precision() {
    use crate::vault::SCALED_UNIT;

    let f = VaultFixture::new();
    let minted = f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    let raw = f.controller.scaled(&f.vault.account_id());
    assert_eq!(
        raw,
        minted * SCALED_UNIT,
        "controller position is Ray-scaled"
    );
    assert_eq!(minted, 1_000_0000000, "shares are at 7 decimals");
    assert_eq!(f.vault.max_withdraw(&f.user), 1_000_0000000);
}

#[test]
fn second_deposit_reuses_the_same_account() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    let account = f.vault.account_id();

    f.vault.deposit(&500_0000000, &f.user, &f.user, &f.user);

    assert_eq!(f.vault.account_id(), account);
    assert_eq!(f.vault.total_supply(), 1_500_0000000);
}

#[test]
fn deposit_after_accrual_mints_fewer_shares_than_assets() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    f.accrue(1_000); // +10%

    let minted = f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    // Each share is now worth 1.1 assets, so the same deposit buys ~909.09 of
    // them, floored. The depositor does not retroactively capture the earlier
    // accrual.
    assert_eq!(minted, 909_0909090);
}

#[test]
fn accrual_raises_share_value_for_existing_holders() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    f.accrue(1_000); // +10%

    assert_eq!(f.vault.convert_to_assets(&1_000_0000000), 1_100_0000000);
    assert_eq!(f.vault.total_assets(), 1_100_0000000);
    // Share count is untouched — only the multiplier moved.
    assert_eq!(f.vault.balance(&f.user), 1_000_0000000);
}

#[test]
fn receiver_gets_the_shares_not_the_funds_owner() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.other, &f.user, &f.user);

    assert_eq!(f.vault.balance(&f.other), 1_000_0000000);
    assert_eq!(f.vault.balance(&f.user), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn zero_deposit_is_refused() {
    let f = VaultFixture::new();
    f.vault.deposit(&0, &f.user, &f.user, &f.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn negative_deposit_is_refused() {
    let f = VaultFixture::new();
    f.vault.deposit(&-1, &f.user, &f.user, &f.user);
}
