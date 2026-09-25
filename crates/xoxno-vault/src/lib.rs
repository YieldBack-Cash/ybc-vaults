#![no_std]

//! SEP-56 tokenized vault over a XOXNO lending position.
//!
//! # What this is
//!
//! XOXNO is a Soroban money market. It stores a supply position as a
//! `scaled_amount`, a figure that does *not* change as interest accrues,
//! against a per-market `supply_index` in RAY. The real balance is
//! `scaled_amount * supply_index / RAY`.
//!
//! That is already the tokenized-vault share primitive: a fixed claim on a
//! growing pool. So this vault does not invent a second one. **Vault shares are
//! XOXNO's scaled units at asset precision**, and the exchange rate *is* the
//! market's supply index. XOXNO keeps `scaled_amount` as a 27-decimal Ray; one
//! share is `10^20` of those for a 7-decimal asset (`vault::SCALED_UNIT`).
//!
//! This is the same choice Pendle makes wrapping Aave: `PendleAaveV3SY` mirrors
//! Aave's scaled balance against the liquidity index rather than running its own
//! share ratio.
//!
//! # The invariant
//!
//! ```text
//! total_supply * SCALED_UNIT  <=  the vault's XOXNO scaled position (Ray)
//! ```
//!
//! Deposits mint exactly the scaled delta XOXNO credited, so the two track
//! exactly. Redeems burn exactly the shares requested while XOXNO's ceil-rounded
//! scaled burn is never more than that, so dust accrues to remaining holders and
//! the vault only ever becomes *more* over-collateralized. It is never the other
//! way round, which is why the relation is `<=` rather than `=`.
//!
//! # What that buys
//!
//! * **No inflation guard.** Share price comes from the market index, not from
//!   any vault balance. A donation into the vault's XOXNO account raises the
//!   position without raising `total_supply`, widening over-collateralization
//!   instead of inflating price. No virtual offset, no dead shares.
//! * **No bootstrap deposit.** `convert_to_assets(1e7)` is positive on an empty
//!   vault, so a consumer probing the rate at market creation succeeds.
//! * **No rounding wedge.** Price moves only when XOXNO's index moves, so a
//!   consumer that ratchets its rate engages only on real write-downs.
//!
//! # What is shared
//!
//! The share token, the operator-allowance rule, the positive-amount guard,
//! the TTL policy, the `Deposit`/`Redeem` events and the widening multiply all
//! come from `vault_common`. This crate holds only what
//! is XOXNO's: the `lending` mirror of `xoxno-contract-sdk`, the account
//! sentinel, and the index maths.

mod contract;
mod errors;
mod events;
mod lending;
mod storage;
mod vault;

#[cfg(any(test, feature = "testutils"))]
pub mod testutils;

#[cfg(test)]
mod tests;

pub use crate::contract::{XoxnoVault, XoxnoVaultArgs, XoxnoVaultClient};
pub use crate::errors::XoxnoError;
pub use crate::lending::constants::RAY;
pub use crate::lending::controller::{HubAssetKey, MarketIndexRaw};
pub use crate::lending::ControllerClient;
pub use crate::storage::Config;
pub use vault_common::VaultError;
