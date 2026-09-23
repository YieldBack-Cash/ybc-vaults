use soroban_sdk::contracterror;

/// XOXNO-specific errors. Codes 300–399 per the workspace numbering rule in
/// `vault_common::VaultError`; the shared codes (not-positive amounts, sweep
/// refusals, initialization) come from there.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum XoxnoError {
    /// A redeem whose asset value floors to zero. Rejected rather than passed
    /// through: XOXNO reads a withdrawal amount of `0` as *withdraw everything*,
    /// so forwarding it would drain the whole pooled position for a dust redeem.
    ZeroAssetRedeem = 300,

    /// A redeem before the vault has ever opened a XOXNO account.
    NoAccount = 301,
}
