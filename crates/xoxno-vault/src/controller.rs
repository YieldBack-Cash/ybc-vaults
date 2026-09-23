//! Hand-written client for the XOXNO lending controller.
//!
//! Deliberately *not* a dependency on XOXNO's own crates: those pin their own
//! `soroban-sdk`, and a mismatched SDK makes `Env`/`Address` incompatible across
//! the call boundary. Declaring the surface by hand costs a few dozen lines and
//! removes the version coupling entirely.
//!
//! Signatures verified against the deployed mainnet controller
//! `CAUCMIN5KSXEVZ7NMXR3LZATGD5EFIEUI5XWTFLYRO2R5OTXI22WE5JX` with
//! `stellar contract info interface`.
//!
//! **Field names matter more than field order.** Soroban encodes a
//! `#[contracttype]` struct as an XDR map keyed by field name, so a renamed
//! field fails silently at the call boundary rather than at compile time.

use soroban_sdk::{contractclient, contracttype, Address, Env, Map, Vec};

/// Interest indexes are 27-decimal fixed point.
pub const RAY: i128 = 1_000_000_000_000_000_000_000_000_000;

/// A market coordinate.
///
/// XOXNO has no per-asset contract: one shared pool holds every market,
/// addressed by `(hub_id, asset)`. The same token under two hub ids is two
/// independent markets with separate cash, indexes and rates.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubAssetKey {
    pub asset: Address,
    pub hub_id: u32,
}

/// Raw interest indexes for one market. Both start at [`RAY`] and grow as
/// interest accrues.
///
/// `supply_index` can also be written *down*: XOXNO socializes bad debt by
/// reducing it (`seize_positions`), scoped to the single market rather than the
/// whole pool.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketIndexRaw {
    pub borrow_index: i128,
    pub supply_index: i128,
}

/// One supply position. `scaled_amount` is the non-rebasing unit — it does not
/// change as interest accrues; the descaled balance is
/// `scaled_amount * supply_index / RAY`.
///
/// The risk fields are snapshotted onto the position when it is opened, so a
/// later governance change does not re-price it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountPositionRaw {
    pub liquidation_bonus: u32,
    pub liquidation_fees: u32,
    pub liquidation_threshold: u32,
    pub loan_to_value: u32,
    pub scaled_amount: i128,
}

/// One debt position. The vault never borrows, so this is always empty for us.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebtPositionRaw {
    pub scaled_amount: i128,
}

/// Risk parameters for one `(spoke, market)` pair.
///
/// Note the granularity: these live on the *pair*, not on the spoke. Spoke 1's
/// rules for hub 1 USDC and spoke 5's rules for the same market are separate
/// records.
///
/// The vault never borrows, so most of this is inert for it. What matters is
/// `supply_cap` (the ceiling on how large the vault can grow), `paused` and
/// `frozen` (which halt it), and `no_seize` (whether the position is exempt from
/// bad-debt socialization).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpokeAssetConfig {
    pub borrow_cap: i128,
    pub frozen: bool,
    pub is_borrowable: bool,
    pub is_collateralizable: bool,
    pub liquidation_bonus: u32,
    pub liquidation_fees: u32,
    pub liquidation_threshold: u32,
    pub loan_to_value: u32,
    pub no_seize: bool,
    pub paused: bool,
    pub supply_cap: i128,
}

/// How much of a market one spoke is currently using.
///
/// Caps are enforced per spoke against shared hub liquidity, so this is what
/// makes headroom answerable at all: the cash is pooled, but each spoke's draw
/// on it is metered separately.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpokeUsageRaw {
    pub borrowed_scaled_ray: i128,
    pub supplied_scaled_ray: i128,
}

/// The subset of the controller this vault calls.
///
/// Two behaviours are invisible in the signatures and are the source of the
/// sharpest hazards in this contract:
///
/// * `supply` with `account_id == 0` **creates** an account bound to `spoke_id`
///   and returns its new id. That binding is permanent for the account.
/// * `withdraw` treats an amount of `0` as *withdraw everything* for that
///   market. Never pass a computed zero — see `contract::redeem`.
// The trait itself is never called — it exists so `contractclient` can generate
// `ControllerClient`, which is what the vault actually uses.
#[allow(dead_code)]
#[contractclient(name = "ControllerClient")]
pub trait Controller {
    fn supply(
        env: Env,
        caller: Address,
        account_id: u64,
        spoke_id: u32,
        assets: Vec<(HubAssetKey, i128)>,
    ) -> u64;

    fn withdraw(
        env: Env,
        caller: Address,
        account_id: u64,
        withdrawals: Vec<(HubAssetKey, i128)>,
        to: Option<Address>,
    ) -> Vec<(HubAssetKey, i128)>;

    fn get_collateral_amount(env: Env, account_id: u64, hub_asset: HubAssetKey) -> i128;

    fn get_market_index(env: Env, hub_asset: HubAssetKey) -> MarketIndexRaw;

    fn get_account_positions(
        env: Env,
        account_id: u64,
    ) -> (
        Map<HubAssetKey, AccountPositionRaw>,
        Map<HubAssetKey, DebtPositionRaw>,
    );

    fn account_exists(env: Env, account_id: u64) -> bool;

    fn get_pool_address(env: Env) -> Address;

    fn get_spoke_asset(env: Env, spoke_id: u32, hub_asset: HubAssetKey) -> SpokeAssetConfig;

    fn get_spoke_usage(env: Env, spoke_id: u32, hub_asset: HubAssetKey) -> SpokeUsageRaw;
}
