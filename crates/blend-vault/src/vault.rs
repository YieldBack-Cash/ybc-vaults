//! The vault's own accounting: a `total_shares / total_b_tokens` ratio on top
//! of the pool's `b_rate`. This is the rate math, and it stays in the adapter
//! on purpose: it is what differs between protocols.
//!
//! Share balances themselves live in OpenZeppelin `Base`; this module only
//! decides how many to mint or burn.

use crate::{constants::SCALAR_12, errors::BlendVaultError, pool, storage};
use soroban_sdk::{contracttype, panic_with_error, Address, Env};
use stellar_tokens::fungible::Base;
use vault_common::math::mul_div_floor;

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
#[contracttype]
pub struct VaultData {
    /// The timestamp of the last update
    pub last_update_timestamp: u64,
    /// The reserve's last bRate
    pub b_rate: i128,
    /// The total shares issued by the vault
    pub total_shares: i128,
    /// The total bToken deposits owned by the vault depositors.
    pub total_b_tokens: i128,
}

/// `ceil(x * y / denominator)` for non-negative operands.
fn mul_div_ceil(e: &Env, x: i128, y: i128, denominator: i128) -> i128 {
    let floor = mul_div_floor(e, x, y, denominator);
    if x != 0 && y != 0 && mul_div_floor(e, floor, denominator, y) < x {
        floor + 1
    } else {
        floor
    }
}

impl VaultData {
    /// Converts a b_token amount to shares rounding down
    pub fn b_tokens_to_shares_down(&self, e: &Env, amount: i128) -> i128 {
        if self.total_shares == 0 || self.total_b_tokens == 0 {
            return amount;
        }
        mul_div_floor(e, amount, self.total_shares, self.total_b_tokens)
    }

    /// Converts a b_token amount to shares rounding up
    pub fn b_tokens_to_shares_up(&self, e: &Env, amount: i128) -> i128 {
        if self.total_shares == 0 || self.total_b_tokens == 0 {
            return amount;
        }
        mul_div_ceil(e, amount, self.total_shares, self.total_b_tokens)
    }

    /// Converts a share amount to a b_token amount rounding down
    pub fn shares_to_b_tokens_down(&self, e: &Env, amount: i128) -> i128 {
        if self.total_shares == 0 {
            // an empty vault mints shares 1:1 with bTokens, so quote the same
            // rate a first deposit would get
            return amount;
        }
        mul_div_floor(e, amount, self.total_b_tokens, self.total_shares)
    }

    /// Converts a b_token amount to an underlying token amount rounding down
    pub fn b_tokens_to_underlying_down(&self, e: &Env, amount: i128) -> i128 {
        mul_div_floor(e, amount, self.b_rate, SCALAR_12)
    }

    /// Converts an underlying amount to a b_token amount rounding down
    pub fn underlying_to_b_tokens_down(&self, e: &Env, amount: i128) -> i128 {
        mul_div_floor(e, amount, SCALAR_12, self.b_rate)
    }

    /// Converts an underlying amount to a b_token amount rounding up
    pub fn underlying_to_b_tokens_up(&self, e: &Env, amount: i128) -> i128 {
        mul_div_ceil(e, amount, SCALAR_12, self.b_rate)
    }

    /// Updates the reserve's bRate
    fn update_rate(&mut self, e: &Env, pool: &Address, asset: &Address) {
        self.last_update_timestamp = e.ledger().timestamp();
        self.b_rate = pool::reserve_b_rate(e, pool, asset);
    }
}

/// Get the vault data from storage and update the bRate
pub fn get_vault_updated(e: &Env, pool: &Address, asset: &Address) -> VaultData {
    let mut vault = storage::get_vault_data(e);
    vault.update_rate(e, pool, asset);
    vault
}

/// Deposit into the vault. Does not perform the call to the pool to deposit the tokens.
///
/// ### Returns
/// * `(i128, i128)` - (The amount of b_tokens minted to the vault, the amount of shares minted to the user)
///
/// ### Panics
/// * If the amount rounds to zero bTokens or zero shares
pub fn deposit(
    e: &Env,
    pool: &Address,
    asset: &Address,
    user: &Address,
    amount: i128,
) -> (i128, i128) {
    let mut vault = get_vault_updated(e, pool, asset);

    let b_tokens_amount = vault.underlying_to_b_tokens_down(e, amount);
    if b_tokens_amount <= 0 {
        panic_with_error!(e, BlendVaultError::InvalidBTokensMinted);
    }
    let share_amount = vault.b_tokens_to_shares_down(e, b_tokens_amount);
    if share_amount <= 0 {
        panic_with_error!(e, BlendVaultError::InvalidSharesMinted);
    }

    vault.total_shares += share_amount;
    vault.total_b_tokens += b_tokens_amount;
    storage::set_vault_data(e, &vault);
    Base::mint(e, user, share_amount);
    (b_tokens_amount, share_amount)
}

/// Redeem an exact share amount from the vault. Does not perform the call to the
/// pool to withdraw the tokens.
///
/// The share amount is not clamped to the user's balance: the caller asked for
/// an exact number of shares, so an over-large request is an error (raised by
/// OpenZeppelin as `InsufficientBalance`) rather than a hint.
///
/// ### Returns
/// * `(i128, i128)` - (The underlying to withdraw from the pool, the amount of b_tokens burned from the vault)
///
/// ### Panics
/// * If the resulting underlying amount rounds down to 0
/// * If the user holds fewer than `shares` shares
pub fn redeem(
    e: &Env,
    pool: &Address,
    asset: &Address,
    user: &Address,
    shares: i128,
) -> (i128, i128) {
    let mut vault = get_vault_updated(e, pool, asset);

    let b_tokens_down = vault.shares_to_b_tokens_down(e, shares);
    let underlying_amount = vault.b_tokens_to_underlying_down(e, b_tokens_down);
    if underlying_amount <= 0 {
        panic_with_error!(e, BlendVaultError::InvalidBTokensBurnt);
    }

    // The blend pool rounds the b_tokens it burns UP from the underlying amount
    // requested, so the vault's own accounting has to use the same figure or
    // total_b_tokens drifts above the real position.
    let b_tokens_amount = vault.underlying_to_b_tokens_up(e, underlying_amount);

    // Burns exactly `shares`; panics with OZ `InsufficientBalance` if short.
    // Before the reserves check so a caller sees their own error, not the
    // accounting invariant's; a panic reverts everything either way.
    Base::update(e, Some(user), None, shares);

    if vault.total_shares < shares || vault.total_b_tokens < b_tokens_amount {
        panic_with_error!(e, BlendVaultError::InsufficientReserves);
    }

    vault.total_shares -= shares;
    vault.total_b_tokens -= b_tokens_amount;
    storage::set_vault_data(e, &vault);

    (underlying_amount, b_tokens_amount)
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutils::create_test_blend_vault;
    use proptest::prelude::*;
    use soroban_sdk::testutils::Address as _;

    // 100 billion tokens at 7 decimals. Keeps every intermediate product
    // (amount * total, amount * b_rate) well below i128::MAX so the properties
    // probe rounding behavior, not overflow traps.
    const MAX_TOKENS: i128 = 1_000_000_000_000_000_000;
    // 0.5x to 10x — covers default scenarios (b_rate below 1.0) and years of yield
    const MIN_B_RATE: i128 = 500_000_000_000;
    const MAX_B_RATE: i128 = 10_000_000_000_000;

    prop_compose! {
        /// A reachable vault state: either empty (both totals 0, as constructed
        /// or after a full drain) or with both totals positive.
        fn vault_state()(
            total_shares in 1i128..=MAX_TOKENS,
            total_b_tokens in 1i128..=MAX_TOKENS,
            b_rate in MIN_B_RATE..=MAX_B_RATE,
            empty in any::<bool>(),
        ) -> VaultData {
            VaultData {
                last_update_timestamp: 0,
                b_rate,
                total_shares: if empty { 0 } else { total_shares },
                total_b_tokens: if empty { 0 } else { total_b_tokens },
            }
        }
    }

    proptest! {
        #[test]
        fn conversions_never_panic(vault in vault_state(), amount in 0i128..=MAX_TOKENS) {
            let e = Env::default();
            vault.b_tokens_to_shares_down(&e, amount);
            vault.b_tokens_to_shares_up(&e, amount);
            vault.shares_to_b_tokens_down(&e, amount);
            vault.b_tokens_to_underlying_down(&e, amount);
            vault.underlying_to_b_tokens_down(&e, amount);
            vault.underlying_to_b_tokens_up(&e, amount);
        }

        /// Minting shares from bTokens and converting back never gains bTokens.
        #[test]
        fn share_round_trip_never_gains(vault in vault_state(), b_tokens in 0i128..=MAX_TOKENS) {
            let e = Env::default();
            let shares = vault.b_tokens_to_shares_down(&e, b_tokens);
            prop_assert!(vault.shares_to_b_tokens_down(&e, shares) <= b_tokens);
        }

        /// Converting underlying to bTokens and back never gains underlying.
        #[test]
        fn underlying_round_trip_never_gains(vault in vault_state(), amount in 0i128..=MAX_TOKENS) {
            let e = Env::default();
            let b_tokens = vault.underlying_to_b_tokens_down(&e, amount);
            prop_assert!(vault.b_tokens_to_underlying_down(&e, b_tokens) <= amount);
        }

        /// The up and down variants bracket the exact ratio and differ by at most 1.
        #[test]
        fn rounding_up_within_one_of_down(vault in vault_state(), amount in 0i128..=MAX_TOKENS) {
            let e = Env::default();
            let shares_down = vault.b_tokens_to_shares_down(&e, amount);
            let shares_up = vault.b_tokens_to_shares_up(&e, amount);
            prop_assert!(shares_down <= shares_up);
            prop_assert!(shares_up - shares_down <= 1);

            let b_tokens_down = vault.underlying_to_b_tokens_down(&e, amount);
            let b_tokens_up = vault.underlying_to_b_tokens_up(&e, amount);
            prop_assert!(b_tokens_down <= b_tokens_up);
            prop_assert!(b_tokens_up - b_tokens_down <= 1);
        }

        /// More shares never convert to fewer bTokens (and likewise for the
        /// other down-rounding conversions).
        #[test]
        fn conversions_monotonic(
            vault in vault_state(),
            a in 0i128..=MAX_TOKENS,
            b in 0i128..=MAX_TOKENS,
        ) {
            let e = Env::default();
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(vault.shares_to_b_tokens_down(&e, lo) <= vault.shares_to_b_tokens_down(&e, hi));
            prop_assert!(vault.b_tokens_to_shares_down(&e, lo) <= vault.b_tokens_to_shares_down(&e, hi));
            prop_assert!(vault.b_tokens_to_underlying_down(&e, lo) <= vault.b_tokens_to_underlying_down(&e, hi));
        }
    }

    prop_compose! {
        /// Like `vault_state`, but with total_shares held within 0.5x-2x of
        /// total_b_tokens. Deposits mint pro-rata, so real vaults never stray
        /// far from 1:1; unbounded ratios would overflow i128 on intermediate
        /// products in states the contract can't actually reach.
        fn proportional_vault_state()(
            total_b_tokens in 1i128..=MAX_TOKENS,
            ratio_bps in 5000i128..=20000,
            b_rate in MIN_B_RATE..=MAX_B_RATE,
            empty in any::<bool>(),
        ) -> VaultData {
            VaultData {
                last_update_timestamp: 0,
                b_rate,
                total_shares: if empty { 0 } else { (total_b_tokens * ratio_bps / 10000).max(1) },
                total_b_tokens: if empty { 0 } else { total_b_tokens },
            }
        }
    }

    proptest! {
        // Each case registers contracts in a fresh Env, so run fewer cases.
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// With an unchanged b_rate, a deposit followed by redeeming every share
        /// it minted can never extract more underlying than was deposited.
        #[test]
        fn deposit_then_full_redeem_never_profits(
            initial_state in proportional_vault_state(),
            amount in 1_0000000i128..=MAX_TOKENS,
        ) {
            let e = Env::default();
            e.mock_all_auths();

            // skip dust deposits the contract (correctly) rejects
            let b_tokens = initial_state.underlying_to_b_tokens_down(&e, amount);
            prop_assume!(b_tokens > 0);
            prop_assume!(initial_state.b_tokens_to_shares_down(&e, b_tokens) > 0);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let (vault_address, pool, asset) =
                create_test_blend_vault(&e, &bombadil, Some(initial_state.b_rate));

            let (position_value, withdrawn) = e.as_contract(&vault_address, || {
                storage::set_vault_data(&e, &initial_state);

                let (_, shares_minted) = deposit(&e, &pool, &asset, &samwise, amount);

                let v = storage::get_vault_data(&e);
                let position_value =
                    v.b_tokens_to_underlying_down(&e, v.shares_to_b_tokens_down(&e, shares_minted));

                if position_value == 0 {
                    return Ok((0, 0));
                }
                let (withdrawn, _) = redeem(&e, &pool, &asset, &samwise, shares_minted);
                prop_assert_eq!(Base::balance(&e, &samwise), 0);
                Ok((position_value, withdrawn))
            })?;

            prop_assert!(position_value <= amount);
            prop_assert!(withdrawn <= amount);
        }
    }
}

#[cfg(test)]
mod generic_tests {
    use super::*;
    use crate::testutils::{
        create_test_blend_vault, fixed_div_floor, fixed_mul_floor, mockpool::MockPoolClient,
        EnvTestUtils,
    };
    use soroban_sdk::{testutils::Address as _, Address};

    #[test]
    fn test_b_tokens_to_shares_down() {
        let e = Env::default();
        let mut vault = VaultData {
            b_rate: 1_000_000_000_000,
            last_update_timestamp: 0,
            total_shares: 0,
            total_b_tokens: 0,
        };

        // rounds down
        vault.total_shares = 200_0000001;
        vault.total_b_tokens = 100_0000000;
        let b_tokens = vault.b_tokens_to_shares_down(&e, 1_0000000);
        assert_eq!(b_tokens, 2_0000000);

        // returns amount if total_shares is 0
        vault.total_shares = 0;
        vault.total_b_tokens = 100_0000000;
        let b_tokens = vault.b_tokens_to_shares_down(&e, 1_0000000);
        assert_eq!(b_tokens, 1_0000000);

        // returns amount if total_b_tokens is 0
        vault.total_shares = 200_0000000;
        vault.total_b_tokens = 0;
        let b_tokens = vault.b_tokens_to_shares_down(&e, 1_0000000);
        assert_eq!(b_tokens, 1_0000000);
    }

    #[test]
    fn test_b_tokens_to_shares_up() {
        let e = Env::default();
        let mut vault = VaultData {
            b_rate: 1_000_000_000_000,
            last_update_timestamp: 0,
            total_shares: 0,
            total_b_tokens: 0,
        };

        // rounds up
        vault.total_shares = 200_0000001;
        vault.total_b_tokens = 100_0000000;
        let b_tokens = vault.b_tokens_to_shares_up(&e, 1_0000000);
        assert_eq!(b_tokens, 2_0000001);

        // returns amount if total_shares is 0
        vault.total_shares = 0;
        vault.total_b_tokens = 100_0000000;
        let b_tokens = vault.b_tokens_to_shares_up(&e, 1_0000000);
        assert_eq!(b_tokens, 1_0000000);

        // returns amount if total_b_tokens is 0
        vault.total_shares = 200_0000000;
        vault.total_b_tokens = 0;
        let b_tokens = vault.b_tokens_to_shares_up(&e, 1_0000000);
        assert_eq!(b_tokens, 1_0000000);
    }

    #[test]
    fn test_shares_to_b_tokens_down() {
        let e = Env::default();
        let mut vault = VaultData {
            b_rate: 1_000_000_000_000,
            last_update_timestamp: 0,
            total_shares: 0,
            total_b_tokens: 0,
        };

        // rounds down
        vault.total_shares = 200_0000001;
        vault.total_b_tokens = 100_0000000;
        let b_tokens = vault.shares_to_b_tokens_down(&e, 2_0000000);
        assert_eq!(b_tokens, 0_9999999);

        // returns 0 if total_b_tokens is 0
        vault.total_shares = 200_0000000;
        vault.total_b_tokens = 0;
        let b_tokens = vault.shares_to_b_tokens_down(&e, 2_0000000);
        assert_eq!(b_tokens, 0);

        // returns amount 1:1 if total_shares is 0 (empty vault, first deposit rate)
        vault.total_shares = 0;
        vault.total_b_tokens = 0;
        let b_tokens = vault.shares_to_b_tokens_down(&e, 2_0000000);
        assert_eq!(b_tokens, 2_0000000);
    }

    #[test]
    fn test_deposit() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        let init_b_rate = 1_100_000_000_000;
        let mock_client = MockPoolClient::new(&e, &pool);
        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: init_b_rate,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);

            // Raise b_rate and deposit: update_rate picks up the new rate
            let new_b_rate = 1_110_000_000_000;
            mock_client.set_b_rate(&new_b_rate);
            e.jump(5);

            let amount = 100_0000000;
            let expected_b_tokens = fixed_div_floor(amount, new_b_rate, SCALAR_12);
            let expected_shares = fixed_mul_floor(expected_b_tokens, 1200_0000000, 1000_0000000);

            let (b_tokens_minted, shares_minted) = deposit(&e, &pool, &asset, &samwise, amount);
            assert_eq!(b_tokens_minted, expected_b_tokens);
            assert_eq!(shares_minted, expected_shares);

            let new_vault = storage::get_vault_data(&e);
            assert_eq!(new_vault.total_shares, 1200_0000000 + expected_shares);
            assert_eq!(new_vault.total_b_tokens, 1000_0000000 + expected_b_tokens);
            assert_eq!(new_vault.b_rate, new_b_rate);

            assert_eq!(Base::balance(&e, &samwise), expected_shares);
        });
    }

    #[test]
    fn test_initial_deposit() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        let init_b_rate = 1_000_000_000_000;
        let mock_client = MockPoolClient::new(&e, &pool);
        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 0,
                total_shares: 0,
                b_rate: init_b_rate,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);

            let new_b_rate = 1_100_000_000_000;
            mock_client.set_b_rate(&new_b_rate);
            e.jump(5);
            let amount = 100_0000000;
            let expected_b_tokens = fixed_div_floor(amount, new_b_rate, SCALAR_12);
            let (b_tokens_minted, shares_minted) = deposit(&e, &pool, &asset, &samwise, amount);

            // first deposit mints shares 1:1 with bTokens
            assert_eq!(b_tokens_minted, expected_b_tokens);
            assert_eq!(shares_minted, expected_b_tokens);
            let new_vault = storage::get_vault_data(&e);
            assert_eq!(new_vault.total_shares, expected_b_tokens);
            assert_eq!(new_vault.total_b_tokens, b_tokens_minted);
            assert_eq!(new_vault.b_rate, new_b_rate);

            assert_eq!(Base::balance(&e, &samwise), expected_b_tokens);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #201)")]
    fn test_deposit_zero_b_tokens() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: 1_100_000_000_000,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);

            deposit(&e, &pool, &asset, &samwise, 1);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #203)")]
    fn test_deposit_zero_shares() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        e.as_contract(&vault_address, || {
            // Not possible config in practice, but just in case
            let vault_data = VaultData {
                total_b_tokens: 10000_0000000,
                total_shares: 1200_0000000,
                b_rate: 1_100_000_000_000,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);

            deposit(&e, &pool, &asset, &samwise, 2);
        });
    }

    #[test]
    fn test_redeem() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: 1_100_000_000_000,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);

            // samwise owns all shares
            let sam_shares = 1200_0000000;
            Base::mint(&e, &samwise, sam_shares);

            let shares_to_redeem = 60_0000000;
            let expected_b_tokens = vault_data.shares_to_b_tokens_down(&e, shares_to_redeem);
            let expected_underlying = vault_data.b_tokens_to_underlying_down(&e, expected_b_tokens);

            let (underlying, b_tokens_burnt) =
                redeem(&e, &pool, &asset, &samwise, shares_to_redeem);
            assert_eq!(underlying, expected_underlying);
            assert_eq!(b_tokens_burnt, expected_b_tokens);

            let new_vault = storage::get_vault_data(&e);
            assert_eq!(new_vault.total_shares, 1200_0000000 - shares_to_redeem);
            assert_eq!(new_vault.total_b_tokens, 1000_0000000 - b_tokens_burnt);
            assert_eq!(new_vault.b_rate, 1_100_000_000_000);

            assert_eq!(Base::balance(&e, &samwise), sam_shares - shares_to_redeem);
        });
    }

    #[test]
    fn test_redeem_all_drains_the_vault() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: 1_100_000_000_000,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);
            Base::mint(&e, &samwise, vault_data.total_shares);

            let (underlying, b_tokens_burnt) =
                redeem(&e, &pool, &asset, &samwise, vault_data.total_shares);
            assert_eq!(
                underlying,
                vault_data.b_tokens_to_underlying_down(&e, 1000_0000000)
            );
            assert_eq!(b_tokens_burnt, 1000_0000000);
            assert_eq!(Base::balance(&e, &samwise), 0);
            let vault_data = storage::get_vault_data(&e);
            assert_eq!(vault_data.total_b_tokens, 0);
            assert_eq!(vault_data.total_shares, 0);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #100)")]
    fn test_redeem_over_balance() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: 1_100_000_000_000,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);
            Base::mint(&e, &samwise, 100_0000000);

            redeem(&e, &pool, &asset, &samwise, 100_0000001);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #200)")]
    fn test_redeem_more_shares_than_vault() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let (vault_address, pool, asset) = create_test_blend_vault(&e, &bombadil, None);

        e.as_contract(&vault_address, || {
            let vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: 1_100_000_000_000,
                last_update_timestamp: e.ledger().timestamp(),
            };
            storage::set_vault_data(&e, &vault_data);
            // an unreachable state: more shares held than the vault issued
            Base::mint(&e, &samwise, vault_data.total_shares + 10);

            redeem(&e, &pool, &asset, &samwise, vault_data.total_shares + 1);
        });
    }

    #[test]
    fn test_update_rate() {
        let e = Env::default();
        e.mock_all_auths();
        e.set_default_info();

        let init_b_rate = 1_100_000_000_000;
        let bombadil = Address::generate(&e);
        let (vault_address, pool, asset) =
            create_test_blend_vault(&e, &bombadil, Some(init_b_rate));

        let mock_client = MockPoolClient::new(&e, &pool);

        e.as_contract(&vault_address, || {
            let mut vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                last_update_timestamp: e.ledger().timestamp(),
                total_shares: 1200_0000000,
                b_rate: init_b_rate,
            };

            // update b_rate to 1.2
            let new_b_rate = 120_000_0000_000;
            mock_client.set_b_rate(&new_b_rate);
            e.jump(5);
            vault_data.update_rate(&e, &pool, &asset);

            // No fees, so b_tokens and shares are unchanged
            assert_eq!(vault_data.total_shares, 1200_000_0000);
            assert_eq!(vault_data.total_b_tokens, 1000_0000000);
            assert_eq!(vault_data.b_rate, new_b_rate);
            assert_eq!(vault_data.last_update_timestamp, e.ledger().timestamp());
        });
    }

    #[test]
    fn test_update_rate_no_change() {
        let e = Env::default();
        e.mock_all_auths();

        let init_b_rate = 1_100_000_000_000;
        let bombadil = Address::generate(&e);
        let (vault_address, pool, asset) =
            create_test_blend_vault(&e, &bombadil, Some(init_b_rate));

        e.as_contract(&vault_address, || {
            let now = e.ledger().timestamp();
            let mut vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: init_b_rate,
                last_update_timestamp: now,
            };

            vault_data.update_rate(&e, &pool, &asset);
            assert_eq!(vault_data.total_shares, 1200_0000000);
            assert_eq!(vault_data.total_b_tokens, 1000_0000000);
            assert_eq!(vault_data.b_rate, init_b_rate);
            assert_eq!(vault_data.last_update_timestamp, now);
        });
    }

    #[test]
    fn test_update_rate_different_timestamp_same_brate() {
        let e = Env::default();
        e.mock_all_auths();

        let init_b_rate = 1_100_000_000_000;
        let bombadil = Address::generate(&e);
        let (vault_address, pool, asset) =
            create_test_blend_vault(&e, &bombadil, Some(init_b_rate));

        e.as_contract(&vault_address, || {
            let now = e.ledger().timestamp();
            let mut vault_data = VaultData {
                total_b_tokens: 1000_0000000,
                total_shares: 1200_0000000,
                b_rate: init_b_rate,
                last_update_timestamp: now,
            };

            e.jump_time(100);

            vault_data.update_rate(&e, &pool, &asset);
            assert_eq!(vault_data.total_shares, 1200_0000000);
            assert_eq!(vault_data.total_b_tokens, 1000_0000000);
            assert_eq!(vault_data.b_rate, init_b_rate);
            // Timestamp gets updated even when b_rate is unchanged
            assert_eq!(vault_data.last_update_timestamp, e.ledger().timestamp());
        });
    }

    #[test]
    fn test_update_rate_decrease() {
        let e = Env::default();
        e.mock_all_auths();
        e.set_default_info();

        let init_b_rate = 1_100_000_000_000;
        let bombadil = Address::generate(&e);
        let (vault_address, pool, asset) =
            create_test_blend_vault(&e, &bombadil, Some(init_b_rate));
        let mock_client = MockPoolClient::new(&e, &pool);

        e.as_contract(&vault_address, || {
            let mut vault_data = VaultData {
                total_b_tokens: 100_0000000,
                last_update_timestamp: e.ledger().timestamp(),
                total_shares: 100_0000000,
                b_rate: init_b_rate,
            };

            // b_rate decreases (e.g. in a default scenario)
            let new_b_rate: i128 = 1_050_000_000_000;
            mock_client.set_b_rate(&new_b_rate);
            vault_data.update_rate(&e, &pool, &asset);

            assert_eq!(vault_data.b_rate, new_b_rate);
            assert_eq!(vault_data.total_b_tokens, 100_0000000);
            assert_eq!(vault_data.total_shares, 100_0000000);
            assert_eq!(vault_data.last_update_timestamp, e.ledger().timestamp());
        });
    }
}
