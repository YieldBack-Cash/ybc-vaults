//! `sweep` is the only admin-held power in this contract. These tests pin down
//! that it cannot reach depositor funds — which is the entire basis for it being
//! acceptable at all.

use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address,
};

use super::fixture::VaultFixture;

fn register_foreign_token(f: &VaultFixture) -> Address {
    let issuer = Address::generate(&f.e);
    f.e.register_stellar_asset_contract_v2(issuer).address()
}

#[test]
fn sweeps_a_stray_token() {
    let f = VaultFixture::new();
    let airdrop = register_foreign_token(&f);

    // An incentive program pays out to whoever held the position — this vault.
    StellarAssetClient::new(&f.e, &airdrop).mint(&f.vault_address, &500_0000000);

    f.vault.sweep(&airdrop, &f.admin, &500_0000000);

    assert_eq!(TokenClient::new(&f.e, &airdrop).balance(&f.admin), 500_0000000);
    assert_eq!(
        TokenClient::new(&f.e, &airdrop).balance(&f.vault_address),
        0
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn cannot_sweep_the_underlying_asset() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    f.vault.sweep(&f.asset, &f.admin, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn cannot_sweep_the_share_token() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);

    f.vault.sweep(&f.vault_address, &f.admin, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn cannot_sweep_a_non_positive_amount() {
    let f = VaultFixture::new();
    let airdrop = register_foreign_token(&f);
    f.vault.sweep(&airdrop, &f.admin, &0);
}

#[test]
fn sweeping_cannot_touch_the_lending_position() {
    let f = VaultFixture::new();
    f.vault.deposit(&1_000_0000000, &f.user, &f.user, &f.user);
    let airdrop = register_foreign_token(&f);
    StellarAssetClient::new(&f.e, &airdrop).mint(&f.vault_address, &1_0000000);

    f.vault.sweep(&airdrop, &f.admin, &1_0000000);

    assert_eq!(f.vault.total_assets(), 1_000_0000000);
    assert_eq!(f.vault.balance(&f.user), 1_000_0000000);
}
