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
use vault_common::math::mul_div_floor;

use crate::controller::RAY;

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
/// Previews and the mock controller only. The deposit path never computes a
/// share count this way: it measures the scaled delta XOXNO actually credited,
/// because XOXNO's own rounding decides that figure and a guess which floors
/// differently would break the invariant cumulatively rather than once.
#[allow(dead_code)]
pub fn assets_to_shares(e: &Env, assets: i128, index: i128) -> i128 {
    mul_div_floor(e, assets, RAY, index)
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
