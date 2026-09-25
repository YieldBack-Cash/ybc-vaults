//! Share maths, isolated from storage and cross-contract calls so it can be
//! tested directly.
//!
//! Both directions floor. Flooring `shares -> assets` is what makes the
//! redeem-side dust fall in the vault's favour:
//!
//! ```text
//! assets = floor(shares * index / RAY)
//!   =>  assets * RAY / index  <=  shares
//!   =>  ceil(assets * RAY / index)  <=  shares      (shares is an integer)
//! ```
//!
//! XOXNO burns `ceil(assets * RAY / index)` scaled units to protect itself, so
//! the position drops by at most what the vault burned. Never more. That is why
//! the crate invariant is `total_supply <= scaled_position` and not equality.
//!
//! The widening multiply itself is `vault_common::math::mul_div_floor`: at the
//! 5,000,000 USDC supply cap on spoke 1, `shares * index` is around 5e40 and an
//! `i128` tops out near 1.7e38.

use soroban_sdk::Env;
use vault_common::math::{mul_div_ceil, mul_div_floor};

use crate::lending::constants::RAY;

/// The only asset precision this vault supports. Share metadata is 7 decimals
/// and every YBC consumer assumes the 1e7 scale, so the constructor refuses an
/// asset with any other `decimals()`.
pub const ASSET_DECIMALS: u32 = 7;

/// XOXNO keeps every scaled amount as a `Ray`: the asset amount rescaled to 27
/// decimals, divided by the index. One vault share is one scaled unit **at
/// asset precision**, so the raw figure the controller reports is divided by
/// `10^(27 - 7)` on the way in and multiplied back on the way out.
///
/// Verified on the testnet controller (2026-09-24): a 10 XLM supply credits a
/// `scaled_amount` of ~1e28, not ~1e8. Treating that as the share count
/// would have minted 1e20 PT per stroop.
pub const SCALED_UNIT: i128 = 100_000_000_000_000_000_000; // 10^20

/// A raw Ray-scaled controller figure, at asset precision (floored).
pub fn ray_scaled_to_shares(raw: i128) -> i128 {
    raw / SCALED_UNIT
}

/// `assets = floor(shares * index / RAY)`.
pub fn shares_to_assets(e: &Env, shares: i128, index: i128) -> i128 {
    mul_div_floor(e, shares, index, RAY)
}

/// `shares = floor(assets * RAY / index)`.
///
/// Views and the mock controller only. The deposit path never computes a
/// share count this way: it measures the scaled delta XOXNO actually credited,
/// because XOXNO's own rounding decides that figure and a guess which floors
/// differently would break the invariant cumulatively rather than once.
pub fn assets_to_shares(e: &Env, assets: i128, index: i128) -> i128 {
    mul_div_floor(e, assets, RAY, index)
}

/// `shares = ceil(assets * RAY / index)`: what `withdraw` burns to pay exactly
/// `assets`.
///
/// XOXNO burns `ceil(assets * 1e20 * RAY / index)` Ray units for the same
/// withdrawal, and `ceil(x * 1e20) <= ceil(x) * 1e20`, so the position drops
/// by at most what the vault burned. The invariant survives `withdraw` for the
/// same reason it survives `redeem`.
pub fn assets_to_shares_up(e: &Env, assets: i128, index: i128) -> i128 {
    mul_div_ceil(e, assets, RAY, index)
}

/// `assets = ceil(shares * index / RAY)`: what `mint` charges to issue exactly
/// `shares`.
///
/// Supplying that many assets makes XOXNO credit
/// `floor(assets * 1e20 * RAY / index) >= shares * 1e20` Ray units, so at least
/// `shares` at share precision; the excess stays unminted in the position.
pub fn shares_to_assets_up(e: &Env, shares: i128, index: i128) -> i128 {
    mul_div_ceil(e, shares, index, RAY)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn scaled_unit_is_ray_over_asset_precision() {
        assert_eq!(SCALED_UNIT, 10i128.pow(27 - ASSET_DECIMALS));
        // 10 XLM at index RAY, as the controller reports it, is 10 XLM of shares.
        assert_eq!(
            ray_scaled_to_shares(1_000_0000000 * SCALED_UNIT),
            1_000_0000000
        );
        // Sub-unit dust floors away; it stays in the position, never in shares.
        assert_eq!(ray_scaled_to_shares(SCALED_UNIT - 1), 0);
    }

    #[test]
    fn identity_at_ray() {
        let e = Env::default();
        assert_eq!(shares_to_assets(&e, 1_0000000, RAY), 1_0000000);
        assert_eq!(assets_to_shares(&e, 1_0000000, RAY), 1_0000000);
    }

    #[test]
    fn grows_with_index() {
        let e = Env::default();
        let index = RAY + RAY / 100; // 1.01
        assert_eq!(shares_to_assets(&e, 1_0000000, index), 1_0100000);
    }

    /// The case a `checked_mul` implementation would panic on.
    #[test]
    fn no_overflow_at_supply_cap() {
        let e = Env::default();
        let shares = 50_000_000_000_000i128; // 5,000,000 USDC at 7 decimals
        assert_eq!(shares_to_assets(&e, shares, RAY * 3), shares * 3);
    }

    #[test]
    fn floors_rather_than_rounds() {
        let e = Env::default();
        // An index a hair under 2.0 must not round a single share up to 2.
        assert_eq!(shares_to_assets(&e, 1, RAY * 2 - 1), 1);
    }

    #[test]
    fn zero_is_zero() {
        let e = Env::default();
        assert_eq!(shares_to_assets(&e, 0, RAY * 2), 0);
        assert_eq!(assets_to_shares(&e, 0, RAY * 2), 0);
    }

    /// The redeem-side dust argument, checked directly: the scaled units XOXNO
    /// would burn for the payout never exceed the shares the vault burned.
    #[test]
    fn round_trip_never_exceeds_shares() {
        let e = Env::default();
        let index = RAY + 123_456_789_012_345_678_901_234i128;
        for shares in [1i128, 2, 7, 1_0000000, 999_999_999, 12_345_678_901] {
            let assets = shares_to_assets(&e, shares, index);
            let back = assets_to_shares(&e, assets, index);
            // ceil(assets * RAY / index)
            let back_ceil = if shares_to_assets(&e, back, index) < assets {
                back + 1
            } else {
                back
            };
            assert!(
                back_ceil <= shares,
                "shares={shares} assets={assets} back_ceil={back_ceil}"
            );
        }
    }
}

/// The `mint` and `withdraw` rounding arguments, checked against a model of
/// the controller's own Ray-precision rounding (`floor` on supply, `ceil` on
/// withdraw), over random indexes and amounts.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use vault_common::math::mul_div_ceil;

    // 1 billion tokens at 7 decimals: keeps the model's `amount * SCALED_UNIT`
    // (an i128 stand-in for the controller's wider Ray arithmetic) in range.
    const MAX_AMOUNT: i128 = 10_000_000_000_000_000;
    // 1.0 to 10.0: an index starts at RAY and only bad debt takes it below.
    const MIN_INDEX: i128 = RAY;
    const MAX_INDEX: i128 = 10 * RAY;

    /// Ray units XOXNO credits for a supply of `assets`.
    fn controller_credits(e: &Env, assets: i128, index: i128) -> i128 {
        mul_div_floor(e, assets * SCALED_UNIT, RAY, index)
    }

    /// Ray units XOXNO burns for a withdrawal of `assets`.
    fn controller_burns(e: &Env, assets: i128, index: i128) -> i128 {
        mul_div_ceil(e, assets * SCALED_UNIT, RAY, index)
    }

    proptest! {
        /// Supplying `preview_mint(shares)` always credits at least `shares`.
        #[test]
        fn mint_rounding_always_covers_the_shares(
            shares in 1i128..=MAX_AMOUNT,
            index in MIN_INDEX..=MAX_INDEX,
        ) {
            let e = Env::default();
            let assets = shares_to_assets_up(&e, shares, index);
            let credited = ray_scaled_to_shares(controller_credits(&e, assets, index));
            prop_assert!(credited >= shares, "assets={assets} credited={credited}");
        }

        /// Burning `preview_withdraw(assets)` shares always covers what XOXNO
        /// burns from the position, so `total_supply * SCALED_UNIT <= position`
        /// survives every withdrawal.
        #[test]
        fn withdraw_rounding_always_covers_the_position_drop(
            assets in 1i128..=MAX_AMOUNT,
            index in MIN_INDEX..=MAX_INDEX,
        ) {
            let e = Env::default();
            let shares = assets_to_shares_up(&e, assets, index);
            let dropped = controller_burns(&e, assets, index);
            prop_assert!(shares * SCALED_UNIT >= dropped, "shares={shares} dropped={dropped}");
        }

        /// The up and down variants bracket the exact value and differ by at most 1.
        #[test]
        fn rounding_up_within_one_of_down(
            amount in 0i128..=MAX_AMOUNT,
            index in MIN_INDEX..=MAX_INDEX,
        ) {
            let e = Env::default();
            let a_down = shares_to_assets(&e, amount, index);
            let a_up = shares_to_assets_up(&e, amount, index);
            prop_assert!(a_down <= a_up && a_up - a_down <= 1);
            let s_down = assets_to_shares(&e, amount, index);
            let s_up = assets_to_shares_up(&e, amount, index);
            prop_assert!(s_down <= s_up && s_up - s_down <= 1);
        }
    }
}
