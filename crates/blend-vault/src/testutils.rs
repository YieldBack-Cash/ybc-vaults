//! Test support: a real Blend deployment from the vendored binaries, a mock
//! pool for unit tests, and the fixture the shared conformance suite runs on.
//!
//! `BlendFixture` is a reimplementation of the one in `blend-contract-sdk`'s
//! `testutils` (which cannot be linked on soroban-sdk 26) against the same
//! WASMs, imported in `crate::blend`.

use crate::blend::{
    backstop, comet, emitter,
    pool::{Client as PoolClient, Request, ReserveConfig, ReserveEmissionMetadata},
    pool_factory,
};
use crate::constants::SCALAR_7;
use crate::storage::ONE_DAY_LEDGERS;
use crate::BlendVault;
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Ledger as _, LedgerInfo},
    token::{StellarAssetClient, TokenClient},
    vec, Address, BytesN, Env, String, Vec,
};

// ── arithmetic ──────────────────────────────────────────────────────────────

/// `floor(x * y / denominator)` for the non-negative values tests use.
pub fn fixed_mul_floor(x: i128, y: i128, denominator: i128) -> i128 {
    x * y / denominator
}

/// `floor(x * denominator / y)` for the non-negative values tests use.
pub fn fixed_div_floor(x: i128, y: i128, denominator: i128) -> i128 {
    x * denominator / y
}

pub fn assert_approx_eq_abs(a: i128, b: i128, delta: i128) {
    assert!(
        a > b - delta && a < b + delta,
        "assertion failed: `(left != right)` \
         (left: `{:?}`, right: `{:?}`, epsilon: `{:?}`)",
        a,
        b,
        delta
    );
}

/// Asserts `a` is within `percentage` of `b`, where `percentage` is a
/// fixed-point number with 7 decimal places.
pub fn assert_approx_eq_rel(a: i128, b: i128, percentage: i128) {
    let rel_delta = fixed_mul_floor(b, percentage, SCALAR_7);
    assert!(
        a > b - rel_delta && a < b + rel_delta,
        "assertion failed: `(left != right)` \
         (left: `{:?}`, right: `{:?}`, epsilon: `{:?}`)",
        a,
        b,
        rel_delta
    );
}

// ── tokens ──────────────────────────────────────────────────────────────────

/// Mint-and-balance handle on a Stellar asset contract, so tests read the
/// same as they did against `sep_41_token`'s mock.
pub struct MockTokenClient<'a> {
    pub address: Address,
    sac: StellarAssetClient<'a>,
    token: TokenClient<'a>,
}

impl<'a> MockTokenClient<'a> {
    pub fn new(e: &'a Env, address: &Address) -> Self {
        MockTokenClient {
            address: address.clone(),
            sac: StellarAssetClient::new(e, address),
            token: TokenClient::new(e, address),
        }
    }

    pub fn mint(&self, to: &Address, amount: &i128) {
        self.sac.mint(to, amount);
    }

    pub fn balance(&self, id: &Address) -> i128 {
        self.token.balance(id)
    }
}

// ── vault ───────────────────────────────────────────────────────────────────

/// Registers a blend vault contract with default share-token metadata.
pub fn register_blend_vault(
    e: &Env,
    admin: &Address,
    pool: &Address,
    asset: &Address,
    blnd_token: &Address,
) -> Address {
    e.register(
        BlendVault {},
        (
            admin.clone(),
            pool.clone(),
            asset.clone(),
            blnd_token.clone(),
            String::from_str(e, "Blend Vault Share"),
            String::from_str(e, "bVS"),
        ),
    )
}

/// Create a test blend vault backed by a mock pool.
/// If no initial b_rate is provided, defaults to 1_100_000_000_000.
///
/// Returns (vault address, mock pool address, mock token address)
pub fn create_test_blend_vault(
    e: &Env,
    admin: &Address,
    b_rate: Option<i128>,
) -> (Address, Address, Address) {
    let pool =
        mockpool::register_mock_pool_with_b_rate(e, b_rate.unwrap_or(1_100_000_000_000)).address;
    let asset = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let blnd_token = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let vault = register_blend_vault(e, admin, &pool, &asset, &blnd_token);
    (vault, pool, asset)
}

// ── Blend protocol ──────────────────────────────────────────────────────────

/// Fixture for deploying and interacting with the Blend Protocol contracts in
/// tests. A port of `blend_contract_sdk::testutils::BlendFixture` onto the
/// vendored binaries.
pub struct BlendFixture<'a> {
    pub backstop: backstop::Client<'a>,
    pub emitter: emitter::Client<'a>,
    pub backstop_token: comet::Client<'a>,
    pub pool_factory: pool_factory::Client<'a>,
}

impl<'a> BlendFixture<'a> {
    /// Deploy a new set of Blend Protocol contracts. Mints 200k backstop
    /// tokens to the deployer that can be used in the future to create up to 4
    /// reward zone pools (50k tokens each).
    ///
    /// This function also resets the env budget via `reset_unlimited`.
    pub fn deploy(
        env: &Env,
        deployer: &Address,
        blnd: &Address,
        usdc: &Address,
    ) -> BlendFixture<'a> {
        env.cost_estimate().budget().reset_unlimited();
        let emitter = env.register(emitter::WASM, ());
        let backstop = Address::generate(env);
        let pool_factory = Address::generate(env);
        let comet = env.register(comet::WASM, ());
        let blnd_client = StellarAssetClient::new(env, blnd);
        let usdc_client = StellarAssetClient::new(env, usdc);
        blnd_client
            .mock_all_auths()
            .mint(deployer, &(1_000_0000000 * 2001));
        usdc_client
            .mock_all_auths()
            .mint(deployer, &(25_0000000 * 2001));

        let comet_client: comet::Client<'a> = comet::Client::new(env, &comet);
        comet_client.mock_all_auths().init(
            deployer,
            &vec![env, blnd.clone(), usdc.clone()],
            &vec![env, 0_8000000, 0_2000000],
            &vec![env, 1_000_0000000, 25_0000000],
            &0_0030000,
        );

        comet_client.mock_all_auths().join_pool(
            &199_900_0000000, // finalize mints 100
            &vec![env, 1_000_0000000 * 2000, 25_0000000 * 2000],
            deployer,
        );

        blnd_client.mock_all_auths().set_admin(&emitter);
        let emitter_client: emitter::Client<'a> = emitter::Client::new(env, &emitter);
        emitter_client
            .mock_all_auths()
            .initialize(blnd, &backstop, &comet);

        env.register_at(
            &backstop,
            backstop::WASM,
            (
                comet,
                emitter,
                blnd,
                usdc,
                pool_factory.clone(),
                Vec::<(Address, i128)>::new(env),
            ),
        );
        let backstop_client: backstop::Client<'a> = backstop::Client::new(env, &backstop);

        let pool_hash = env
            .deployer()
            .upload_contract_wasm(crate::blend::pool::WASM);

        env.register_at(
            &pool_factory,
            pool_factory::WASM,
            (pool_factory::PoolInitMeta {
                backstop,
                blnd_id: blnd.clone(),
                pool_hash,
            },),
        );
        let pool_factory_client = pool_factory::Client::new(env, &pool_factory);

        env.cost_estimate().budget().reset_default();

        BlendFixture {
            backstop: backstop_client,
            emitter: emitter_client,
            backstop_token: comet_client,
            pool_factory: pool_factory_client,
        }
    }
}

/// Deploys a pool with usdc (reserve 0) and xlm (reserve 1) at a fixed 10%
/// borrow rate and 0% backstop take rate, starts emissions to every reserve
/// token evenly, and advances a week so the first emission cycle has run.
pub fn create_blend_pool(
    e: &Env,
    blend_fixture: &BlendFixture,
    admin: &Address,
    usdc: &MockTokenClient,
    xlm: &MockTokenClient,
) -> Address {
    usdc.mint(admin, &200_000_0000000);
    xlm.mint(admin, &200_000_0000000);

    // $1.00 usdc, $0.01 xlm
    let (oracle, oracle_client) = create_mock_oracle(e);
    oracle_client.set_price(&usdc.address, &1_000_0000);
    oracle_client.set_price(&xlm.address, &100_0000);

    let salt = BytesN::<32>::random(e);
    let pool = blend_fixture.pool_factory.deploy(
        admin,
        &String::from_str(e, "TEST"),
        &salt,
        &oracle,
        &0,
        &4,
        &1_0000000,
    );
    let pool_client = PoolClient::new(e, &pool);
    blend_fixture
        .backstop
        .deposit(admin, &pool, &50_000_0000000);
    let reserve_config = ReserveConfig {
        c_factor: 900_0000,
        decimals: 7,
        index: 0,
        l_factor: 900_0000,
        max_util: 900_0000,
        reactivity: 0,
        r_base: 100_0000,
        r_one: 0,
        r_two: 0,
        r_three: 0,
        util: 0,
        supply_cap: i64::MAX as i128,
        enabled: true,
    };
    pool_client.queue_set_reserve(&usdc.address, &reserve_config);
    pool_client.set_reserve(&usdc.address);
    pool_client.queue_set_reserve(&xlm.address, &reserve_config);
    pool_client.set_reserve(&xlm.address);
    let emission_config = vec![
        e,
        ReserveEmissionMetadata {
            res_index: 0,
            res_type: 0,
            share: 250_0000,
        },
        ReserveEmissionMetadata {
            res_index: 0,
            res_type: 1,
            share: 250_0000,
        },
        ReserveEmissionMetadata {
            res_index: 1,
            res_type: 0,
            share: 250_0000,
        },
        ReserveEmissionMetadata {
            res_index: 1,
            res_type: 1,
            share: 250_0000,
        },
    ];
    pool_client.set_emissions_config(&emission_config);
    pool_client.set_status(&0);
    blend_fixture.backstop.add_reward(&pool, &None);

    // wait a week and start emissions
    e.jump(ONE_DAY_LEDGERS * 7);
    blend_fixture.emitter.distribute();
    blend_fixture.backstop.distribute();
    pool
}

/// Supplies 200k usdc and 200k xlm to `pool` as `admin`, then borrows
/// `usdc_borrow` usdc and 100k xlm against it to establish the pool's
/// utilization rate (~50% with 100k usdc borrowed).
pub fn setup_pool_util_rate(
    e: &Env,
    pool: &Address,
    admin: &Address,
    usdc: &Address,
    xlm: &Address,
    usdc_borrow: i128,
) {
    PoolClient::new(e, pool).mock_all_auths().submit(
        admin,
        admin,
        admin,
        &vec![
            e,
            Request {
                address: usdc.clone(),
                amount: 200_000_0000000,
                request_type: 2,
            },
            Request {
                address: usdc.clone(),
                amount: usdc_borrow,
                request_type: 4,
            },
            Request {
                address: xlm.clone(),
                amount: 200_000_0000000,
                request_type: 2,
            },
            Request {
                address: xlm.clone(),
                amount: 100_000_0000000,
                request_type: 4,
            },
        ],
    );
}

/// Everything `create_funded_blend_vault` stands up.
pub struct FundedBlendVault {
    pub vault: Address,
    pub usdc: Address,
    pub xlm: Address,
    pub pool: Address,
    pub admin: Address,
}

/// Deploys a real Blend deployment, a pool holding usdc and xlm at ~50%
/// utilization, and a vault over the pool's usdc reserve. Unlike the mock pool,
/// this exercises the real `submit` path, so supply and withdraw actually move
/// tokens and the pool's own rounding applies.
///
/// Mocks all auths and advances the ledger by a day to accrue interest.
pub fn create_funded_blend_vault_full(e: &Env) -> FundedBlendVault {
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(e);
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

    setup_pool_util_rate(e, &pool, &bombadil, &usdc, &xlm, 100_000_0000000);
    e.jump(ONE_DAY_LEDGERS);

    FundedBlendVault {
        vault,
        usdc,
        xlm,
        pool,
        admin: bombadil,
    }
}

/// See [`create_funded_blend_vault_full`]. Returns (vault address, usdc address).
pub fn create_funded_blend_vault(e: &Env) -> (Address, Address) {
    let f = create_funded_blend_vault_full(e);
    (f.vault, f.usdc)
}

// ── ledger ──────────────────────────────────────────────────────────────────

pub trait EnvTestUtils {
    /// Jump the env by the given amount of ledgers. Assumes 5 seconds per ledger.
    fn jump(&self, ledgers: u32);

    /// Jump the env by the given amount of seconds. Increments the sequence by 1.
    fn jump_time(&self, seconds: u64);

    /// Set the ledger to the default LedgerInfo
    ///
    /// Time -> 1441065600 (Sept 1st, 2015 12:00:00 AM UTC)
    /// Sequence -> 100
    fn set_default_info(&self);
}

impl EnvTestUtils for Env {
    fn jump(&self, ledgers: u32) {
        self.ledger().set(LedgerInfo {
            timestamp: self.ledger().timestamp().saturating_add(ledgers as u64 * 5),
            protocol_version: 26,
            sequence_number: self.ledger().sequence().saturating_add(ledgers),
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 30 * ONE_DAY_LEDGERS,
            min_persistent_entry_ttl: 30 * ONE_DAY_LEDGERS,
            max_entry_ttl: 365 * ONE_DAY_LEDGERS,
        });
    }

    fn jump_time(&self, seconds: u64) {
        self.ledger().set(LedgerInfo {
            timestamp: self.ledger().timestamp().saturating_add(seconds),
            protocol_version: 26,
            sequence_number: self.ledger().sequence().saturating_add(1),
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 30 * ONE_DAY_LEDGERS,
            min_persistent_entry_ttl: 30 * ONE_DAY_LEDGERS,
            max_entry_ttl: 365 * ONE_DAY_LEDGERS,
        });
    }

    fn set_default_info(&self) {
        self.ledger().set(LedgerInfo {
            timestamp: 1441065600, // Sept 1st, 2015 12:00:00 AM UTC
            protocol_version: 26,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 30 * ONE_DAY_LEDGERS,
            min_persistent_entry_ttl: 30 * ONE_DAY_LEDGERS,
            max_entry_ttl: 365 * ONE_DAY_LEDGERS,
        });
    }
}

// ── oracle ──────────────────────────────────────────────────────────────────

/// SEP-40 types, XDR-equivalent to the ones the pool expects. Soroban encodes
/// `contracttype` values by field and variant name, so these are
/// wire-compatible as long as the names match.
#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum Asset {
    Stellar(Address),
    Other(soroban_sdk::Symbol),
}

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

/// A SEP-40 price oracle that reports whatever `set_price` stored.
pub mod mock_oracle {
    use super::{Asset, PriceData};
    use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        /// Set the USD price for an asset (7 decimals, e.g. $1.00 = 1_000_0000).
        pub fn set_price(e: Env, asset: Address, price: i128) {
            e.storage().persistent().set(&asset, &price);
        }

        pub fn base(e: Env) -> Asset {
            Asset::Other(Symbol::new(&e, "USD"))
        }

        pub fn decimals(_e: Env) -> u32 {
            7
        }

        pub fn resolution(_e: Env) -> u32 {
            300
        }

        pub fn lastprice(e: Env, asset: Asset) -> Option<PriceData> {
            if let Asset::Stellar(addr) = asset {
                let price: Option<i128> = e.storage().persistent().get(&addr);
                price.map(|p| PriceData {
                    price: p,
                    timestamp: e.ledger().timestamp(),
                })
            } else {
                None
            }
        }

        pub fn price(e: Env, asset: Asset, _timestamp: u64) -> Option<PriceData> {
            Self::lastprice(e, asset)
        }

        pub fn prices(e: Env, asset: Asset, _records: u32) -> Option<Vec<PriceData>> {
            let data = Self::lastprice(e.clone(), asset)?;
            let mut v = Vec::new(&e);
            v.push_back(data);
            Some(v)
        }
    }
}

pub fn create_mock_oracle<'a>(e: &Env) -> (Address, mock_oracle::MockOracleClient<'a>) {
    let contract_id = e.register(mock_oracle::MockOracle {}, ());
    (
        contract_id.clone(),
        mock_oracle::MockOracleClient::new(e, &contract_id),
    )
}

// ── conformance ─────────────────────────────────────────────────────────────

/// The shared conformance suite, run on a real Blend pool. See
/// `tests/conformance.rs`.
pub struct BlendConformanceFixture {
    pub e: Env,
    pub vault: Address,
    pub asset: Address,
    pub admin: Address,
    pub pool: Address,
}

impl BlendConformanceFixture {
    pub fn new() -> Self {
        let e = Env::default();
        let f = create_funded_blend_vault_full(&e);
        BlendConformanceFixture {
            e,
            vault: f.vault,
            asset: f.usdc,
            admin: f.admin,
            pool: f.pool,
        }
    }
}

impl vault_testkit::ConformanceFixture for BlendConformanceFixture {
    fn env(&self) -> &Env {
        &self.e
    }

    fn vault(&self) -> Address {
        self.vault.clone()
    }

    fn asset(&self) -> Address {
        self.asset.clone()
    }

    fn mint(&self, to: &Address, amount: i128) {
        StellarAssetClient::new(&self.e, &self.asset).mint(to, &amount);
    }

    /// Real interest, not a dial: a month at ~50% utilization moves `b_rate`
    /// by a fraction of a percent, which is all the suite asserts on. Blend
    /// only writes a new `b_rate` on a pool interaction, so a stranger's tiny
    /// supply pokes it.
    fn accrue(&self, _bps: i128) {
        self.e.jump(ONE_DAY_LEDGERS * 30);
        let poke = Address::generate(&self.e);
        StellarAssetClient::new(&self.e, &self.asset).mint(&poke, &1_0000000);
        PoolClient::new(&self.e, &self.pool).submit(
            &poke,
            &poke,
            &poke,
            &vec![
                &self.e,
                Request {
                    address: self.asset.clone(),
                    amount: 1_0000000,
                    request_type: 0,
                },
            ],
        );
    }

    /// A Blend supply position cannot be written down from outside the pool.
    fn write_down(&self, _bps: i128) -> bool {
        false
    }
}

// ── mocks ───────────────────────────────────────────────────────────────────

/// Mock Soroswap router for claim_emissions integration tests.
/// Performs a 1:1 swap of token_in → token_out from its pre-funded balance.
pub mod mocksoroswap {
    use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Env, Vec};

    #[contract]
    pub struct MockSoroswapRouter;

    #[contractimpl]
    impl MockSoroswapRouter {
        pub fn swap_exact_tokens_for_tokens(
            e: Env,
            amount_in: i128,
            _amount_out_min: i128,
            path: Vec<Address>,
            to: Address,
            _deadline: u64,
        ) -> Vec<i128> {
            let token_out = path.get(1).unwrap();
            TokenClient::new(&e, &token_out).transfer(
                &e.current_contract_address(),
                &to,
                &amount_in,
            );
            soroban_sdk::vec![&e, amount_in, amount_in]
        }
    }

    pub fn register_mock_soroswap_router(e: &soroban_sdk::Env) -> MockSoroswapRouterClient<'_> {
        let addr = e.register(MockSoroswapRouter {}, ());
        MockSoroswapRouterClient::new(e, &addr)
    }
}

/// Mock pool to test b_rate updates. Only the reserve read-side is
/// implemented, so it cannot back a real deposit; see the real fixture above
/// for that.
pub mod mockpool {

    use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

    use crate::constants::SCALAR_7;

    const BRATE: Symbol = symbol_short!("b_rate");
    const CONFIG: Symbol = symbol_short!("config");
    const DATA: Symbol = symbol_short!("data");
    const BACKSTOP_RATE: Symbol = symbol_short!("backstop");

    #[derive(Clone, Debug)]
    #[contracttype]
    pub struct Reserve {
        pub asset: Address,        // the underlying asset address
        pub config: ReserveConfig, // the reserve configuration
        pub data: ReserveData,     // the reserve data
        pub scalar: i128,
    }

    #[derive(Clone, Debug, Default)]
    #[contracttype]
    pub struct ReserveConfig {
        pub index: u32,       // the index of the reserve in the list
        pub decimals: u32,    // the decimals used in both the bToken and underlying contract
        pub c_factor: u32, // the collateral factor for the reserve scaled expressed in 7 decimals
        pub l_factor: u32, // the liability factor for the reserve scaled expressed in 7 decimals
        pub util: u32,     // the target utilization rate scaled expressed in 7 decimals
        pub max_util: u32, // the maximum allowed utilization rate scaled expressed in 7 decimals
        pub r_base: u32, // the R0 value (base rate) in the interest rate formula scaled expressed in 7 decimals
        pub r_one: u32,  // the R1 value in the interest rate formula scaled expressed in 7 decimals
        pub r_two: u32,  // the R2 value in the interest rate formula scaled expressed in 7 decimals
        pub r_three: u32, // the R3 value in the interest rate formula scaled expressed in 7 decimals
        pub reactivity: u32, // the reactivity constant for the reserve scaled expressed in 7 decimals
        pub supply_cap: i128, // the total amount of underlying tokens that can be used as collateral
        pub enabled: bool,    // the flag of the reserve
    }

    #[derive(Clone, Debug, Default)]
    #[contracttype]
    pub struct ReserveData {
        pub d_rate: i128,   // the conversion rate from dToken to underlying with 12 decimals
        pub b_rate: i128,   // the conversion rate from bToken to underlying with 12 decimals
        pub ir_mod: i128,   // the interest rate curve modifier with 7 decimals
        pub b_supply: i128, // the total supply of b tokens, in the underlying token's decimals
        pub d_supply: i128, // the total supply of d tokens, in the underlying token's decimals
        pub backstop_credit: i128, // the amount of underlying tokens currently owed to the backstop
        pub last_time: u64, // the last block the data was updated
    }

    #[derive(Clone, Debug)]
    #[contracttype]
    pub struct PoolConfig {
        pub oracle: Address,      // the contract address of the oracle
        pub min_collateral: i128, // the minimum amount of collateral required to open a liability position
        pub bstop_rate: u32, // the rate the backstop takes on accrued debt interest, expressed in 7 decimals
        pub status: u32,     // the status of the pool
        pub max_positions: u32, // the maximum number of effective positions a single user can hold, and the max assets an auction can contain
    }

    #[contract]
    pub struct MockPool;

    #[contractimpl]
    impl MockPool {
        /// Set the reserve b_rate. This overrides any set reserve data.
        pub fn set_b_rate(e: Env, b_rate: i128) {
            e.storage().instance().set(&BRATE, &b_rate);
        }

        /// Set the backstop rate
        pub fn set_backstop_rate(e: Env, bstop_rate: u32) {
            e.storage().instance().set(&BACKSTOP_RATE, &bstop_rate);
        }

        /// Set the reserve data. Clears any set b_rate
        pub fn set_data(e: Env, data: ReserveData) {
            if e.storage().instance().has(&BRATE) {
                e.storage().instance().remove(&BRATE);
            }
            e.storage().instance().set(&DATA, &data);
        }

        /// Set the reserve config
        pub fn set_config(e: Env, config: ReserveConfig) {
            e.storage().instance().set(&CONFIG, &config);
        }

        /// Note: All functionality only cares about the b_rate, except `max_deposit`.
        pub fn get_reserve(e: Env, reserve: Address) -> Reserve {
            let mut r_data = e
                .storage()
                .instance()
                .get(&DATA)
                .unwrap_or(ReserveData::default());
            if let Some(b_rate) = e.storage().instance().get(&BRATE) {
                r_data.b_rate = b_rate;
            }
            Reserve {
                asset: reserve,
                config: e
                    .storage()
                    .instance()
                    .get(&CONFIG)
                    .unwrap_or(ReserveConfig::default()),
                data: r_data,
                scalar: SCALAR_7,
            }
        }

        /// Note: We are only interested in the bstop_rate and status.
        pub fn get_config(e: Env) -> PoolConfig {
            PoolConfig {
                oracle: e.current_contract_address(),
                min_collateral: 0,
                bstop_rate: e.storage().instance().get(&BACKSTOP_RATE).unwrap_or(0),
                status: 0,
                max_positions: 4,
            }
        }
    }

    pub fn register_mock_pool_with_b_rate(e: &Env, b_rate: i128) -> MockPoolClient<'_> {
        let pool_address = e.register(MockPool {}, ());
        let client = MockPoolClient::new(e, &pool_address);
        client.set_b_rate(&b_rate);
        client
    }

    pub fn register_mock_pool_with_config_and_data(
        e: &Env,
        bstop_rate: u32,
        config: ReserveConfig,
        data: ReserveData,
    ) -> MockPoolClient<'_> {
        let pool_address = e.register(MockPool {}, ());
        let client = MockPoolClient::new(e, &pool_address);
        client.set_backstop_rate(&bstop_rate);
        client.set_config(&config);
        client.set_data(&data);
        client
    }
}
