#![cfg(test)]

//! `convert_to_assets` against a pool whose `b_rate` can be set directly, so
//! the ratio maths can be pinned at exact values the real pool never lands on.
//! Called through the trait-generated `VaultClient`, the client YBC-side code
//! is written against, rather than this contract's own.

use crate::{
    storage,
    testutils::{
        assert_approx_eq_rel, create_funded_blend_vault, mockpool, register_blend_vault,
        EnvTestUtils, MockTokenClient,
    },
    vault::VaultData,
    BlendVaultClient,
};
use soroban_sdk::{testutils::Address as _, Address, Env};
use stellar_tokens::fungible::Base;
use vault_testkit::VaultClient;

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

/// `convert_to_assets` of a holder's whole balance is their `max_withdraw`.
#[test]
fn test_convert_to_assets_matches_max_withdraw() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let (vault, _pool, samwise, frodo) = setup_mock(&e);
    let vault_client = BlendVaultClient::new(&e, &vault);
    let client = VaultClient::new(&e, &vault);

    let samwise_shares = vault_client.balance(&samwise);
    let frodo_shares = vault_client.balance(&frodo);

    assert_eq!(
        client.convert_to_assets(&samwise_shares),
        vault_client.max_withdraw(&samwise)
    );
    assert_eq!(
        client.convert_to_assets(&frodo_shares),
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
    let client = VaultClient::new(&e, &vault);

    for shares in [
        0_i128,
        1,
        120_0000000,
        600_0000000,
        1080_0000000,
        1200_0000000,
    ] {
        assert_eq!(
            client.convert_to_assets(&shares),
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
    let client = VaultClient::new(&e, &vault);

    let shares = vault_client.balance(&samwise);
    let before = client.convert_to_assets(&shares);

    let mock_client = mockpool::MockPoolClient::new(&e, &pool);
    mock_client.set_b_rate(&2_000_000_000_000_i128);
    e.jump(5);

    let after = client.convert_to_assets(&shares);

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
    assert_eq!(VaultClient::new(&e, &vault).convert_to_assets(&0), 0);
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
        VaultClient::new(&e, &vault).convert_to_assets(&1_0000000),
        1_1000000
    );
}

/// convert_to_assets immediately before and immediately after a minimal first
/// deposit must agree, on a real pool.
#[test]
fn test_convert_to_assets_first_deposit_invariant() {
    let e = Env::default();
    let (vault, usdc) = create_funded_blend_vault(&e);
    let frodo = Address::generate(&e);
    MockTokenClient::new(&e, &usdc).mint(&frodo, &10_000_0000000);
    let client = VaultClient::new(&e, &vault);

    let probe = 10_0000000_i128;
    let before = client.convert_to_assets(&probe);

    client.deposit(&1_0000000, &frodo, &frodo, &frodo);

    let after = client.convert_to_assets(&probe);
    assert_eq!(before, after);
}
