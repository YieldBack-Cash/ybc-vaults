//! Test support for the Blend adapter.
//!
//! The Blend protocol itself (deployer, pools, oracle, ledger movement) lives
//! in `vault_testkit::protocols::blend` and is re-exported here; this module
//! adds only what is this crate's: registering the vault, the fixture the
//! shared conformance suite runs on, and three mocks for unit tests (a pool
//! that is nothing but a settable `b_rate`, a 1:1 swap router, and a router
//! that pays less than it reports).

use crate::blend::pool::PoolDataKey;
use crate::constants::{SCALAR_12, SCALAR_7};
use crate::BlendVault;
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    vec, Address, Env, String,
};

pub use vault_testkit::ledger::{EnvTestUtils, ONE_DAY_LEDGERS};
pub use vault_testkit::protocols::blend::{setup_pool_util_rate, BlendFixture};

use vault_testkit::protocols::blend::{
    self as blend_protocol, pool::Client as PoolClient, pool::Request,
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

/// Mint-and-balance handle on a Stellar asset contract.
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

/// `vault_testkit::protocols::blend::create_blend_pool`, taking this crate's
/// token handles.
pub fn create_blend_pool(
    e: &Env,
    blend_fixture: &BlendFixture,
    admin: &Address,
    usdc: &MockTokenClient,
    xlm: &MockTokenClient,
) -> Address {
    blend_protocol::create_blend_pool(e, blend_fixture, admin, &usdc.address, &xlm.address)
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

    let blend_fixture = BlendFixture::deploy(e, &bombadil, &blnd, &usdc);
    let pool = blend_protocol::create_blend_pool(e, &blend_fixture, &bombadil, &usdc, &xlm);
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

impl Default for BlendConformanceFixture {
    fn default() -> Self {
        Self::new()
    }
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

    /// A default, done the way `tests/test_default.rs` does it: lower the
    /// reserve's `b_rate` in the pool's own storage by `bps`, and move the
    /// lost value onto the debt side so the reserve's books still balance.
    /// Nothing outside the pool can cause this, which is why it is written
    /// straight into storage; the point is that the vault reports it.
    fn write_down(&self, bps: i128) -> bool {
        let pool_client = PoolClient::new(&self.e, &self.pool);
        let reserve = pool_client.get_reserve(&self.asset);
        let pre_supply = fixed_mul_floor(reserve.data.b_rate, reserve.data.b_supply, SCALAR_12);
        self.e.as_contract(&self.pool, || {
            let mut data = reserve.data.clone();
            data.b_rate = data.b_rate - fixed_mul_floor(data.b_rate, bps, 10_000);
            let new_supply = fixed_mul_floor(data.b_supply, data.b_rate, SCALAR_12);
            data.d_supply = fixed_div_floor(pre_supply - new_supply, data.d_rate, SCALAR_12);
            self.e
                .storage()
                .persistent()
                .set(&PoolDataKey::ResData(self.asset.clone()), &data);
        });
        true
    }

    /// The vault's bTokens as the POOL records them, at the pool's rate. The
    /// vault keeps its own `total_b_tokens`; this deliberately does not read
    /// it.
    fn backing(&self) -> i128 {
        let b_tokens =
            crate::pool::vault_b_token_balance(&self.e, &self.pool, &self.asset, &self.vault);
        let b_rate = crate::pool::reserve_b_rate(&self.e, &self.pool, &self.asset);
        fixed_mul_floor(b_tokens, b_rate, SCALAR_12)
    }
}

// ── mocks ───────────────────────────────────────────────────────────────────

/// Mock Soroswap router for claim_emissions integration tests.
/// Performs a 1:1 swap of token_in → token_out from its pre-funded balance.
///
/// It takes the input exactly as Soroswap's router does: `to.require_auth()`,
/// then a `transfer` from `to` to the pair (here, itself), inside the swap.
/// The vault has to have authorised that call; a test that enforces
/// authorisation fails here if it has not.
pub mod mocksoroswap {
    use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Env, Vec};

    #[contract]
    pub struct MockSoroswapRouter;

    #[contractimpl]
    impl MockSoroswapRouter {
        pub fn router_pair_for(e: Env, _token_a: Address, _token_b: Address) -> Address {
            e.current_contract_address()
        }

        pub fn swap_exact_tokens_for_tokens(
            e: Env,
            amount_in: i128,
            _amount_out_min: i128,
            path: Vec<Address>,
            to: Address,
            _deadline: u64,
        ) -> Vec<i128> {
            to.require_auth();
            let me = e.current_contract_address();
            let token_in = path.get(0).unwrap();
            let token_out = path.last().unwrap();
            TokenClient::new(&e, &token_in).transfer(&to, &me, &amount_in);
            TokenClient::new(&e, &token_out).transfer(&me, &to, &amount_in);
            soroban_sdk::vec![&e, amount_in, amount_in]
        }
    }

    pub fn register_mock_soroswap_router(e: &soroban_sdk::Env) -> MockSoroswapRouterClient<'_> {
        let addr = e.register(MockSoroswapRouter {}, ());
        MockSoroswapRouterClient::new(e, &addr)
    }
}

/// A router that does not do what it says: it takes the whole input, pays a
/// fixed `payout` of the output token whatever floor it was given, and
/// reports the floor as met. What the vault must not believe.
pub mod mockshortrouter {
    use soroban_sdk::{
        contract, contractimpl, symbol_short, token::TokenClient, Address, Env, Vec,
    };

    #[contract]
    pub struct MockShortRouter;

    #[contractimpl]
    impl MockShortRouter {
        pub fn __constructor(e: Env, payout: i128) {
            e.storage()
                .instance()
                .set(&symbol_short!("payout"), &payout);
        }

        pub fn router_pair_for(e: Env, _token_a: Address, _token_b: Address) -> Address {
            e.current_contract_address()
        }

        pub fn swap_exact_tokens_for_tokens(
            e: Env,
            amount_in: i128,
            amount_out_min: i128,
            path: Vec<Address>,
            to: Address,
            _deadline: u64,
        ) -> Vec<i128> {
            to.require_auth();
            let router = e.current_contract_address();
            let token_in = path.get(0).unwrap();
            let token_out = path.last().unwrap();
            TokenClient::new(&e, &token_in).transfer(&to, &router, &amount_in);

            let payout: i128 = e
                .storage()
                .instance()
                .get(&symbol_short!("payout"))
                .unwrap();
            if payout > 0 {
                TokenClient::new(&e, &token_out).transfer(&router, &to, &payout);
            }
            soroban_sdk::vec![&e, amount_in, amount_out_min.max(amount_in)]
        }
    }

    pub fn register_mock_short_router(
        e: &soroban_sdk::Env,
        payout: i128,
    ) -> MockShortRouterClient<'_> {
        let addr = e.register(MockShortRouter {}, (payout,));
        MockShortRouterClient::new(e, &addr)
    }
}

/// Mock pool to test b_rate updates: a settable `b_rate` and nothing else.
/// Only the reserve read-side is implemented, so it cannot back a real
/// deposit; see the real fixture above for that.
pub mod mockpool {

    use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

    use crate::constants::SCALAR_7;

    const BRATE: Symbol = symbol_short!("b_rate");

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

    const UTIL: &str = "util";

    #[contractimpl]
    impl MockPool {
        /// Set the reserve b_rate.
        pub fn set_b_rate(e: Env, b_rate: i128) {
            e.storage().instance().set(&BRATE, &b_rate);
        }

        /// Set what the reserve has been lent (`b_supply`) and borrowed
        /// (`d_supply` at `d_rate`); the difference is what `withdraw_limit`
        /// sees as cash on hand.
        pub fn set_utilization(e: Env, b_supply: i128, d_supply: i128, d_rate: i128) {
            e.storage()
                .instance()
                .set(&UTIL, &(b_supply, d_supply, d_rate));
        }

        /// Only `b_rate` and the utilisation are real; the rest of the reserve
        /// is defaulted. Unset, the reserve holds a billion tokens and has lent
        /// none, so nothing here caps a withdrawal.
        pub fn get_reserve(e: Env, reserve: Address) -> Reserve {
            let (b_supply, d_supply, d_rate): (i128, i128, i128) = e
                .storage()
                .instance()
                .get(&UTIL)
                .unwrap_or((1_000_000_000_0000000, 0, 0));
            let data = ReserveData {
                b_rate: e.storage().instance().get(&BRATE).unwrap_or(0),
                b_supply,
                d_supply,
                d_rate,
                ..ReserveData::default()
            };
            Reserve {
                asset: reserve,
                config: ReserveConfig::default(),
                data,
                scalar: SCALAR_7,
            }
        }

        /// An active pool with no backstop take rate.
        pub fn get_config(e: Env) -> PoolConfig {
            PoolConfig {
                oracle: e.current_contract_address(),
                min_collateral: 0,
                bstop_rate: 0,
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
}
