use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contractimpl, panic_with_error,
    token::TokenClient,
    vec, Address, Env, IntoVal, MuxedAddress, String, Symbol,
};
use stellar_tokens::fungible::{Base, FungibleToken};

use crate::controller::{ControllerClient, HubAssetKey};
use crate::errors::VaultError;
use crate::events;
use crate::storage::{self, Config};
use crate::vault::shares_to_assets;

#[contract]
pub struct XoxnoVault;

fn hub_asset(cfg: &Config) -> HubAssetKey {
    HubAssetKey {
        asset: cfg.asset.clone(),
        hub_id: cfg.hub_id,
    }
}

/// The vault's scaled position, or 0 before it has opened an account.
fn scaled_position(controller: &ControllerClient, account_id: u64, key: &HubAssetKey) -> i128 {
    if account_id == 0 {
        return 0;
    }
    controller
        .get_account_positions(&account_id)
        .0
        .get(key.clone())
        .map(|p| p.scaled_amount)
        .unwrap_or(0)
}

#[contractimpl]
impl XoxnoVault {
    /// `hub_id` and `spoke_id` are parameters rather than constants because an
    /// account's spoke binding is permanent — baking the choice into the WASM
    /// would make it unrecoverable without a new binary.
    ///
    /// The `get_market_index` call is a probe: it reverts if that market is not
    /// listed on that hub, so a typo in `hub_id` fails at deployment rather than
    /// at the first deposit.
    pub fn __constructor(
        e: Env,
        controller: Address,
        asset: Address,
        admin: Address,
        hub_id: u32,
        spoke_id: u32,
        name: String,
        symbol: String,
    ) {
        if storage::has_config(&e) {
            panic_with_error!(&e, VaultError::AlreadyInitialized);
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
                admin,
                hub_id,
                spoke_id,
            },
        );

        // 7 decimals matches the underlying Stellar asset contract and the
        // 1e7 fixed-point scale consumers assume; there is no virtual offset to
        // account for, because this vault needs no inflation cushion.
        Base::set_metadata(&e, 7, name, symbol);
        storage::extend_instance_ttl(&e);
    }

    // ── SEP-56 ──────────────────────────────────────────────────────────────

    pub fn query_asset(e: &Env) -> Address {
        storage::get_config(e).asset
    }

    /// `shares * supply_index / RAY`, floored.
    ///
    /// One cross-contract call, and no read of the vault's own supply or
    /// position. That is the whole point of mirroring XOXNO's scaled unit: the
    /// price is the market's index, so it cannot be moved by anything that
    /// happens to this contract's balances — including a donation.
    ///
    /// It is also positive on an empty vault, so a consumer probing the rate at
    /// market creation does not need a bootstrap deposit first.
    pub fn convert_to_assets(e: &Env, shares: i128) -> i128 {
        let cfg = storage::get_config(e);
        let index = ControllerClient::new(e, &cfg.controller)
            .get_market_index(&hub_asset(&cfg))
            .supply_index;
        shares_to_assets(e, shares, index)
    }

    /// Assets backing the whole vault, read from the lending position rather
    /// than from any token balance this contract holds.
    pub fn total_assets(e: &Env) -> i128 {
        let cfg = storage::get_config(e);
        let account_id = storage::get_account_id(e);
        if account_id == 0 {
            return 0;
        }
        ControllerClient::new(e, &cfg.controller)
            .get_collateral_amount(&account_id, &hub_asset(&cfg))
    }

    /// Remaining room under the spoke's supply cap, in assets.
    ///
    /// The cap is market-wide for the spoke, not per-vault, so this reports the
    /// shared headroom — the honest figure, since another depositor consuming it
    /// first is exactly what would make a deposit revert. Returns 0 when the
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
        let supplied = shares_to_assets(
            e,
            controller.get_spoke_usage(&cfg.spoke_id, &key).supplied_scaled_ray,
            index,
        );

        if spoke_asset.supply_cap <= supplied {
            0
        } else {
            spoke_asset.supply_cap - supplied
        }
    }

    /// What `owner` could redeem right now, in assets.
    ///
    /// Not consulted by YBC, which calls only the four core functions — but a
    /// caller sizing a withdrawal has no other honest source for this, and
    /// discovering the limit through a revert is worse.
    pub fn max_withdraw(e: &Env, owner: Address) -> i128 {
        Self::convert_to_assets(e, Base::balance(e, &owner))
    }

    /// Pulls `assets` from `from`, supplies them to XOXNO, and mints the
    /// resulting shares to `receiver`.
    ///
    /// The share count is **measured**, not computed: XOXNO's own rounding
    /// decides how many scaled units the supply credited, and a locally computed
    /// guess that floored differently would break the
    /// `total_supply <= scaled_position` invariant cumulatively rather than once.
    pub fn deposit(
        e: &Env,
        assets: i128,
        receiver: Address,
        from: Address,
        operator: Address,
    ) -> i128 {
        if assets <= 0 {
            panic_with_error!(e, VaultError::AmountNotPositive);
        }
        operator.require_auth();
        storage::extend_instance_ttl(e);

        let cfg = storage::get_config(e);
        let key = hub_asset(&cfg);
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
            token.transfer(&from, &vault, &assets);
        } else {
            // Delegated entry: the operator spends an allowance the funds-owner
            // granted on the underlying asset.
            token.transfer_from(&operator, &from, &vault, &assets);
        }
        let received = token.balance(&vault) - balance_before;
        if received <= 0 {
            panic_with_error!(e, VaultError::AmountNotPositive);
        }

        // XOXNO's pool pulls the tokens out of this contract during `supply`,
        // so that exact transfer has to be pre-authorized. The argument values
        // must match the call the pool will make.
        e.authorize_as_current_contract(vec![
            e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: cfg.asset.clone(),
                    fn_name: Symbol::new(e, "transfer"),
                    args: (vault.clone(), cfg.pool.clone(), received).into_val(e),
                },
                sub_invocations: vec![e],
            }),
        ]);

        // `account_id == 0` is XOXNO's "open me an account on this spoke"
        // sentinel; the returned id is the vault's from then on.
        let new_id = controller.supply(
            &vault,
            &account_id,
            &cfg.spoke_id,
            &vec![e, (key.clone(), received)],
        );
        if account_id == 0 {
            storage::set_account_id(e, new_id);
            events::account_opened(e, new_id, cfg.spoke_id);
        }

        let minted = scaled_position(&controller, new_id, &key) - before;
        if minted <= 0 {
            panic_with_error!(e, VaultError::AmountNotPositive);
        }

        Base::mint(e, &receiver, minted);
        events::deposit(e, &from, &receiver, received, minted);
        minted
    }

    /// Burns **exactly** `shares` from `owner` and withdraws the corresponding
    /// assets to `receiver`.
    ///
    /// Never clamps to the owner's balance: consumers detect a short payout only
    /// through their own slippage bound, so a silent clamp would surface as an
    /// unexplained failure far from its cause.
    pub fn redeem(
        e: &Env,
        shares: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        if shares <= 0 {
            panic_with_error!(e, VaultError::AmountNotPositive);
        }
        operator.require_auth();
        storage::extend_instance_ttl(e);

        let cfg = storage::get_config(e);
        let key = hub_asset(&cfg);
        let controller = ControllerClient::new(e, &cfg.controller);
        let vault = e.current_contract_address();

        let account_id = storage::get_account_id(e);
        if account_id == 0 {
            panic_with_error!(e, VaultError::NoAccount);
        }

        let index = controller.get_market_index(&key).supply_index;
        let assets = shares_to_assets(e, shares, index);

        // A dust redeem can floor to zero assets. XOXNO reads a withdrawal
        // amount of zero as "withdraw everything from this market", so passing
        // it through would empty the entire pooled position. Refuse instead.
        if assets <= 0 {
            panic_with_error!(e, VaultError::ZeroAssetRedeem);
        }

        // Delegated exit: the operator spends a share allowance. Skipped when
        // owner and operator are the same address — a second `require_auth` on
        // an address already authorized in this frame is a host error.
        if operator != owner {
            Base::spend_allowance(e, &owner, &operator, shares);
        }

        Base::update(e, Some(&owner), None, shares);

        controller.withdraw(
            &vault,
            &account_id,
            &vec![e, (key, assets)],
            &Some(receiver.clone()),
        );

        events::redeem(e, &owner, &receiver, shares, assets);
        assets
    }

    // ── operations ──────────────────────────────────────────────────────────

    /// Moves a stray token out of the vault.
    ///
    /// Exists because incentive programs distribute to whichever address held
    /// the position — this vault — and without a route out, anything that lands
    /// here is stranded permanently.
    ///
    /// The underlying asset and the vault's own share token are hard-refused.
    /// Depositor funds are never reachable through this, which is the only
    /// reason an admin-held sweep is acceptable at all.
    pub fn sweep(e: &Env, token: Address, to: Address, amount: i128) {
        let cfg = storage::get_config(e);
        cfg.admin.require_auth();
        storage::extend_instance_ttl(e);

        if token == cfg.asset || token == e.current_contract_address() {
            panic_with_error!(e, VaultError::SweepForbidden);
        }
        if amount <= 0 {
            panic_with_error!(e, VaultError::AmountNotPositive);
        }

        TokenClient::new(e, &token).transfer(&e.current_contract_address(), &to, &amount);
        events::sweep(e, &token, &to, amount);
    }

    // ── views ───────────────────────────────────────────────────────────────

    /// The vault's XOXNO account, or 0 before the first deposit.
    pub fn account_id(e: &Env) -> u64 {
        storage::get_account_id(e)
    }

    pub fn config(e: &Env) -> Config {
        storage::get_config(e)
    }
}

/// SEP-41. Required on the same address as the SEP-56 surface: consumers
/// custody these shares and hold them as an AMM reserve.
#[contractimpl]
impl FungibleToken for XoxnoVault {
    /// `Base`, not `Vault`. OpenZeppelin's `Vault` derives `total_assets` from
    /// the contract's own token balance, which is always zero here — the assets
    /// live in the lending position — and it exposes no hook to override that.
    type ContractType = Base;

    fn total_supply(e: &Env) -> i128 {
        Base::total_supply(e)
    }

    fn balance(e: &Env, account: Address) -> i128 {
        Base::balance(e, &account)
    }

    fn allowance(e: &Env, owner: Address, spender: Address) -> i128 {
        Base::allowance(e, &owner, &spender)
    }

    fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
        Base::transfer(e, &from, &to, amount)
    }

    fn transfer_from(e: &Env, spender: Address, from: Address, to: Address, amount: i128) {
        Base::transfer_from(e, &spender, &from, &to, amount)
    }

    fn approve(e: &Env, owner: Address, spender: Address, amount: i128, live_until_ledger: u32) {
        Base::approve(e, &owner, &spender, amount, live_until_ledger)
    }

    fn decimals(e: &Env) -> u32 {
        Base::decimals(e)
    }

    fn name(e: &Env) -> String {
        Base::name(e)
    }

    fn symbol(e: &Env) -> String {
        Base::symbol(e)
    }
}
