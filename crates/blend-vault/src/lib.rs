#![no_std]

//! SEP-56 tokenized vault over a Blend pool supply position.
//!
//! # What this is
//!
//! Blend represents a supply position as bTokens whose value in the underlying
//! grows with the reserve's `b_rate`. This vault holds one bToken position in
//! one reserve and issues shares against it. Unlike the XOXNO adapter, shares
//! are *not* 1:1 with the protocol's unit: the vault keeps its own
//! `total_shares / total_b_tokens` ratio ([`vault::VaultData`]) so that BLND
//! emissions, which `claim_emissions` swaps and re-supplies, accrue to every
//! holder pro rata without a second token.
//!
//! The exchange rate YBC reads is therefore two conversions deep:
//! `shares -> bTokens` through the vault's ratio, then `bTokens -> underlying`
//! through the pool's `b_rate`. Both round down.
//!
//! # What is shared
//!
//! The share token (OpenZeppelin `Base`), the operator-allowance rule, the
//! positive-amount guard, the TTL policy and the `Deposit`/`Withdraw` events all
//! come from `vault_common`. This crate holds only what is
//! Blend's: the pool client, the bToken ratio maths, and emissions harvesting.
//!
//! # Heritage
//!
//! Forked from Script3's `fee-vault-v2`. The vault ratio maths and its
//! property tests are theirs; the fee modes, signer gate and hand-rolled share
//! token that fork carried have been removed.

#[cfg(any(test, feature = "testutils"))]
extern crate std;
#[cfg(any(test, feature = "testutils"))]
pub mod testutils;

pub mod blend;
pub mod constants;
pub mod contract;
pub mod errors;
pub mod events;
pub mod pool;
pub mod storage;
pub mod swap;
pub mod vault;

pub use contract::*;
pub use errors::BlendVaultError;
pub use vault_common::VaultError;

#[cfg(test)]
mod tests;
