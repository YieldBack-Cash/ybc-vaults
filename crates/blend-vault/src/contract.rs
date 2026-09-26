use crate::{
    constants::SCALAR_12,
    errors::BlendVaultError,
    events, pool, storage, swap,
    vault::{self, VaultData},
};

use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contractimpl, panic_with_error, vec, Address, Env, IntoVal, String, Symbol,
};
use stellar_tokens::fungible::Base;
use vault_common::{
    auth::spend_operator_allowance, events as shared_events, guard::require_positive,
    math::mul_div_floor, ttl,
};

#[contract]
pub struct BlendVault;

// SEP-41 on the same address as the SEP-56 surface, delegated to OpenZeppelin.
vault_common::impl_share_token!(BlendVault);

// The five SEP-56 views a fee-less vault fully determines, and the
// compile-time check that the twelve below complete the standard.
vault_common::impl_sep56!(BlendVault);

#[contractimpl]
impl BlendVault {
    /// Initializes the vault over one reserve of one Blend pool.
    ///
    /// ### Arguments
    /// * `admin` - Authorized for `set_admin` and `set_router`
    /// * `pool` - The Blend pool the vault will supply into
    /// * `asset` - The reserve asset the vault supports
    /// * `blnd_token` - The BLND token, for emissions harvesting
    /// * `name`, `symbol` - Share token metadata; decimals are always 7
    pub fn __constructor(
        e: &Env,
        admin: Address,
        pool: Address,
        asset: Address,
        blnd_token: Address,
        name: String,
        symbol: String,
    ) {
        admin.require_auth();

        storage::set_admin(e, &admin);
        storage::set_pool(e, &pool);
        storage::set_asset(e, &asset);
        storage::set_blnd_token(e, &blnd_token);

        storage::set_vault_data(
            e,
            &VaultData {
                b_rate: pool::reserve_b_rate(e, &pool, &asset),
                last_update_timestamp: e.ledger().timestamp(),
                total_shares: 0,
                total_b_tokens: 0,
            },
        );

        // 7 decimals matches the underlying Stellar asset contract and the
        // 1e7 fixed-point scale consumers assume.
        Base::set_metadata(e, 7, name, symbol);
        ttl::extend_instance_ttl(e);
    }

    // ── SEP-56: asset and conversions ───────────────────────────────────────

    pub fn query_asset(e: &Env) -> Address {
        storage::get_asset(e)
    }

    /// Converts a share amount to underlying tokens: shares to bTokens through
    /// the vault's ratio, then bTokens to underlying through the pool's
    /// `b_rate`. Both round down. Positive on an empty vault (1:1 through
    /// `b_rate`), so a consumer probing the rate at market creation succeeds.
    pub fn convert_to_assets(e: &Env, shares: i128) -> i128 {
        if shares <= 0 {
            return 0;
        }
        let vault = Self::updated_vault(e);
        let b_tokens = vault.shares_to_b_tokens_down(e, shares);
        vault.b_tokens_to_underlying_down(e, b_tokens)
    }

    /// Converts an underlying amount to shares: underlying to bTokens through
    /// the pool's `b_rate`, then bTokens to shares through the vault's ratio.
    /// Both round down.
    pub fn convert_to_shares(e: &Env, assets: i128) -> i128 {
        if assets <= 0 {
            return 0;
        }
        let vault = Self::updated_vault(e);
        let b_tokens = vault.underlying_to_b_tokens_down(e, assets);
        vault.b_tokens_to_shares_down(e, b_tokens)
    }

    /// Underlying value of the vault's whole bToken position.
    pub fn total_assets(e: &Env) -> i128 {
        let vault = Self::updated_vault(e);
        vault.b_tokens_to_underlying_down(e, vault.total_b_tokens)
    }

    // ── SEP-56: deposit ─────────────────────────────────────────────────────

    /// Remaining room under the reserve's supply cap, in assets, or 0 when the
    /// pool is frozen or the reserve disabled. The cap is pool-wide, so this
    /// is the shared headroom: another supplier consuming it first is exactly
    /// what would make a deposit revert.
    pub fn max_deposit(e: &Env, _receiver: Address) -> i128 {
        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        if pool::status(e, &pool) >= 4 {
            return 0;
        }
        let reserve = pool::reserve(e, &pool, &asset);
        if !reserve.config.enabled {
            return 0;
        }
        let supplied = mul_div_floor(e, reserve.data.b_supply, reserve.data.b_rate, SCALAR_12);
        if reserve.config.supply_cap <= supplied {
            0
        } else {
            reserve.config.supply_cap - supplied
        }
    }

    /// Supplies `assets` from `from` into the pool and mints the resulting
    /// shares to `receiver`.
    ///
    /// ### Returns
    /// * `i128` - The number of shares minted to the receiver
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

        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        pool::supply(e, &pool, &asset, &from, assets);
        let (_, new_shares) = vault::deposit(e, &pool, &asset, &receiver, assets);

        shared_events::deposit(e, &operator, &from, &receiver, assets, new_shares);
        new_shares
    }

    // ── SEP-56: mint ────────────────────────────────────────────────────────

    /// Assets needed to mint `shares`: shares to bTokens through the vault's
    /// ratio, then bTokens to underlying through `b_rate`. Both round up.
    pub fn preview_mint(e: &Env, shares: i128) -> i128 {
        if shares <= 0 {
            return 0;
        }
        let vault = Self::updated_vault(e);
        let b_tokens = vault.shares_to_b_tokens_up(e, shares);
        vault.b_tokens_to_underlying_up(e, b_tokens)
    }

    /// Mints **exactly** `shares` to `receiver`, supplying the underlying that
    /// costs (rounded up) from `from`. Returns the assets deposited.
    pub fn mint(e: &Env, shares: i128, receiver: Address, from: Address, operator: Address) -> i128 {
        require_positive(e, shares);
        operator.require_auth();
        ttl::extend_instance_ttl(e);

        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        let assets = Self::preview_mint(e, shares);
        require_positive(e, assets);
        pool::supply(e, &pool, &asset, &from, assets);
        vault::mint(e, &pool, &asset, &receiver, shares, assets);

        shared_events::deposit(e, &operator, &from, &receiver, assets, shares);
        assets
    }

    // ── SEP-56: withdraw ────────────────────────────────────────────────────

    /// Shares a withdrawal of `assets` would burn: underlying to bTokens
    /// through `b_rate`, then bTokens to shares through the vault's ratio.
    /// Both round up, matching what `withdraw` burns.
    pub fn preview_withdraw(e: &Env, assets: i128) -> i128 {
        if assets <= 0 {
            return 0;
        }
        let vault = Self::updated_vault(e);
        let b_tokens = vault.underlying_to_b_tokens_up(e, assets);
        vault.b_tokens_to_shares_up(e, b_tokens)
    }

    /// Withdraws **exactly** `assets` to `receiver`, burning the shares that
    /// costs (rounded up) from `owner`. Returns the shares burned.
    ///
    /// Never clamps to the owner's balance: an over-large request fails
    /// (OpenZeppelin `InsufficientBalance`) rather than paying short.
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

        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        let (_, shares) = vault::withdraw(e, &pool, &asset, &owner, assets);
        // The share cost is only known once the rounding above has run, so the
        // allowance is consumed here; a panic reverts the burn with it.
        spend_operator_allowance(e, &owner, &operator, shares);
        pool::withdraw(e, &pool, &asset, &receiver, assets);

        shared_events::withdraw(e, &operator, &receiver, &owner, assets, shares);
        shares
    }

    // ── SEP-56: redeem ──────────────────────────────────────────────────────

    /// Burns **exactly** `shares` from `owner` and withdraws the corresponding
    /// underlying to `receiver`.
    ///
    /// Never clamps to the owner's balance: the caller named an exact share
    /// count, so an over-large request fails (OpenZeppelin `InsufficientBalance`)
    /// rather than paying short.
    ///
    /// ### Returns
    /// * `i128` - The amount of underlying tokens sent to the receiver
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
        spend_operator_allowance(e, &owner, &operator, shares);

        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        let (withdraw_amount, _) = vault::redeem(e, &pool, &asset, &owner, shares);
        pool::withdraw(e, &pool, &asset, &receiver, withdraw_amount);

        shared_events::withdraw(e, &operator, &receiver, &owner, withdraw_amount, shares);
        withdraw_amount
    }

    // ── operations ──────────────────────────────────────────────────────────

    /// Sets the admin address. Requires auth from both the current and new admin.
    pub fn set_admin(e: &Env, admin: Address) {
        ttl::extend_instance_ttl(e);
        storage::get_admin(e).require_auth();
        admin.require_auth();
        storage::set_admin(e, &admin);
    }

    /// Sets the Soroswap router address used for harvesting BLND emissions.
    pub fn set_router(e: &Env, router: Address) {
        ttl::extend_instance_ttl(e);
        storage::get_admin(e).require_auth();
        storage::set_router(e, &router);
    }

    /// Claims accrued BLND emissions from the pool, swaps them for the underlying
    /// asset via Soroswap, and supplies the proceeds back into the pool. Every
    /// depositor's share value increases automatically.
    ///
    /// Unprivileged. `amount_out_min` is the caller's slippage floor on the
    /// swap leg.
    ///
    /// ### Returns
    /// * `i128` - The amount of underlying tokens received and re-supplied
    pub fn claim_emissions(e: &Env, amount_out_min: i128) -> i128 {
        ttl::extend_instance_ttl(e);
        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        let blnd = storage::get_blnd_token(e);
        let router = storage::get_router(e)
            .unwrap_or_else(|| panic_with_error!(e, BlendVaultError::SwapNotConfigured));

        let supply_token_id = pool::reserve_supply_token_id(e, &pool, &asset);
        let blnd_claimed = pool::claim(
            e,
            &pool,
            &vec![e, supply_token_id],
            &e.current_contract_address(),
        );
        if blnd_claimed == 0 {
            return 0;
        }

        let underlying_received =
            swap::swap_blnd_for_asset(e, &router, &blnd, &asset, blnd_claimed, amount_out_min);

        let vault = e.current_contract_address();
        e.authorize_as_current_contract(vec![
            e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: asset.clone(),
                    fn_name: Symbol::new(e, "transfer"),
                    args: (vault.clone(), pool.clone(), underlying_received).into_val(e),
                },
                sub_invocations: vec![e],
            }),
        ]);
        pool::supply(e, &pool, &asset, &vault, underlying_received);

        let mut vault = Self::updated_vault(e);
        vault.total_b_tokens =
            pool::vault_b_token_balance(e, &pool, &asset, &e.current_contract_address());
        storage::set_vault_data(e, &vault);

        events::emissions_claim(e, &pool, blnd_claimed, underlying_received);
        underlying_received
    }

    // ── views ───────────────────────────────────────────────────────────────

    /// The protocol contract this vault supplies to: the Blend pool.
    ///
    /// Informational only. Nothing on chain calls it: the YBC indexer reads it
    /// so a curator can confirm which protocol a vault is built on before
    /// listing its markets. Every adapter in the workspace exposes it with
    /// this exact signature. Not part of SEP-56; the underlying asset comes
    /// from the standard's `query_asset`.
    pub fn get_protocol(e: &Env) -> Address {
        storage::get_pool(e)
    }

    /// The vault's ratio state with an up-to-date `b_rate`.
    pub fn get_vault(e: &Env) -> VaultData {
        Self::updated_vault(e)
    }

    /// `user`'s position in bTokens.
    pub fn get_b_tokens(e: &Env, user: Address) -> i128 {
        let shares = Base::balance(e, &user);
        if shares > 0 {
            Self::updated_vault(e).shares_to_b_tokens_down(e, shares)
        } else {
            0
        }
    }

    pub fn get_admin(e: &Env) -> Address {
        storage::get_admin(e)
    }

    // ── internals ───────────────────────────────────────────────────────────

    /// The vault's ratio state with the pool's current `b_rate` applied.
    fn updated_vault(e: &Env) -> VaultData {
        let pool = storage::get_pool(e);
        let asset = storage::get_asset(e);
        vault::get_vault_updated(e, &pool, &asset)
    }
}
