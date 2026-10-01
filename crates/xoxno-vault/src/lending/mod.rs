//! A hand-written mirror of `xoxno_contract_sdk::lending`.
//!
//! XOXNO publishes [`xoxno-contract-sdk`](https://crates.io/crates/xoxno-contract-sdk)
//! with generated clients, the protocol's constants and a `LendingFixture`
//! that deploys the real protocol into a test `Env`. This crate cannot link it
//! yet: the SDK is built on soroban-sdk 28 and its embedded WASM is protocol
//! 28, while this workspace is held on soroban-sdk 26 by OpenZeppelin's
//! `stellar-tokens` 0.7.2. A protocol-28 WASM does not load on a 26 host, so
//! the Blend trick of vendoring the binaries does not apply either.
//!
//! Until the workspace moves to 28, this module reproduces the slice of the
//! SDK the vault uses, under the SDK's own paths, names and doc comments:
//!
//! | here                                   | in the SDK                                     |
//! |----------------------------------------|------------------------------------------------|
//! | `crate::lending::constants`            | `xoxno_contract_sdk::lending::constants`        |
//! | `crate::lending::controller::{types}`  | `xoxno_contract_sdk::lending::controller::*`    |
//! | `crate::lending::controller::Client`   | `xoxno_contract_sdk::lending::controller::Client` |
//! | `crate::lending::ControllerClient`     | `xoxno_contract_sdk::lending::ControllerClient`  |
//! | `crate::lending::helpers`              | `xoxno_contract_sdk::lending::helpers`          |
//!
//! The migration is then a path change: replace `crate::lending` with
//! `xoxno_contract_sdk::lending`, delete this directory, and replace
//! `testutils::MockController` with the SDK's `LendingFixture`.
//!
//! Every type and signature here was checked field-for-field against the
//! contract spec of the SDK's `controller.wasm` (rs-lending-xlm `v1.1.0`) and
//! of the testnet controller this vault is deployed against; see the crate
//! README, "Reference release".

pub mod constants;
pub mod helpers;

/// The controller: accounts, supply, withdraw and every view the vault reads.
///
/// The SDK generates this module from the controller WASM and its `Client`
/// carries every user, keeper and view function. This copy carries only the
/// nine it declares; eight are called, and `account_exists` is kept so the SDK
/// swap is a path change.
pub mod controller;

pub use controller::Client as ControllerClient;
