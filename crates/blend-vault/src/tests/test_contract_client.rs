#![cfg(test)]

//! The vault called through a client generated from YBC's `vault_interface`
//! trait rather than from this contract, proving the on-chain ABI matches what
//! the protocol dispatches by name.

use crate::{
    storage,
    testutils::{
        assert_approx_eq_rel, create_blend_pool, mockpool, register_blend_vault, BlendFixture,
        EnvTestUtils, MockTokenClient,
    },
    vault::VaultData,
    BlendVaultClient,
};
use soroban_sdk::{contractclient, testutils::Address as _, Address, Env};
use stellar_tokens::fungible::Base;

/// The SEP-56 subset YBC calls, as `ybc-contracts/vault/vault_interface`
/// declares it. Never implemented; it exists to generate the client.
#[allow(dead_code)]
#[contractclient(name = "VaultContractClient")]
pub trait VaultTrait {
    fn query_asset(e: &Env) -> Address;
    fn convert_to_assets(e: &Env, shares: i128) -> i128;
    fn deposit(e: &Env, assets: i128, receiver: Address, from: Address, operator: Address) -> i128;
    fn redeem(e: &Env, shares: i128, receiver: Address, owner: Address, operator: Address) -> i128;
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Sets up a full Blend pool + blend vault with usdc as the asset.
/// Returns (vault_address, usdc_client, frodo, samwise).
fn setup_blend(e: &Env) -> (Address, MockTokenClient<'_>, Address, Address) {
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(e);
    let frodo = Address::generate(e);
    let samwise = Address::generate(e);

    let blnd = e
        .register_stellar_asset_contract_v2(bombadil.clone())
        .address();
    let usdc = e
        .register_stellar_asset_contract_v2(bombadil.clone())
        .address();
    let xlm = e
        .register_stellar_asset_contract_v2(bombadil.clone())
        .address();
    let usdc_client = MockTokenClient::new(e, &usdc);
    let xlm_client = MockTokenClient::new(e, &xlm);

    let blend_fixture = BlendFixture::deploy(e, &bombadil, &blnd, &usdc);
    let pool = create_blend_pool(e, &blend_fixture, &bombadil, &usdc_client, &xlm_client);

    let vault = register_blend_vault(e, &bombadil, &pool, &usdc, &blnd);

    usdc_client.mint(&frodo, &10_000_0000000);
    usdc_client.mint(&samwise, &10_000_0000000);

    (vault, usdc_client, frodo, samwise)
}

/// Sets up a vault with directly-written storage state (no real pool).
/// Returns (vault_address, pool_address, samwise, frodo).
fn setup_mock(e: &Env) -> (Address, Address, Address, Address) {
    let admin = Address::generate(e);
    let samwise = Address::generate(e);
    let frodo = Address::generate(e);

    let b_rate = 1_000_000_000_000_i128;
    let pool = mockpool::register_mock_pool_with_b_rate(e, b_rate).address;
    let reserve = Address::generate(e);
    let blnd_token = Address::generate(e);
    let vault = register_blend_vault(e, &admin, &pool, &reserve, &blnd_token);

    e.as_contract(&vault, || {
        storage::set_vault_data(
            e,
            &VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate,
                last_update_timestamp: e.ledger().timestamp(),
            },
        );
        // samwise: 10 %, frodo: 90 %
        Base::mint(e, &samwise, 120_0000000);
        Base::mint(e, &frodo, 1080_0000000);
    });

    (vault, pool, samwise, frodo)
}

// ── convert_to_assets ─────────────────────────────────────────────────────────

/// VaultContractClient::convert_to_assets produces the same result as
/// BlendVaultClient::max_withdraw for each user's share balance.
#[test]
fn test_convert_to_assets_matches_max_withdraw() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let (vault, _pool, samwise, frodo) = setup_mock(&e);
    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    let samwise_shares = vault_client.balance(&samwise);
    let frodo_shares = vault_client.balance(&frodo);

    assert_eq!(
        oz_client.convert_to_assets(&samwise_shares),
        vault_client.max_withdraw(&samwise)
    );
    assert_eq!(
        oz_client.convert_to_assets(&frodo_shares),
        vault_client.max_withdraw(&frodo)
    );
}

/// Both clients call the same on-chain function, so they must always agree for
/// any arbitrary share amount.
#[test]
fn test_convert_to_assets_agrees_with_blend_vault_client() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let (vault, _pool, _samwise, _frodo) = setup_mock(&e);
    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    for shares in [
        0_i128,
        1,
        120_0000000,
        600_0000000,
        1080_0000000,
        1200_0000000,
    ] {
        assert_eq!(
            oz_client.convert_to_assets(&shares),
            vault_client.convert_to_assets(&shares),
            "mismatch at shares = {}",
            shares
        );
    }
}

/// convert_to_assets must reflect the current b_rate, not a stale cached value.
#[test]
fn test_convert_to_assets_reflects_rate_change() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let (vault, pool, samwise, _frodo) = setup_mock(&e);
    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    let shares = vault_client.balance(&samwise);
    let before = oz_client.convert_to_assets(&shares);

    let mock_client = mockpool::MockPoolClient::new(&e, &pool);
    mock_client.set_b_rate(&2_000_000_000_000_i128);
    e.jump(5);

    let after = oz_client.convert_to_assets(&shares);

    // depositors keep 100 % of the 100 % gain → 200 % of original
    assert_approx_eq_rel(after, before * 2, 0_0000001);
}

/// 0 shares must always return 0.
#[test]
fn test_convert_to_assets_zero_shares() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let (vault, _pool, _samwise, _frodo) = setup_mock(&e);
    assert_eq!(
        VaultContractClient::new(&e, &vault).convert_to_assets(&0),
        0
    );
}

/// convert_to_assets on a vault with total_shares == 0 must not trap: it quotes
/// the first-deposit rate (shares 1:1 with bTokens, then through b_rate).
#[test]
fn test_convert_to_assets_empty_vault() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let admin = Address::generate(&e);
    let b_rate = 1_100_000_000_000_i128;
    let pool = mockpool::register_mock_pool_with_b_rate(&e, b_rate).address;
    let reserve = Address::generate(&e);
    let blnd_token = Address::generate(&e);
    // freshly constructed vault: total_shares == 0, total_b_tokens == 0
    let vault = register_blend_vault(&e, &admin, &pool, &reserve, &blnd_token);

    assert_eq!(
        VaultContractClient::new(&e, &vault).convert_to_assets(&1_0000000),
        1_1000000
    );
}

/// convert_to_assets immediately before and immediately after a minimal first
/// deposit must agree.
#[test]
fn test_convert_to_assets_first_deposit_invariant() {
    let e = Env::default();
    let (vault, _usdc, frodo, _samwise) = setup_blend(&e);
    let oz_client = VaultContractClient::new(&e, &vault);

    let probe = 10_0000000_i128;
    let before = oz_client.convert_to_assets(&probe);

    oz_client.deposit(&1_0000000, &frodo, &frodo, &frodo);

    let after = oz_client.convert_to_assets(&probe);
    assert_eq!(before, after);
}

// ── deposit ───────────────────────────────────────────────────────────────────

/// VaultContractClient::deposit matches BlendVault::deposit's signature
/// and can execute a real deposit end-to-end.
#[test]
fn test_deposit_via_contract_client() {
    let e = Env::default();
    let (vault, _usdc, frodo, _samwise) = setup_blend(&e);

    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    let deposit_amount = 1_000_0000000_i128;

    // frodo is both the asset provider and the share receiver
    let shares_minted = oz_client.deposit(&deposit_amount, &frodo, &frodo, &frodo);

    assert!(shares_minted > 0);
    assert_eq!(vault_client.balance(&frodo), shares_minted);
    assert_eq!(
        oz_client.convert_to_assets(&shares_minted),
        vault_client.max_withdraw(&frodo)
    );
}

/// VaultContractClient::deposit supports a split receiver/from: frodo provides
/// the assets but samwise receives the shares.
#[test]
fn test_deposit_split_receiver_and_from() {
    let e = Env::default();
    let (vault, _usdc, frodo, samwise) = setup_blend(&e);

    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    let deposit_amount = 1_000_0000000_i128;

    // frodo pays, samwise receives shares, frodo is the operator
    let shares_minted = oz_client.deposit(&deposit_amount, &samwise, &frodo, &frodo);

    assert!(shares_minted > 0);
    assert_eq!(vault_client.balance(&samwise), shares_minted);
    assert_eq!(vault_client.balance(&frodo), 0);
}

// ── redeem ────────────────────────────────────────────────────────────────────

/// VaultContractClient::redeem matches BlendVault::redeem's signature
/// and can execute a real redemption end-to-end.
#[test]
fn test_redeem_via_contract_client() {
    let e = Env::default();
    let (vault, usdc, frodo, _samwise) = setup_blend(&e);

    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    let deposit_amount = 1_000_0000000_i128;
    let minted = vault_client.deposit(&deposit_amount, &frodo, &frodo, &frodo);

    let balance_before = usdc.balance(&frodo);

    let assets = oz_client.redeem(&(minted / 2), &frodo, &frodo, &frodo);

    assert!(assets > 0);
    assert_eq!(usdc.balance(&frodo), balance_before + assets);
    assert_eq!(vault_client.balance(&frodo), minted - minted / 2);
}

/// VaultContractClient::redeem supports a split receiver/owner: samwise owns
/// the shares but frodo receives the underlying tokens.
#[test]
fn test_redeem_split_receiver_and_owner() {
    let e = Env::default();
    let (vault, usdc, frodo, samwise) = setup_blend(&e);

    let vault_client = BlendVaultClient::new(&e, &vault);
    let oz_client = VaultContractClient::new(&e, &vault);

    let deposit_amount = 1_000_0000000_i128;
    let minted = vault_client.deposit(&deposit_amount, &samwise, &samwise, &samwise);

    let frodo_balance_before = usdc.balance(&frodo);
    let samwise_balance_before = usdc.balance(&samwise);

    // samwise burns shares, frodo receives the underlying
    oz_client.redeem(&(minted / 2), &frodo, &samwise, &samwise);

    assert!(usdc.balance(&frodo) > frodo_balance_before);
    assert_eq!(usdc.balance(&samwise), samwise_balance_before);
}
