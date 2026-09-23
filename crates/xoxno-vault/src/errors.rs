use soroban_sdk::contracterror;

/// Codes are grouped: 1x construction/config, 2x amounts, 3x balances and
/// allowances, 4x sweep.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VaultError {
    NotInitialized = 10,
    AlreadyInitialized = 11,

    AmountNotPositive = 20,
    /// A redeem whose asset value floors to zero. Rejected rather than passed
    /// through: XOXNO reads a withdrawal amount of `0` as *withdraw everything*,
    /// so forwarding it would drain the whole pooled position for a dust redeem.
    ZeroAssetRedeem = 21,

    InsufficientBalance = 30,
    InsufficientAllowance = 31,
    NoAccount = 32,

    /// `sweep` was aimed at the underlying asset or at the vault's own share
    /// token. Both are refused — either would let the admin take depositor funds.
    SweepForbidden = 40,
}
