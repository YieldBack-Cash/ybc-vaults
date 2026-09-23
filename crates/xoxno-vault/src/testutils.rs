//! A stand-in for the XOXNO controller.
//!
//! Modelled on `blend-vault-v2`'s `mod mockpool`: enough of the real contract to
//! drive the vault, with the market state settable directly so tests can force
//! conditions the real protocol only produces occasionally.
//!
//! Two behaviours are reproduced deliberately, because the vault's correctness
//! arguments depend on them:
//!
//! * **Asymmetric rounding.** A supply credits `floor(amount * RAY / index)`
//!   scaled units; a withdrawal burns `ceil(amount * RAY / index)`. That is what
//!   makes the redeem-side dust land in the vault's favour, and a mock that
//!   floored both ways would let a broken vault pass.
//! * **The zero sentinel.** A withdrawal amount of `0` means *take everything*.
//!   Without this the guard in `redeem` would be untestable.
//!
//! `set_supply_index` accepts a *lower* value than the current one, so the
//! `seize_positions` bad-debt write-down can be simulated.

use soroban_sdk::{contract, contractimpl, contracttype, token::TokenClient, Address, Env, Map, Vec};

use crate::controller::{
    AccountPositionRaw, DebtPositionRaw, HubAssetKey, MarketIndexRaw, SpokeAssetConfig,
    SpokeUsageRaw, RAY,
};
use crate::vault::{assets_to_shares, shares_to_assets};

/// Spoke 1's live mainnet supply cap for hub 1 USDC: 5,000,000 at 7 decimals.
pub const DEFAULT_SUPPLY_CAP: i128 = 50_000_000_000_000;

#[contracttype]
pub enum MockKey {
    Index,
    NextId,
    HubAsset,
    Scaled(u64),
    Spoke(u64),
    TotalScaled,
    SupplyCap,
    Paused,
}

#[contract]
pub struct MockController;

#[contractimpl]
impl MockController {
    pub fn __constructor(e: Env, asset: Address, hub_id: u32) {
        e.storage().instance().set(&MockKey::Index, &RAY);
        e.storage().instance().set(&MockKey::NextId, &1u64);
        e.storage()
            .instance()
            .set(&MockKey::HubAsset, &HubAssetKey { asset, hub_id });
        e.storage()
            .instance()
            .set(&MockKey::SupplyCap, &DEFAULT_SUPPLY_CAP);
        e.storage().instance().set(&MockKey::Paused, &false);
    }

    pub fn set_supply_cap(e: Env, cap: i128) {
        e.storage().instance().set(&MockKey::SupplyCap, &cap);
    }

    pub fn set_paused(e: Env, paused: bool) {
        e.storage().instance().set(&MockKey::Paused, &paused);
    }

    // ── test controls ───────────────────────────────────────────────────────

    /// Move the market's supply index. Raising it simulates interest accrual;
    /// lowering it simulates `seize_positions` socializing bad debt.
    pub fn set_supply_index(e: Env, index: i128) {
        e.storage().instance().set(&MockKey::Index, &index);
    }

    /// Credit an account without any vault share being minted — a third party
    /// supplying into someone else's position.
    pub fn donate(e: Env, from: Address, account_id: u64, amount: i128) {
        let key = Self::hub_asset(e.clone());
        TokenClient::new(&e, &key.asset).transfer(
            &from,
            &e.current_contract_address(),
            &amount,
        );
        let index = Self::index(e.clone());
        let delta = assets_to_shares(&e, amount, index);
        let scaled = Self::scaled(e.clone(), account_id) + delta;
        e.storage()
            .instance()
            .set(&MockKey::Scaled(account_id), &scaled);
        Self::bump_total(&e, delta);
    }

    pub fn scaled(e: Env, account_id: u64) -> i128 {
        e.storage()
            .instance()
            .get(&MockKey::Scaled(account_id))
            .unwrap_or(0)
    }

    // ── controller surface ──────────────────────────────────────────────────

    pub fn supply(
        e: Env,
        caller: Address,
        account_id: u64,
        spoke_id: u32,
        assets: Vec<(HubAssetKey, i128)>,
    ) -> u64 {
        let (key, amount) = assets.get(0).expect("empty payment vector");
        let index = Self::index(e.clone());

        // The vault pre-authorized exactly this transfer before calling.
        TokenClient::new(&e, &key.asset).transfer(
            &caller,
            &e.current_contract_address(),
            &amount,
        );

        let id = if account_id == 0 {
            let next: u64 = e.storage().instance().get(&MockKey::NextId).unwrap_or(1);
            e.storage().instance().set(&MockKey::NextId, &(next + 1));
            e.storage().instance().set(&MockKey::Spoke(next), &spoke_id);
            next
        } else {
            account_id
        };

        // Credit floors — the depositor never gains from rounding.
        let delta = assets_to_shares(&e, amount, index);
        let scaled = Self::scaled(e.clone(), id) + delta;
        e.storage().instance().set(&MockKey::Scaled(id), &scaled);
        Self::bump_total(&e, delta);
        id
    }

    pub fn withdraw(
        e: Env,
        caller: Address,
        account_id: u64,
        withdrawals: Vec<(HubAssetKey, i128)>,
        to: Option<Address>,
    ) -> Vec<(HubAssetKey, i128)> {
        let (key, requested) = withdrawals.get(0).expect("empty withdrawal vector");
        let index = Self::index(e.clone());
        let scaled = Self::scaled(e.clone(), account_id);

        // A requested amount of zero means "everything in this market".
        let amount = if requested == 0 {
            shares_to_assets(&e, scaled, index)
        } else {
            requested
        };

        // The burn ceils — the protocol never loses to rounding.
        let floor = assets_to_shares(&e, amount, index);
        let burn = if shares_to_assets(&e, floor, index) < amount {
            floor + 1
        } else {
            floor
        };
        assert!(burn <= scaled, "withdraw exceeds position");

        e.storage()
            .instance()
            .set(&MockKey::Scaled(account_id), &(scaled - burn));
        Self::bump_total(&e, -burn);

        let recipient = to.unwrap_or(caller);
        TokenClient::new(&e, &key.asset).transfer(
            &e.current_contract_address(),
            &recipient,
            &amount,
        );

        soroban_sdk::vec![&e, (key, amount)]
    }

    pub fn get_collateral_amount(e: Env, account_id: u64, _hub_asset: HubAssetKey) -> i128 {
        let index = Self::index(e.clone());
        shares_to_assets(&e, Self::scaled(e.clone(), account_id), index)
    }

    pub fn get_market_index(e: Env, _hub_asset: HubAssetKey) -> MarketIndexRaw {
        MarketIndexRaw {
            borrow_index: RAY,
            supply_index: Self::index(e),
        }
    }

    pub fn get_account_positions(
        e: Env,
        account_id: u64,
    ) -> (
        Map<HubAssetKey, AccountPositionRaw>,
        Map<HubAssetKey, DebtPositionRaw>,
    ) {
        let key = Self::hub_asset(e.clone());
        let mut supplies = Map::new(&e);
        supplies.set(
            key,
            AccountPositionRaw {
                liquidation_bonus: 400,
                liquidation_fees: 1000,
                liquidation_threshold: 8000,
                loan_to_value: 7600,
                scaled_amount: Self::scaled(e.clone(), account_id),
            },
        );
        (supplies, Map::new(&e))
    }

    pub fn account_exists(e: Env, account_id: u64) -> bool {
        let next: u64 = e.storage().instance().get(&MockKey::NextId).unwrap_or(1);
        account_id != 0 && account_id < next
    }

    /// The mock is its own pool: it holds the cash as well as the accounting.
    pub fn get_pool_address(e: Env) -> Address {
        e.current_contract_address()
    }

    pub fn get_spoke_asset(e: Env, _spoke_id: u32, _hub_asset: HubAssetKey) -> SpokeAssetConfig {
        // Mainnet spoke 1 / hub 1 USDC, as read from the deployed controller.
        SpokeAssetConfig {
            borrow_cap: 37_500_000_000_000,
            frozen: false,
            is_borrowable: true,
            is_collateralizable: true,
            liquidation_bonus: 400,
            liquidation_fees: 1000,
            liquidation_threshold: 8000,
            loan_to_value: 7600,
            no_seize: false,
            paused: e.storage().instance().get(&MockKey::Paused).unwrap_or(false),
            supply_cap: e
                .storage()
                .instance()
                .get(&MockKey::SupplyCap)
                .unwrap_or(DEFAULT_SUPPLY_CAP),
        }
    }

    pub fn get_spoke_usage(e: Env, _spoke_id: u32, _hub_asset: HubAssetKey) -> SpokeUsageRaw {
        SpokeUsageRaw {
            borrowed_scaled_ray: 0,
            supplied_scaled_ray: Self::total_scaled(e),
        }
    }

    pub fn total_scaled(e: Env) -> i128 {
        e.storage()
            .instance()
            .get(&MockKey::TotalScaled)
            .unwrap_or(0)
    }

    // ── internals ───────────────────────────────────────────────────────────

    /// Keeps the spoke's market-wide usage in step with per-account balances,
    /// so `get_spoke_usage` — and therefore `max_deposit` — answers truthfully.
    fn bump_total(e: &Env, delta: i128) {
        let total = Self::total_scaled(e.clone()) + delta;
        e.storage().instance().set(&MockKey::TotalScaled, &total);
    }

    fn index(e: Env) -> i128 {
        e.storage().instance().get(&MockKey::Index).unwrap_or(RAY)
    }

    fn hub_asset(e: Env) -> HubAssetKey {
        e.storage()
            .instance()
            .get(&MockKey::HubAsset)
            .expect("mock not constructed")
    }
}
