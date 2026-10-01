use crate::blend::pool::Client as PoolClient;
use crate::errors::BlendVaultError;
use crate::testutils::{
    create_blend_pool, mockshortrouter, mocksoroswap, register_blend_vault, setup_pool_util_rate,
    BlendFixture, EnvTestUtils, MockTokenClient,
};
use crate::BlendVaultClient;
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    vec, Address, Env, Error, IntoVal,
};
use vault_testkit::ledger::ONE_DAY_LEDGERS;

/// Signs the next call as `admin` and nothing else: `claim_emissions` is
/// admin-only, and every other signature it needs is the vault's own,
/// granted inside the call. With authorisation otherwise enforced, the
/// router's pull of BLND fails unless the vault authorised exactly that.
fn as_admin(e: &Env, admin: &Address, vault: &Address, amount_out_min: i128) {
    e.mock_auths(&[MockAuth {
        address: admin,
        invoke: &MockAuthInvoke {
            contract: vault,
            fn_name: "claim_emissions",
            args: (amount_out_min,).into_val(e),
            sub_invokes: &[],
        },
    }]);
}

/// Full claim_emissions flow using a mock Soroswap router.
///
/// Emission cycle:
///   1. create_blend_pool sets baseline via emitter.distribute + backstop.distribute (returns 0)
///   2. Vault accrues a supply position
///   3. Jump 7 days → second distribute cycle → backstop.distribute writes rz_emis.accrued > 0
///   4. gulp_emissions → backstop sets eps for next 7 days (rate, not lump sum)
///   5. Jump 3 more days → eps × 3 days accumulates in the emission index
///   6. claim_emissions: pool.claim (BLND) → soroswap swap (BLND→USDC) → pool.supply → more b_tokens
#[test]
fn test_claim_emissions_swaps_blnd_for_underlying() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);

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
    let pool = create_blend_pool(&e, &blend_fixture, &bombadil, &usdc_client, &xlm_client);
    let pool_client = PoolClient::new(&e, &pool);

    // Register mock router and pre-fund it with USDC so it can pay out swaps 1:1
    let router_client = mocksoroswap::register_mock_soroswap_router(&e);
    let router = router_client.address.clone();
    usdc_client.mint(&router, &10_000_000_0000000);

    let vault = register_blend_vault(&e, &bombadil, &pool, &usdc, &blnd);
    let blend_vault_client = BlendVaultClient::new(&e, &vault);
    blend_vault_client.set_router(&router);

    // Establish pool liquidity so the vault's supply position accrues against real utilisation
    setup_pool_util_rate(&e, &pool, &bombadil, &usdc, &xlm, 100_000_0000000);

    // Frodo deposits into vault: vault now has a supply position in the pool
    let deposit = 10_000_0000000_i128;
    usdc_client.mint(&frodo, &deposit);
    blend_vault_client.deposit(&deposit, &frodo, &frodo, &frodo);

    let pool_position_before = pool_client.get_positions(&vault).supply.get(0).unwrap_or(0);
    assert!(
        pool_position_before > 0,
        "vault must have a pool supply position before claiming"
    );

    let b_tokens_before = blend_vault_client.get_vault().total_b_tokens;

    // Second emission cycle; see the doc comment above.
    e.jump(ONE_DAY_LEDGERS * 7);
    blend_fixture.emitter.distribute();
    blend_fixture.backstop.distribute();
    pool_client.gulp_emissions();

    // Let 3 days of eps accumulate in the pool's emission index before claiming
    e.jump(ONE_DAY_LEDGERS * 3);

    // Real authorisation from here, with only the admin's signature supplied.
    as_admin(&e, &bombadil, &vault, 0);

    // claim_emissions: pool.claim(BLND) → soroswap swap(BLND→USDC) → pool.supply → b_tokens
    let underlying_received = blend_vault_client.claim_emissions(&0);
    assert!(
        underlying_received > 0,
        "claim_emissions must return > 0 underlying after emissions accumulated: got {}",
        underlying_received
    );

    let b_tokens_after = blend_vault_client.get_vault().total_b_tokens;
    assert!(
        b_tokens_after > b_tokens_before,
        "vault bToken balance must grow after harvest: before={}, after={}",
        b_tokens_before,
        b_tokens_after,
    );

    let pool_position_after = pool_client.get_positions(&vault).supply.get(0).unwrap_or(0);
    assert!(
        pool_position_after > pool_position_before,
        "pool bToken position must grow: before={}, after={}",
        pool_position_before,
        pool_position_after,
    );
}

/// If no BLND has accrued (vault has never had a supply position), claim_emissions
/// should return 0 without panicking.
#[test]
fn test_claim_emissions_zero_blnd_returns_zero() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);

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
    let pool = create_blend_pool(&e, &blend_fixture, &bombadil, &usdc_client, &xlm_client);

    let router_client = mocksoroswap::register_mock_soroswap_router(&e);
    let router = router_client.address.clone();

    let vault = register_blend_vault(&e, &bombadil, &pool, &usdc, &blnd);
    let blend_vault_client = BlendVaultClient::new(&e, &vault);
    blend_vault_client.set_router(&router);

    // no deposit → no supply position → no BLND accrued
    let result = blend_vault_client.claim_emissions(&0);
    assert_eq!(result, 0);
}

// ── the floor is the vault's to enforce ──────────────────────────────────────

/// A vault with a supply position and BLND emissions ready to claim, whose
/// router pays `payout` of the underlying for any swap and reports the floor
/// as met.
fn vault_with_emissions_and_short_router(e: &Env, payout: i128) -> (BlendVaultClient<'_>, Address) {
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(e);
    let frodo = Address::generate(e);

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
    let pool_client = PoolClient::new(e, &pool);

    let router = mockshortrouter::register_mock_short_router(e, payout).address;
    usdc_client.mint(&router, &10_000_000_0000000);

    let vault = register_blend_vault(e, &bombadil, &pool, &usdc, &blnd);
    let blend_vault_client = BlendVaultClient::new(e, &vault);
    blend_vault_client.set_router(&router);

    setup_pool_util_rate(e, &pool, &bombadil, &usdc, &xlm, 100_000_0000000);

    let deposit = 10_000_0000000_i128;
    usdc_client.mint(&frodo, &deposit);
    blend_vault_client.deposit(&deposit, &frodo, &frodo, &frodo);

    e.jump(ONE_DAY_LEDGERS * 7);
    blend_fixture.emitter.distribute();
    blend_fixture.backstop.distribute();
    pool_client.gulp_emissions();
    e.jump(ONE_DAY_LEDGERS * 3);

    // Authorisation enforced from here; each test signs its claims as the admin.
    e.set_auths(&[]);

    (blend_vault_client, bombadil)
}

/// The router takes the BLND, pays ten stroops and says the floor was met.
/// The vault measures what arrived and refuses.
#[test]
fn test_claim_emissions_enforces_the_floor_on_what_arrived() {
    let e = Env::default();
    let (vault, admin) = vault_with_emissions_and_short_router(&e, 10);

    as_admin(&e, &admin, &vault.address, 1_000_0000000);
    let result = vault.try_claim_emissions(&1_000_0000000);
    assert_eq!(
        result.err(),
        Some(Ok(Error::from_contract_error(
            BlendVaultError::SwapBelowMinimum as u32
        )))
    );

    // One stroop over what the router pays is refused; exactly that is not.
    as_admin(&e, &admin, &vault.address, 11);
    assert_eq!(
        vault.try_claim_emissions(&11).err(),
        Some(Ok(Error::from_contract_error(
            BlendVaultError::SwapBelowMinimum as u32
        )))
    );
    as_admin(&e, &admin, &vault.address, 10);
    assert_eq!(vault.claim_emissions(&10), 10);
}

/// What is returned, supplied and reported is the amount that arrived, not
/// the amount the router answered with.
#[test]
fn test_claim_emissions_returns_what_arrived_not_what_the_router_reports() {
    let e = Env::default();
    let (vault, admin) = vault_with_emissions_and_short_router(&e, 10);

    // The mock reports max(floor, BLND in), far more than the 10 it pays.
    as_admin(&e, &admin, &vault.address, 0);
    assert_eq!(vault.claim_emissions(&0), 10);
}

/// A swap that delivers nothing is refused even with no floor: there is
/// nothing to supply, and the BLND must not be given away for it.
#[test]
fn test_claim_emissions_refuses_a_swap_that_delivers_nothing() {
    let e = Env::default();
    let (vault, admin) = vault_with_emissions_and_short_router(&e, 0);

    as_admin(&e, &admin, &vault.address, 0);
    assert_eq!(
        vault.try_claim_emissions(&0).err(),
        Some(Ok(Error::from_contract_error(
            BlendVaultError::SwapNoOutput as u32
        )))
    );
}

// ── who may harvest, and along which route ───────────────────────────────────

/// Without the admin's signature the harvest is refused before it touches
/// anything: the floor is only meaningful from a party the vault trusts.
#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_claim_emissions_without_admin_auth_reverts() {
    let e = Env::default();
    let (vault, _admin) = vault_with_emissions_and_short_router(&e, 10);

    // No signatures at all (set_auths(&[]) is in force from the helper).
    vault.claim_emissions(&0);
}

/// The route is the admin's to set, must start at BLND and end at the
/// underlying, and the harvest follows it.
#[test]
fn test_swap_path_is_admin_set_and_checked() {
    let e = Env::default();
    let (vault, admin) = vault_with_emissions_and_short_router(&e, 10);
    let blnd = vault.get_swap_path().get(0).unwrap();
    let asset = vault.get_swap_path().last().unwrap();
    let via = Address::generate(&e);

    // Default route is the direct pair.
    assert_eq!(vault.get_swap_path(), vec![&e, blnd.clone(), asset.clone()]);

    // Not the admin: refused.
    assert!(vault
        .try_set_swap_path(&vec![&e, blnd.clone(), via.clone(), asset.clone()])
        .is_err());

    let set_as_admin = |path: soroban_sdk::Vec<Address>| {
        e.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &vault.address,
                fn_name: "set_swap_path",
                args: (path.clone(),).into_val(&e),
                sub_invokes: &[],
            },
        }]);
        vault.try_set_swap_path(&path)
    };

    // Wrong ends or too short: the typed error.
    let invalid = Some(Ok(Error::from_contract_error(
        BlendVaultError::SwapPathInvalid as u32,
    )));
    assert_eq!(set_as_admin(vec![&e, blnd.clone()]).err(), invalid);
    assert_eq!(
        set_as_admin(vec![&e, via.clone(), asset.clone()]).err(),
        invalid
    );
    assert_eq!(
        set_as_admin(vec![&e, blnd.clone(), via.clone()]).err(),
        invalid
    );

    // A valid two-hop route is stored and the harvest completes along it.
    assert!(set_as_admin(vec![&e, blnd.clone(), via.clone(), asset.clone()]).is_ok());
    assert_eq!(vault.get_swap_path(), vec![&e, blnd, via, asset]);
    as_admin(&e, &admin, &vault.address, 10);
    assert_eq!(vault.claim_emissions(&10), 10);
}
