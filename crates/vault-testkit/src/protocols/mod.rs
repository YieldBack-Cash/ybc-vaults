//! The protocols the adapters supply into, as tests stand them up.
//!
//! One home for each protocol fixture. The adapter crates run their own tests
//! and the conformance suite on these, and `ybc-contracts/tests/vaults` builds
//! YBC markets on the adapter binaries over the same fixtures, so a protocol
//! behaviour a mock reproduces is reproduced once.

pub mod blend;
pub mod xoxno;
