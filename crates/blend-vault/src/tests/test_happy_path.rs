#![cfg(test)]

use crate::blend::pool::{Client as PoolClient, Request};
use crate::constants::SCALAR_12;
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils::{
    assert_approx_eq_abs, create_blend_pool, fixed_div_floor, register_blend_vault,
    setup_pool_util_rate, BlendFixture, EnvTestUtils, MockTokenClient,
};
use crate::BlendVaultClient;
use soroban_sdk::testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation};
use soroban_sdk::{unwrap::UnwrapOptimized, vec, Address, Env, Error, IntoVal, Symbol};

/// OpenZeppelin `FungibleTokenError::InsufficientBalance`.
const INSUFFICIENT_BALANCE: u32 = 100;

#[test]
fn test_happy_path() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let gandalf = Address::generate(&e);
    let frodo = Address::generate(&e);
    let samwise = Address::generate(&e);
    let merry = Address::generate(&e);

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
    let blend_vault_client = BlendVaultClient::new(&e, &blend_vault);

    // Bombadil deposits 200k tokens and borrows 100k tokens for a 50% util rate
    setup_pool_util_rate(&e, &pool, &bombadil, &usdc, &xlm, 100_000_0000000);

    blend_vault_client.set_admin(&gandalf);
    // -> verify set_admin auth
    assert_eq!(
        e.auths()[0],
        (
            bombadil.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    blend_vault.clone(),
                    Symbol::new(&e, "set_admin"),
                    vec![&e, gandalf.to_val(),]
                )),
                sub_invocations: std::vec![]
            }
        )
    );
    assert_eq!(
        e.auths()[1],
        (
            gandalf.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    blend_vault.clone(),
                    Symbol::new(&e, "set_admin"),
                    vec![&e, gandalf.to_val(),]
                )),
                sub_invocations: std::vec![]
            }
        )
    );

    // jump 1 day to accrue some interest for pool
    e.jump(ONE_DAY_LEDGERS);

    /*
     * Deposit into pool
     * -> deposit 100 into blend vault for each frodo and samwise
     * -> deposit 200 into pool for merry
     * -> bombadil borrow from pool to return to 50% util rate
     */
    let pool_usdc_balance_start = usdc_client.balance(&pool);
    let starting_balance = 100_0000000;
    usdc_client.mint(&frodo, &starting_balance);
    usdc_client.mint(&samwise, &starting_balance);

    blend_vault_client.deposit(&starting_balance, &frodo, &frodo, &frodo);
    // -> verify deposit auth: the depositor signs the vault call, and the
    //    pool's transfer of their funds sits under it
    let deposit_request = vec![
        &e,
        Request {
            request_type: 0,
            address: usdc.clone(),
            amount: starting_balance.clone(),
        },
    ];
    assert_eq!(
        e.auths(),
        [(
            frodo.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    blend_vault.clone(),
                    Symbol::new(&e, "deposit"),
                    vec![
                        &e,
                        starting_balance.into_val(&e),
                        frodo.to_val(),
                        frodo.to_val(),
                        frodo.to_val(),
                    ]
                )),
                sub_invocations: std::vec![AuthorizedInvocation {
                    function: AuthorizedFunction::Contract((
                        pool.clone(),
                        Symbol::new(&e, "submit"),
                        vec![
                            &e,
                            blend_vault.to_val(),
                            frodo.to_val(),
                            frodo.to_val(),
                            deposit_request.to_val(),
                        ]
                    )),
                    sub_invocations: std::vec![AuthorizedInvocation {
                        function: AuthorizedFunction::Contract((
                            usdc.clone(),
                            Symbol::new(&e, "transfer"),
                            vec![
                                &e,
                                frodo.to_val(),
                                pool.to_val(),
                                starting_balance.into_val(&e)
                            ]
                        )),
                        sub_invocations: std::vec![]
                    }]
                }]
            }
        )]
    );

    blend_vault_client.deposit(&starting_balance, &samwise, &samwise, &samwise);

    // verify deposit
    assert_eq!(usdc_client.balance(&frodo), 0);
    assert_eq!(usdc_client.balance(&samwise), 0);
    let usdc_reserve = pool_client.get_reserve(&usdc);
    let b_tokens_starting_balance =
        fixed_div_floor(starting_balance, usdc_reserve.data.b_rate, SCALAR_12);
    assert_eq!(
        blend_vault_client.balance(&frodo),
        b_tokens_starting_balance
    );
    assert_eq!(
        blend_vault_client.balance(&samwise),
        b_tokens_starting_balance
    );
    assert_eq!(
        blend_vault_client.total_supply(),
        b_tokens_starting_balance * 2
    );
    assert_eq!(
        usdc_client.balance(&pool),
        pool_usdc_balance_start + starting_balance * 2
    );
    let vault_positions = pool_client.get_positions(&blend_vault);
    assert_eq!(
        vault_positions.supply.get(0).unwrap_optimized(),
        b_tokens_starting_balance * 2
    );

    // merry deposit directly into pool
    let merry_starting_balance = 200_0000000;
    usdc_client.mint(&merry, &merry_starting_balance);
    pool_client.submit(
        &merry,
        &merry,
        &merry,
        &vec![
            &e,
            Request {
                request_type: 0,
                address: usdc.clone(),
                amount: merry_starting_balance,
            },
        ],
    );

    // bombadil borrow back to 50% util rate
    let borrow_amount = (merry_starting_balance + starting_balance * 2) / 2;
    pool_client.submit(
        &bombadil,
        &bombadil,
        &bombadil,
        &vec![
            &e,
            Request {
                request_type: 4,
                address: usdc.clone(),
                amount: borrow_amount,
            },
        ],
    );

    /*
     * Allow 1 week to pass
     */
    e.jump(ONE_DAY_LEDGERS * 7);

    // vault state agrees with the share ledger
    let vault_data = blend_vault_client.get_vault();
    assert_eq!(blend_vault_client.get_protocol(), pool);
    assert_eq!(blend_vault_client.query_asset(), usdc);
    assert_eq!(blend_vault_client.get_admin(), gandalf);
    let frodo_shares = blend_vault_client.balance(&frodo);
    let samwise_shares = blend_vault_client.balance(&samwise);
    assert_eq!(vault_data.total_shares, frodo_shares + samwise_shares);
    assert_eq!(vault_data.total_shares, blend_vault_client.total_supply());

    /*
     * Redeem from pool
     * -> withdraw all funds from pool for merry
     * -> redeem every share for frodo and samwise
     * -> verify a redeem from an empty position fails
     */

    // withdraw all funds from pool for merry
    pool_client.submit(
        &merry,
        &merry,
        &merry,
        &vec![
            &e,
            Request {
                request_type: 1,
                address: usdc.clone(),
                amount: merry_starting_balance * 2,
            },
        ],
    );
    let merry_final_balance = usdc_client.balance(&merry);
    let merry_profit = merry_final_balance - merry_starting_balance;

    // redeem from blend vault for frodo and samwise
    // they are expected to receive half of the profit of merry (no vault fee)
    let expected_frodo_profit = merry_profit / 2;
    let expected_payout = starting_balance + expected_frodo_profit;

    let frodo_paid = blend_vault_client.redeem(&frodo_shares, &frodo, &frodo, &frodo);
    // -> verify redeem auth
    assert_eq!(
        e.auths(),
        [(
            frodo.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    blend_vault.clone(),
                    Symbol::new(&e, "redeem"),
                    vec![
                        &e,
                        frodo_shares.into_val(&e),
                        frodo.to_val(),
                        frodo.to_val(),
                        frodo.to_val(),
                    ]
                )),
                sub_invocations: std::vec![]
            }
        )]
    );

    let samwise_paid = blend_vault_client.redeem(&samwise_shares, &samwise, &samwise, &samwise);

    // -> verify redeem: within a few stroops of the pro-rata share of merry's
    //    profit (the vault and the pool both round down)
    assert_approx_eq_abs(frodo_paid, expected_payout, 10);
    assert_approx_eq_abs(samwise_paid, expected_payout, 10);
    assert_eq!(usdc_client.balance(&frodo), frodo_paid);
    assert_eq!(usdc_client.balance(&samwise), samwise_paid);
    assert_eq!(blend_vault_client.balance(&frodo), 0);
    assert_eq!(blend_vault_client.balance(&samwise), 0);
    assert_eq!(blend_vault_client.total_supply(), 0);

    // -> verify redeem from an empty position fails
    let result = blend_vault_client.try_redeem(&1, &samwise, &samwise, &samwise);
    assert_eq!(
        result.err(),
        Some(Ok(Error::from_contract_error(INSUFFICIENT_BALANCE)))
    );

    // -> verify vault position is empty and fully unwound
    assert!(pool_client.get_positions(&blend_vault).supply.is_empty());
    let reserve_vault = blend_vault_client.get_vault();
    assert_eq!(reserve_vault.total_b_tokens, 0);
    assert_eq!(reserve_vault.total_shares, 0);

    // vault claim_emissions requires a Soroswap router
    let result = blend_vault_client.try_claim_emissions(&0);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(205))));
}
