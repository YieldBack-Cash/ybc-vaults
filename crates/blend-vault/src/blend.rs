//! Typed clients for the Blend protocol, generated from the vendored binaries
//! in `wasm/blend/` (see `MANIFEST.md` there).
//!
//! This is exactly what `blend-contract-sdk` does internally. That crate pins
//! its own `soroban-sdk`, which is why it cannot be a dependency here; the
//! generated clients are identical because they come from the same WASM.
//!
//! Only `pool` is needed by the contract. The rest exist so the test harness
//! can deploy a real Blend stack.

pub mod pool {
    soroban_sdk::contractimport!(file = "../../wasm/blend/pool.wasm");
}

#[cfg(any(test, feature = "testutils"))]
pub mod pool_factory {
    soroban_sdk::contractimport!(file = "../../wasm/blend/pool_factory.wasm");
}

#[cfg(any(test, feature = "testutils"))]
pub mod backstop {
    soroban_sdk::contractimport!(file = "../../wasm/blend/backstop.wasm");
}

#[cfg(any(test, feature = "testutils"))]
pub mod emitter {
    soroban_sdk::contractimport!(file = "../../wasm/blend/emitter.wasm");
}

#[cfg(any(test, feature = "testutils"))]
pub mod comet {
    soroban_sdk::contractimport!(file = "../../wasm/blend/comet.wasm");
}
