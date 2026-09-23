//! Widening fixed-point helpers.
//!
//! `soroban-fixed-point-math` is deliberately absent from this workspace: its
//! releases pin their own `soroban-sdk`, so its `Env` is a distinct type from
//! ours and every env-taking helper fails to typecheck. The one operation the
//! adapters need lives here on the SDK's own `U256`.

use soroban_sdk::{Env, U256};

/// `floor(x * y / denominator)`, carrying the intermediate in 256 bits.
///
/// The widening is not optional. With a 27-decimal index in `y`, a balance of
/// a few million tokens at 7 decimals puts `x * y` around 5e40, and an `i128`
/// tops out near 1.7e38, so a plain `checked_mul` would panic on a perfectly
/// ordinary balance.
///
/// # Panics
///
/// On a negative `x` or `y`, a non-positive `denominator`, or a quotient that
/// does not fit in `i128`. Callers guard amounts positive at the entry points,
/// and indexes and rates are positive by construction, so none of these is
/// reachable from a well-formed call.
pub fn mul_div_floor(e: &Env, x: i128, y: i128, denominator: i128) -> i128 {
    assert!(x >= 0 && y >= 0, "mul_div_floor: negative operand");
    assert!(denominator > 0, "mul_div_floor: non-positive denominator");
    if x == 0 || y == 0 {
        return 0;
    }

    let numerator = U256::from_u128(e, x as u128).mul(&U256::from_u128(e, y as u128));
    let quotient = numerator.div(&U256::from_u128(e, denominator as u128));

    let quotient = quotient
        .to_u128()
        .expect("mul_div_floor: quotient exceeds u128");
    assert!(
        quotient <= i128::MAX as u128,
        "mul_div_floor: quotient exceeds i128"
    );
    quotient as i128
}

#[cfg(test)]
mod test {
    use super::*;

    const RAY: i128 = 1_000_000_000_000_000_000_000_000_000;

    #[test]
    fn identity() {
        let e = Env::default();
        assert_eq!(mul_div_floor(&e, 1_0000000, RAY, RAY), 1_0000000);
    }

    /// The case a `checked_mul` implementation would panic on.
    #[test]
    fn no_overflow_at_large_balances() {
        let e = Env::default();
        let shares = 50_000_000_000_000i128; // 5,000,000 tokens at 7 decimals
        assert_eq!(mul_div_floor(&e, shares, RAY * 3, RAY), shares * 3);
    }

    #[test]
    fn floors_rather_than_rounds() {
        let e = Env::default();
        assert_eq!(mul_div_floor(&e, 1, RAY * 2 - 1, RAY), 1);
    }

    #[test]
    fn zero_is_zero() {
        let e = Env::default();
        assert_eq!(mul_div_floor(&e, 0, RAY * 2, RAY), 0);
        assert_eq!(mul_div_floor(&e, RAY, 0, RAY), 0);
    }

    #[test]
    #[should_panic(expected = "negative operand")]
    fn rejects_a_negative_operand() {
        let e = Env::default();
        mul_div_floor(&e, -1, RAY, RAY);
    }
}
