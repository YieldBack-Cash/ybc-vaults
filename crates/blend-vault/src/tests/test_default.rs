//! A pool default: the reserve's `b_rate` falls and every holder's redeemable
//! value falls with it. The vault must report the loss, not hide it.

use crate::blend::pool::{Client as PoolClient, PoolDataKey};
use crate::constants::{SCALAR_12, SCALAR_7};
use crate::testutils::{
    assert_approx_eq_abs, create_blend_pool, fixed_div_floor, fixed_mul_floor,
    register_blend_vault, setup_pool_util_rate, BlendFixture, EnvTestUtils, MockTokenClient,
};
use crate::BlendVaultClient;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

#[test]
fn test_default() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);
    let samwise = Address::generate(&e);

    let blnd = e
        .register_stellar_asset_contract_v2(bombadil.clone())
        .address();
    let usdc = e
        .register_stellar_asset_contract_v2(bombadil.clone())
        .address();
    let xlm = e
        .register_stellar_asset_contract_v2(bombadil.clone())
        .address();
    let usdc_client = MockTokenClient::new(&e, &usdc);
    let xlm_client = MockTokenClient::new(&e, &xlm);

    let blend_fixture = BlendFixture::deploy(&e, &bombadil, &blnd, &usdc);

    // usdc (0) and xlm (1) charge a fixed 10% borrow rate with 0% backstop take rate
    // emits to each reserve token evenly, and starts emissions
    let pool = create_blend_pool(&e, &blend_fixture, &bombadil, &usdc_client, &xlm_client);
    let pool_client = PoolClient::new(&e, &pool);
    let blend_vault = register_blend_vault(&e, &bombadil, &pool, &usdc, &blnd);
    let vault_client = BlendVaultClient::new(&e, &blend_vault);

    // Bombadil deposits 200k tokens and borrows 105k usdc for a ~52% util rate
    setup_pool_util_rate(&e, &pool, &bombadil, &usdc, &xlm, 105_000_0000000);

    let pool_usdc_balance_start = usdc_client.balance(&pool);

    // have samwise and frodo deposit funds into the vault
    let samwise_deposit: i128 = 1_000_0000000;
    let frodo_deposit: i128 = 9_000_0000000;
    usdc_client.mint(&samwise, &(samwise_deposit * 2));
    usdc_client.mint(&frodo, &(frodo_deposit * 2));

    vault_client.deposit(&samwise_deposit, &samwise, &samwise, &samwise);
    vault_client.deposit(&frodo_deposit, &frodo, &frodo, &frodo);

    assert_eq!(vault_client.max_withdraw(&samwise), samwise_deposit);
    assert_eq!(vault_client.max_withdraw(&frodo), frodo_deposit);
    assert_eq!(
        usdc_client.balance(&pool),
        pool_usdc_balance_start + samwise_deposit + frodo_deposit
    );

    // pass 30 days to accrue some yield (approx 0.41% gain at ~50% util)
    e.jump_time(30 * 86400);

    // have frodo do a 10 stroop deposit to trigger a b_rate update this block
    vault_client.deposit(&10, &frodo, &frodo, &frodo);

    // snapshot underlying values before the default
    let pre_default_frodo = vault_client.max_withdraw(&frodo);
    let pre_default_samwise = vault_client.max_withdraw(&samwise);
    assert!(pre_default_frodo > frodo_deposit, "yield must have accrued");

    let usdc_data = pool_client.get_reserve(&usdc);
    let pre_supply = fixed_mul_floor(usdc_data.data.b_rate, usdc_data.data.b_supply, SCALAR_12);
    // use magic to simulate a default situation of 10%
    e.as_contract(&pool, || {
        let res_data_key = PoolDataKey::ResData(usdc.clone());
        let mut new_res_data = usdc_data.data.clone();
        new_res_data.b_rate = fixed_mul_floor(new_res_data.b_rate, 0_9000000, SCALAR_7);
        let new_supply = fixed_mul_floor(new_res_data.b_supply, new_res_data.b_rate, SCALAR_12);
        new_res_data.d_supply =
            fixed_div_floor(pre_supply - new_supply, new_res_data.d_rate, SCALAR_12);
        e.storage().persistent().set(&res_data_key, &new_res_data);
    });

    // estimate expected loss: 10% b_rate drop applied to pre-default underlying
    let expected_frodo_value = fixed_mul_floor(pre_default_frodo, 0_9000000, SCALAR_7);
    let expected_samwise_value = fixed_mul_floor(pre_default_samwise, 0_9000000, SCALAR_7);

    // the vault reports the loss immediately
    assert_approx_eq_abs(
        vault_client.max_withdraw(&frodo),
        expected_frodo_value,
        0_0010000,
    );

    // frodo exits and takes the expected loss
    let frodo_before = usdc_client.balance(&frodo);
    let frodo_paid = vault_client.redeem(&vault_client.balance(&frodo), &frodo, &frodo, &frodo);
    assert_approx_eq_abs(frodo_paid, expected_frodo_value, 0_0010000);
    assert_eq!(usdc_client.balance(&frodo), frodo_before + frodo_paid);

    // skip some time
    e.jump_time(100);

    // samwise exits and takes the expected loss
    let samwise_paid = vault_client.redeem(
        &vault_client.balance(&samwise),
        &samwise,
        &samwise,
        &samwise,
    );
    assert_approx_eq_abs(samwise_paid, expected_samwise_value, 0_0010000);
}
