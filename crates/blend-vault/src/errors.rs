use soroban_sdk::contracterror;

/// Blend-specific errors. Codes 200–299 per the workspace numbering rule in
/// `vault_common::VaultError`; the shared codes (not-positive amounts,
/// initialization) come from there and the share-token codes
/// (100–114) from OpenZeppelin.
///
/// The fork this crate came from numbered its errors 100–113, which collided
/// with OpenZeppelin's range once the token was delegated. Renumbered here.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BlendVaultError {
    /// The vault's own totals could not cover the burn: an accounting
    /// invariant failure, not a user error.
    InsufficientReserves = 200,
    /// A deposit too small to mint a single bToken at the current `b_rate`.
    InvalidBTokensMinted = 201,
    /// A redeem too small to be worth a single underlying unit.
    InvalidBTokensBurnt = 202,
    /// A deposit too small to mint a single share at the current ratio.
    InvalidSharesMinted = 203,
    /// A withdrawal too small to cost a single share at the current ratio.
    InvalidSharesBurnt = 204,
    /// `claim_emissions` was called before `set_router`.
    SwapNotConfigured = 205,
    /// The swap router returned no output amount for the asset leg.
    SwapNoOutput = 206,
}
