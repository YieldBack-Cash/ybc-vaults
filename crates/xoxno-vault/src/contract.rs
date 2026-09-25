use soroban_sdk::{
    contract, contractimpl, panic_with_error, token::TokenClient, vec, Address, Env, String,
};
use stellar_tokens::fungible::Base;
use vault_common::{auth::spend_operator_allowance, events, guard::require_positive, ttl};

use crate::errors::XoxnoError;
use crate::events::account_opened;
use crate::lending::constants::{NEW_ACCOUNT, WITHDRAW_ALL};
use crate::lending::controller::HubAssetKey;
use crate::lending::helpers::authorize_transfer_as_current;
use crate::lending::ControllerClient;
use crate::storage::{self, Config};
use crate::vault::{
    assets_to_shares, assets_to_shares_up, ray_scaled_to_shares, shares_to_assets,
    shares_to_assets_up, ASSET_DECIMALS,
};

#[contract]
pub struct XoxnoVault;

// SEP-41 on the same address as the SEP-56 surface, delegated to OpenZeppelin.
vault_common::impl_share_token!(XoxnoVault);

fn hub_asset(cfg: &Config) -> HubAssetKey {
    HubAssetKey {
        asset: cfg.asset.clone(),
        hub_id: cfg.hub_id,
    }
}

/// The vault's scaled position at asset precision, or 0 before it has opened
/// an account. The controller reports a Ray (27-decimal) figure; see
/// `vault::SCALED_UNIT`.
fn scaled_position(controller: &ControllerClient, account_id: u64, key: &HubAssetKey) -> i128 {
    if account_id == NEW_ACCOUNT {
        return 0;
    }
    let raw = controller
        .get_account_positions(&account_id)
        .0
        .get(key.clone())
        .map(|p| p.scaled_amount)
        .unwrap_or(0);
    ray_scaled_to_shares(raw)
}

/// The market's current supply index.
fn supply_index(e: &Env, cfg: &Config) -> i128 {
    ControllerClient::new(e, &cfg.controller)
        .get_market_index(&hub_asset(cfg))
        .supply_index
}

/// Pulls `assets` from `from` and supplies them to XOXNO on the vault's
/// account, opening the account on the first call.
///
/// Returns `(received, credited)`: what actually arrived in the vault, and how
/// many scaled units (at share precision) the supply added to the position.
/// `credited` is **measured**, not computed: XOXNO's own rounding decides it,
/// and a locally computed guess that floored differently would break the
/// `total_supply * SCALED_UNIT <= scaled_position` invariant cumulatively
/// rather than once.
fn supply(e: &Env, cfg: &Config, from: &Address, operator: &Address, assets: i128) -> (i128, i128) {
    let key = hub_asset(cfg);
    let controller = ControllerClient::new(e, &cfg.controller);
    let vault = e.current_contract_address();

    let account_id = storage::get_account_id(e);
    let before = scaled_position(&controller, account_id, &key);

    // Measure what actually arrived rather than trusting `assets`: a
    // fee-on-transfer or rebasing asset would otherwise mint shares against
    // funds the vault never received.
    let token = TokenClient::new(e, &cfg.asset);
    let balance_before = token.balance(&vault);
    if operator == from {
        token.transfer(from, &vault, &assets);
    } else {
        // Delegated entry: the operator spends an allowance the funds-owner
        // granted on the underlying asset.
        token.transfer_from(operator, from, &vault, &assets);
    }
    let received = token.balance(&vault) - balance_before;
    require_positive(e, received);

    // XOXNO's pool pulls the tokens out of this contract during `supply`,
    // so that exact transfer has to be pre-authorized. The argument values
    // must match the call the pool will make.
    authorize_transfer_as_current(e, &cfg.asset, &vault, &cfg.pool, received);

    // `NEW_ACCOUNT` is XOXNO's "open me an account on this spoke" sentinel;
    // the returned id is the vault's from then on.
    let new_id = controller.supply(
        &vault,
        &account_id,
        &cfg.spoke_id,
        &vec![e, (key.clone(), received)],
    );
    if account_id == NEW_ACCOUNT {
        storage::set_account_id(e, new_id);
        account_opened(e, new_id, cfg.spoke_id);
    }

    let credited = scaled_position(&controller, new_id, &key) - before;
    require_positive(e, credited);
    (received, credited)
}

/// Burns exactly `shares` from `owner` and withdraws exactly `assets` from
/// the vault's account to `receiver`. The caller has already decided the two
/// figures and their rounding; this is the shared tail of `redeem` and
/// `withdraw`.
fn exit(
    e: &Env,
    cfg: &Config,
    owner: &Address,
    operator: &Address,
    receiver: &Address,
    shares: i128,
    assets: i128,
) {
    let account_id = storage::get_account_id(e);
    if account_id == NEW_ACCOUNT {
        panic_with_error!(e, XoxnoError::NoAccount);
    }

    // XOXNO reads a withdrawal amount of `WITHDRAW_ALL` (0) as "withdraw
    // everything from this market", so passing it through would empty the
    // entire pooled position. Refuse instead. `withdraw` guards its input
    // positive, so this only trips for a dust `redeem` that floored to zero.
    if assets <= WITHDRAW_ALL {
        panic_with_error!(e, XoxnoError::ZeroAssetRedeem);
    }

    // Delegated exit: the operator spends a share allowance.
    spend_operator_allowance(e, owner, operator, shares);

    // Burns exactly `shares`; panics with OpenZeppelin `InsufficientBalance`
    // if the owner holds fewer. Never clamps: consumers detect a short payout
    // only through their own slippage bound, so a silent clamp would surface
    // as an unexplained failure far from its cause.
    Base::update(e, Some(owner), None, shares);

    ControllerClient::new(e, &cfg.controller).withdraw(
        &e.current_contract_address(),
        &account_id,
        &vec![e, (hub_asset(cfg), assets)],
        &Some(receiver.clone()),
    );

    events::withdraw(e, operator, receiver, owner, assets, shares);
}

#[contractimpl]
impl XoxnoVault {
    /// `hub_id` and `spoke_id` are parameters rather than constants because an
    /// account's spoke binding is permanent: baking the choice into the WASM
    /// would make it unrecoverable without a new binary.
    ///
    /// The `get_market_index` call is a probe: it reverts if that market is not
    /// listed on that hub, so a typo in `hub_id` fails at deployment rather than
    /// at the first deposit. The `decimals()` probe pins the asset precision the
    /// Ray-to-share conversion assumes.
    ///
    /// There is no admin: nothing in this contract is privileged.
    pub fn __constructor(
        e: Env,
        controller: Address,
        asset: Address,
        hub_id: u32,
        spoke_id: u32,
        name: String,
        symbol: String,
    ) {
        if storage::has_config(&e) {
            panic_with_error!(&e, vault_common::VaultError::AlreadyInitialized);
        }
        if TokenClient::new(&e, &asset).decimals() != ASSET_DECIMALS {
            panic_with_error!(&e, XoxnoError::UnsupportedDecimals);
        }

        let client = ControllerClient::new(&e, &controller);
        let key = HubAssetKey {
            asset: asset.clone(),
            hub_id,
        };
        client.get_market_index(&key);
        let pool = client.get_pool_address();

        storage::set_config(
            &e,
            &Config {
                controller,
                pool,
                asset,
                hub_id,
                spoke_id,
            },
        );

        // 7 decimals matches the underlying Stellar asset contract and the
        // 1e7 fixed-point scale consumers assume; there is no virtual offset to
        // account for, because this vault needs no inflation cushion.
        Base::set_metadata(&e, ASSET_DECIMALS, name, symbol);
        ttl::extend_instance_ttl(&e);
    }

    // ── SEP-56: asset and conversions ───────────────────────────────────────

    pub fn query_asset(e: &Env) -> Address {
        storage::get_config(e).asset
    }

    /// Assets backing the whole vault, read from the lending position rather
    /// than from any token balance this contract holds.
    pub fn total_assets(e: &Env) -> i128 {
        let cfg = storage::get_config(e);
        let account_id = storage::get_account_id(e);
        if account_id == NEW_ACCOUNT {
            return 0;
        }
        ControllerClient::new(e, &cfg.controller)
            .get_collateral_amount(&account_id, &hub_asset(&cfg))
    }

    /// `shares * supply_index / RAY`, floored.
    ///
    /// One cross-contract call, and no read of the vault's own supply or
    /// position. That is the whole point of mirroring XOXNO's scaled unit: the
    /// price is the market's index, so it cannot be moved by anything that
    /// happens to this contract's balances, including a donation.
    ///
    /// It is also positive on an empty vault, so a consumer probing the rate at
    /// market creation does not need a bootstrap deposit first.
    pub fn convert_to_assets(e: &Env, shares: i128) -> i128 {
        let cfg = storage::get_config(e);
        shares_to_assets(e, shares, supply_index(e, &cfg))
    }

    /// `assets * RAY / supply_index`, floored.
    pub fn convert_to_shares(e: &Env, assets: i128) -> i128 {
        let cfg = storage::get_config(e);
        assets_to_shares(e, assets, supply_index(e, &cfg))
    }

    // ── SEP-56: deposit ─────────────────────────────────────────────────────

    /// Remaining room under the spoke's supply cap, in assets.
    ///
    /// The cap is market-wide for the spoke, not per-vault, so this reports the
    /// shared headroom, which is the honest figure: another depositor consuming
    /// it first is exactly what would make a deposit revert. Returns 0 when the
    /// market is paused or frozen.
    ///
    /// Not on any hot path: two extra cross-contract reads are fine for a view
    /// that exists so callers can size a deposit instead of discovering the
    /// limit through a revert.
    pub fn max_deposit(e: &Env, _receiver: Address) -> i128 {
        let cfg = storage::get_config(e);
        let key = hub_asset(&cfg);
        let controller = ControllerClient::new(e, &cfg.controller);

        let spoke_asset = controller.get_spoke_asset(&cfg.spoke_id, &key);
        if spoke_asset.paused || spoke_asset.frozen {
            return 0;
        }

        let index = controller.get_market_index(&key).supply_index;
        let supplied_scaled = ray_scaled_to_shares(
            controller
                .get_spoke_usage(&cfg.spoke_id, &key)
                .supplied_scaled_ray,
        );
        let supplied = shares_to_assets(e, supplied_scaled, index);

        if spoke_asset.supply_cap <= supplied {
            0
        } else {
            spoke_asset.supply_cap - supplied
        }
    }

    /// Shares a deposit of `assets` would mint, floored. No fees, so this is
    /// `convert_to_shares`; the actual mint is measured from XOXNO and can only
    /// be this or more.
    pub fn preview_deposit(e: &Env, assets: i128) -> i128 {
        Self::convert_to_shares(e, assets)
    }

    /// Pulls `assets` from `from`, supplies them to XOXNO, and mints the
    /// resulting shares to `receiver`. Returns the shares minted.
    pub fn deposit(
        e: &Env,
        assets: i128,
        receiver: Address,
        from: Address,
        operator: Address,
    ) -> i128 {
        require_positive(e, assets);
        operator.require_auth();
        ttl::extend_instance_ttl(e);

        let cfg = storage::get_config(e);
        let (received, minted) = supply(e, &cfg, &from, &operator, assets);

        Base::mint(e, &receiver, minted);
        events::deposit(e, &operator, &from, &receiver, received, minted);
        minted
    }

    // ── SEP-56: mint ────────────────────────────────────────────────────────

    /// `max_deposit` in shares, floored.
    pub fn max_mint(e: &Env, receiver: Address) -> i128 {
        let cfg = storage::get_config(e);
        let index = supply_index(e, &cfg);
        assets_to_shares(e, Self::max_deposit(e, receiver), index)
    }

    /// Assets needed to mint `shares`, rounded up.
    pub fn preview_mint(e: &Env, shares: i128) -> i128 {
        let cfg = storage::get_config(e);
        shares_to_assets_up(e, shares, supply_index(e, &cfg))
    }

    /// Mints **exactly** `shares` to `receiver`, pulling the assets that costs
    /// (rounded up) from `from`. Returns the assets deposited.
    ///
    /// The supply is measured like `deposit`'s. Rounding up the asset side
    /// means XOXNO credits at least `shares` scaled units; any excess stays
    /// unminted in the position, in every holder's favour. If the protocol
    /// ever credited less, the vault would be minting shares it does not
    /// hold, so that case reverts rather than being papered over.
    pub fn mint(
        e: &Env,
        shares: i128,
        receiver: Address,
        from: Address,
        operator: Address,
    ) -> i128 {
        require_positive(e, shares);
        operator.require_auth();
        ttl::extend_instance_ttl(e);

        let cfg = storage::get_config(e);
        let assets = shares_to_assets_up(e, shares, supply_index(e, &cfg));
        let (received, credited) = supply(e, &cfg, &from, &operator, assets);
        if credited < shares {
            panic_with_error!(e, XoxnoError::MintShortfall);
        }

        Base::mint(e, &receiver, shares);
        events::deposit(e, &operator, &from, &receiver, received, shares);
        received
    }

    // ── SEP-56: withdraw ────────────────────────────────────────────────────

    /// What `owner` could redeem right now, in assets.
    pub fn max_withdraw(e: &Env, owner: Address) -> i128 {
        Self::convert_to_assets(e, Base::balance(e, &owner))
    }

    /// Shares a withdrawal of `assets` would burn, rounded up.
    pub fn preview_withdraw(e: &Env, assets: i128) -> i128 {
        let cfg = storage::get_config(e);
        assets_to_shares_up(e, assets, supply_index(e, &cfg))
    }

    /// Withdraws **exactly** `assets` to `receiver`, burning the shares that
    /// costs (rounded up) from `owner`. Returns the shares burned.
    ///
    /// Rounding the share side up at share precision always covers the
    /// ceiling XOXNO applies at Ray precision, so the position never drops by
    /// more than the vault burned. That keeps the crate invariant intact; see
    /// `vault.rs`.
    pub fn withdraw(
        e: &Env,
        assets: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        require_positive(e, assets);
        operator.require_auth();
        ttl::extend_instance_ttl(e);

        let cfg = storage::get_config(e);
        let shares = assets_to_shares_up(e, assets, supply_index(e, &cfg));
        exit(e, &cfg, &owner, &operator, &receiver, shares, assets);
        shares
    }

    // ── SEP-56: redeem ──────────────────────────────────────────────────────

    /// The owner's share balance: every share can be redeemed.
    pub fn max_redeem(e: &Env, owner: Address) -> i128 {
        Base::balance(e, &owner)
    }

    /// Assets a redeem of `shares` would pay, floored. No fees, so this is
    /// `convert_to_assets`, and `redeem` pays exactly this.
    pub fn preview_redeem(e: &Env, shares: i128) -> i128 {
        Self::convert_to_assets(e, shares)
    }

    /// Burns **exactly** `shares` from `owner` and withdraws the corresponding
    /// assets (floored) to `receiver`. Returns the assets paid.
    pub fn redeem(
        e: &Env,
        shares: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        require_positive(e, shares);
        operator.require_auth();
        ttl::extend_instance_ttl(e);

        let cfg = storage::get_config(e);
        let assets = shares_to_assets(e, shares, supply_index(e, &cfg));
        exit(e, &cfg, &owner, &operator, &receiver, shares, assets);
        assets
    }

    // ── views ───────────────────────────────────────────────────────────────

    /// The vault's XOXNO account, or `NEW_ACCOUNT` (0) before the first deposit.
    pub fn account_id(e: &Env) -> u64 {
        storage::get_account_id(e)
    }

    pub fn config(e: &Env) -> Config {
        storage::get_config(e)
    }

    /// The protocol contract this vault supplies to: the XOXNO controller.
    ///
    /// Informational only. Nothing on chain calls it: the YBC indexer reads it
    /// so a curator can confirm which protocol a vault is built on before
    /// listing its markets. Every adapter in the workspace exposes it with
    /// this exact signature. Not part of SEP-56; the underlying asset comes
    /// from the standard's `query_asset`.
    pub fn get_protocol(e: &Env) -> Address {
        storage::get_config(e).controller
    }
}
