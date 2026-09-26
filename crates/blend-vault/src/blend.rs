//! Typed client for the Blend pool, generated from the binary vendored in
//! `wasm/blend/` (see `MANIFEST.md` there).
//!
//! This is exactly what `blend-contract-sdk` does internally. That crate pins
//! its own `soroban-sdk`, which is why it cannot be a dependency here; the
//! generated client is identical because it comes from the same WASM. The
//! rest of the protocol (factory, backstop, emitter, Comet) is only needed to
//! stand up a test deployment, which `vault_testkit::protocols::blend` does.

pub mod pool {
    soroban_sdk::contractimport!(file = "../../wasm/blend/pool.wasm");
}
