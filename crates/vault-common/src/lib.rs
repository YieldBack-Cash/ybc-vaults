#![no_std]

//! What every YBC vault adapter has in common, and nothing else.
//!
//! A bug here is a bug in every vault, so the crate is kept deliberately small
//! and holds **no rate math**. How shares convert to assets is the one thing
//! that differs between protocols, and it is where the confirmed vault bug in
//! the threat model lived. Each adapter owns its own.
//!
//! What is here:
//!
//! * the SEP-41 share token, delegated to OpenZeppelin's `Base` through
//!   [`impl_share_token!`] so no adapter hand-rolls balances or allowances;
//! * the operator-allowance rule for delegated exits ([`auth`]);
//! * the positive-amount guard every entry point must call ([`guard`]);
//! * a widening `mul_div_floor` on `U256` ([`math`]): the SDK's `i128` overflows
//!   on ordinary balances once an index or rate is in the multiplier;
//! * instance TTL policy ([`ttl`]);
//! * the `Deposit` / `Redeem` / `Sweep` events, with the holder as a topic so an
//!   indexer can attribute by look-through ([`events`]);
//! * the stray-token `sweep` that hard-refuses depositor funds ([`sweep`]);
//! * the shared error codes and the numbering rule that keeps adapters and
//!   OpenZeppelin from colliding ([`errors`]).

pub mod auth;
pub mod errors;
pub mod events;
pub mod guard;
pub mod math;
pub mod sweep;
pub mod token;
pub mod ttl;

pub use errors::VaultError;

// Re-exported for `impl_share_token!`, which expands inside the adapter crate
// and must name these without assuming what the adapter imported.
#[doc(hidden)]
pub use soroban_sdk;
#[doc(hidden)]
pub use stellar_tokens;
