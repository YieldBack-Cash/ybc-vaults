//! The failure modes this vault exists to avoid. Each of these would be a real
//! bug in a plausible alternative implementation.

use super::fixture::VaultFixture;
use crate::lending::constants::RAY;
use crate::testutils::DEFAULT_SUPPLY_CAP;

/// The sharpest edge in the whole integration.
///
/// XOXNO reads a withdrawal amount of `0` as *withdraw everything from this
/// market*. A dust redeem whose asset value floors to zero would therefore
/// hand the entire pooled position to whoever asked for one share.
#[test]
#[should_panic(expected = "Error(Contract, #300)")]
fn a_dust_redeem_flooring_to_zero_assets_is_refused() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    // Half the index: one share is now worth 0.5 asset units, which floors to 0.
    f.controller.set_supply_index(&(RAY / 2));
    assert_eq!(f.vault.convert_to_assets(&1), 0);

    f.vault.redeem(&1, &f.user, &f.user, &f.user);
}

/// Proof that the dust redeem above would have been catastrophic rather than
/// merely wrong: the position is untouched after the refusal.
#[test]
fn the_position_survives_a_refused_dust_redeem() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    f.controller.set_supply_index(&(RAY / 2));

    let before = f.vault.total_assets();
    let result = f.vault.try_redeem(&1, &f.user, &f.user, &f.user);

    assert!(result.is_err());
    assert_eq!(f.vault.total_assets(), before);
}

/// The property that removes the need for a virtual-share cushion: share price
/// comes from the market index, not from any balance this contract holds, so
/// supplying into the vault's position cannot move it.
#[test]
fn a_donation_into_the_vaults_position_does_not_move_share_price() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_0000000, &f.user, &f.user, &f.user);

    let price_before = f.vault.convert_to_assets(&1_0000000);
    let supply_before = f.vault.total_supply();

    // A third party supplies straight into the vault's XOXNO account.
    f.mint_to(&f.other, 10_000_0000000);
    f.controller
        .donate(&f.other, &f.vault.account_id(), &10_000_0000000);

    assert_eq!(
        f.vault.convert_to_assets(&1_0000000),
        price_before,
        "donation must not inflate share price"
    );
    assert_eq!(f.vault.total_supply(), supply_before);
    // The donated value is simply stranded — the vault is over-collateralized,
    // which is the safe direction.
    assert!(f.vault.total_assets() > f.vault.convert_to_assets(&f.vault.total_supply()));
}

/// The classic first-depositor inflation attack, which this share model makes
/// structurally impossible rather than merely expensive.
#[test]
fn the_inflation_attack_does_not_work() {
    let f = VaultFixture::new();

    // Attacker seeds the vault with one unit.
    f.mint_to(&f.other, 100_000_0000000);
    f.vault.deposit(&1, &f.other, &f.other, &f.other);

    // Attacker donates a large amount into the vault's position.
    f.controller
        .donate(&f.other, &f.vault.account_id(), &10_000_0000000);

    // Victim deposits. In a ratio-based vault this would mint 0 shares.
    let minted = f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    assert_eq!(minted, 1_000_0000000, "victim must get full value");
    assert_eq!(f.vault.convert_to_assets(&minted), 1_000_0000000);
}

/// A `seize_positions` write-down must be reported, not hidden. The consumer's
/// high-water mark is what decides how to treat it; the vault's job is to tell
/// the truth.
#[test]
fn an_index_write_down_is_reported_honestly() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    f.accrue(1_000); // +10%
    assert_eq!(f.vault.convert_to_assets(&1_000_0000000), 1_100_0000000);

    f.write_down(2_000); // -20% of the current index

    assert_eq!(f.vault.convert_to_assets(&1_000_0000000), 880_0000000);
    assert_eq!(f.vault.total_assets(), 880_0000000);
}

/// A consumer probing the rate at market creation must get a sane answer from
/// an empty vault. A ratio-based vault divides by zero here and needs a
/// bootstrap deposit first.
#[test]
fn an_empty_vault_still_quotes_a_rate() {
    let f = VaultFixture::new();
    assert_eq!(f.vault.total_supply(), 0);
    assert_eq!(f.vault.account_id(), 0);

    assert_eq!(f.vault.convert_to_assets(&1_0000000), 1_0000000);
    assert_eq!(f.vault.total_assets(), 0);
}

#[test]
fn max_withdraw_tracks_the_holder_position() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    assert_eq!(f.vault.max_withdraw(&f.user), 1_000_0000000);

    f.accrue(500); // +5%
    assert_eq!(f.vault.max_withdraw(&f.user), 1_050_0000000);
    assert_eq!(f.vault.max_withdraw(&f.other), 0);
}

#[test]
fn max_deposit_reports_headroom_under_the_spoke_cap() {
    let f = VaultFixture::new();
    assert_eq!(f.vault.max_deposit(&f.user), DEFAULT_SUPPLY_CAP);

    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    assert_eq!(
        f.vault.max_deposit(&f.user),
        DEFAULT_SUPPLY_CAP - 1_000_0000000
    );
}

/// The cap is market-wide for the spoke, not per-vault, so a third party
/// consuming it must shrink the vault's reported headroom too — that is exactly
/// what would make a deposit revert.
#[test]
fn max_deposit_accounts_for_supply_the_vault_did_not_make() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    f.mint_to(&f.other, 5_000_0000000);
    f.controller
        .donate(&f.other, &f.vault.account_id(), &5_000_0000000);

    assert_eq!(
        f.vault.max_deposit(&f.user),
        DEFAULT_SUPPLY_CAP - 6_000_0000000
    );
}

#[test]
fn max_deposit_is_zero_when_the_market_is_paused() {
    let f = VaultFixture::new();
    f.controller.set_paused(&true);
    assert_eq!(f.vault.max_deposit(&f.user), 0);
}

#[test]
fn max_deposit_is_zero_rather_than_negative_past_the_cap() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    f.controller.set_supply_cap(&100_0000000);

    assert_eq!(f.vault.max_deposit(&f.user), 0);
}

/// The metadata `create_market`-style consumers read.
#[test]
fn share_token_metadata_matches_the_underlying_scale() {
    let f = VaultFixture::new();
    assert_eq!(f.vault.decimals(), 7);
    assert_eq!(f.vault.query_asset(), f.asset);
}
