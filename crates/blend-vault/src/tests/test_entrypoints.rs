#![cfg(test)]

use crate::{
    storage,
    testutils::{
        create_funded_blend_vault, mockpool, register_blend_vault, EnvTestUtils, MockTokenClient,
    },
    vault::VaultData,
    BlendVaultClient,
};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    vec, Address, Env, IntoVal, String, Symbol,
};
use stellar_tokens::fungible::Base;

const INIT_B_RATE: i128 = 1_000_000_000_000;

/// Registers a vault administered by `admin`, backed by a mock pool at
/// `INIT_B_RATE`. Returns (vault address, vault client, mock pool client).
fn setup<'a>(
    e: &'a Env,
    admin: &Address,
) -> (Address, BlendVaultClient<'a>, mockpool::MockPoolClient<'a>) {
    let pool_client = mockpool::register_mock_pool_with_b_rate(e, INIT_B_RATE);
    let reserve = Address::generate(e);
    let blnd_token = Address::generate(e);
    let vault_address = register_blend_vault(e, admin, &pool_client.address, &reserve, &blnd_token);
    let vault_client = BlendVaultClient::new(e, &vault_address);
    (vault_address, vault_client, pool_client)
}

/// Writes vault state of 1000 bTokens / 1200 shares, split 10% to samwise
/// and 90% to frodo.
fn seed_vault_positions(e: &Env, vault_address: &Address, samwise: &Address, frodo: &Address) {
    e.as_contract(vault_address, || {
        let vault_data = VaultData {
            total_b_tokens: 1000_0000000,
            total_shares: 1200_0000000,
            b_rate: INIT_B_RATE,
            last_update_timestamp: e.ledger().timestamp(),
        };
        storage::set_vault_data(e, &vault_data);
        Base::mint(e, samwise, 120_0000000);
        Base::mint(e, frodo, 1080_0000000);
    });
}

#[test]
fn test_constructor_ok() {
    let e = Env::default();
    e.mock_all_auths();

    let samwise = Address::generate(&e);

    // registered inline (not via `setup`) so the constructor args can be
    // asserted against the recorded authorization below
    let pool = mockpool::register_mock_pool_with_b_rate(&e, INIT_B_RATE).address;
    let reserve = Address::generate(&e);
    let blnd_token = Address::generate(&e);
    let name = String::from_str(&e, "Blend Vault Share");
    let symbol = String::from_str(&e, "bVS");
    let vault_address = e.register(
        crate::BlendVault {},
        (
            samwise.clone(),
            pool.clone(),
            reserve.clone(),
            blnd_token.clone(),
            name.clone(),
            symbol.clone(),
        ),
    );

    assert_eq!(
        e.auths()[0],
        (
            samwise.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    vault_address.clone(),
                    Symbol::new(&e, "__constructor"),
                    vec![
                        &e,
                        samwise.into_val(&e),
                        pool.into_val(&e),
                        reserve.into_val(&e),
                        blnd_token.into_val(&e),
                        name.into_val(&e),
                        symbol.into_val(&e),
                    ]
                )),
                sub_invocations: std::vec![]
            }
        )
    );

    let client = BlendVaultClient::new(&e, &vault_address);
    assert_eq!(client.get_protocol(), pool);
    assert_eq!(client.query_asset(), reserve);
    assert_eq!(client.get_admin(), samwise);
    assert_eq!(client.decimals(), 7);
    assert_eq!(client.name(), name);
    assert_eq!(client.symbol(), symbol);
    assert_eq!(client.total_supply(), 0);
    let vault_data = client.get_vault();
    assert_eq!(vault_data.total_b_tokens, 0);
    assert_eq!(vault_data.total_shares, 0);
    assert_eq!(vault_data.b_rate, INIT_B_RATE);
    assert_eq!(vault_data.last_update_timestamp, e.ledger().timestamp());
}

#[test]
fn test_get_b_tokens() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let samwise = Address::generate(&e);
    let frodo = Address::generate(&e);

    let (vault_address, vault_client, mock_client) = setup(&e, &samwise);
    seed_vault_positions(&e, &vault_address, &samwise, &frodo);

    assert_eq!(vault_client.get_b_tokens(&samwise), 100_0000000);
    assert_eq!(vault_client.get_b_tokens(&frodo), 900_0000000);

    // b_rate increases by 10%; without fees all b_tokens stay with depositors
    mock_client.set_b_rate(&1_100_000_000_000);
    e.jump(5);

    assert_eq!(vault_client.get_b_tokens(&samwise), 100_0000000);
    assert_eq!(vault_client.get_b_tokens(&frodo), 900_0000000);

    // The view function shouldn't mutate the state
    e.as_contract(&vault_address, || {
        let reserve_vault = storage::get_vault_data(&e);
        assert_eq!(reserve_vault.total_b_tokens, 1000_0000000);
        assert_eq!(reserve_vault.total_shares, 1200_0000000);
        assert_eq!(reserve_vault.b_rate, INIT_B_RATE);
    });

    // Should return 0 if user doesn't have any shares
    let non_existent_user = Address::generate(&e);
    assert_eq!(vault_client.get_b_tokens(&non_existent_user), 0);
}

/// `max_withdraw` is the holder's share of the vault's underlying value and
/// grows with the rate; `total_assets` is the whole vault's.
#[test]
fn test_max_withdraw_and_total_assets() {
    let e = Env::default();
    e.mock_all_auths();
    e.set_default_info();

    let samwise = Address::generate(&e);
    let frodo = Address::generate(&e);

    let (vault_address, vault_client, mock_client) = setup(&e, &samwise);
    seed_vault_positions(&e, &vault_address, &samwise, &frodo);

    let total = 1000_0000000; // 1000 bTokens at b_rate 1.0
    assert_eq!(vault_client.total_assets(), total);
    let frodo_value = vault_client.max_withdraw(&frodo);
    let samwise_value = vault_client.max_withdraw(&samwise);
    assert_eq!(frodo_value + samwise_value, total);
    assert_eq!(frodo_value, 9 * samwise_value);

    // b_rate increases by 10%; all yield goes to depositors with no fee
    mock_client.set_b_rate(&1_100_000_000_000);
    e.jump(5);

    assert_eq!(vault_client.total_assets(), 110 * total / 100);
    assert_eq!(vault_client.max_withdraw(&frodo), 110 * frodo_value / 100);
    assert_eq!(
        vault_client.max_withdraw(&samwise),
        110 * samwise_value / 100
    );

    let non_existent_user = Address::generate(&e);
    assert_eq!(vault_client.max_withdraw(&non_existent_user), 0);
}

/// Against a real pool, `max_deposit` is the reserve's remaining supply cap.
#[test]
fn test_max_deposit_reports_reserve_headroom() {
    let e = Env::default();
    let (vault, usdc) = create_funded_blend_vault(&e);
    let vault_client = BlendVaultClient::new(&e, &vault);
    let user = Address::generate(&e);

    let before = vault_client.max_deposit(&user);
    assert!(before > 0);
    assert_eq!(vault_client.total_assets(), 0);

    let deposit = 1_000_0000000;
    MockTokenClient::new(&e, &usdc).mint(&user, &deposit);
    vault_client.deposit(&deposit, &user, &user, &user);

    // the deposit consumed exactly its size of headroom, give or take rounding
    let after = vault_client.max_deposit(&user);
    assert!(before - after >= deposit - 1 && before - after <= deposit + 1);
    assert!(vault_client.total_assets() >= deposit - 1);
}

#[test]
fn test_set_admin() {
    let e = Env::default();
    e.mock_all_auths();

    let samwise = Address::generate(&e);
    let frodo = Address::generate(&e);

    let (vault_address, vault_client, _mock_client) = setup(&e, &samwise);

    e.as_contract(&vault_address, || {
        assert_eq!(storage::get_admin(&e), samwise.clone());
    });

    vault_client.set_admin(&frodo);

    let authorized_function = AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            vault_address.clone(),
            Symbol::new(&e, "set_admin"),
            vec![&e, frodo.into_val(&e)],
        )),
        sub_invocations: std::vec![],
    };
    assert_eq!(
        e.auths(),
        std::vec![
            (samwise.clone(), authorized_function.clone()),
            (frodo.clone(), authorized_function)
        ]
    );

    e.as_contract(&vault_address, || {
        assert_eq!(storage::get_admin(&e), frodo);
    });

    let new_admin = Address::generate(&e);
    vault_client.set_admin(&new_admin);

    let new_authorized_function = AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            vault_address.clone(),
            Symbol::new(&e, "set_admin"),
            vec![&e, new_admin.into_val(&e)],
        )),
        sub_invocations: std::vec![],
    };
    assert_eq!(
        e.auths(),
        std::vec![
            (frodo.clone(), new_authorized_function.clone()),
            (new_admin.clone(), new_authorized_function)
        ]
    );
}
