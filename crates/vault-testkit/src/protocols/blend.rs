//! A real Blend deployment from the binaries vendored in `wasm/blend/` (see
//! the `MANIFEST.md` there).
//!
//! `blend-contract-sdk` ships an equivalent fixture, but it pins its own
//! `soroban-sdk` and cannot be linked into a workspace on 26; the binaries
//! themselves are SDK-agnostic, so this is a port of that fixture onto them.
//! Two pool shapes are provided: a single-reserve pool at ~50% utilisation for
//! market tests, and the two-reserve pool with emissions the Blend adapter's
//! own harvesting tests need.

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, BytesN as _},
    token::StellarAssetClient,
    vec, Address, BytesN, Env, String, Symbol, Vec,
};

use crate::ledger::{EnvTestUtils, ONE_DAY_LEDGERS};

pub mod pool {
    soroban_sdk::contractimport!(file = "../../wasm/blend/pool.wasm");
}
pub mod pool_factory {
    soroban_sdk::contractimport!(file = "../../wasm/blend/pool_factory.wasm");
}
pub mod backstop {
    soroban_sdk::contractimport!(file = "../../wasm/blend/backstop.wasm");
}
pub mod emitter {
    soroban_sdk::contractimport!(file = "../../wasm/blend/emitter.wasm");
}
pub mod comet {
    soroban_sdk::contractimport!(file = "../../wasm/blend/comet.wasm");
}

// ── SEP-40 oracle ────────────────────────────────────────────────────────────

/// XDR-equivalent to the types the pool expects: Soroban encodes by field and
/// variant name, so these are wire-compatible as long as the names match.
#[contracttype]
#[derive(Clone)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

/// A SEP-40 price oracle that reports whatever `set_price` stored.
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

pub fn create_mock_oracle<'a>(e: &Env) -> (Address, MockOracleClient<'a>) {
    let id = e.register(MockOracle {}, ());
    (id.clone(), MockOracleClient::new(e, &id))
}

// ── protocol ─────────────────────────────────────────────────────────────────

/// The Blend protocol contracts one deployment stands up.
pub struct BlendFixture<'a> {
    pub backstop: backstop::Client<'a>,
    pub emitter: emitter::Client<'a>,
    pub backstop_token: comet::Client<'a>,
    pub pool_factory: pool_factory::Client<'a>,
}

impl<'a> BlendFixture<'a> {
    /// Deploys emitter, Comet backstop token, backstop and pool factory. Mints
    /// 200k backstop tokens to `deployer`, enough to create up to four
    /// reward-zone pools at 50k each.
    ///
    /// Lifts the budget to unlimited and leaves it there: a Blend deployment
    /// is far past the default budget, and every caller wants it lifted for
    /// what follows too.
    pub fn deploy(env: &Env, deployer: &Address, blnd: &Address, usdc: &Address) -> Self {
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

        let pool_hash = env.deployer().upload_contract_wasm(pool::WASM);
        env.register_at(
            &pool_factory,
            pool_factory::WASM,
            (pool_factory::PoolInitMeta {
                backstop,
                blnd_id: blnd.clone(),
                pool_hash,
            },),
        );

        BlendFixture {
            backstop: backstop_client,
            emitter: emitter_client,
            backstop_token: comet_client,
            pool_factory: pool_factory::Client::new(env, &pool_factory),
        }
    }
}

/// The SDK's "good enough" reserve config.
pub fn default_reserve_config() -> pool::ReserveConfig {
    pool::ReserveConfig {
        decimals: 7,
        c_factor: 0_7500000,
        l_factor: 0_7500000,
        util: 0_7500000,
        max_util: 0_9500000,
        r_base: 0_0100000,
        r_one: 0_0500000,
        r_two: 0_5000000,
        r_three: 1_5000000,
        reactivity: 0_0000020,
        index: 0,
        supply_cap: 100_000_000_0000000,
        enabled: true,
    }
}

/// Deploys a pool with one reserve (`underlying`, priced at $1), activates it,
/// and seeds ~50% utilisation so `b_rate` accrues over time. `admin` must
/// already hold enough `underlying` for the seed (20M at 7 decimals).
///
/// The shape YBC market tests want: one asset, real interest, nothing else.
pub fn deploy_pool(env: &Env, protocol: &BlendFixture, admin: &Address, underlying: &Address) -> Address {
    let (oracle, oracle_client) = create_mock_oracle(env);
    oracle_client.set_price(underlying, &1_000_0000);

    let pool_addr = protocol.pool_factory.deploy(
        admin,
        &String::from_str(env, "YBC"),
        &BytesN::random(env),
        &oracle,
        &0u32,          // backstop take rate (0%)
        &4u32,          // max positions
        &1_0000000i128, // min collateral ($1)
    );
    let blend_pool = pool::Client::new(env, &pool_addr);

    blend_pool.queue_set_reserve(underlying, &default_reserve_config());
    blend_pool.set_reserve(underlying);

    protocol.backstop.deposit(admin, &pool_addr, &50_000_0000000i128);
    blend_pool.set_status(&3u32);
    blend_pool.update_status();

    // Admin supplies collateral then borrows at ~50% utilisation.
    blend_pool.submit(
        admin,
        admin,
        admin,
        &vec![
            env,
            pool::Request {
                address: underlying.clone(),
                amount: 10_000_000_0000000i128,
                request_type: 2, // supply as collateral
            },
            pool::Request {
                address: underlying.clone(),
                amount: 5_000_000_0000000i128,
                request_type: 4, // borrow
            },
        ],
    );

    pool_addr
}

/// Deploys a pool with usdc (reserve 0) and xlm (reserve 1) at a fixed 10%
/// borrow rate and 0% backstop take rate, starts emissions to every reserve
/// token evenly, and advances a week so the first emission cycle has run.
///
/// The shape the Blend adapter's emissions tests want. Mints 200k of each
/// asset to `admin` for [`setup_pool_util_rate`].
pub fn create_blend_pool(
    e: &Env,
    fixture: &BlendFixture,
    admin: &Address,
    usdc: &Address,
    xlm: &Address,
) -> Address {
    StellarAssetClient::new(e, usdc).mint(admin, &200_000_0000000);
    StellarAssetClient::new(e, xlm).mint(admin, &200_000_0000000);

    // $1.00 usdc, $0.01 xlm
    let (oracle, oracle_client) = create_mock_oracle(e);
    oracle_client.set_price(usdc, &1_000_0000);
    oracle_client.set_price(xlm, &100_0000);

    let salt = BytesN::<32>::random(e);
    let pool = fixture.pool_factory.deploy(
        admin,
        &String::from_str(e, "TEST"),
        &salt,
        &oracle,
        &0,
        &4,
        &1_0000000,
    );
    let pool_client = pool::Client::new(e, &pool);
    fixture.backstop.deposit(admin, &pool, &50_000_0000000);
    let reserve_config = pool::ReserveConfig {
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
    pool_client.queue_set_reserve(usdc, &reserve_config);
    pool_client.set_reserve(usdc);
    pool_client.queue_set_reserve(xlm, &reserve_config);
    pool_client.set_reserve(xlm);
    let emission_config = vec![
        e,
        pool::ReserveEmissionMetadata {
            res_index: 0,
            res_type: 0,
            share: 250_0000,
        },
        pool::ReserveEmissionMetadata {
            res_index: 0,
            res_type: 1,
            share: 250_0000,
        },
        pool::ReserveEmissionMetadata {
            res_index: 1,
            res_type: 0,
            share: 250_0000,
        },
        pool::ReserveEmissionMetadata {
            res_index: 1,
            res_type: 1,
            share: 250_0000,
        },
    ];
    pool_client.set_emissions_config(&emission_config);
    pool_client.set_status(&0);
    fixture.backstop.add_reward(&pool, &None);

    // wait a week and start emissions
    e.jump(ONE_DAY_LEDGERS * 7);
    fixture.emitter.distribute();
    fixture.backstop.distribute();
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
    pool::Client::new(e, pool).mock_all_auths().submit(
        admin,
        admin,
        admin,
        &vec![
            e,
            pool::Request {
                address: usdc.clone(),
                amount: 200_000_0000000,
                request_type: 2,
            },
            pool::Request {
                address: usdc.clone(),
                amount: usdc_borrow,
                request_type: 4,
            },
            pool::Request {
                address: xlm.clone(),
                amount: 200_000_0000000,
                request_type: 2,
            },
            pool::Request {
                address: xlm.clone(),
                amount: 100_000_0000000,
                request_type: 4,
            },
        ],
    );
}
